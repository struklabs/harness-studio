//! Token policy and rate limits against the real gateway (in-process axum listener on a
//! loopback port, real token store, real model definitions, real analytics database). No
//! llama-server is needed: every request here is decided before a model would load.
//!
//! Temporary data root (never the user's) and in-memory secrets.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use harness_core::analytics::{self, RequestQuery};
use harness_core::control_api::{
    GatewayEndpoint, RateLimits, Scope, TokenPolicy, TokenStore, TokenUpdate,
};
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
            .prefix(&harness_core::paths::test_dir_prefix("gw-policy"))
            .tempdir()
            .unwrap()
            .keep();
        harness_core::paths::init(dir.clone());
        let mut s = harness_core::config::AppSettings::load();
        s.control_api.port = 18080;
        s.save().unwrap();
        // Two real definitions on (small) files on disk: resolvable, never loaded here.
        let models = dir.join("models");
        std::fs::create_dir_all(&models).unwrap();
        for alias in ["team-chat", "private-coder"] {
            let file = models.join(format!("{alias}.gguf"));
            std::fs::write(&file, b"GGUF").unwrap();
            harness_core::domain::store::Domain::global()
                .save_definition(ModelDefinition::new(alias, ModelRef::Local { path: file }))
                .unwrap();
        }
        dir
    })
}

/// One gateway at a time (the gateway status is process-wide).
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    LOCK.lock().await
}

fn tokens() -> TokenStore {
    TokenStore::at(&data_root().join("config"))
}

struct Gw {
    server: GatewayServer,
    base: String,
    http: reqwest::Client,
}

impl Gw {
    async fn start(require_token: bool) -> Self {
        let dir = data_root().to_path_buf();
        let ctx = GatewayContext {
            registry: Arc::new(ServerRegistry::default()),
            runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
            gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
            data_dir: dir,
            downloads: None,
        };
        let s = GatewaySettings {
            enabled: true,
            port: 0,
            require_token,
            ..Default::default()
        };
        let server = GatewayServer::new();
        let status = server.start_ephemeral(ctx, &s).await.unwrap();
        Gw {
            base: status.endpoint.unwrap().host_url,
            server,
            http: reqwest::Client::new(),
        }
    }

    async fn post(&self, path: &str, token: Option<&str>, body: Value) -> Resp {
        let mut req = self.http.post(format!("{}{}", self.base, path)).json(&body);
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        Resp::of(req.send().await.unwrap()).await
    }

    async fn get(&self, path: &str, token: Option<&str>) -> Resp {
        let mut req = self.http.get(format!("{}{}", self.base, path));
        if let Some(t) = token {
            req = req.header("x-api-key", t);
        }
        Resp::of(req.send().await.unwrap()).await
    }
}

struct Resp {
    status: u16,
    headers: reqwest::header::HeaderMap,
    body: Value,
}

impl Resp {
    async fn of(r: reqwest::Response) -> Self {
        let status = r.status().as_u16();
        let headers = r.headers().clone();
        let text = r.text().await.unwrap();
        Self {
            status,
            headers,
            body: serde_json::from_str(&text).unwrap_or(Value::String(text)),
        }
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
}

fn chat(model: &str) -> Value {
    json!({"model": model, "messages": [{"role": "user", "content": "hi"}]})
}

fn create(name: &str, scopes: &[Scope], policy: TokenPolicy) -> (String, String) {
    let c = tokens().create_with_policy(name, scopes, &policy).unwrap();
    (c.token, c.info.id)
}

#[tokio::test]
async fn endpoint_and_model_policy_is_enforced() {
    let _s = serial().await;
    data_root();
    let gw = Gw::start(true).await;
    let (tok, _) = create(
        "policy-chat-only",
        &[Scope::Inference],
        TokenPolicy {
            models: vec!["team-*".into()],
            endpoints: vec![GatewayEndpoint::Chat, GatewayEndpoint::Models],
            ..Default::default()
        },
    );

    // Endpoint groups: OpenAI, Anthropic and Ollama error shapes.
    let r = gw
        .post(
            "/v1/embeddings",
            Some(&tok),
            json!({"model": "team-chat", "input": "x"}),
        )
        .await;
    assert_eq!(r.status, 403, "{}", r.body);
    assert_eq!(r.body["error"]["code"], "endpoint_not_allowed");
    assert_eq!(r.body["error"]["type"], "permission_error");
    assert_eq!(r.header("x-harness-error"), Some("endpoint_not_allowed"));
    let r = gw
        .post(
            "/v1/messages",
            Some(&tok),
            json!({"model": "team-chat", "max_tokens": 8, "messages": []}),
        )
        .await;
    assert_eq!(r.status, 403);
    assert_eq!(r.body["type"], "error");
    assert_eq!(r.body["error"]["type"], "permission_error");
    let r = gw.post("/api/chat", Some(&tok), chat("team-chat")).await;
    assert_eq!(r.status, 403);
    assert!(
        r.body["error"].as_str().unwrap().contains("ollama"),
        "{}",
        r.body
    );
    let r = gw.get("/metrics", Some(&tok)).await;
    assert_eq!(r.status, 403);

    // Model patterns: the listing shows only allowed aliases, others are 403 before any load.
    let r = gw.get("/v1/models", Some(&tok)).await;
    assert_eq!(r.status, 200);
    let ids: Vec<&str> = r.body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["team-chat"]);
    let r = gw.get("/v1/models/private-coder", Some(&tok)).await;
    assert_eq!(r.status, 403);
    let r = gw
        .post("/v1/chat/completions", Some(&tok), chat("private-coder"))
        .await;
    assert_eq!(r.status, 403, "{}", r.body);
    assert_eq!(r.body["error"]["code"], "model_not_allowed");
    assert_eq!(r.body["error"]["param"], "model");
    // An unknown model: 404, and the list of available models is limited to the token's.
    let r = gw
        .post("/v1/chat/completions", Some(&tok), chat("nope"))
        .await;
    assert_eq!(r.status, 404);
    assert_eq!(
        r.body["error"]["harness"]["available_models"],
        json!(["team-chat"])
    );
    assert!(!r.body.to_string().contains("private-coder"));

