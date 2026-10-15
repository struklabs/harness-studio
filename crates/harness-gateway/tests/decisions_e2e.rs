//! Decision endpoints end to end with a real llama-server and real models (no mocks):
//!
//! ```text
//! HS_LLAMA_SERVER=<llama-server> HS_TEST_DECISION_MODEL=<native decision gguf>
//! HS_TEST_MODEL_SMALL=<small chat gguf>
//! cargo test -p harness-gateway --test decisions_e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Checked with llama.cpp b11574 (CUDA), ggml-org/vllm-d3-edge-GGUF Q8_0 and Qwen3-0.6B Q8_0.
//! Temporary data root (never the user's), in-memory secrets. Every test stops what it started.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::params::ParamValue;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

fn env_path(name: &str) -> Option<PathBuf> {
    let p = std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_file());
    if p.is_none() {
        eprintln!("{name} not set, skipping");
    }
    p
}

async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    LOCK.lock().await
}

fn data_root(bin: &Path) -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        harness_core::secrets::use_memory_store();
        let dir = tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("gw-decisions-e2e"))
            .tempdir()
            .unwrap()
            .keep();
        harness_core::paths::init(dir.clone());
        let mut s = harness_core::config::AppSettings::load();
        s.runtime_mode = harness_core::config::RuntimeMode::Custom;
        s.custom_binary.binary_path = Some(bin.to_path_buf());
        s.custom_binary.is_valid = true;
        s.server.port = 18960;
        s.control_api.port = 18959;
        s.save().unwrap();
        dir
    })
}

fn define(alias: &str, model: &Path) -> ModelDefinition {
    let mut d = ModelDefinition::new(alias, ModelRef::Local { path: model.into() });
    d.load.insert("gpu_layers".into(), ParamValue::Int(99));
    d.load.insert("context_size".into(), ParamValue::Int(4096));
    harness_core::domain::store::Domain::global()
        .save_definition(d)
        .unwrap()
}

struct Gw {
    server: GatewayServer,
    registry: Arc<ServerRegistry>,
    base: String,
    http: reqwest::Client,
}

