//! Task runs end to end with a real llama-server and a small real model through the gateway
//! (no mocks): a 3-step run is killed mid-step (the runner is a separate process that is
//! killed), recovered, resumed and finished.
//!
//! ```text
//! HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<small chat gguf, e.g. Qwen2.5-0.5B-Instruct Q4>
//! cargo test -p harness-gateway --test taskflow_e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Temporary data root (never the user's), in-memory secrets, gateway on 12480.

use std::path::{Path, PathBuf};
use std::sync::{atomic::AtomicBool, Arc};
use std::time::{Duration, Instant};

use harness_core::compaction::Endpoint;
use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::params::ParamValue;
use harness_core::servers::ServerRegistry;
use harness_core::state::{NewTaskRun, RunPolicy, StateDb, StepInput};
use harness_core::taskflow::{self, RunOptions};
use harness_gateway::{GatewayContext, GatewayServer};

const PORT: u16 = 12480;
const ALIAS: &str = "tf-e2e-qwen";

fn env_path(name: &str) -> Option<PathBuf> {
    let p = std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_file());
    if p.is_none() {
        eprintln!("{name} not set, skipping");
    }
    p
}

fn init_data_root(dir: &Path, bin: Option<&Path>) {
    harness_core::secrets::use_memory_store();
    harness_core::paths::init(dir.to_path_buf());
    if let Some(bin) = bin {
        let mut s = harness_core::config::AppSettings::load();
        s.runtime_mode = harness_core::config::RuntimeMode::Custom;
        s.custom_binary.binary_path = Some(bin.to_path_buf());
        s.custom_binary.is_valid = true;
        s.server.port = PORT;
        s.control_api.port = PORT + 19;
        s.save().unwrap();
    }
}

fn endpoint(base: &str) -> Endpoint {
    Endpoint {
        base_url: format!("{base}/v1"),
        ..Default::default()
    }
}

fn steps() -> Vec<StepInput> {
    vec![
        StepInput {
            title: "Name".into(),
            prompt: "Name one fruit that is red. Answer with the single word only.".into(),
            gate: false,
        },
        StepInput {
            title: "Essay".into(),
            // Long enough that the process can be killed while the model is still writing.
            prompt: "Write a detailed essay of at least 500 words about why that fruit is healthy."
                .into(),
            gate: false,
        },
        StepInput {
            title: "Summary".into(),
            prompt: "Summarize the essay in one sentence.".into(),
            gate: false,
        },
    ]
}

/// The runner as its own process: runs the task given by the environment until it is killed.
#[tokio::test]
#[ignore]
async fn taskflow_child() {
    let (Ok(id), Ok(base), Ok(dir)) = (
        std::env::var("HS_TF_RUN"),
        std::env::var("HS_TF_BASE"),
        std::env::var("HS_TF_DATA"),
    ) else {
        return;
    };
    init_data_root(Path::new(&dir), None);
    let db = Arc::new(StateDb::open_default().unwrap());
    let _ = taskflow::run_task(
        db,
        &id,
        RunOptions::new(endpoint(&base)),
        Arc::new(AtomicBool::new(false)),
    )
    .await;
}

fn define(model: &Path) {
    let mut d = ModelDefinition::new(ALIAS, ModelRef::Local { path: model.into() });
    d.load.insert("gpu_layers".into(), ParamValue::Int(99));
    d.load.insert("context_size".into(), ParamValue::Int(4096));
    harness_core::domain::store::Domain::global()
        .save_definition(d)
        .unwrap();
}

