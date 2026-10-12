//! Chat and speech models on one small GPU through the real gateway (no mocks): the shared
//! scheduler (`footprint`, `make_room`, `lru_victim`, `whisper_vram_need`) decides what leaves
//! the card when a `/v1/audio/transcriptions` request needs room that a loaded chat model holds
//! (and the other way round).
//!
//! ```text
//! HS_LLAMA_SERVER=<llama-server (CUDA build)> HS_TEST_MODEL=<chat gguf, e.g. Qwen2.5-3B Q4_K_M>
//! HS_TEST_MODEL_SMALL=<second chat gguf, e.g. Qwen2.5-0.5B Q4_K_M>
//! HS_WHISPER_SERVER=<whisper-server.exe (CUDA build)> HS_WHISPER_MODEL=<ggml-medium.en.bin>
//! HS_WHISPER_WAV=<speech.wav> HS_WHISPER_TEXT=<words the wav says> [HS_CHAT_CTX=16384]
//! cargo test -p harness-gateway --test audio_scheduler_e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Temporary data root (never the user's), in-memory secrets. Every step prints what the
//! scheduler evicted, the VRAM the OS reports per process (`harness_core::gpu`) and the whisper
//! estimate next to the measured value, so the numbers can be read from the output. Everything
//! started here is stopped here.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::params::ParamValue;
use harness_core::process::ServerStatus;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

const MIB: u64 = 1024 * 1024;

fn env_path(name: &str) -> Option<PathBuf> {
    let p = std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_file());
    if p.is_none() {
        eprintln!("{name} not set, skipping");
    }
    p
}

struct Env {
    llama: PathBuf,
    chat: PathBuf,
    small: Option<PathBuf>,
    whisper_server: PathBuf,
    whisper: PathBuf,
    wav: PathBuf,
    text: String,
    ctx: i64,
}

fn env() -> Option<Env> {
    Some(Env {
        llama: env_path("HS_LLAMA_SERVER")?,
        chat: env_path("HS_TEST_MODEL")?,
        small: env_path("HS_TEST_MODEL_SMALL"),
        whisper_server: env_path("HS_WHISPER_SERVER")?,
        whisper: env_path("HS_WHISPER_MODEL")?,
        wav: env_path("HS_WHISPER_WAV")?,
        text: std::env::var("HS_WHISPER_TEXT")
            .unwrap_or_else(|_| "ask not".into())
            .to_lowercase(),
        ctx: std::env::var("HS_CHAT_CTX")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(16384),
    })
}

fn data_root(e: &Env) -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        harness_core::secrets::use_memory_store();
        let dir = tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("audio-sched-e2e"))
            .tempdir()
            .unwrap()
            .keep();
        harness_core::paths::init(dir.clone());
        let mut s = harness_core::config::AppSettings::load();
        s.runtime_mode = harness_core::config::RuntimeMode::Custom;
        s.custom_binary.binary_path = Some(e.llama.clone());
        s.custom_binary.is_valid = true;
        s.server.port = 19960;
        s.control_api.port = 19969;
        s.save().unwrap();
        harness_core::runtimes::RuntimeRegistry::global()
            .add_local(&harness_core::runtimes::AddLocal {
                path: e.whisper_server.parent().unwrap().to_path_buf(),
                name: "whisper e2e".into(),
                id: None,
                copy: false,
                family: "whisper.cpp".into(),
            })
            .unwrap();
        dir
    })
}

/// `(used, total)` MiB of the primary GPU, all processes (the OS counter Harness's telemetry
/// reads: PDH on Windows, NVML / sysfs elsewhere), so the test also runs without `nvidia-smi`
/// (AMD). `(0, 0)` when the OS reports nothing.
fn gpu_used() -> (u64, u64) {
    harness_core::gpu::list_adapters()
        .first()
        .and_then(harness_core::gpu::vram_usage)
        .map(|(used, total)| (used / MIB, total / MIB))
        .unwrap_or((0, 0))
}

fn form(parts: &[(&str, Option<&str>, Vec<u8>)]) -> (String, Vec<u8>) {
    let boundary = "----hs-sched-boundary";
    let mut body = Vec::new();
    for (name, file, data) in parts {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"").as_bytes(),
        );
        if let Some(f) = file {
            body.extend_from_slice(
                format!("; filename=\"{f}\"\r\nContent-Type: application/octet-stream").as_bytes(),
            );
        }
        body.extend_from_slice(b"\r\n\r\n");
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

