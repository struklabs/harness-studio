//! Token-budget admission through the real gateway with a real llama-server (no mocks).
//!
//! A server with 2 slots and a unified 8192-token KV pool fails *both* of two concurrent
//! ~4.5k-token requests ("Context size has been exceeded"). Through the gateway the second
//! one waits until the first has finished and both answer.
//!
//! ```text
//! HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<chat gguf, e.g. Qwen3-4B-Instruct-2507 Q4_K_M>
//! cargo test -p harness-gateway --test admission_e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Temporary data root (never the user's), in-memory secrets, the gateway on 19900 and its
//! model on 19950. Everything started here is stopped here.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::events::CoreEvent;
use harness_core::gateway::record::RequestRecord;
use harness_core::gateway::GatewaySettings;
use harness_core::params::ParamValue;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

const ALIAS: &str = "adm-qwen";

fn env_path(name: &str) -> Option<PathBuf> {
    let p = std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_file());
    if p.is_none() {
        eprintln!("{name} not set, skipping");
    }
    p
}

fn data_root(bin: &Path) -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        harness_core::secrets::use_memory_store();
        let dir = tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("admission-e2e"))
            .tempdir()
            .unwrap()
            .keep();
        harness_core::paths::init(dir.clone());
        let mut s = harness_core::config::AppSettings::load();
        s.runtime_mode = harness_core::config::RuntimeMode::Custom;
        s.custom_binary.binary_path = Some(bin.to_path_buf());
        s.custom_binary.is_valid = true;
        s.server.port = 19950;
        s.control_api.port = 19999;
        s.save().unwrap();
        dir
    })
}

struct Gw {
    server: GatewayServer,
    registry: Arc<ServerRegistry>,
    base: String,
    http: reqwest::Client,
}

