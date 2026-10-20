//! BYOK cloud protocols through the gateway, against a local mock of the three provider
//! families: OpenAI (chat, Responses, embeddings), Anthropic (native Messages, chat
//! translated to Messages) and Gemini (its OpenAI-compatible endpoint).

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

const KEY: &str = "sk-test-protocol-secret-key";

#[derive(Clone, Default)]
struct Mock {
    status: Arc<AtomicU16>,
    seen: Arc<Mutex<Vec<(String, HeaderMap, Value)>>>,
}

impl Mock {
    fn last(&self, path: &str) -> (HeaderMap, Value) {
        let seen = self.seen.lock().unwrap();
        let (_, h, b) = seen.iter().rev().find(|(p, _, _)| p == path).unwrap();
        (h.clone(), b.clone())
    }

    /// The error answer of the current mode, if any.
    fn failure(&self, anthropic: bool) -> Option<Response> {
        match self.status.load(Ordering::SeqCst) {
            401 => Some(
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": {"message": format!("bad key {KEY}")}})),
                )
                    .into_response(),
            ),
            429 => {
                let body = if anthropic {
                    json!({"type": "error", "error": {"type": "rate_limit_error", "message": "slow down"}})
                } else {
                    json!({"error": {"message": "slow down", "type": "rate_limit"}})
                };
                Some(
                    (
                        StatusCode::TOO_MANY_REQUESTS,
                        [("retry-after", "7")],
                        Json(body),
                    )
                        .into_response(),
                )
            }
            _ => None,
        }
    }

    fn record(&self, path: &str, headers: HeaderMap, body: &Value) {
        self.seen
            .lock()
            .unwrap()
            .push((path.to_string(), headers, body.clone()));
    }
}

async fn openai_models() -> Response {
    Json(json!({"data": [{"id": "gpt-4o"}, {"id": "text-embedding-3-small"}]})).into_response()
}

async fn anthropic_models() -> Response {
    Json(json!({"data": [{"id": "claude-x", "display_name": "Claude X"}], "has_more": false}))
        .into_response()
}

async fn gemini_models() -> Response {
    Json(json!({"data": [{"id": "models/gemini-2.5-flash"}, {"id": "models/text-embedding-004"}]}))
        .into_response()
}

async fn chat(State(m): State<Mock>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    m.record("chat", headers, &body);
    if let Some(r) = m.failure(false) {
        return r;
    }
    if body["stream"] == true {
        return (
            [("content-type", "text/event-stream")],
            "data: {\"choices\":[{\"delta\":{\"content\":\"chat-hi\"}}]}\n\ndata: [DONE]\n\n",
        )
            .into_response();
    }
    Json(
        json!({"id": "c1", "choices": [{"message": {"role": "assistant", "content": null,
        "tool_calls": [{"id": "t1", "type": "function",
            "function": {"name": "get_weather", "arguments": "{}"}}]}}],
        "usage": {"prompt_tokens": 3, "completion_tokens": 2}}),
    )
    .into_response()
}

async fn responses(State(m): State<Mock>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    m.record("responses", headers, &body);
    if let Some(r) = m.failure(false) {
        return r;
    }
    if body["stream"] == true {
        return (
            [("content-type", "text/event-stream")],
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"resp-hi\"}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"usage\":{\"input_tokens\":4,\"output_tokens\":1}}}\n\n",
        )
            .into_response();
    }
    Json(json!({"id": "resp_1", "object": "response", "status": "completed",
        "output": [{"type": "function_call", "call_id": "c1", "name": "get_weather", "arguments": "{}"}],
        "usage": {"input_tokens": 4, "output_tokens": 1}}))
    .into_response()
}

async fn embeddings(
    State(m): State<Mock>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    m.record("embeddings", headers, &body);
    if let Some(r) = m.failure(false) {
        return r;
    }
    Json(
        json!({"object": "list", "data": [{"index": 0, "embedding": [0.25, -0.5]}],
        "usage": {"prompt_tokens": 2, "total_tokens": 2}}),
    )
    .into_response()
}

