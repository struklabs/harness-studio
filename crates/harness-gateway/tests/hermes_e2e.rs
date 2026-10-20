//! Opt in: HS_LLAMA_SERVER and HS_TEST_MODEL must point to real local files.
//! cargo test -p harness-gateway --test hermes_e2e -- --ignored --nocapture
//! Uses an existing upstream Hermes CLI; all configuration and tokens are temporary.
use harness_core::{
    domain::{ModelDefinition, ModelRef},
    modules::agents,
    params::ParamValue,
    servers::ServerRegistry,
};
use harness_gateway::{GatewayContext, GatewayServer};
use std::{path::PathBuf, sync::Arc, time::Duration};

async fn terminal() -> agents::TaskStatus {
    tokio::time::timeout(Duration::from_secs(300), async {
        loop {
            let status = agents::task_status();
            if status.status != "running" {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .expect("Hermes task exceeded test deadline")
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires real upstream Hermes, llama-server, and a chat GGUF"]
async fn real_hermes_gateway_connect_task_stop_disconnect() {
    let bin = PathBuf::from(std::env::var_os("HS_LLAMA_SERVER").expect("HS_LLAMA_SERVER required"));
    let model = PathBuf::from(std::env::var_os("HS_TEST_MODEL").expect("HS_TEST_MODEL required"));
    assert!(bin.is_file() && model.is_file());
    let data = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("hermes-e2e"))
        .tempdir()
        .unwrap();
    let home = tempfile::Builder::new()
        .prefix("harness-hermes-e2e-home-")
        .tempdir()
        .unwrap();
    std::env::set_var("HERMES_HOME", home.path());
    harness_core::paths::init(data.path().to_path_buf());
    harness_core::secrets::use_memory_store();
    let mut settings = harness_core::config::AppSettings::load();
    settings.runtime_mode = harness_core::config::RuntimeMode::Custom;
    settings.custom_binary.binary_path = Some(bin);
    settings.custom_binary.is_valid = true;
    settings.server.port = 19340;
    settings.control_api.port = 19399;
    settings.save().unwrap();
    let mut def = ModelDefinition::new("hermes-e2e", ModelRef::Local { path: model });
    def.load.insert("gpu_layers".into(), ParamValue::Int(99));
    def.load
        .insert("context_size".into(), ParamValue::Int(16384));
    harness_core::domain::store::Domain::global()
        .save_definition(def)
        .unwrap();
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
                port: 19300,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let result = async move {
        let detection = tokio::task::spawn_blocking(agents::detect)
            .await
            .unwrap()
            .unwrap();
        assert!(detection.installed, "{:?}", detection.missing_dependencies);
        eprintln!("Upstream version: {:?}", detection.version);
        let old = b"# Original configuration\nmodel: previous\n";
        std::fs::write(home.path().join("config.yaml"), old).unwrap();
        agents::connect(&status.endpoint.unwrap().base_url, "hermes-e2e").unwrap();
        let tokens = harness_core::control_api::TokenStore::global();
        assert_eq!(
            tokens.list().unwrap()[0].scopes,
            vec![harness_core::control_api::Scope::Inference]
        );
        agents::start_task("Reply with exactly HARNESS_HERMES_OK and nothing else.".into())
            .unwrap();
        let task = terminal().await;
        eprintln!("Task {}: {} {:?}", task.status, task.output, task.error);
        assert_eq!(task.status, "completed");
        assert!(task.output.contains("HARNESS_HERMES_OK"));
        assert_eq!(task.model, "hermes-e2e");
        assert!(task.started_at > 0 && task.finished_at.is_some());
        assert!(task.tokens.is_some());
        let file_scope = tempfile::tempdir().unwrap();
        let input = file_scope.path().join("input.txt");
        std::fs::write(&input, "HARNESS_SCOPED_FILE_OK").unwrap();
        agents::start_task_with_grants(
            format!(
                "Use harness_read_file to read {} and return its exact content.",
                input.to_string_lossy().replace('\\', "/")
            ),
            agents::RunGrants {
                read_folders: vec![file_scope.path().to_path_buf()],
            },
        )
        .unwrap();
        let scoped = terminal().await;
        eprintln!(
            "Scoped task: {} {} {:?}",
            scoped.status, scoped.output, scoped.audit
        );
        assert_eq!(scoped.status, "completed", "{}", scoped.output);
        assert!(
            scoped.output.contains("HARNESS_SCOPED_FILE_OK"),
            "{}",
            scoped.output
        );
        assert!(
            scoped
                .audit
                .iter()
                .any(|e| e.contains("harness_read_file") && e.contains("completed")),
            "{:?}",
            scoped.audit
        );
        let history = agents::task_history().unwrap();
        assert!(history
            .iter()
            .any(|e| e.id == task.id && e.output == task.output));
        agents::delete_task(&task.id).unwrap();
        assert!(!agents::task_history()
            .unwrap()
            .iter()
            .any(|e| e.id == task.id));
        assert_eq!(
            std::fs::read_to_string(&input).unwrap(),
            "HARNESS_SCOPED_FILE_OK"
        );
        agents::start_task("Write an extremely long essay about the solar system.".into()).unwrap();
        agents::stop();
        assert_eq!(terminal().await.status, "stopped");
        tokens.revoke(&tokens.list().unwrap()[0].id).unwrap();
        agents::start_task("Say hello.".into()).unwrap();
        let failed = terminal().await;
        eprintln!("Revoked-token task status: {}", failed.status);
        assert_eq!(failed.status, "failed", "{}", failed.output);
        assert!(failed.error.is_some());
        agents::disconnect().unwrap();
        assert!(tokens.list().unwrap().is_empty());
        assert_eq!(std::fs::read(home.path().join("config.yaml")).unwrap(), old);
        assert!(!home.path().join("config.yaml.harness.bak").exists());
        eprintln!(
            "Real Hermes → Harness Gateway → llama-server passed; Stop and restore/revoke passed."
        );
    };
    // Always terminate test-owned processes, including assertion failure paths.
    let outcome = tokio::spawn(result).await;
    agents::stop();
    let _ = terminal().await;
    let _ = agents::disconnect();
    gateway.stop().await;
    registry.stop_all().await;
    eprintln!("Temporary Harness data: {}", data.path().display());
    outcome.unwrap();
}