impl Gw {
    async fn start(bin: &Path, port: u16) -> Self {
        let registry = Arc::new(ServerRegistry::default());
        let ctx = GatewayContext {
            registry: registry.clone(),
            runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
            gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
            data_dir: data_root(bin).to_path_buf(),
            downloads: None,
        };
        let server = GatewayServer::new();
        let status = server
            .start(
                ctx,
                &GatewaySettings {
                    enabled: true,
                    port,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
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
}

#[derive(Debug)]
struct Answer {
    status: u16,
    body: Value,
    queue_ms: u64,
    took: Duration,
}

async fn chat(http: &reqwest::Client, base: &str, prompt: &str, max_tokens: u32) -> Answer {
    let t = Instant::now();
    let r = http
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({
            "model": ALIAS, "temperature": 0, "max_tokens": max_tokens,
            "messages": [{"role": "user", "content": prompt}],
        }))
        .send()
        .await
        .unwrap();
    let status = r.status().as_u16();
    let queue_ms = r
        .headers()
        .get("x-harness-queue-ms")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let text = r.text().await.unwrap();
    Answer {
        status,
        body: serde_json::from_str(&text).unwrap_or(Value::String(text)),
        queue_ms,
        took: t.elapsed(),
    }
}

/// Distinct pseudo-random text (no shared prefix, so the prompt cache cannot help).
fn gibberish(seed: u64, words: usize) -> String {
    const W: [&str; 16] = [
        "alpha", "river", "kernel", "window", "maple", "orbit", "quartz", "lantern", "pixel",
        "harbor", "violet", "engine", "meadow", "saddle", "cobalt", "ribbon",
    ];
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut out = String::new();
    for _ in 0..words {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        out.push_str(W[(x % 16) as usize]);
        out.push_str(&((x >> 8) % 1000).to_string());
        out.push(' ');
    }
    out
}

/// A prompt of about `target` tokens by the model's own tokenizer.
async fn prompt_of(http: &reqwest::Client, port: u16, seed: u64, target: usize) -> (String, usize) {
    let count = |text: String| async move {
        let v: Value = http
            .post(format!("http://127.0.0.1:{port}/tokenize"))
            .json(&json!({"content": text}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        v["tokens"].as_array().unwrap().len()
    };
    let probe = count(gibberish(seed, 500)).await.max(1);
    let words = 500 * target / probe;
    let text = gibberish(seed, words);
    let n = count(text.clone()).await;
    (
        format!("{text}\nSummarize the text above in one sentence."),
        n,
    )
}

fn records(rx: &mut tokio::sync::broadcast::Receiver<CoreEvent>) -> Vec<RequestRecord> {
    let mut out = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let CoreEvent::GatewayRequest { record } = ev {
            if record.endpoint == "/v1/chat/completions" {
                out.push(*record);
            }
        }
    }
    out
}

async fn slot(sched: &Arc<harness_core::gateway::scheduler::Scheduler>) -> Value {
    let all = sched.snapshot().await;
    let s = all.iter().find(|s| s.alias == ALIAS).unwrap();
    serde_json::to_value(s).unwrap()
}

#[tokio::test]
#[ignore]
async fn e2e_token_budget_queues_requests_that_would_overflow_the_pool() {
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL")) else {
        return;
    };
    data_root(&bin);
    let mut d = ModelDefinition::new(ALIAS, ModelRef::Local { path: model });
    d.load.insert("gpu_layers".into(), ParamValue::Int(99));
    d.load.insert("context_size".into(), ParamValue::Int(8192));
    d.load.insert("parallel_slots".into(), ParamValue::Int(2));
    harness_core::domain::store::Domain::global()
        .save_definition(d)
        .unwrap();
    let gw = Gw::start(&bin, 19900).await;
    let sched = harness_core::gateway::scheduler().unwrap();
    let mut events = harness_core::events::subscribe();

    // Warm up: JIT load, slots and pool are read from the running server.
    let warm = chat(&gw.http, &gw.base, "Hi", 4).await;
    assert_eq!(warm.status, 200, "{warm:?}");
    let s = slot(&sched).await;
    eprintln!("slot: {s}");
    assert_eq!(s["concurrency"], 2);
    assert_eq!(s["token_pool"], 8192, "unified pool = -c");
    assert_eq!(s["tokens_in_use"], 0);
    let port = s["port"].as_u64().unwrap() as u16;

    let (p1, n1) = prompt_of(&gw.http, port, 1, 4500).await;
    let (p2, n2) = prompt_of(&gw.http, port, 2, 4500).await;
    eprintln!("prompts: {n1} and {n2} tokens (pool 8192)");
    assert!(n1 + n2 > 8192 && n1 < 8192 - 100 && n2 < 8192 - 100);

    // Control: straight at llama-server, two such requests fail (the problem admission solves).
    let direct = |prompt: String| {
        let http = gw.http.clone();
        async move {
            http.post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
                .json(&json!({"model": "x", "temperature": 0, "max_tokens": 32,
                    "messages": [{"role": "user", "content": prompt}]}))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        }
    };
    let (a, b) = tokio::join!(direct(p1.clone()), direct(p2.clone()));
    eprintln!("direct to llama-server: {a} and {b}");
    assert!(
        a >= 500 || b >= 500,
        "the pool overflow should fail: {a} {b}"
    );

    // Through the gateway both answer; the second waited for the first.
    let _ = records(&mut events);
    let (p3, _) = prompt_of(&gw.http, port, 3, 4500).await;
    let (p4, _) = prompt_of(&gw.http, port, 4, 4500).await;
    let (a, b) = tokio::join!(
        chat(&gw.http, &gw.base, &p3, 32),
        chat(&gw.http, &gw.base, &p4, 32)
    );
    eprintln!(
        "gateway: {} after {:?} (queued {} ms), {} after {:?} (queued {} ms)",
        a.status, a.took, a.queue_ms, b.status, b.took, b.queue_ms
    );
    assert_eq!((a.status, b.status), (200, 200), "{a:?} {b:?}");
    for x in [&a, &b] {
        assert!(x.body["usage"]["prompt_tokens"].as_u64().unwrap() > 4000);
        assert!(!x.body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .is_empty());
    }
    let (first, second) = if a.queue_ms <= b.queue_ms {
        (&a, &b)
    } else {
        (&b, &a)
    };
    assert!(
        first.queue_ms < 1000,
        "first runs at once: {}",
        first.queue_ms
    );
    assert!(
        second.queue_ms >= 1000,
        "second waited for the pool: {} ms",
        second.queue_ms
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    let recs = records(&mut events);
    assert_eq!(recs.len(), 2, "{recs:?}");
    assert!(recs.iter().all(|r| r.slots == Some(2) && r.status == 200));
    assert_eq!(
        recs.iter()
            .filter(|r| r.waited.as_deref() == Some("tokens"))
            .count(),
        1,
        "{recs:?}"
    );
    let s = slot(&sched).await;
    assert_eq!(s["tokens_in_use"], 0, "budget returned: {s}");
    assert_eq!(s["queued"], 0);
    assert!(s["waited_tokens"].as_u64().unwrap() >= 1, "{s}");
    assert_eq!(
        s["waited"], 0,
        "a slot was free: this was the pool, not the slots"
    );

    // Small requests are not serialized: both slots work at once, no waiting.
    let (a, b) = tokio::join!(
        chat(&gw.http, &gw.base, "Name a colour.", 8),
        chat(&gw.http, &gw.base, "Name an animal.", 8)
    );
    assert_eq!((a.status, b.status), (200, 200));
    let recs = records(&mut events);
    assert!(recs.iter().all(|r| r.waited.is_none()), "{recs:?}");

    // A waiting request that is cancelled gives nothing back wrongly and blocks nobody.
    let (p5, _) = prompt_of(&gw.http, port, 5, 4500).await;
    let (p6, _) = prompt_of(&gw.http, port, 6, 4500).await;
    let first = {
        let (http, base) = (gw.http.clone(), gw.base.clone());
        tokio::spawn(async move { chat(&http, &base, &p5, 64).await })
    };
    tokio::time::sleep(Duration::from_millis(400)).await;
    let waiting = tokio::time::timeout(
        Duration::from_millis(600),
        chat(&gw.http, &gw.base, &p6, 32),
    )
    .await;
    assert!(
        waiting.is_err(),
        "the second request should still be queued"
    );
    let first = first.await.unwrap();
    assert_eq!(first.status, 200, "{first:?}");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let s = slot(&sched).await;
    assert_eq!(
        s["tokens_in_use"], 0,
        "cancelled waiter left no reservation: {s}"
    );
    assert_eq!(s["queued"], 0);
    assert_eq!(s["in_flight"], 0);
    let after = chat(&gw.http, &gw.base, "Say OK.", 4).await;
    assert_eq!(after.status, 200);
    assert!(after.queue_ms < 1000, "{}", after.queue_ms);

    gw.stop().await;
}