#[tokio::test]
#[ignore]
async fn kill_mid_run_then_resume_and_finish() {
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL")) else {
        return;
    };
    let dir = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("tf-e2e"))
        .tempdir()
        .unwrap();
    init_data_root(dir.path(), Some(&bin));
    define(&model);
    let registry = Arc::new(ServerRegistry::default());
    let ctx = GatewayContext {
        registry: registry.clone(),
        runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
        gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
        data_dir: dir.path().to_path_buf(),
        downloads: None,
    };
    let server = GatewayServer::new();
    let status = server
        .start(
            ctx,
            &GatewaySettings {
                enabled: true,
                port: PORT,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let db = Arc::new(StateDb::open_default().unwrap());

    let run = db
        .create_task_run(&NewTaskRun {
            kind: "agent_task".into(),
            title: "Fruit".into(),
            goal: "Write about a red fruit.".into(),
            policy: RunPolicy {
                // The first model is not served: the fallback has to say why.
                models: vec!["no-such-model".into(), ALIAS.into()],
                min_context: Some(2048),
                max_tokens: 700,
                ..Default::default()
            },
            steps: steps(),
            ..Default::default()
        })
        .unwrap();

    // The requirement check sees both candidates.
    let checks = taskflow::preflight(&db, &run.id, &endpoint(&base))
        .await
        .unwrap();
    assert!(!checks[0].ok && checks[1].ok, "{checks:?}");
    assert_eq!(checks[1].info.as_ref().unwrap().context, Some(4096));

    // The runner process starts; kill it while step 2 is in flight.
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args(["taskflow_child", "--exact", "--ignored", "--test-threads=1"])
        .env("HS_TF_RUN", &run.id)
        .env("HS_TF_BASE", &base)
        .env("HS_TF_DATA", dir.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        let steps = db.task_steps(&run.id).unwrap();
        if steps[0].status == "done" && steps[1].status == "running" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "step 2 never started: {steps:#?}"
        );
        assert!(
            child.try_wait().unwrap().is_none() || steps[1].status != "pending",
            "runner exited early"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    child.kill().unwrap();
    child.wait().unwrap();
    let first_output = db.task_steps(&run.id).unwrap()[0].output.clone();
    assert!(!first_output.is_empty());
    assert_eq!(
        db.task_steps(&run.id).unwrap()[0].model.as_deref(),
        Some(ALIAS)
    );
    assert!(db.task_steps(&run.id).unwrap()[0]
        .fallback_reason
        .as_deref()
        .unwrap()
        .contains("taskrun.reason.unavailable"));

    // A restart: the run is still "running" with a dead heartbeat; recovery marks it.
    assert_eq!(db.task_run(&run.id).unwrap().status, "running");
    let db2 = Arc::new(StateDb::open_default().unwrap());
    assert_eq!(
        taskflow::recover(&db2, Some(0)).unwrap(),
        vec![run.id.clone()]
    );
    assert_eq!(db2.task_run(&run.id).unwrap().status, "interrupted");
    let s = db2.task_steps(&run.id).unwrap();
    assert_eq!(
        (s[0].status.as_str(), s[1].status.as_str()),
        ("done", "pending")
    );

    // Resume: only steps 2 and 3 run; step 1 keeps its output; the tally keeps growing.
    let tokens_before = db2.task_run(&run.id).unwrap().tokens_out;
    let done = taskflow::run_task(
        db2.clone(),
        &run.id,
        RunOptions::new(endpoint(&base)),
        Arc::new(AtomicBool::new(false)),
    )
    .await
    .unwrap();
    assert_eq!(done.status, "completed", "{:?}", done.error);
    let s = db2.task_steps(&run.id).unwrap();
    assert!(
        s.iter().all(|s| s.status == "done" && !s.output.is_empty()),
        "{s:#?}"
    );
    assert_eq!(s[0].output, first_output);
    assert!(done.tokens_out > tokens_before);
    assert_eq!(db2.task_checkpoints(&run.id).unwrap().len(), 3);
    let events = db2.task_events(&run.id).unwrap();
    assert!(events.iter().any(|e| e.kind == "recovered"));
    eprintln!("outputs:");
    for st in &s {
        eprintln!(
            "  {} [{}]: {}",
            st.title,
            st.model.clone().unwrap_or_default(),
            st.output
                .chars()
                .take(120)
                .collect::<String>()
                .replace('\n', " ")
        );
    }
    eprintln!(
        "tally: {} in / {} out tokens, {} micro-USD",
        done.tokens_in, done.tokens_out, done.cost_micro
    );

    // The finished run moves to another project as a bundle and can be rewound there.
    let bundle_path = dir.path().join("fruit.taskrun.json");
    taskflow::bundle::export(&db2, &run.id, &bundle_path, true).unwrap();
    let imported = taskflow::bundle::import(&db2, &bundle_path, None).unwrap();
    assert_eq!(imported.run.status, "completed");
    let rewound = db2.rewind_task_run(&imported.run.id, 2).unwrap();
    assert_eq!(rewound.status, "paused");
    let again = taskflow::run_task(
        db2.clone(),
        &imported.run.id,
        RunOptions::new(endpoint(&base)),
        Arc::new(AtomicBool::new(false)),
    )
    .await
    .unwrap();
    assert_eq!(again.status, "completed");

    server.stop().await;
    registry.stop_all().await;
}
