//! Opt in: HS_LLAMA_SERVER and HS_TEST_MODEL must point to real local files; needs network.
//! cargo test -p harness-gateway --test hermes_managed_e2e -- --ignored --nocapture
//! Installs the Harness-managed Hermes Agent into a temporary data folder (and a temporary
//! home, so an external Hermes is never seen), runs a task, then removes it again.
use harness_core::{
    domain::{ModelDefinition, ModelRef},
    modules::{self, agents, Status},
    params::ParamValue,
    servers::ServerRegistry,
};
use harness_gateway::{GatewayContext, GatewayServer};
use std::{path::PathBuf, sync::Arc, time::Duration};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads and installs upstream Hermes; needs llama-server and a chat GGUF"]
async fn managed_hermes_install_detect_task_remove() {
    let bin = PathBuf::from(std::env::var_os("HS_LLAMA_SERVER").expect("HS_LLAMA_SERVER required"));
    let model = PathBuf::from(std::env::var_os("HS_TEST_MODEL").expect("HS_TEST_MODEL required"));
    assert!(bin.is_file() && model.is_file());
    // HS_TEST_ROOT keeps the folders out of a virtualised AppData (packaged shells).
    let root = std::env::var_os("HS_TEST_ROOT").map(PathBuf::from);
    let make = |prefix: &str| {
        let mut builder = tempfile::Builder::new();
        builder.prefix(prefix);
        match &root {
            Some(root) => builder.tempdir_in(root),
            None => builder.tempdir(),
        }
        .unwrap()
    };
    let data = make(&harness_core::paths::test_dir_prefix("hermes-managed"));
    let home = make("hm-home-");
    // Nothing of the real user profile is visible to the installer or to Hermes.
    std::env::remove_var("HERMES_HOME");
    std::env::set_var("USERPROFILE", home.path());
    std::env::set_var("HOME", home.path());
    std::env::set_var("LOCALAPPDATA", home.path().join("AppData/Local"));
    std::env::set_var("APPDATA", home.path().join("AppData/Roaming"));
    std::fs::create_dir_all(home.path().join("AppData/Local")).unwrap();
    harness_core::paths::init(data.path().to_path_buf());
    harness_core::secrets::use_memory_store();
    let mut settings = harness_core::config::AppSettings::load();
    settings.runtime_mode = harness_core::config::RuntimeMode::Custom;
    settings.custom_binary.binary_path = Some(bin);
    settings.custom_binary.is_valid = true;
    settings.server.port = 19540;
    settings.control_api.port = 19599;
    settings.save().unwrap();
    let mut def = ModelDefinition::new("hermes-managed", ModelRef::Local { path: model });
    def.load.insert("gpu_layers".into(), ParamValue::Int(0));
    def.load
        .insert("context_size".into(), ParamValue::Int(8192));
    harness_core::domain::store::Domain::global()
        .save_definition(def)
        .unwrap();

    // The install is a background job: the call returns once it has started.
    let started = std::time::Instant::now();
    modules::start("agents", "install").await.unwrap();
    eprintln!("install call returned after {:?}", started.elapsed());
    assert!(started.elapsed() < Duration::from_secs(60));
    let finished = tokio::time::timeout(Duration::from_secs(40 * 60), async {
        loop {
            let view = tokio::task::spawn_blocking(modules::list)
                .await
                .unwrap()
                .unwrap()
                .into_iter()
                .find(|m| m.manifest.id == "agents")
                .unwrap();
            if view.state.status != Status::Installing {
                return view;
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    })
    .await
    .expect("managed Hermes install exceeded the deadline");
    eprintln!(
        "state {:?} error {:?}",
        finished.state.status, finished.state.error
    );
    if finished.state.status != Status::Installed {
        let log = harness_core::paths::logs_dir().join("agents-install.log");
        eprintln!(
            "--- {} ---
{}",
            log.display(),
            std::fs::read_to_string(&log).unwrap_or_default()
        );
    }
    assert_eq!(finished.state.status, Status::Installed);
    let detection = tokio::task::spawn_blocking(agents::detect)
        .await
        .unwrap()
        .unwrap();
    assert!(detection.installed, "{:?}", detection.missing_dependencies);
    let exe = detection.executable.clone().unwrap();
    assert!(
        exe.starts_with(&agents::managed_root().to_string_lossy().to_string()),
        "{exe}"
    );
    eprintln!("Managed Hermes {:?} at {exe}", detection.version);

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
                port: 19500,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let result = async move {
        agents::connect(&status.endpoint.unwrap().base_url, "hermes-managed").unwrap();
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
        assert!(task.output.contains("HARNESS_HERMES_OK"), "{}", task.output);
        agents::disconnect().unwrap();
    };
    let outcome = tokio::spawn(result).await;
    agents::stop();
    let _ = agents::disconnect();
    gateway.stop().await;
    registry.stop_all().await;
    outcome.unwrap();

    // Remove: only the managed folder goes; nothing else in the data folder is touched.
    tokio::task::spawn_blocking(|| modules::remove("agents"))
        .await
        .unwrap()
        .unwrap();
    assert!(!agents::managed_root().exists());
    let after = tokio::task::spawn_blocking(modules::list)
        .await
        .unwrap()
        .unwrap();
    let agents_view = after.iter().find(|m| m.manifest.id == "agents").unwrap();
    assert_eq!(agents_view.state.status, Status::NotInstalled);
    assert_eq!(agents_view.disk_bytes, 0);
    eprintln!("Managed install, task and removal passed.");
}