struct Gw {
    server: GatewayServer,
    registry: Arc<ServerRegistry>,
    base: String,
    http: reqwest::Client,
    audio: Vec<u8>,
}

#[derive(Debug)]
struct Step {
    status: u16,
    evicted: Vec<String>,
    load_ms: Option<u64>,
    body: String,
    took_ms: u128,
}

impl Gw {
    async fn start(e: &Env, port: u16) -> Self {
        let dir = data_root(e).to_path_buf();
        let registry = Arc::new(ServerRegistry::default());
        let ctx = GatewayContext {
            registry: registry.clone(),
            runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
            gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
            data_dir: dir,
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
            audio: std::fs::read(&e.wav).unwrap(),
        }
    }

    async fn stop(self) {
        self.server.stop().await;
        self.registry.stop_all().await;
    }

    fn running(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .registry
            .all()
            .into_iter()
            .filter(|(_, pm)| pm.get_status() == ServerStatus::Running)
            .map(|(id, pm)| {
                pm.launch_config()
                    .and_then(|c| c.alias)
                    .unwrap_or_else(|| id.clone())
            })
            .collect();
        v.sort();
        v
    }

    /// Per running server: alias, OS-reported dedicated VRAM and the whisper estimate.
    fn vram_lines(&self, tag: &str) {
        let (used, total) = gpu_used();
        eprintln!(
            "[{tag}] GPU used {used} / {total} MiB; running: {:?}",
            self.running()
        );
        for (_, pm) in self.registry.all() {
            if pm.get_status() != ServerStatus::Running {
                continue;
            }
            let (Some(pid), Some(cfg)) = (pm.pid(), pm.launch_config()) else {
                continue;
            };
            let measured = harness_core::gpu::process_vram_bytes(pid).map(|b| b / MIB);
            let alias = cfg.alias.clone().unwrap_or_default();
            if harness_core::engine::server_for(&cfg).family() != "llama.cpp" {
                let est = harness_core::engine::whisper_vram_need(&cfg) / MIB;
                let pct = measured
                    .filter(|m| *m > 0)
                    .map(|m| (est as f64 - m as f64) * 100.0 / m as f64);
                eprintln!(
                    "[{tag}]   {alias} (whisper, pid {pid}): measured {measured:?} MiB, whisper_vram_need {est} MiB, error {pct:?} %"
                );
            } else {
                eprintln!("[{tag}]   {alias} (llama, pid {pid}): measured {measured:?} MiB");
            }
        }
    }

    async fn transcribe(&self, alias: &str) -> Step {
        let (ct, body) = form(&[
            ("model", None, alias.as_bytes().to_vec()),
            ("language", None, b"en".to_vec()),
            ("file", Some("speech.wav"), self.audio.clone()),
        ]);
        let t = Instant::now();
        let r = self
            .http
            .post(format!("{}/v1/audio/transcriptions", self.base))
            .header("content-type", ct)
            .body(body)
            .send()
            .await
            .unwrap();
        step(r, t).await
    }

    async fn chat(&self, alias: &str, prompt: &str) -> Step {
        let t = Instant::now();
        let r = self
            .http
            .post(format!("{}/v1/chat/completions", self.base))
            .json(&json!({"model": alias, "temperature": 0, "max_tokens": 24,
                "messages": [{"role": "user", "content": prompt}]}))
            .send()
            .await
            .unwrap();
        step(r, t).await
    }
}

async fn step(r: reqwest::Response, t: Instant) -> Step {
    let status = r.status().as_u16();
    let evicted = r
        .headers()
        .get("x-harness-evicted")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(',').map(str::to_string).collect())
        .unwrap_or_default();
    let load_ms = r
        .headers()
        .get("x-harness-load-ms")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());
    let body = r.text().await.unwrap();
    Step {
        status,
        evicted,
        load_ms,
        body,
        took_ms: t.elapsed().as_millis(),
    }
}