    // An unrestricted token of the same kind sees everything (no regression).
    let (open, _) = create("policy-open", &[Scope::Inference], TokenPolicy::default());
    let r = gw.get("/v1/models", Some(&open)).await;
    assert_eq!(r.body["data"].as_array().unwrap().len(), 2);
    assert!(r.header("x-ratelimit-limit-requests").is_none());
    let r = gw.get("/metrics", Some(&open)).await;
    assert_eq!(r.status, 200);
    // Tokens without "inference" still get 403 insufficient_scope.
    let (reader, _) = create("policy-reader", &[Scope::Read], TokenPolicy::default());
    let r = gw.get("/v1/models", Some(&reader)).await;
    assert_eq!(r.status, 403);
    assert_eq!(r.body["error"]["code"], "insufficient_scope");

    gw.server.stop().await;
}

/// A valid request body of each typed pipeline route.
fn typed_routes(model: &str) -> Vec<(&'static str, Value)> {
    vec![
        (
            "/v1/classifications",
            json!({"model": model, "input": "great"}),
        ),
        ("/v1/entities", json!({"model": model, "input": "Ada"})),
        (
            "/v1/zero-shot",
            json!({"model": model, "input": "x", "candidate_labels": ["a", "b"]}),
        ),
        (
            "/v1/detections",
            json!({"model": model, "image": "data:image/png;base64,AQID"}),
        ),
        ("/v1/moderations", json!({"model": model, "input": "hello"})),
        (
            "/v1/images/upscale",
            json!({"model": model, "image": "data:image/png;base64,AQID"}),
        ),
    ]
}

#[tokio::test]
async fn typed_pipeline_routes_follow_the_pipelines_scope() {
    let _s = serial().await;
    data_root();
    let gw = Gw::start(true).await;
    let (chat_only, _) = create(
        "policy-typed-chat",
        &[Scope::Inference],
        TokenPolicy {
            endpoints: vec![GatewayEndpoint::Chat],
            ..Default::default()
        },
    );
    let (pipelines, _) = create(
        "policy-typed-pipelines",
        &[Scope::Inference],
        TokenPolicy {
            endpoints: vec![GatewayEndpoint::Pipelines],
            ..Default::default()
        },
    );
    for (path, body) in typed_routes("team-chat") {
        let r = gw.post(path, None, body.clone()).await;
        assert_eq!(r.status, 401, "{path}: {}", r.body);
        let r = gw.post(path, Some(&chat_only), body.clone()).await;
        assert_eq!(r.status, 403, "{path}: {}", r.body);
        assert_eq!(r.body["error"]["code"], "endpoint_not_allowed", "{path}");
        // Allowed: the request reaches model resolution (no such model: 404).
        let (_, nope) = typed_routes("nope")
            .into_iter()
            .find(|(p, _)| *p == path)
            .unwrap();
        let r = gw.post(path, Some(&pipelines), nope).await;
        assert_eq!(r.status, 404, "{path}: {}", r.body);
    }
    // A request that names no model is refused with the field, before any model work.
    let r = gw
        .post(
            "/v1/moderations",
            Some(&pipelines),
            json!({"input": "hello"}),
        )
        .await;
    assert_eq!(r.status, 400, "{}", r.body);
    assert_eq!(r.body["error"]["param"], "model");
    gw.server.stop().await;
}

