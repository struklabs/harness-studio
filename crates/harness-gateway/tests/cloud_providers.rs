//! BYOK cloud providers through the gateway, against a local OpenAI-compatible mock.

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
use std::sync::{
    atomic::{AtomicU16, Ordering},
    Arc, Mutex,
};

const KEY: &str = "sk-test-cloud-provider-secret";

#[derive(Clone, Default)]
struct Provider {
    status: Arc<AtomicU16>,
    seen: Arc<Mutex<Vec<(HeaderMap, Value)>>>,
}

async fn models(headers: HeaderMap) -> Response {
    // OpenRouter lists its models without a key.
    Json(json!({"data": [{
        "id": "org/chat-model",
        "name": "Org: Chat Model",
        "context_length": 32768,
        "pricing": {"prompt": "0.000001", "completion": "0.000002"},
        "supported_parameters": ["tools"],
        "architecture": {"input_modalities": ["text"]},
        "authorized": headers.contains_key("authorization"),
    }]}))
    .into_response()
}

async fn key_info(headers: HeaderMap) -> Response {
    assert_eq!(headers["authorization"], format!("Bearer {KEY}"));
    Json(json!({"data": {"limit": 10.0, "usage": 1.25, "limit_remaining": 8.75}})).into_response()
}

async fn chat(
    State(provider): State<Provider>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    provider.seen.lock().unwrap().push((headers, body.clone()));
    match provider.status.load(Ordering::SeqCst) {
        401 => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": {"message": format!("Incorrect API key provided: {KEY}")}})),
        )
            .into_response(),
        429 => (
            StatusCode::TOO_MANY_REQUESTS,
            [("retry-after", "7")],
            Json(json!({"error": {"message": "Rate limit reached", "type": "rate_limit"}})),
        )
            .into_response(),
        _ if body["stream"] == true => (
            [("content-type", "text/event-stream")],
            "data: {\"id\":\"c1\",\"model\":\"org/chat-model\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: [DONE]\n\n",
        )
            .into_response(),
        _ => Json(json!({
            "id": "c1", "model": "org/chat-model",
            "choices": [{"message": {"role": "assistant", "content": null,
                "tool_calls": [{"id": "t1", "type": "function",
                    "function": {"name": "get_weather", "arguments": "{\"city\":\"Paris\"}"}}]}}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 2}
        }))
        .into_response(),
    }
}