impl Gw {
    async fn start(bin: &Path, emulation: bool) -> Self {
        let registry = Arc::new(ServerRegistry::default());
        let ctx = GatewayContext {
            registry: registry.clone(),
            runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
            gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
            data_dir: data_root(bin).to_path_buf(),
            downloads: None,
        };
        let s = GatewaySettings {
            enabled: true,
            port: 0,
            decision_emulation: emulation,
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

    async fn post(&self, path: &str, body: Value) -> (u16, reqwest::header::HeaderMap, Value) {
        let r = self
            .http
            .post(format!("{}{}", self.base, path))
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = r.status().as_u16();
        let headers = r.headers().clone();
        let text = r.text().await.unwrap();
        (
            status,
            headers,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    async fn stop(&self) {
        self.server.stop().await;
        self.registry.stop_all().await;
    }
}

impl Drop for Gw {
    fn drop(&mut self) {
        let registry = self.registry.clone();
        let _ = std::thread::spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                rt.block_on(registry.stop_all());
            }
        })
        .join();
    }
}

fn systemone(model: &str) -> Value {
    json!({
        "model": model,
        "state": "Customer message: I was charged twice for my order last week and nobody has replied.",
        "questions": {
            "route": {"type": "choice", "instructions": "Which team should handle this?",
                "criteria": {"billing": null, "shipping": null, "technical": null}},
            "angry": {"type": "noul", "instructions": "Is the customer angry?"},
            "urgency": {"type": "score", "instructions": "How urgent is this?",
                "criteria": ["can wait", "this week", "today", "right now"]}
        }
    })
}

fn decisions(model: &str) -> Value {
    json!({
        "model": model,
        "input": "Customer message: I was charged twice for my order last week and nobody has replied.",
        "questions": [
            {"type": "predicate", "name": "angry", "instructions": "Is the customer angry?"},
            {"type": "choice", "name": "route", "instructions": "Which team should handle this?",
                "choices": [{"value": "billing"}, {"value": "shipping"}, {"value": "technical"}]},
            {"type": "score", "instructions": "How urgent is this?",
                "levels": [{"label": "can wait"}, {"label": "this week"}, {"label": "today"}, {"label": "right now"}]}
        ]
    })
}

fn sums_to_one(probabilities: impl Iterator<Item = f64>) {
    let sum: f64 = probabilities.sum();
    assert!((sum - 1.0).abs() < 1e-6, "probabilities sum to {sum}");
}

fn check_systemone(b: &Value) {
    eprintln!("{b}");
    let a = &b["answers"];
    assert_eq!(a["route"]["type"], "choice", "{b}");
    sums_to_one(
        a["route"]["probabilities"]
            .as_object()
            .unwrap()
            .values()
            .map(|v| v.as_f64().unwrap()),
    );
    let noul = a["angry"]["noul"].as_f64().unwrap();
    assert!((0.0..=1.0).contains(&noul), "{b}");
    let score = a["urgency"]["score"].as_f64().unwrap();
    assert!((0.0..=3.0).contains(&score), "{b}");
    assert!(b["usage"]["input_tokens"].as_u64().unwrap() > 0, "{b}");
}

fn check_decisions(b: &Value) {
    eprintln!("{b}");
    let a = b["answers"].as_array().unwrap();
    assert_eq!(a.len(), 3, "{b}");
    assert_eq!(a[0]["type"], "predicate");
    assert_eq!(a[1]["name"], "route");
    sums_to_one(
        a[1]["probabilities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["probability"].as_f64().unwrap()),
    );
    assert_eq!(a[2]["name"], Value::Null);
    assert_eq!(a[2]["probabilities"][3]["label"], "right now");
    for k in ["id", "object", "created_at"] {
        assert!(b.get(k).is_none(), "undocumented field {k}: {b}");
    }
}

/// A native decision model: cold JIT load on the first request, answered by llama-server's
/// own `/v1/systemone` through both APIs.
#[tokio::test]
#[ignore]
async fn e2e_native_decision_model_jit_both_apis() {
    let _s = serial().await;
    let (Some(bin), Some(model)) = (
        env_path("HS_LLAMA_SERVER"),
        env_path("HS_TEST_DECISION_MODEL"),
    ) else {
        return;
    };
    assert!(
        harness_core::models::is_decision_model(&model),
        "not a decision GGUF"
    );
    let gw = Gw::start(&bin, true).await;
    define("decider", &model);
    let (status, h, b) = gw.post("/v1/systemone", systemone("decider")).await;
    assert_eq!(status, 200, "{b}");
    assert_eq!(h["x-harness-decision-mode"], "native");
    assert!(
        h.get("x-harness-load-ms").is_some(),
        "the first request loads the model"
    );
    assert_eq!(b["model"], "decider");
    check_systemone(&b);
    assert_eq!(b["answers"]["route"]["choice"], "billing", "{b}");
    let (status, h, b) = gw.post("/v1/decisions", decisions("decider")).await;
    assert_eq!(status, 200, "{b}");
    assert_eq!(h["x-harness-decision-mode"], "native");
    assert!(h.get("x-harness-load-ms").is_none(), "already loaded");
    check_decisions(&b);
    assert_eq!(b["answers"][1]["choice"], "billing", "{b}");
    gw.stop().await;
}

/// A chat model with emulation on (the default): grammar-constrained answers with
/// probabilities from logprobs.
#[tokio::test]
#[ignore]
async fn e2e_chat_model_is_emulated() {
    let _s = serial().await;
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL_SMALL"))
    else {
        return;
    };
    let gw = Gw::start(&bin, true).await;
    define("chatty", &model);
    let (status, h, b) = gw.post("/v1/systemone", systemone("chatty")).await;
    assert_eq!(status, 200, "{b}");
    assert_eq!(h["x-harness-decision-mode"], "emulated");
    check_systemone(&b);
    // One generated token per question.
    assert_eq!(b["usage"]["output_tokens"], 3, "{b}");
    let (status, h, b) = gw.post("/v1/decisions", decisions("chatty")).await;
    assert_eq!(status, 200, "{b}");
    assert_eq!(h["x-harness-decision-mode"], "emulated");
    check_decisions(&b);
    gw.stop().await;
}

/// Emulation off: llama-server's 501 for a chat model is the 400 Ollama and OpenAI clients get.
#[tokio::test]
#[ignore]
async fn e2e_emulation_off_maps_llama_servers_501() {
    let _s = serial().await;
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL_SMALL"))
    else {
        return;
    };
    let gw = Gw::start(&bin, false).await;
    define("chatty-off", &model);
    let (status, _, b) = gw.post("/v1/systemone", systemone("chatty-off")).await;
    assert_eq!(status, 400, "{b}");
    assert!(
        b["error"]
            .as_str()
            .unwrap()
            .contains("not a decision model"),
        "{b}"
    );
    let (status, _, b) = gw.post("/v1/decisions", decisions("chatty-off")).await;
    assert_eq!(status, 400, "{b}");
    assert_eq!(b["error"]["code"], "model_not_decision", "{b}");
    assert_eq!(b["error"]["type"], "invalid_request_error", "{b}");
    assert_eq!(b["error"]["param"], "model", "{b}");
    assert!(b["error"]["harness"]["error_key"]
        .as_str()
        .unwrap()
        .starts_with("gateway.decision_model_required"));
    gw.stop().await;
}
