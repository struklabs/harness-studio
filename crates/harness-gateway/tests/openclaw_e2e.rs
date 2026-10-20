//! Opt in: HS_LLAMA_SERVER and HS_TEST_MODEL must point to real local files, and `node`
//! (24.16+ or 26.1+) and `npm` must be on PATH.
//! cargo test -p harness-gateway --test openclaw_e2e -- --ignored --nocapture
//! Installs the pinned upstream OpenClaw into Harness's own temporary prefix. The OpenClaw
//! state, home, tokens and data directories are all temporary.
use harness_core::{
    domain::{ModelDefinition, ModelRef},
    modules::agents::{self, AgentKind},
    params::ParamValue,
    servers::ServerRegistry,
};
use harness_gateway::{GatewayContext, GatewayServer};
use std::{path::PathBuf, sync::Arc, time::Duration};

async fn terminal() -> agents::TaskStatus {
    tokio::time::timeout(Duration::from_secs(600), async {
        loop {
            let status = agents::task_status();
            if status.status != "running" {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .expect("OpenClaw task exceeded test deadline")
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Node, npm, network for npm, llama-server, and a chat GGUF"]
async fn real_openclaw_install_connect_task_stop_disconnect() {
    let bin = PathBuf::from(std::env::var_os("HS_LLAMA_SERVER").expect("HS_LLAMA_SERVER required"));
    let model = PathBuf::from(std::env::var_os("HS_TEST_MODEL").expect("HS_TEST_MODEL required"));
    assert!(bin.is_file() && model.is_file());
    let data = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("openclaw-e2e"))
        .tempdir()
        .unwrap();
    let profile = tempfile::Builder::new()
        .prefix("harness-openclaw-e2e-home-")
        .tempdir()
        .unwrap();
    let state = profile.path().join(".openclaw");
    std::env::set_var("OPENCLAW_STATE_DIR", &state);
    std::env::remove_var("OPENCLAW_CONFIG_PATH");
    std::env::set_var("USERPROFILE", profile.path());
    std::env::set_var("HOME", profile.path());
    std::env::set_var("APPDATA", profile.path().join("AppData/Roaming"));
    harness_core::paths::init(data.path().to_path_buf());
    harness_core::secrets::use_memory_store();
    let mut settings = harness_core::config::AppSettings::load();
    settings.runtime_mode = harness_core::config::RuntimeMode::Custom;
    settings.custom_binary.binary_path = Some(bin);
    settings.custom_binary.is_valid = true;
    settings.server.port = 11640;
    settings.control_api.port = 11699;
    settings.save().unwrap();
    let mut def = ModelDefinition::new("openclaw-e2e", ModelRef::Local { path: model });
    def.load.insert("gpu_layers".into(), ParamValue::Int(99));
    def.load
        .insert("context_size".into(), ParamValue::Int(32768));
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
                port: 11620,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base_url = status.endpoint.unwrap().base_url;
    let bin_root = data.path().to_path_buf();
    let result = async move {
        let before = tokio::task::spawn_blocking(|| agents::detect_kind(AgentKind::OpenClaw))
            .await
            .unwrap()
            .unwrap();
        eprintln!(
            "Before install: installed={} missing={:?}",
            before.installed, before.missing_dependencies
        );
        assert!(
            !before.installed,
            "an OpenClaw outside the test prefix was found"
        );
        assert!(
            before.missing_dependencies.is_empty(),
            "{:?}",
            before.missing_dependencies
        );
        if let Err(e) = agents::install_openclaw().await {
            let shown = agents::detect_kind(AgentKind::OpenClaw)
                .unwrap()
                .installation;
            panic!(
                "install failed: {e}
{}",
                shown.output
            );
        }
        let detection = tokio::task::spawn_blocking(|| agents::detect_kind(AgentKind::OpenClaw))
            .await
            .unwrap()
            .unwrap();
        eprintln!(
            "Installed version: {:?} at {:?}",
            detection.version, detection.executable
        );
        assert!(detection.installed && detection.version.unwrap().contains("OpenClaw"));
        assert!(detection.executable.unwrap().contains("modules"));

        let config = state.join("openclaw.json");
        std::fs::create_dir_all(&state).unwrap();
        let old = b"// Original configuration\n{\n  \"agents\": {\"defaults\": {\"workspace\": \"~/keep\"}},\n}\n";
        std::fs::write(&config, old).unwrap();
        agents::connect_kind(AgentKind::OpenClaw, &base_url, "openclaw-e2e").unwrap();
        let tokens = harness_core::control_api::TokenStore::global();
        let token = tokens.list().unwrap().remove(0);
        assert_eq!(
            token.scopes,
            vec![harness_core::control_api::Scope::Inference]
        );
        assert_eq!(token.policy.models, vec!["openclaw-e2e"]);
        let written = std::fs::read_to_string(&config).unwrap();
        assert!(written.contains("harness/openclaw-e2e"));
        let secret = serde_json::from_str::<serde_json::Value>(&written).unwrap()["models"]
            ["providers"]["harness"]["apiKey"]
            .as_str()
            .unwrap()
            .to_string();

        agents::start_task_kind(
            AgentKind::OpenClaw,
            "Reply with exactly HARNESS_OPENCLAW_OK and nothing else.".into(),
            agents::RunGrants::default(),
        )
        .unwrap();
        let task = terminal().await;
        eprintln!(
            "Task {}: {:?} tokens={:?} err={:?} audit={:?}",
            task.status, task.output, task.tokens, task.error, task.audit
        );
        assert_eq!(task.status, "completed", "{}", task.output);
        assert!(!task.output.trim().is_empty());
        assert_eq!(task.agent, AgentKind::OpenClaw);
        assert_eq!(task.model, "openclaw-e2e");
        assert!(task.started_at > 0 && task.finished_at.is_some());
        assert!(task.tokens.is_some());
        assert!(!task.output.contains(&secret));
        assert!(task.audit.iter().any(|e| e == "completed"));

        let folder = tempfile::tempdir().unwrap();
        let input = folder.path().join("input.txt");
        std::fs::write(&input, "HARNESS_SCOPED_FILE_OK").unwrap();
        agents::start_task_kind(
            AgentKind::OpenClaw,
            "Use the read tool on input.txt and return its exact content.".into(),
            agents::RunGrants {
                read_folders: vec![folder.path().to_path_buf()],
            },
        )
        .unwrap();
        let scoped = terminal().await;
        eprintln!(
            "Scoped task {}: {:?} audit={:?}",
            scoped.status, scoped.output, scoped.audit
        );
        assert_eq!(scoped.status, "completed", "{}", scoped.output);
        assert!(scoped.audit.iter().any(|e| e.contains("\"read\"")));
        assert_eq!(
            std::fs::read_to_string(&input).unwrap(),
            "HARNESS_SCOPED_FILE_OK"
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

        agents::start_task_kind(
            AgentKind::OpenClaw,
            "Write an extremely long essay about the solar system.".into(),
            agents::RunGrants::default(),
        )
        .unwrap();
        tokio::time::sleep(Duration::from_secs(3)).await;
        let stopping = std::time::Instant::now();
        agents::stop();
        assert_eq!(terminal().await.status, "stopped");
        eprintln!("Stop took {:?}", stopping.elapsed());

        agents::disconnect_kind(AgentKind::OpenClaw).unwrap();
        assert!(tokens.list().unwrap().is_empty());
        assert_eq!(std::fs::read(&config).unwrap(), old);
        let mut backup = config.as_os_str().to_os_string();
        backup.push(".harness.bak");
        assert!(!PathBuf::from(backup).exists());
        assert!(!state.join("harness-connection.json").exists());
        // The revoked token no longer reaches the gateway.
        let denied = reqwest::Client::new()
            .get(format!("{base_url}/models"))
            .bearer_auth(&secret)
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), 401);

        agents::remove_openclaw().unwrap();
        assert!(!bin_root.join("bin/modules/openclaw").exists());
        eprintln!("Real OpenClaw install, connect, task, Stop and restore/revoke passed.");
    };
    // Always terminate test-owned processes, including assertion failure paths.
    let outcome = tokio::spawn(result).await;
    agents::stop();
    let _ = terminal().await;
    let _ = agents::disconnect_kind(AgentKind::OpenClaw);
    gateway.stop().await;
    registry.stop_all().await;
    outcome.unwrap();
}