fn define(e: &Env, pinned_chat: bool) {
    let store = harness_core::domain::store::Domain::global();
    // Both tests share one data root per process: replace the other test's definitions.
    for d in store.definitions().unwrap() {
        if d.alias.starts_with("sch-") {
            store.delete_definition(&d.id).unwrap();
        }
    }
    let mut chat = ModelDefinition::new(
        "sch-chat",
        ModelRef::Local {
            path: e.chat.clone(),
        },
    );
    chat.load.insert("gpu_layers".into(), ParamValue::Int(99));
    chat.load
        .insert("context_size".into(), ParamValue::Int(e.ctx));
    chat.pinned = pinned_chat;
    store.save_definition(chat).unwrap();
    if let Some(small) = &e.small {
        let mut d = ModelDefinition::new(
            "sch-small",
            ModelRef::Local {
                path: small.clone(),
            },
        );
        d.load.insert("gpu_layers".into(), ParamValue::Int(99));
        d.load.insert("context_size".into(), ParamValue::Int(4096));
        store.save_definition(d).unwrap();
    }
    let w = ModelDefinition::new(
        "sch-whisper",
        ModelRef::Local {
            path: e.whisper.clone(),
        },
    );
    store.save_definition(w).unwrap();
}

fn show(name: &str, s: &Step) {
    eprintln!(
        "STEP {name}: {} in {} ms, load {:?} ms, evicted {:?}, body {}",
        s.status,
        s.took_ms,
        s.load_ms,
        s.evicted,
        s.body
            .chars()
            .take(140)
            .collect::<String>()
            .replace('\n', " ")
    );
}

/// Nothing pinned: the least recently used idle model leaves first, in both directions, and no
/// request ends in an out-of-memory crash.
#[tokio::test]
#[ignore]
async fn e2e_whisper_and_chat_share_the_card_lru() {
    let Some(e) = env() else { return };
    data_root(&e);
    define(&e, false);
    let gw = Gw::start(&e, 19910).await;
    eprintln!(
        "gateway settings: {:?}",
        harness_core::gateway::scheduler().unwrap().settings()
    );
    gw.vram_lines("start");

    // 1. Chat model alone (oldest in the LRU order).
    let s = gw.chat("sch-chat", "Say hi.").await;
    show("chat 3B", &s);
    assert_eq!(s.status, 200, "{s:?}");
    gw.vram_lines("after chat");

    // 2. Optional second chat model (newer than the first).
    if e.small.is_some() {
        let s = gw.chat("sch-small", "Say hi.").await;
        show("chat small", &s);
        assert_eq!(s.status, 200, "{s:?}");
        gw.vram_lines("after small");
    }

    // 3. Speech: needs room. The least recently used model must be the first to go.
    let s = gw.transcribe("sch-whisper").await;
    show("whisper (1st)", &s);
    assert!(s.status == 200 || s.status == 503, "{s:?}");
    if s.status == 200 {
        let v: Value = serde_json::from_str(&s.body).unwrap();
        assert!(
            v["text"].as_str().unwrap().to_lowercase().contains(&e.text),
            "{s:?}"
        );
        if let Some(first) = s.evicted.first() {
            assert_eq!(first, "sch-chat", "LRU order: {:?}", s.evicted);
        }
    }
    gw.vram_lines("after whisper");

    // 4. Back to chat: the idle whisper server (or whatever is oldest) makes room.
    let s = gw.chat("sch-chat", "Say hi again.").await;
    show("chat 3B (2nd)", &s);
    assert!(s.status == 200 || s.status == 503, "{s:?}");
    gw.vram_lines("after chat again");

    // 5. And speech again.
    let s = gw.transcribe("sch-whisper").await;
    show("whisper (2nd)", &s);
    assert!(s.status == 200 || s.status == 503, "{s:?}");
    gw.vram_lines("after whisper again");

    gw.stop().await;
}

/// The chat model is pinned: speech may evict the other idle model, but never the pinned one;
/// when that is not enough the request is refused with `503 insufficient_memory`.
#[tokio::test]
#[ignore]
async fn e2e_pinned_chat_model_survives_a_speech_request() {
    let Some(e) = env() else { return };
    data_root(&e);
    define(&e, true);
    let gw = Gw::start(&e, 19920).await;

    let s = gw.chat("sch-chat", "Say hi.").await;
    show("chat 3B (pinned)", &s);
    assert_eq!(s.status, 200, "{s:?}");
    gw.vram_lines("after chat");

    let s = gw.transcribe("sch-whisper").await;
    show("whisper with a pinned chat model", &s);
    assert!(s.status == 200 || s.status == 503, "{s:?}");
    assert!(
        !s.evicted.iter().any(|a| a == "sch-chat"),
        "the pinned model was evicted: {s:?}"
    );
    if s.status == 503 {
        assert!(s.body.contains("insufficient_memory"), "{s:?}");
    }
    assert!(
        gw.running().iter().any(|a| a == "sch-chat"),
        "the pinned chat model is still loaded: {:?}",
        gw.running()
    );
    gw.vram_lines("after whisper");

    gw.stop().await;
}
