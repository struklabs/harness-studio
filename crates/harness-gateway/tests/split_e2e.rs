//! Playground split view (P2) through the real gateway with a real llama-server (no mocks):
//! one prompt, two sides, streamed at the same time when they fit, else one after the other.
//!
//! ```text
//! HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<chat gguf, e.g. Qwen3-4B-Instruct-2507 Q4_K_M>
//! cargo test -p harness-gateway --test split_e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Temporary data root (never the user's), in-memory secrets, gateways on 19800 / 19820 and
//! their models on the ports right after them. Every test stops what it started.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use harness_core::domain::presets::{builtin_presets, InferencePreset};
use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::events::CoreEvent;
use harness_core::gateway::record::RequestRecord;
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
            .prefix(&harness_core::paths::test_dir_prefix("split-e2e"))
            .tempdir()
            .unwrap()
            .keep();
        harness_core::paths::init(dir.clone());
        let mut s = harness_core::config::AppSettings::load();
        s.runtime_mode = harness_core::config::RuntimeMode::Custom;
        s.custom_binary.binary_path = Some(bin.to_path_buf());
        s.custom_binary.is_valid = true;
        s.server.port = 19850;
        s.control_api.port = 19899;
        s.save().unwrap();
        dir
    })
}

fn define(alias: &str, model: &Path, ctx: i64, slots: i64) -> ModelDefinition {
    let mut d = ModelDefinition::new(alias, ModelRef::Local { path: model.into() });
    d.load.insert("gpu_layers".into(), ParamValue::Int(99));
    d.load.insert("context_size".into(), ParamValue::Int(ctx));
    d.load
        .insert("parallel_slots".into(), ParamValue::Int(slots));
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
    async fn start(bin: &Path, port: u16, configure: impl FnOnce(&mut GatewaySettings)) -> Self {
        let dir = data_root(bin).to_path_buf();
        let registry = Arc::new(ServerRegistry::default());
        let ctx = GatewayContext {
            registry: registry.clone(),
            runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
            gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
            data_dir: dir,
            downloads: None,
        };
        let mut s = GatewaySettings {
            enabled: true,
            port,
            ..Default::default()
        };
        configure(&mut s);
        let server = GatewayServer::new();
        let status = server.start(ctx, &s).await.unwrap();
        Gw {
            base: status.endpoint.unwrap().host_url,
            server,
            registry,
            http: reqwest::Client::new(),
        }
    }

    async fn stop(self) {
        self.server.stop().await;
        self.registry.stop_all().await;
    }

    fn running(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .registry
            .all()
            .into_iter()
            .filter(|(_, pm)| pm.get_status() == harness_core::process::ServerStatus::Running)
            .map(|(id, _)| id)
            .collect();
        out.sort();
        out
    }
}

fn preset(key: &str) -> InferencePreset {
    builtin_presets()
        .into_iter()
        .find(|p| p.id == format!("builtin:{key}"))
        .unwrap()
}

/// The body the Playground sends for one side: that side's history, model and preset.
fn side_body(model: &str, p: &InferencePreset, prompt: &str, max_tokens: u32) -> Value {
    let mut body = json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "stream": true,
        "stream_options": {"include_usage": true},
    });
    for (k, v) in p.request_params() {
        body[k] = v;
    }
    body["max_tokens"] = json!(max_tokens);
    body
}

/// One side's stream as the split view reads it.
#[derive(Debug, Default)]
struct SideRun {
    status: u16,
    code: Option<String>,
    evicted: Option<String>,
    content: String,
    /// From the common start: first content token, end of stream.
    first: Option<Duration>,
    end: Duration,
    predicted_n: Option<u64>,
}

async fn run_side(gw: &Gw, body: Value, t0: Instant) -> SideRun {
    let resp = gw
        .http
        .post(format!("{}/v1/chat/completions", gw.base))
        .header("x-harness-client", "Harness Playground")
        .json(&body)
        .send()
        .await
        .unwrap();
    let mut run = SideRun {
        status: resp.status().as_u16(),
        evicted: resp
            .headers()
            .get("x-harness-evicted")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string),
        ..Default::default()
    };
    if run.status != 200 {
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        run.code = v["error"]["code"].as_str().map(str::to_string);
        run.end = t0.elapsed();
        return run;
    }
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    while let Some(Ok(chunk)) = stream.next().await {
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(end) = buf.find("\n\n") {
            let block: String = buf.drain(..end + 2).collect();
            for line in block.lines() {
                let Some(d) = line.strip_prefix("data:").map(str::trim) else {
                    continue;
                };
                let Ok(v) = serde_json::from_str::<Value>(d) else {
                    continue;
                };
                if let Some(c) = v["choices"][0]["delta"]["content"].as_str() {
                    if !c.is_empty() && run.first.is_none() {
                        run.first = Some(t0.elapsed());
                    }
                    run.content.push_str(c);
                }
                if let Some(n) = v["timings"]["predicted_n"].as_u64() {
                    run.predicted_n = Some(n);
                }
            }
        }
    }
    run.end = t0.elapsed();
    run
}

