//! BYOK setup end to end against local mocks: "Test Connection" before saving, a custom
//! Anthropic-compatible endpoint with extra headers served through the gateway's `/v1/messages`
//! and `/v1/chat/completions`, user prices on models, and the provider's rate-limit answer
//! turned into a keyed error with a retry hint. Never a real key.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use harness_core::{cloud, gateway::GatewaySettings};
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

const KEY: &str = "sk-test-byok-secret-key";
const HEADER_VALUE: &str = "org-secret-value-123";

#[derive(Clone, Default)]
struct Mock {
    seen: Arc<Mutex<Vec<(String, HeaderMap, Value)>>>,
}

impl Mock {
    fn last(&self, path: &str) -> (HeaderMap, Value) {
        let seen = self.seen.lock().unwrap();
        let (_, h, b) = seen.iter().rev().find(|(p, _, _)| p == path).unwrap();
        (h.clone(), b.clone())
    }
}

async fn anthropic_models(State(m): State<Mock>, headers: HeaderMap) -> Response {
    m.seen
        .lock()
        .unwrap()
        .push(("models".into(), headers.clone(), Value::Null));
    if headers.get("x-org-id").is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    Json(json!({"data": [{
        "id": "claude-mini", "display_name": "Claude Mini", "max_input_tokens": 200000,
        "capabilities": {"image_input": {"supported": true}, "thinking": {"supported": true}}
    }]}))
    .into_response()
}

async fn messages(State(m): State<Mock>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    m.seen
        .lock()
        .unwrap()
        .push(("messages".into(), headers, body));
    Json(json!({
        "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-mini",
        "content": [{"type": "text", "text": "hello"}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 1000, "output_tokens": 500}
    }))
    .into_response()
}

async fn limited() -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [("retry-after", "7")],
        Json(json!({"error": {"message": "Rate limit reached"}})),
    )
        .into_response()
}

fn error_key(error: &str) -> (String, Value) {
    let (key, params) = harness_core::error_key::parse(error).expect("keyed error");
    (key.to_string(), Value::Object(params))
}

