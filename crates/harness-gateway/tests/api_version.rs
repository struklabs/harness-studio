//! The gateway names its contract version on every response and answers `/v1/harness/capabilities`
//! without a token.

use std::sync::Arc;

use harness_core::control_api::{API_VERSION, API_VERSION_HEADER};
use harness_core::gateway::GatewaySettings;
use harness_gateway::{GatewayContext, GatewayServer};

#[tokio::test]
async fn gateway_responses_carry_the_api_version() {
    harness_core::secrets::use_memory_store();
    let dir = tempfile::tempdir().unwrap();
    harness_core::paths::init(dir.path().to_path_buf());
    let gateway = GatewayServer::new();
    let status = gateway
        .start_ephemeral(
            GatewayContext {
                registry: Arc::new(harness_core::servers::ServerRegistry::default()),
                runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
                gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
                data_dir: dir.path().to_path_buf(),
                downloads: None,
            },
            &GatewaySettings {
                enabled: true,
                port: 0,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();
    let want = API_VERSION.to_string();

    // Two successful routes and an unknown route all say which contract they speak.
    for (path, code) in [
        ("/health", 200),
        ("/v1/models", 200),
        ("/v1/no-such-route", 404),
    ] {
        let resp = http.get(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(resp.status(), code, "{path}");
        assert_eq!(resp.headers()[API_VERSION_HEADER], want.as_str(), "{path}");
    }

    let caps: serde_json::Value = http
        .get(format!("{base}/v1/harness/capabilities"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(caps["api_version"], 1);
    assert_eq!(caps["module_manifest_versions"], serde_json::json!([1]));
    assert_eq!(caps["features"]["gateway"], true);
}
