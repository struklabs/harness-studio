//! `/v1/systemone` and `/v1/decisions` against the real in-process gateway: origin guard, token,
//! error shapes of each API. No llama-server is needed: every request here is decided before a
//! model would load (the generation side is `decisions_e2e.rs`, `--ignored`).
//!
//! Temporary data root (never the user's) and in-memory secrets.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use harness_core::gateway::GatewaySettings;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

const EVIL: &str = "https://evil.example";

fn data_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        harness_core::secrets::use_memory_store();
        let dir = tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("gw-decisions"))
            .tempdir()
            .unwrap()
            .keep();
        harness_core::paths::init(dir.clone());
        let mut s = harness_core::config::AppSettings::load();
        s.control_api.port = 18080;
        s.save().unwrap();
        dir
    })
}

async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    LOCK.lock().await
}

struct Gw {
    server: GatewayServer,
    registry: Arc<ServerRegistry>,
    base: String,
    http: reqwest::Client,
}

impl Gw {
    async fn start(require_token: bool) -> Self {
        let registry = Arc::new(ServerRegistry::default());
        let ctx = GatewayContext {
            registry: registry.clone(),
            runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
            gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
            data_dir: data_root().to_path_buf(),
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
            registry,
            http: reqwest::Client::new(),
        }
    }

    async fn post(&self, path: &str, body: Value, origin: Option<&str>) -> (u16, Value) {
        let mut req = self.http.post(format!("{}{}", self.base, path)).json(&body);
        if let Some(o) = origin {
            req = req.header("origin", o);
        }
        let r = req.send().await.unwrap();
        let status = r.status().as_u16();
        let text = r.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    fn no_workers(&self) {
        let running: Vec<String> = self
            .registry
            .all()
            .into_iter()
            .filter(|(_, pm)| pm.get_status() != harness_core::process::ServerStatus::Stopped)
            .map(|(id, _)| id)
            .collect();
        assert!(running.is_empty(), "a worker was started: {running:?}");
    }
}

fn systemone(model: &str) -> Value {
    json!({"model": model, "state": "I was charged twice.",
        "questions": {"angry": {"type": "noul", "instructions": "Is the customer angry?"}}})
}

fn decisions(model: &str) -> Value {
    json!({"model": model, "input": "I was charged twice.",
        "questions": [{"type": "predicate", "name": "angry", "instructions": "Is the customer angry?"}]})
}

#[tokio::test]
async fn foreign_origins_are_refused_before_anything_happens() {
    let _s = serial().await;
    let gw = Gw::start(false).await;
    for (path, body) in [
        ("/v1/systemone", systemone("nope")),
        ("/v1/decisions", decisions("nope")),
    ] {
        let (status, b) = gw.post(path, body, Some(EVIL)).await;
        assert_eq!(status, 403, "{path}: {b}");
        assert_eq!(b["error"]["code"], "forbidden_origin", "{path}: {b}");
    }
    gw.no_workers();
    gw.server.stop().await;
}

#[tokio::test]
async fn a_token_is_required_like_on_every_inference_route() {
    let _s = serial().await;
    let gw = Gw::start(true).await;
    for (path, body) in [
        ("/v1/systemone", systemone("nope")),
        ("/v1/decisions", decisions("nope")),
    ] {
        let (status, b) = gw.post(path, body, None).await;
        assert_eq!(status, 401, "{path}: {b}");
    }
    gw.no_workers();
    gw.server.stop().await;
}

#[tokio::test]
async fn errors_use_each_apis_own_shape() {
    let _s = serial().await;
    let gw = Gw::start(false).await;
    // Ollama: `{"error": "<message>"}`, 404 for a model that is not there.
    let (status, b) = gw.post("/v1/systemone", systemone("nope"), None).await;
    assert_eq!(status, 404, "{b}");
    assert!(b["error"].as_str().unwrap().contains("nope"), "{b}");
    // OpenAI: `{"error": {"message", "type", "param", "code"}}`.
    let (status, b) = gw.post("/v1/decisions", decisions("nope"), None).await;
    assert_eq!(status, 404, "{b}");
    assert_eq!(b["error"]["type"], "invalid_request_error", "{b}");
    assert_eq!(b["error"]["param"], "model", "{b}");
    // Invalid requests: 400 before the model is looked up.
    let mut bad = systemone("nope");
    bad["questions"]["angry"]["type"] = json!("bool");
    let (status, b) = gw.post("/v1/systemone", bad, None).await;
    assert_eq!(status, 400, "{b}");
    assert!(
        b["error"].as_str().unwrap().contains("choice, score, noul"),
        "{b}"
    );
    let mut bad = decisions("nope");
    bad["questions"][0]["type"] = json!("noul");
    let (status, b) = gw.post("/v1/decisions", bad, None).await;
    assert_eq!(status, 400, "{b}");
    assert_eq!(b["error"]["code"], "invalid_request", "{b}");
    assert_eq!(b["error"]["param"], "questions[0].type", "{b}");
    gw.no_workers();
    gw.server.stop().await;
}