fn records(rx: &mut tokio::sync::broadcast::Receiver<CoreEvent>) -> Vec<RequestRecord> {
    let mut out = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let CoreEvent::GatewayRequest { record } = ev {
            out.push(*record);
        }
    }
    out
}

const PROMPT: &str = "List ten animals, one per line, each with a short fact.";

/// Two presets on one model with 2 slots: both sides stream at the same time (their token
/// streams overlap), neither waits in the queue, and analytics records both as Playground
/// requests.
#[tokio::test]
#[ignore]
async fn e2e_split_two_presets_stream_concurrently() {
    let _serial = serial().await;
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL")) else {
        return;
    };
    data_root(&bin);
    define("split-qwen", &model, 4096, 2);
    let gw = Gw::start(&bin, 19800, |_| {}).await;
    let sched = harness_core::gateway::scheduler().unwrap();

    // One model: no limit applies; its slots are unknown until it is loaded.
    let before = sched.co_residency(&["split-qwen".into()]).await.unwrap();
    assert!(before.concurrent);
    assert_eq!(before.slots, None);

    // Warm up (JIT load) so the comparison measures generation, not loading.
    let warm = run_side(
        &gw,
        side_body("split-qwen", &preset("default"), "Hi", 4),
        Instant::now(),
    )
    .await;
    assert_eq!(warm.status, 200, "{warm:?}");
    let loaded = sched.co_residency(&["split-qwen".into()]).await.unwrap();
    assert!(loaded.concurrent);
    assert_eq!(loaded.slots, Some(2));

    let mut events = harness_core::events::subscribe();
    let a_body = side_body("split-qwen", &preset("deterministic"), PROMPT, 160);
    let b_body = side_body("split-qwen", &preset("creative"), PROMPT, 160);
    assert_ne!(a_body, b_body);
    let t0 = Instant::now();
    let (a, b) = tokio::join!(run_side(&gw, a_body, t0), run_side(&gw, b_body, t0));
    eprintln!(
        "A: first {:?} end {:?} tokens {:?}\nB: first {:?} end {:?} tokens {:?}",
        a.first, a.end, a.predicted_n, b.first, b.end, b.predicted_n
    );
    assert_eq!((a.status, b.status), (200, 200), "{a:?} {b:?}");
    assert!(!a.content.trim().is_empty() && !b.content.trim().is_empty());
    let (fa, fb) = (a.first.unwrap(), b.first.unwrap());
    // Concurrent: each side's first token arrives before the other side has finished.
    assert!(fa < b.end && fb < a.end, "streams did not overlap");
    // Neither side waited for the other: first tokens are close together, far from a
    // whole generation apart.
    let gap = fa.abs_diff(fb);
    let shorter = a.end.min(b.end);
    assert!(
        gap < shorter / 2,
        "first tokens {gap:?} apart (shorter run {shorter:?})"
    );

    tokio::time::sleep(Duration::from_millis(300)).await;
    let recs: Vec<RequestRecord> = records(&mut events)
        .into_iter()
        .filter(|r| r.endpoint == "/v1/chat/completions")
        .collect();
    assert_eq!(recs.len(), 2, "{recs:?}");
    for r in &recs {
        assert_eq!(r.status, 200);
        assert_eq!(r.alias.as_deref(), Some("split-qwen"));
        assert_eq!(r.client.label.as_deref(), Some("Harness Playground"));
        assert!(r.queue_wait_ms < 1000, "queued {} ms", r.queue_wait_ms);
        assert!(r.stream);
    }
    gw.stop().await;
}