#[tokio::test]
async fn cloud_provider_models_route_through_the_gateway() {
    harness_core::secrets::use_memory_store();
    let dir = tempfile::tempdir().unwrap();
    harness_core::paths::init(dir.path().to_path_buf());
    let provider = Provider::default();
    let app = Router::new()
        .route("/api/v1/models", get(models))
        .route("/api/v1/key", get(key_info))
        .route("/api/v1/chat/completions", post(chat))
        .with_state(provider.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let base_url = format!("http://{address}/api/v1/");

    // Without a key the models are discovered but not offered.
    let added = cloud::add(cloud::ProviderInput {
        name: "router".into(),
        kind: cloud::Kind::Openrouter,
        base_url: base_url.clone(),
        key: None,
        enabled: true,
        headers: None,
    })
    .await
    .unwrap();
    assert_eq!(added.status.state, cloud::State::KeyMissing);
    assert_eq!(added.status.model_count, 1);
    assert!(cloud::models().is_empty());
    assert!(cloud::add(cloud::ProviderInput {
        name: "router".into(),
        kind: cloud::Kind::Openai,
        base_url: String::new(),
        key: Some(KEY.into()),
        enabled: true,
        headers: None,
    })
    .await
    .is_err());

    let id = added.provider.id.clone();
    let input = |key: Option<&str>| cloud::ProviderInput {
        name: "router".into(),
        kind: cloud::Kind::Openrouter,
        base_url: base_url.clone(),
        key: key.map(str::to_string),
        enabled: true,
        headers: None,
    };
    let updated = cloud::update(id.clone(), input(Some(KEY))).await.unwrap();
    assert_eq!(updated.status.state, cloud::State::Online);
    assert!(updated.provider.key_set);
    let quota = updated.status.quota.clone().unwrap();
    assert_eq!(quota.remaining, Some(8.75));
    let models = cloud::models();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "cloud:router/org/chat-model");
    assert_eq!(models[0].label, "OpenRouter");
    assert_eq!(models[0].info.context_length, Some(32768));
    assert_eq!(models[0].info.input_price, Some(1.0));
    assert_eq!(models[0].info.tools, Some(true));
    // The key never reaches files or views.
    let file =
        std::fs::read_to_string(harness_core::paths::config_dir().join("cloud_providers.json"))
            .unwrap();
    assert!(file.contains("router"));
    assert!(!file.contains(KEY));
    assert!(!serde_json::to_string(&cloud::list()).unwrap().contains(KEY));
    // A saved key does not follow a change of host.
    let moved = cloud::update(
        id.clone(),
        cloud::ProviderInput {
            base_url: "https://example.com/v1".into(),
            ..input(None)
        },
    )
    .await;
    assert_eq!(
        harness_core::error_key::parse(&moved.unwrap_err())
            .unwrap()
            .0,
        "cloud.key_required_for_new_address"
    );

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
    let session = gateway.client_token().await.unwrap();
    let listed: Value = http
        .get(format!("{base}/v1/models"))
        .bearer_auth(&session)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let entry = listed["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == "cloud:router/org/chat-model")
        .unwrap()
        .clone();
    assert_eq!(entry["owned_by"], "harness-cloud");
    assert_eq!(entry["harness"]["cloud"]["label"], "OpenRouter");
    assert_eq!(entry["harness"]["cloud"]["tools"], true);

    let chat_url = format!("{base}/v1/chat/completions");
    let request = json!({
        "model": "cloud:router/org/chat-model",
        "messages": [{"role": "user", "content": "weather?"}],
        "tools": [{"type": "function", "function": {"name": "get_weather", "parameters": {"type": "object"}}}],
    });
    let answer: Value = http
        .post(&chat_url)
        .bearer_auth(&session)
        .json(&request)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        answer["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
        "get_weather"
    );
    {
        let seen = provider.seen.lock().unwrap();
        let (headers, body) = seen.last().unwrap();
        assert_eq!(headers["authorization"], format!("Bearer {KEY}"));
        assert!(headers.get("x-harness-link-hop").is_none());
        assert!(headers.get("traceparent").is_none());
        assert_eq!(body["model"], "org/chat-model");
        assert_eq!(body["tools"][0]["function"]["name"], "get_weather");
    }

    let mut streamed = request.clone();
    streamed["stream"] = json!(true);
    let response = http
        .post(&chat_url)
        .bearer_auth(&session)
        .json(&streamed)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(response.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/event-stream"));
    let text = response.text().await.unwrap();
    assert!(text.contains("\"hi\"") && text.contains("[DONE]"));

    // OpenRouter has no Messages API: only chat, Responses and embeddings are routed.
    let response = http
        .post(format!("{base}/v1/messages"))
        .bearer_auth(&session)
        .json(&json!({"model": "cloud:router/org/chat-model", "max_tokens": 8, "messages": [{"role": "user", "content": "x"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    let error: Value = response.json().await.unwrap();
    assert_eq!(error["error"]["code"], "cloud_unsupported_endpoint");

    // Provider errors pass through, except a rejected key, whose text could echo it.
    provider.status.store(429, Ordering::SeqCst);
    let response = http
        .post(&chat_url)
        .bearer_auth(&session)
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 429);
    assert_eq!(response.headers()["retry-after"], "7");
    provider.status.store(401, Ordering::SeqCst);
    let response = http
        .post(&chat_url)
        .bearer_auth(&session)
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 502);
    let text = response.text().await.unwrap();
    assert!(text.contains("cloud_provider_unauthorized"));
    assert!(!text.contains(KEY));
    provider.status.store(200, Ordering::SeqCst);

    // The model test goes straight to the provider and reports the tool call.
    let tested = cloud::test_model(&id, "org/chat-model").await.unwrap();
    assert!(tested.ok && tested.tool_called, "{tested:?}");

    // Removing the key takes the models out of the gateway.
    let revoked = cloud::update(id.clone(), input(Some(""))).await.unwrap();
    assert!(!revoked.provider.key_set);
    assert_eq!(revoked.status.state, cloud::State::KeyMissing);
    assert!(harness_core::secrets::cloud_key_get(&id).unwrap().is_none());
    assert!(cloud::models().is_empty());
    let response = http
        .post(&chat_url)
        .bearer_auth(&session)
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 404);

    cloud::update(id.clone(), input(Some(KEY))).await.unwrap();
    assert!(cloud::remove(&id).await.unwrap().is_none());
    assert!(harness_core::secrets::cloud_key_get(&id).unwrap().is_none());
    assert!(cloud::list().is_empty());
    gateway.stop().await;
}