async fn messages(State(m): State<Mock>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    m.record("messages", headers, &body);
    if let Some(r) = m.failure(true) {
        return r;
    }
    if body["stream"] == true {
        return (
            [("content-type", "text/event-stream")],
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-x\",\"usage\":{\"input_tokens\":7}}}\n\n\
event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"claude-hi\"}}\n\n\
event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n\
event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        )
            .into_response();
    }
    Json(json!({"id": "msg_1", "type": "message", "role": "assistant", "model": "claude-x",
        "content": [{"type": "tool_use", "id": "tu1", "name": "get_weather", "input": {"city": "Paris"}}],
        "stop_reason": "tool_use", "usage": {"input_tokens": 5, "output_tokens": 2}}))
    .into_response()
}

async fn count_tokens(
    State(m): State<Mock>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    m.record("count_tokens", headers, &body);
    Json(json!({"input_tokens": 11})).into_response()
}

fn openai_like(m: &Mock) -> Router {
    Router::new()
        .route("/chat/completions", post(chat))
        .route("/responses", post(responses))
        .route("/embeddings", post(embeddings))
        .with_state(m.clone())
}

fn input(name: &str, kind: cloud::Kind, base: String) -> cloud::ProviderInput {
    cloud::ProviderInput {
        name: name.into(),
        kind,
        base_url: base,
        key: Some(KEY.into()),
        enabled: true,
    }
}

struct Gw {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl Gw {
    async fn post(&self, path: &str, body: Value, headers: &[(&str, &str)]) -> reqwest::Response {
        let mut req = self
            .http
            .post(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .json(&body);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        req.send().await.unwrap()
    }
}

#[tokio::test]
async fn cloud_models_speak_responses_messages_and_embeddings() {
    harness_core::secrets::use_memory_store();
    let dir = tempfile::tempdir().unwrap();
    harness_core::paths::init(dir.path().to_path_buf());
    let mock = Mock::default();
    let app = Router::new()
        .nest(
            "/oa/v1",
            openai_like(&mock).route("/models", get(openai_models)),
        )
        .nest(
            "/gm/v1beta/openai",
            openai_like(&mock).route("/models", get(gemini_models)),
        )
        .nest(
            "/an/v1",
            Router::new()
                .route("/models", get(anthropic_models))
                .route("/messages", post(messages))
                .route("/messages/count_tokens", post(count_tokens))
                .with_state(mock.clone()),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    for (name, kind, path) in [
        ("oa", cloud::Kind::Openai, "oa/v1"),
        ("an", cloud::Kind::Anthropic, "an/v1"),
        ("gm", cloud::Kind::Gemini, "gm/v1beta/openai"),
    ] {
        let added = cloud::add(input(name, kind, format!("http://{address}/{path}")))
            .await
            .unwrap();
        assert_eq!(added.status.state, cloud::State::Online, "{name}");
    }
    // Embedding models are listed apart from the chat models.
    assert!(cloud::models().iter().all(|m| !m.info.embedding));
    let mut embedders: Vec<_> = cloud::embedding_models()
        .into_iter()
        .map(|m| m.id)
        .collect();
    embedders.sort();
    assert_eq!(
        embedders,
        [
            "cloud:gm/text-embedding-004",
            "cloud:oa/text-embedding-3-small"
        ]
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
    let gw = Gw {
        http: reqwest::Client::new(),
        base: status.endpoint.unwrap().host_url,
        token: gateway.client_token().await.unwrap(),
    };
    let tool = json!({"type": "function", "name": "get_weather", "parameters": {"type": "object"}});

    // --- OpenAI Responses passthrough ---
    let body = json!({"model": "cloud:oa/gpt-4o", "input": "weather?", "tools": [tool],
        "previous_response_id": "resp_0"});
    let answer: Value = gw
        .post("/v1/responses", body.clone(), &[])
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(answer["output"][0]["name"], "get_weather");
    let (headers, seen) = mock.last("responses");
    assert_eq!(headers["authorization"], format!("Bearer {KEY}"));
    assert_eq!(seen["model"], "gpt-4o");
    assert_eq!(seen["previous_response_id"], "resp_0");
    assert_eq!(seen["tools"][0]["name"], "get_weather");
    let mut streamed = body.clone();
    streamed["stream"] = json!(true);
    let text = gw
        .post("/v1/responses", streamed, &[])
        .await
        .text()
        .await
        .unwrap();
    assert!(text.contains("resp-hi") && text.contains("response.completed"));
    // Providers without a Responses API refuse it.
    for model in ["cloud:gm/gemini-2.5-flash", "cloud:an/claude-x"] {
        let r = gw
            .post("/v1/responses", json!({"model": model, "input": "x"}), &[])
            .await;
        assert_eq!(r.status(), 400, "{model}");
        let e: Value = r.json().await.unwrap();
        assert_eq!(e["error"]["code"], "cloud_unsupported_endpoint");
    }

    // --- Embeddings: OpenAI and Gemini ---
    for (model, upstream) in [
        ("cloud:oa/text-embedding-3-small", "text-embedding-3-small"),
        ("cloud:gm/text-embedding-004", "text-embedding-004"),
    ] {
        let answer: Value = gw
            .post(
                "/v1/embeddings",
                json!({"model": model, "input": ["a"]}),
                &[],
            )
            .await
            .json()
            .await
            .unwrap();
        assert_eq!(
            answer["data"][0]["embedding"],
            json!([0.25, -0.5]),
            "{model}"
        );
        let (headers, seen) = mock.last("embeddings");
        assert_eq!(seen["model"], upstream);
        assert_eq!(headers["authorization"], format!("Bearer {KEY}"));
    }
    let r = gw
        .post(
            "/v1/embeddings",
            json!({"model": "cloud:an/claude-x", "input": "x"}),
            &[],
        )
        .await;
    assert_eq!(r.status(), 400);

    // --- Gemini chat through its OpenAI-compatible endpoint (stream and tools) ---
    let chat_body = json!({"model": "cloud:gm/gemini-2.5-flash",
        "messages": [{"role": "user", "content": "weather?"}],
        "tools": [{"type": "function", "function": {"name": "get_weather", "parameters": {"type": "object"}}}]});
    let answer: Value = gw
        .post("/v1/chat/completions", chat_body.clone(), &[])
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        answer["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
        "get_weather"
    );
    assert_eq!(mock.last("chat").1["model"], "gemini-2.5-flash");
    let mut streamed = chat_body.clone();
    streamed["stream"] = json!(true);
    let text = gw
        .post("/v1/chat/completions", streamed, &[])
        .await
        .text()
        .await
        .unwrap();
    assert!(text.contains("chat-hi") && text.contains("[DONE]"));

    // --- Anthropic: native Messages passthrough ---
    let native = json!({"model": "cloud:an/claude-x", "max_tokens": 64,
        "messages": [{"role": "user", "content": "weather?"}],
        "tools": [{"name": "get_weather", "input_schema": {"type": "object"}}]});
    let answer: Value = gw
        .post(
            "/v1/messages",
            native.clone(),
            &[("anthropic-version", "2023-06-01")],
        )
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(answer["content"][0]["type"], "tool_use");
    let (headers, seen) = mock.last("messages");
    assert_eq!(headers["x-api-key"], KEY);
    assert!(headers.get("authorization").is_none());
    assert_eq!(headers["anthropic-version"], "2023-06-01");
    assert_eq!(seen["model"], "claude-x");
    assert_eq!(seen["tools"][0]["name"], "get_weather");
    // Without a version header the gateway adds the default one.
    gw.post("/v1/messages", native.clone(), &[]).await;
    assert_eq!(
        mock.last("messages").0["anthropic-version"],
        cloud::ANTHROPIC_VERSION
    );
    let mut streamed = native.clone();
    streamed["stream"] = json!(true);
    let text = gw
        .post("/v1/messages", streamed, &[])
        .await
        .text()
        .await
        .unwrap();
    assert!(text.contains("claude-hi") && text.contains("message_stop"));
    let counted: Value = gw
        .post("/v1/messages/count_tokens", native.clone(), &[])
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(counted["input_tokens"], 11);
    assert_eq!(mock.last("count_tokens").0["x-api-key"], KEY);
    // The other providers do not take Messages.
    let r = gw
        .post(
            "/v1/messages",
            json!({"model": "cloud:oa/gpt-4o", "max_tokens": 8,
                "messages": [{"role": "user", "content": "x"}]}),
            &[],
        )
        .await;
    assert_eq!(r.status(), 400);
    let e: Value = r.json().await.unwrap();
    assert_eq!(e["type"], "error");

    // --- Anthropic: chat completions translated to Messages ---
    let chat_body = json!({"model": "cloud:an/claude-x", "max_tokens": 32,
        "messages": [{"role": "system", "content": "Be brief."},
            {"role": "user", "content": "weather?"}],
        "tools": [{"type": "function", "function": {"name": "get_weather", "parameters": {"type": "object"}}}]});
    let answer: Value = gw
        .post("/v1/chat/completions", chat_body.clone(), &[])
        .await
        .json()
        .await
        .unwrap();
    let call = &answer["choices"][0]["message"]["tool_calls"][0];
    assert_eq!(call["function"]["name"], "get_weather");
    assert_eq!(call["function"]["arguments"], "{\"city\":\"Paris\"}");
    assert_eq!(answer["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(answer["usage"]["total_tokens"], 7);
    let (headers, seen) = mock.last("messages");
    assert_eq!(headers["x-api-key"], KEY);
    assert_eq!(seen["model"], "claude-x");
    assert_eq!(seen["system"], "Be brief.");
    assert_eq!(seen["tools"][0]["input_schema"], json!({"type": "object"}));
    let mut streamed = chat_body.clone();
    streamed["stream"] = json!(true);
    let response = gw.post("/v1/chat/completions", streamed, &[]).await;
    assert!(response.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/event-stream"));
    let text = response.text().await.unwrap();
    assert!(text.contains("\"content\":\"claude-hi\""), "{text}");
    assert!(text.contains("\"finish_reason\":\"stop\""));
    assert!(text.contains("\"prompt_tokens\":7") && text.ends_with("data: [DONE]\n\n"));

    // --- 429 keeps Retry-After; 401 is mapped and never echoes the key ---
    mock.status.store(429, Ordering::SeqCst);
    for (path, body) in [
        (
            "/v1/responses",
            json!({"model": "cloud:oa/gpt-4o", "input": "x"}),
        ),
        (
            "/v1/embeddings",
            json!({"model": "cloud:oa/text-embedding-3-small", "input": "x"}),
        ),
        ("/v1/chat/completions", chat_body.clone()),
        ("/v1/messages", native.clone()),
    ] {
        let r = gw.post(path, body, &[]).await;
        assert_eq!(r.status(), 429, "{path}");
        assert_eq!(r.headers()["retry-after"], "7", "{path}");
    }
    // A translated chat error has the OpenAI shape.
    let e: Value = gw
        .post("/v1/chat/completions", chat_body.clone(), &[])
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(e["error"]["message"], "slow down");
    mock.status.store(401, Ordering::SeqCst);
    for (path, body, code) in [
        (
            "/v1/responses",
            json!({"model": "cloud:oa/gpt-4o", "input": "x"}),
            "cloud_provider_unauthorized",
        ),
        (
            "/v1/embeddings",
            json!({"model": "cloud:gm/text-embedding-004", "input": "x"}),
            "cloud_provider_unauthorized",
        ),
        (
            "/v1/chat/completions",
            chat_body.clone(),
            "cloud_provider_unauthorized",
        ),
        (
            "/v1/messages",
            native.clone(),
            "cloud_provider_unauthorized",
        ),
    ] {
        let r = gw.post(path, body, &[]).await;
        assert_eq!(r.status(), 502, "{path}");
        let text = r.text().await.unwrap();
        assert!(text.contains(code), "{path}: {text}");
        assert!(!text.contains(KEY), "{path}");
    }
    mock.status.store(0, Ordering::SeqCst);
    gateway.stop().await;
}