#[tokio::test]
async fn byok_setup_discovery_headers_prices_and_errors() {
    harness_core::secrets::use_memory_store();
    let dir = tempfile::tempdir().unwrap();
    harness_core::paths::init(dir.path().to_path_buf());
    let mock = Mock::default();
    let app = Router::new()
        .nest(
            "/an/v1",
            Router::new()
                .route("/models", get(anthropic_models))
                .route("/messages", post(messages))
                .with_state(mock.clone()),
        )
        .nest("/limited/v1", Router::new().route("/models", get(limited)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let base = format!("http://{address}/an/v1");
    let input =
        |headers: Option<Vec<cloud::HeaderInput>>, key: Option<&str>| cloud::ProviderInput {
            name: "mine".into(),
            kind: cloud::Kind::CustomAnthropic,
            base_url: base.clone(),
            key: key.map(str::to_string),
            enabled: true,
            headers,
        };
    let org = || {
        Some(vec![cloud::HeaderInput {
            name: "X-Org-Id".into(),
            value: HEADER_VALUE.into(),
        }])
    };

    // "Test Connection" before saving: nothing is stored, the key and header reach the mock.
    let probe = cloud::probe(input(org(), Some(KEY)), None).await.unwrap();
    assert_eq!(probe.state, cloud::State::Online);
    assert_eq!(probe.model_count, 1);
    assert_eq!(
        (
            probe.models[0].info.vision,
            probe.models[0].info.reasoning,
            probe.models[0].info.context_length
        ),
        (Some(true), Some(true), Some(200000))
    );
    let (headers, _) = mock.last("models");
    assert_eq!(headers["x-api-key"], KEY);
    assert_eq!(headers["x-org-id"], HEADER_VALUE);
    assert_eq!(headers["anthropic-version"], cloud::ANTHROPIC_VERSION);
    assert!(headers.get("authorization").is_none());
    assert!(cloud::list().is_empty(), "a probe stores nothing");
    // Without the header the endpoint answers 404: a clear keyed error, not a panic.
    let bad = cloud::probe(input(None, Some(KEY)), None).await.unwrap();
    assert_eq!(bad.state, cloud::State::Offline);
    assert_eq!(
        error_key(bad.error.as_deref().unwrap()).0,
        "cloud.not_found_upstream"
    );
    // A missing key on a provider that needs one never reaches the network.
    let mut anthropic = input(None, None);
    anthropic.kind = cloud::Kind::Anthropic;
    let missing = cloud::probe(anthropic, None).await.unwrap();
    assert_eq!(missing.state, cloud::State::KeyMissing);

    // Saved: the view carries header names and never a value or the key.
    let added = cloud::add(input(org(), Some(KEY))).await.unwrap();
    assert_eq!(added.status.state, cloud::State::Online);
    assert_eq!(added.provider.headers, ["x-org-id"]);
    let view = serde_json::to_string(&cloud::list()).unwrap();
    assert!(!view.contains(KEY) && !view.contains(HEADER_VALUE));
    let on_disk = std::fs::read_to_string(dir.path().join("config").join("cloud_providers.json"))
        .or_else(|_| {
            std::fs::read_to_string(harness_core::paths::config_dir().join("cloud_providers.json"))
        })
        .unwrap();
    assert!(!on_disk.contains(KEY) && !on_disk.contains(HEADER_VALUE));
    let id = added.provider.id.clone();
    // An edit that keeps headers (None) and key keeps them working.
    let kept = cloud::update(id.clone(), input(None, None)).await.unwrap();
    assert_eq!(kept.status.state, cloud::State::Online);
    assert_eq!(kept.provider.headers, ["x-org-id"]);
    // Moving to another host needs the key and headers again.
    let mut moved = input(None, None);
    moved.base_url = format!("http://localhost:{}/an/v1", address.port());
    assert!(cloud::update(id.clone(), moved).await.is_err());

    // Prices: the user's output price wins, the input price stays unknown (none listed).
    let mut prices = BTreeMap::new();
    prices.insert(
        "claude-mini".to_string(),
        cloud::ModelPrice {
            input: Some(2.0),
            output: Some(8.0),
        },
    );
    cloud::set_prices(&id, prices).await.unwrap();
    let model = cloud::model_info("cloud:mine/claude-mini").unwrap();
    assert_eq!(
        (
            model.info.input_price,
            model.info.output_price,
            model.info.price_custom
        ),
        (Some(2.0), Some(8.0), true)
    );
    assert_eq!(
        cloud::request_cost("cloud:mine/claude-mini", Some(1000), Some(500)),
        Some(0.006)
    );
    assert!(cloud::set_prices(
        &id,
        BTreeMap::from([(
            "claude-mini".to_string(),
            cloud::ModelPrice {
                input: Some(-1.0),
                output: None
            }
        )])
    )
    .await
    .is_err());

    // Through the gateway: Messages natively and chat translated, both with the extra header.
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
    let http = reqwest::Client::new();
    let host = status.endpoint.unwrap().host_url;
    let token = gateway.client_token().await.unwrap();
    let models: Value = http
        .get(format!("{host}/v1/models"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let listed = models["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == "cloud:mine/claude-mini")
        .expect("the custom endpoint's model is listed");
    assert_eq!(listed["harness"]["cloud"]["reasoning"], true);
    assert_eq!(listed["harness"]["cloud"]["price_custom"], true);
    let native: Value = http
        .post(format!("{host}/v1/messages"))
        .bearer_auth(&token)
        .json(&json!({"model": "cloud:mine/claude-mini", "max_tokens": 16,
            "messages": [{"role": "user", "content": "hi"}]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(native["content"][0]["text"], "hello");
    let (headers, seen) = mock.last("messages");
    assert_eq!(headers["x-api-key"], KEY);
    assert_eq!(headers["x-org-id"], HEADER_VALUE);
    assert_eq!(seen["model"], "claude-mini");
    let chat: Value = http
        .post(format!("{host}/v1/chat/completions"))
        .bearer_auth(&token)
        .json(&json!({"model": "cloud:mine/claude-mini", "max_tokens": 16,
            "messages": [{"role": "user", "content": "hi"}]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(chat["choices"][0]["message"]["content"], "hello");
    assert_eq!(mock.last("messages").0["x-org-id"], HEADER_VALUE);
    // An Anthropic-style endpoint has no embeddings or Responses API.
    let r = http
        .post(format!("{host}/v1/responses"))
        .bearer_auth(&token)
        .json(&json!({"model": "cloud:mine/claude-mini", "input": "x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);

    // Rate limits: the keyed error says when to retry.
    let limited = cloud::probe(
        cloud::ProviderInput {
            name: "slow".into(),
            kind: cloud::Kind::Custom,
            base_url: format!("http://{address}/limited/v1"),
            key: None,
            enabled: true,
            headers: None,
        },
        None,
    )
    .await
    .unwrap();
    let (key, params) = error_key(limited.error.as_deref().unwrap());
    assert_eq!(key, "cloud.rate_limited_retry");
    assert_eq!(params["retry_after"], 7);
    gateway.stop().await;
}