#[tokio::test]
async fn rate_limits_answer_429_in_each_api_shape_and_reach_analytics() {
    let _s = serial().await;
    data_root();
    let gw = Gw::start(true).await;
    let name = "policy-rpm";
    let (tok, id) = create(
        name,
        &[Scope::Inference],
        TokenPolicy {
            limits: RateLimits {
                requests_per_minute: Some(2),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    // Two requests pass the limiter (and end as 404: no such model), the third is 429.
    for remaining in ["1", "0"] {
        let r = gw
            .post("/v1/chat/completions", Some(&tok), chat("nope"))
            .await;
        assert_eq!(r.status, 404, "{}", r.body);
        assert_eq!(r.header("x-ratelimit-limit-requests"), Some("2"));
        assert_eq!(r.header("x-ratelimit-remaining-requests"), Some(remaining));
    }
    let r = gw
        .post("/v1/chat/completions", Some(&tok), chat("nope"))
        .await;
    assert_eq!(r.status, 429, "{}", r.body);
    let retry: u64 = r.header("retry-after").unwrap().parse().unwrap();
    assert!(
        (1..=30).contains(&retry),
        "2/min refills one every 30 s: {retry}"
    );
    assert_eq!(r.header("x-harness-error"), Some("rate_limited"));
    assert_eq!(r.header("x-ratelimit-remaining-requests"), Some("0"));
    assert_eq!(r.body["error"]["code"], "rate_limited");
    assert_eq!(r.body["error"]["type"], "rate_limit_error");
    assert_eq!(r.body["error"]["harness"]["limit"], "requests_per_minute");
    assert_eq!(r.body["error"]["harness"]["value"], 2);
    assert!(r.body["error"]["message"].as_str().unwrap().contains(name));

    // Anthropic shape.
    let r = gw
        .post(
            "/v1/messages",
            Some(&tok),
            json!({"model": "nope", "max_tokens": 8, "messages": []}),
        )
        .await;
    assert_eq!(r.status, 429);
    assert_eq!(r.body["type"], "error");
    assert_eq!(r.body["error"]["type"], "rate_limit_error");
    assert!(r.header("retry-after").is_some());
    assert_eq!(
        r.header("anthropic-ratelimit-requests-remaining"),
        Some("0")
    );

    // Ollama shape.
    let r = gw.post("/api/chat", Some(&tok), chat("nope")).await;
    assert_eq!(r.status, 429);
    assert!(r.body["error"].as_str().unwrap().contains("Rate limit"));
    assert!(r.header("retry-after").is_some());

    // Listing endpoints are not rate limited (they cost nothing).
    assert_eq!(gw.get("/v1/models", Some(&tok)).await.status, 200);

    // A changed policy applies on the next request (the token file is read per request).
    tokens()
        .update(
            &id,
            &TokenUpdate {
                limits: Some(RateLimits::default()),
                ..Default::default()
            },
        )
        .unwrap();
    let r = gw
        .post("/v1/chat/completions", Some(&tok), chat("nope"))
        .await;
    assert_eq!(r.status, 404);
    assert!(r.header("x-ratelimit-limit-requests").is_none());

    // Every rejected request is in Analytics with its token and error code.
    assert!(analytics::flush(Duration::from_secs(10)));
    let page = analytics::requests(&RequestQuery {
        limit: Some(1000),
        ..Default::default()
    })
    .unwrap();
    let mine: Vec<_> = page
        .requests
        .iter()
        .filter(|r| r.token_name.as_deref() == Some(name))
        .collect();
    let limited: Vec<_> = mine
        .iter()
        .filter(|r| r.error_code.as_deref() == Some("rate_limited"))
        .collect();
    assert_eq!(limited.len(), 3, "{:#?}", mine);
    assert!(limited.iter().all(|r| r.status == 429));
    let endpoints: Vec<&str> = limited.iter().map(|r| r.endpoint.as_str()).collect();
    for e in ["/v1/chat/completions", "/v1/messages", "/api/chat"] {
        assert!(endpoints.contains(&e), "{endpoints:?}");
    }
    assert_eq!(
        mine.iter()
            .filter(|r| r.error_code.as_deref() == Some("model_not_found"))
            .count(),
        3
    );
    gw.server.stop().await;
}

#[tokio::test]
async fn limit_headers_report_the_token_and_concurrency_budgets() {
    // The token budget is charged with the usage of finished requests; a request without
    // usage (404 here) charges nothing. Charging and blocking with real completions is in
    // `e2e_limits` (real llama-server).
    let _s = serial().await;
    data_root();
    let gw = Gw::start(true).await;
    let (tok, _) = create(
        "policy-tpm",
        &[Scope::Inference],
        TokenPolicy {
            limits: RateLimits {
                tokens_per_minute: Some(1000),
                max_concurrent: Some(4),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    for _ in 0..5 {
        let r = gw
            .post("/v1/chat/completions", Some(&tok), chat("nope"))
            .await;
        assert_eq!(r.status, 404);
        assert_eq!(r.header("x-ratelimit-limit-tokens"), Some("1000"));
        assert_eq!(r.header("x-ratelimit-remaining-tokens"), Some("1000"));
        assert_eq!(r.header("x-harness-ratelimit-concurrency"), Some("4"));
        assert_eq!(r.header("x-harness-ratelimit-in-flight"), Some("1"));
    }
    gw.server.stop().await;
}

#[tokio::test]
async fn policy_binds_whoever_presents_the_token_on_an_open_loopback_gateway() {
    let _s = serial().await;
    data_root();
    let gw = Gw::start(false).await;
    let (tok, _) = create(
        "policy-open-gw",
        &[Scope::Inference],
        TokenPolicy {
            models: vec!["team-chat".into()],
            limits: RateLimits {
                requests_per_minute: Some(1),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let r = gw
        .post("/v1/chat/completions", Some(&tok), chat("private-coder"))
        .await;
    assert_eq!(r.status, 403, "presented token: its policy applies");
    let r = gw
        .post("/v1/chat/completions", Some(&tok), chat("private-coder"))
        .await;
    assert_eq!(r.status, 429);
    // Without a token the open loopback gateway stays open (documented; turn on
    // "Require a token" to make the policy binding).
    let r = gw.get("/v1/models", None).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.body["data"].as_array().unwrap().len(), 2);
    gw.server.stop().await;
}

#[tokio::test]
async fn a_used_budget_survives_a_gateway_restart() {
    let _s = serial().await;
    data_root();
    let gw = Gw::start(true).await;
    let (tok, id) = create(
        "policy-restart",
        &[Scope::Inference],
        TokenPolicy {
            limits: RateLimits {
                requests_per_minute: Some(2),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    for _ in 0..2 {
        let r = gw
            .post("/v1/chat/completions", Some(&tok), chat("nope"))
            .await;
        assert_eq!(r.status, 404, "{}", r.body);
    }
    // The background writer stores the counter (every couple of seconds, nothing on the
    // request path).
    let db = harness_core::analytics::shared().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let stored = db
            .rate_counters_load(harness_core::analytics::now_ms())
            .unwrap();
        if stored
            .iter()
            .any(|r| r.token_id == id && r.kind == "rpm" && r.level < 1.0)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "counter was not written: {stored:?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    gw.server.stop().await;
    drop(gw);

    // A new gateway (a restart): the budget is still used up.
    let gw = Gw::start(true).await;
    let r = gw
        .post("/v1/chat/completions", Some(&tok), chat("nope"))
        .await;
    assert_eq!(r.status, 429, "{}", r.body);
    assert_eq!(r.body["error"]["harness"]["limit"], "requests_per_minute");
    gw.server.stop().await;
}

#[tokio::test]
async fn document_routes_require_inference_and_pipeline_policy() {
    let _s = serial().await;
    let gw = Gw::start(true).await;
    let (chat, _) = create(
        "document-chat-only",
        &[Scope::Inference],
        TokenPolicy {
            endpoints: vec![GatewayEndpoint::Chat],
            ..Default::default()
        },
    );
    let (pipeline, _) = create(
        "document-pipelines",
        &[Scope::Inference],
        TokenPolicy {
            endpoints: vec![GatewayEndpoint::Pipelines],
            ..Default::default()
        },
    );
    for path in ["/v1/ocr", "/v1/document/qa"] {
        let r = gw.post(path, None, json!({})).await;
        assert_eq!(r.status, 401, "{}", r.body);
        let r = gw.post(path, Some(&chat), json!({})).await;
        assert_eq!(r.status, 403, "{}", r.body);
        assert_eq!(r.body["error"]["code"], "endpoint_not_allowed");
        let r = gw
            .post(
                path,
                Some(&pipeline),
                json!({"model":"", "document":"x", "question":"q"}),
            )
            .await;
        assert_eq!(r.status, 400, "{}", r.body);
        assert_eq!(r.body["error"]["param"], "model");
    }
    gw.server.stop().await;
}
