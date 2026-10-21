//! Opt in: HS_LLAMA_SERVER, HS_TEST_MODEL and HS_EXTERNAL_HERMES_HOME must point to real local
//! files/folders. HS_EXTERNAL_HERMES_HOME is a THROWAWAY new-style Hermes home made by the
//! upstream installer (never the user's own); it is only read, and the test fails if any file
//! outside its checkout changes.
//! cargo test -p harness-gateway --test hermes_external_e2e -- --ignored --nocapture
use harness_core::{
    domain::{ModelDefinition, ModelRef},
    modules::agents,
    params::ParamValue,
    servers::ServerRegistry,
};
use harness_gateway::{GatewayContext, GatewayServer};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

fn snapshot(root: &Path) -> BTreeMap<PathBuf, (u64, std::time::SystemTime)> {
    let mut found = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if name == "node_modules" || name == ".git" || name == "hermes-agent" {
                continue;
            }
            match entry.metadata() {
                Ok(m) if m.is_dir() => stack.push(path),
                Ok(m) => {
                    found.insert(path, (m.len(), m.modified().unwrap()));
                }
                Err(_) => {}
            }
        }
    }
    found
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a throwaway external Hermes, llama-server and a chat GGUF"]
async fn external_new_style_hermes_runs_a_task_without_touching_its_home() {
    let bin = PathBuf::from(std::env::var_os("HS_LLAMA_SERVER").expect("HS_LLAMA_SERVER required"));
    let model = PathBuf::from(std::env::var_os("HS_TEST_MODEL").expect("HS_TEST_MODEL required"));
    let external = PathBuf::from(
        std::env::var_os("HS_EXTERNAL_HERMES_HOME").expect("HS_EXTERNAL_HERMES_HOME required"),
    );
    assert!(bin.is_file() && model.is_file() && external.join("installs").is_dir());
    let data = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("hermes-external"))
        .tempdir()
        .unwrap();
    std::env::set_var("HERMES_HOME", &external);
    harness_core::paths::init(data.path().to_path_buf());
    harness_core::secrets::use_memory_store();
    let mut settings = harness_core::config::AppSettings::load();
    settings.runtime_mode = harness_core::config::RuntimeMode::Custom;
    settings.custom_binary.binary_path = Some(bin);
    settings.custom_binary.is_valid = true;
    settings.server.port = 19640;
    settings.control_api.port = 19699;
    settings.save().unwrap();
    let mut def = ModelDefinition::new("hermes-external", ModelRef::Local { path: model });
    def.load.insert("gpu_layers".into(), ParamValue::Int(0));
    def.load
        .insert("context_size".into(), ParamValue::Int(8192));
    harness_core::domain::store::Domain::global()
        .save_definition(def)
        .unwrap();

    let detection = tokio::task::spawn_blocking(agents::detect)
        .await
        .unwrap()
        .unwrap();
    assert!(
        detection.installed && !detection.managed,
        "{:?}",
        detection.executable
    );
    let before = snapshot(&external);

    let registry = Arc::new(ServerRegistry::default());
    let gateway = GatewayServer::new();
    let status = gateway
        .start(
            GatewayContext {
                registry: registry.clone(),
                runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
                gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
                data_dir: data.path().to_path_buf(),
                downloads: None,
            },
            &harness_core::gateway::GatewaySettings {
                enabled: true,
                require_token: true,
                port: 19600,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let result = async move {
        agents::connect(&status.endpoint.unwrap().base_url, "hermes-external").unwrap();
        agents::start_task("Reply with exactly HARNESS_HERMES_OK and nothing else.".into())
            .unwrap();
        let task = tokio::time::timeout(Duration::from_secs(600), async {
            loop {
                let status = agents::task_status();
                if status.status != "running" {
                    return status;
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        })
        .await
        .expect("task exceeded the deadline");
        eprintln!("Task {}: {} {:?}", task.status, task.output, task.error);
        assert_eq!(task.status, "completed", "{}", task.output);
        // A tiny test model may not follow the instruction; the run itself must produce text.
        assert!(
            !task.output.trim().is_empty(),
            "the task produced no output"
        );
        agents::disconnect().unwrap();
    };
    let outcome = tokio::spawn(result).await;
    agents::stop();
    let _ = agents::disconnect();
    gateway.stop().await;
    registry.stop_all().await;
    outcome.unwrap();

    assert_eq!(
        before,
        snapshot(&external),
        "the external Hermes home must stay unchanged"
    );
    eprintln!("External new-style Hermes verify and task passed.");
}
