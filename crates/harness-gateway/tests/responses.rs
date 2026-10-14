//! Stateful Responses routes against the real gateway (in-process axum listener on a loopback
//! port, real token store, real state.db, a real model definition on disk). No llama-server
//! is needed: every request here is decided before a model would load. The generation side
//! is `responses_e2e.rs` (`--ignored`, real llama-server).

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use harness_core::control_api::{Scope, TokenStore};
use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

fn data_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        harness_core::secrets::use_memory_store();
        let dir = tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("gw-responses"))
            .tempdir()
            .unwrap()
            .keep();
        harness_core::paths::init(dir.clone());
        let mut s = harness_core::config::AppSettings::load();
        s.control_api.port = 18080;
        s.save().unwrap();
        let file = dir.join("models").join("resp-model.gguf");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"GGUF").unwrap();
        harness_core::domain::store::Domain::global()
            .save_definition(ModelDefinition::new(
                "resp-model",
                ModelRef::Local { path: file },
            ))
            .unwrap();
        dir
    })
}

async fn start() -> (GatewayServer, String) {
    let ctx = GatewayContext {
        registry: Arc::new(ServerRegistry::default()),
        runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
        gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
        data_dir: data_root().to_path_buf(),
        downloads: None,
    };
    let server = GatewayServer::new();
    let ephemeral = GatewaySettings {
        port: 0,
        ..Default::default()
    };
    assert_eq!(
        server.start(ctx.clone(), &ephemeral).await.unwrap_err(),
        harness_core::gateway::ERR_INVALID_PORT
    );
    assert_eq!(
        harness_core::gateway::validate_settings(&ephemeral, 0, None).unwrap_err(),
        harness_core::gateway::ERR_INVALID_PORT
    );
    let status = server
        .start_ephemeral(
            ctx,
            &GatewaySettings {
                enabled: true,
                port: 0,
                require_token: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(status.port.unwrap() >= 1024);
    let base = status.endpoint.unwrap().host_url;
    assert_eq!(reqwest::Url::parse(&base).unwrap().port(), status.port);
    (server, base)
}

async fn call(req: reqwest::RequestBuilder) -> (u16, Value) {
    let r = req.send().await.unwrap();
    let status = r.status().as_u16();
    let text = r.text().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

#[tokio::test]
async fn unknown_ids_get_openai_style_errors() {
    let (server, base) = start().await;
    let token = TokenStore::at(&data_root().join("config"))
        .create("resp-client", &[Scope::Inference])
        .unwrap()
        .token;
    let http = reqwest::Client::new();

    for (method, path) in [
        (reqwest::Method::GET, "/v1/responses/resp_missing"),
        (reqwest::Method::DELETE, "/v1/responses/resp_missing"),
        (
            reqwest::Method::GET,
            "/v1/responses/resp_missing/input_items",
        ),
    ] {
        let (status, body) = call(
            http.request(method.clone(), format!("{base}{path}"))
                .bearer_auth(&token),
        )
        .await;
        assert_eq!(status, 404, "{method} {path}: {body}");
        assert_eq!(
            body["error"]["message"],
            "Response with id 'resp_missing' not found."
        );
        assert_eq!(body["error"]["type"], "invalid_request_error");
    }

    // Unknown previous response: decided before any model load.
    let (status, body) = call(
        http.post(format!("{base}/v1/responses"))
            .bearer_auth(&token)
            .json(
                &json!({"model": "resp-model", "previous_response_id": "resp_nope", "input": "hi"}),
            ),
    )
    .await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["error"]["code"], "previous_response_not_found");
    assert_eq!(body["error"]["param"], "previous_response_id");
    assert_eq!(
        body["error"]["message"],
        "Previous response with id 'resp_nope' not found."
    );

    // Local Responses only exposes data llama-server can actually produce.
    let (status, body) = call(
        http.post(format!("{base}/v1/responses"))
            .bearer_auth(&token)
            .json(&json!({"model": "resp-model", "input": "hi",
                "include": ["file_search_call.results"]})),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"]["code"], "unsupported_value");
    assert_eq!(body["error"]["param"], "include");
    // Wrong types are rejected with the field named.
    let (status, body) = call(
        http.post(format!("{base}/v1/responses"))
            .bearer_auth(&token)
            .json(&json!({"model": "resp-model", "store": "yes", "input": "hi"})),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"]["param"], "store");
    let (status, body) = call(
        http.post(format!("{base}/v1/responses"))
            .bearer_auth(&token)
            .json(&json!({"model": "resp-model", "previous_response_id": 7, "input": "hi"})),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"]["param"], "previous_response_id");

    // Without a token (required here) nothing is readable.
    let (status, _) = call(http.get(format!("{base}/v1/responses/resp_missing"))).await;
    assert_eq!(status, 401);

    // DELETE is allowed by CORS for the app's origin.
    let r = http
        .request(
            reqwest::Method::OPTIONS,
            format!("{base}/v1/responses/resp_missing"),
        )
        .header("origin", "http://localhost:1420")
        .header("access-control-request-method", "DELETE")
        .send()
        .await
        .unwrap();
    let allowed = r
        .headers()
        .get("access-control-allow-methods")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(allowed.contains("DELETE"), "{allowed}");

    server.stop().await;
}