/// Two models with `max_loaded_models = 1`: the pre-check says they cannot run together; sent
/// at once anyway (what the split view does when the check is unavailable), one side gets
/// `503 max_loaded_models` while the other streams, and running it after the other one
/// finishes works (the first model is evicted). Sequential mode from the start works too.
#[tokio::test]
#[ignore]
async fn e2e_split_two_models_fall_back_to_sequential() {
    let _serial = serial().await;
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL")) else {
        return;
    };
    data_root(&bin);
    define("split-a", &model, 2048, 1);
    define("split-b", &model, 2048, 1);
    let gw = Gw::start(&bin, 19820, |s| s.max_loaded_models = 1).await;
    let sched = harness_core::gateway::scheduler().unwrap();

    let plan = sched
        .co_residency(&["split-a".into(), "split-b".into()])
        .await
        .unwrap();
    eprintln!("co-residency: {plan:?}");
    assert!(!plan.concurrent);
    assert_eq!(plan.reason.as_deref(), Some("max_loaded_models"));
    assert_eq!(plan.limit, Some(1));
    assert_eq!(plan.to_load, ["split-a", "split-b"]);

    // At once: one side loads and streams, the other is refused for capacity.
    let p = preset("deterministic");
    let t0 = Instant::now();
    let (a, b) = tokio::join!(
        run_side(&gw, side_body("split-a", &p, PROMPT, 96), t0),
        run_side(&gw, side_body("split-b", &p, PROMPT, 96), t0)
    );
    eprintln!(
        "at once: A {} {:?} / B {} {:?}",
        a.status, a.code, b.status, b.code
    );
    let (won, lost, lost_alias, won_alias) = if a.status == 200 {
        (&a, &b, "split-b", "split-a")
    } else {
        (&b, &a, "split-a", "split-b")
    };
    assert_eq!(won.status, 200, "{won:?}");
    assert!(!won.content.trim().is_empty());
    assert_eq!(lost.status, 503, "{lost:?}");
    assert_eq!(lost.code.as_deref(), Some("max_loaded_models"));
    // The refusal came while the other side was still busy (not after it finished).
    assert!(
        lost.end < won.end,
        "refused at {:?}, other ended {:?}",
        lost.end,
        won.end
    );

    // Sequential fallback: the refused side runs now; the idle winner is evicted for it.
    let retry = run_side(&gw, side_body(lost_alias, &p, PROMPT, 96), Instant::now()).await;
    assert_eq!(retry.status, 200, "{retry:?}");
    assert!(!retry.content.trim().is_empty());
    assert_eq!(retry.evicted.as_deref(), Some(won_alias));
    assert_eq!(gw.running(), [format!("gw-{lost_alias}")]);

    // Sequential mode from the start: A, then B.
    let t0 = Instant::now();
    let a = run_side(&gw, side_body("split-a", &p, PROMPT, 48), t0).await;
    let b = run_side(&gw, side_body("split-b", &p, PROMPT, 48), t0).await;
    assert_eq!((a.status, b.status), (200, 200), "{a:?} {b:?}");
    assert!(b.first.unwrap() > a.end, "B started after A finished");
    assert_eq!(b.evicted.as_deref(), Some("split-a"));
    eprintln!(
        "sequential: A ended {:?}, B first token {:?}",
        a.end, b.first
    );
    gw.stop().await;
}

/// Memory side of the pre-check: two definitions whose (estimated) footprint together exceeds
/// the free GPU memory are reported as `insufficient_memory` without loading anything.
#[tokio::test]
#[ignore]
async fn e2e_split_memory_precheck_without_loading() {
    let _serial = serial().await;
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL")) else {
        return;
    };
    data_root(&bin);
    // 256k context each with an f16 KV cache: far beyond one consumer GPU.
    define("split-big-a", &model, 262_144, 1);
    define("split-big-b", &model, 262_144, 1);
    let gw = Gw::start(&bin, 19840, |_| {}).await;
    let sched = harness_core::gateway::scheduler().unwrap();
    let plan = sched
        .co_residency(&["split-big-a".into(), "split-big-b".into()])
        .await
        .unwrap();
    eprintln!("co-residency: {plan:?}");
    if let Some(need) = plan.need_bytes {
        assert!(!plan.concurrent);
        assert_eq!(plan.reason.as_deref(), Some("insufficient_memory"));
        assert!(need > plan.available_bytes.unwrap());
    } else {
        eprintln!("no GPU memory information on this machine; nothing to check");
    }
    assert!(gw.running().is_empty(), "the check loads nothing");
    gw.stop().await;
}
