//! Gateway end to end with a real llama-server and real models (no mocks):
//!
//! ```text
//! HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<chat gguf> HS_TEST_EMBED_MODEL=<embedding gguf>
//! HS_TEST_RERANK_MODEL=<reranker gguf> HS_TEST_MODEL_B=<big gguf> HS_TEST_MODEL_C=<big gguf>
//! HS_TEST_MODEL_SMALL=<small chat gguf>
//! cargo test -p harness-gateway --test e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Temporary data root (never the user's), in-memory secrets, gateway on 18890+, models on
//! the ports after it. Every test stops what it started.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
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

/// One test at a time: they share the data root and the GPU.
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    LOCK.lock().await
}

fn data_root(bin: &Path) -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        harness_core::secrets::use_memory_store();
        let dir = tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("gw-e2e"))
            .tempdir()
            .unwrap()
            .keep();
        harness_core::paths::init(dir.clone());
        let mut s = harness_core::config::AppSettings::load();
        s.runtime_mode = harness_core::config::RuntimeMode::Custom;
        s.custom_binary.binary_path = Some(bin.to_path_buf());
        s.custom_binary.is_valid = true;
        s.server.port = 18800;
        s.control_api.port = 18899;
        s.save().unwrap();
        dir
    })
}

fn define(alias: &str, model: &Path, ctx: i64, extra: &[(&str, ParamValue)]) -> ModelDefinition {
    let mut d = ModelDefinition::new(alias, ModelRef::Local { path: model.into() });
    d.load.insert("gpu_layers".into(), ParamValue::Int(99));
    d.load.insert("context_size".into(), ParamValue::Int(ctx));
    for (k, v) in extra {
        d.load.insert(k.to_string(), v.clone());
    }
    harness_core::domain::store::Domain::global()
        .save_definition(d)
        .unwrap()
}

fn save(def: ModelDefinition) -> ModelDefinition {
    harness_core::domain::store::Domain::global()
        .save_definition(def)
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

    async fn post(&self, path: &str, body: Value) -> reqwest::Response {
        self.http
            .post(format!("{}{}", self.base, path))
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    async fn stop(&self) {
        self.server.stop().await;
        self.registry.stop_all().await;
    }

    fn running(&self) -> Vec<String> {
        self.registry
            .all()
            .into_iter()
            .filter(|(_, pm)| pm.get_status() == harness_core::process::ServerStatus::Running)
            .map(|(id, _)| id)
            .collect()
    }
}

/// A test that panics before `stop()` must not leave its llama-server on the card: the next
/// test of the same (sequential) run would see a nearly full GPU and fail for the wrong reason.
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

async fn json_of(resp: reqwest::Response) -> (u16, reqwest::header::HeaderMap, Value) {
    let status = resp.status().as_u16();
    let headers = resp.headers().clone();
    let text = resp.text().await.unwrap();
    (
        status,
        headers,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

/// SSE `data:` payloads with the arrival time of each chunk.
async fn sse(resp: reqwest::Response) -> (Vec<Value>, Vec<Duration>) {
    let t = Instant::now();
    let mut stream = resp.bytes_stream();
    let (mut buf, mut out, mut times) = (String::new(), Vec::new(), Vec::new());
    while let Some(Ok(chunk)) = stream.next().await {
        times.push(t.elapsed());
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(end) = buf.find("\n\n") {
            let block: String = buf.drain(..end + 2).collect();
            for line in block.lines() {
                if let Some(d) = line.strip_prefix("data:") {
                    let d = d.trim();
                    if d != "[DONE]" {
                        if let Ok(v) = serde_json::from_str(d) {
                            out.push(v);
                        }
                    }
                }
            }
        }
    }
    (out, times)
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

/// JIT load by alias, streaming passthrough, embeddings/rerank on their own servers, idle
/// TTL, per-request records.
#[tokio::test]
#[ignore]
async fn e2e_jit_stream_ttl_embed_rerank() {
    let _serial = serial().await;
    let (Some(bin), Some(model), Some(embed), Some(rerank)) = (
        env_path("HS_LLAMA_SERVER"),
        env_path("HS_TEST_MODEL"),
        env_path("HS_TEST_EMBED_MODEL"),
        env_path("HS_TEST_RERANK_MODEL"),
    ) else {
        return;
    };
    data_root(&bin);
    let mut chat = define("gw-e2e-chat", &model, 4096, &[]);
    chat.ttl_seconds = Some(12);
    let chat = save(chat);
    define("gw-e2e-embed", &embed, 2048, &[]);
    define("gw-e2e-rerank", &rerank, 2048, &[]);
    let mut events = harness_core::events::subscribe();
    let gw = Gw::start(&bin, 18890, |_| {}).await;

    let (status, _, models) = json_of(
        gw.http
            .get(format!("{}/v1/models", gw.base))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, 200);
    let ids: Vec<&str> = models["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    for a in ["gw-e2e-chat", "gw-e2e-embed", "gw-e2e-rerank"] {
        assert!(ids.contains(&a), "{ids:?}");
    }

    // JIT: nothing runs, the request loads the definition.
    assert!(gw.running().is_empty());
    let t = Instant::now();
    let (status, headers, body) = json_of(
        gw.post(
            "/v1/chat/completions",
            json!({"model": "GW-E2E-CHAT", "max_tokens": 16,
            "messages": [{"role": "user", "content": "Say hello in one word."}]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let load_ms: u64 = headers["x-harness-load-ms"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    eprintln!(
        "JIT load {} ms, first answer after {:?}: {}",
        load_ms,
        t.elapsed(),
        body["choices"][0]["message"]["content"]
    );
    assert_eq!(headers["x-harness-server"], "gw-gw-e2e-chat");
    assert_eq!(gw.running(), ["gw-gw-e2e-chat"]);

    // Second request: no load.
    let (status, headers, _) = json_of(
        gw.post(
            "/v1/completions",
            json!({"model": "gw-e2e-chat", "prompt": "1, 2, 3,", "max_tokens": 8}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200);
    assert!(headers.get("x-harness-load-ms").is_none());

    // Streaming passthrough: chunks arrive while the model generates.
    let resp = gw
        .post(
            "/v1/chat/completions",
            json!({"model": "gw-e2e-chat", "stream": true, "max_tokens": 200,
            "messages": [{"role": "user", "content": "Count from 1 to 60 separated by spaces."}]}),
        )
        .await;
    assert_eq!(
        resp.headers()["content-type"]
            .to_str()
            .unwrap()
            .split(';')
            .next(),
        Some("text/event-stream")
    );
    let (events_sse, times) = sse(resp).await;
    let first = times[0];
    let last = *times.last().unwrap();
    eprintln!(
        "stream: {} events in {} chunks, first {:?}, last {:?}",
        events_sse.len(),
        times.len(),
        first,
        last
    );
    assert!(times.len() > 10, "streamed in many chunks");
    assert!(first * 4 < last, "first chunk long before the end");

    // Anthropic count_tokens goes to the same server.
    let (status, _, body) = json_of(
        gw.post(
            "/v1/messages/count_tokens",
            json!({"model": "gw-e2e-chat",
            "messages": [{"role": "user", "content": "hello there"}]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(body["input_tokens"].as_u64().unwrap() > 0);

    // Embeddings and rerank: their own servers, started in their modes.
    let (status, _, body) = json_of(
        gw.post(
            "/v1/embeddings",
            json!({"model": "gw-e2e-embed", "input": ["a panda", "the city of Paris"]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"][0]["embedding"].as_array().unwrap().len(), 768);
    let (status, _, body) = json_of(
        gw.post("/v1/rerank", json!({"model": "gw-e2e-rerank", "query": "What is a panda?",
            "documents": ["Paris is the capital of France.", "The giant panda is a bear native to China."]})).await,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let best = body["results"]
        .as_array()
        .unwrap()
        .iter()
        .max_by(|a, b| {
            a["relevance_score"]
                .as_f64()
                .partial_cmp(&b["relevance_score"].as_f64())
                .unwrap()
        })
        .unwrap()["index"]
        .as_u64();
    assert_eq!(best, Some(1));
    let mut running = gw.running();
    running.sort();
    let states: Vec<String> = gw
        .registry
        .all()
        .iter()
        .map(|(id, pm)| format!("{id}={:?}", pm.get_status()))
        .collect();
    assert_eq!(
        running,
        ["gw-gw-e2e-chat", "gw-gw-e2e-embed", "gw-gw-e2e-rerank"],
        "{states:?}"
    );

    // Idle TTL (12 s on the chat definition only).
    let t = Instant::now();
    while gw.running().contains(&"gw-gw-e2e-chat".to_string())
        && t.elapsed() < Duration::from_secs(30)
    {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    eprintln!("TTL unload after {:?} idle", t.elapsed());
    assert!(
        !gw.running().contains(&"gw-gw-e2e-chat".to_string()),
        "TTL unloaded the chat model"
    );
    assert_eq!(gw.running().len(), 2, "no TTL on the others");
    let sched = harness_core::gateway::scheduler().unwrap();
    let t = Instant::now();
    loop {
        let snap = sched.snapshot().await;
        let slot = snap.iter().find(|s| s.definition_id == chat.id).unwrap();
        if slot.state == harness_core::gateway::scheduler::SlotState::Unloaded {
            break;
        }
        assert!(t.elapsed() < Duration::from_secs(10), "{slot:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Records: metadata of every request.
    let recs = records(&mut events);
    let first_rec = recs
        .iter()
        .find(|r| r.endpoint == "/v1/chat/completions" && r.load_ms.is_some())
        .unwrap();
    assert_eq!(first_rec.alias.as_deref(), Some("gw-e2e-chat"));
    assert!(first_rec.usage.completion_tokens.unwrap() > 0);
    assert!(first_rec.usage.prompt_tokens.unwrap() > 0);
    let streamed = recs.iter().find(|r| r.stream).unwrap();
    assert!(
        streamed.ttft_ms.is_some() && streamed.usage.completion_tokens.unwrap_or(0) > 10,
        "{streamed:?}"
    );
    assert!(recs
        .iter()
        .any(|r| r.endpoint == "/v1/rerank" && r.status == 200));
    eprintln!(
        "{} records, e.g. {}",
        recs.len(),
        serde_json::to_string(first_rec).unwrap()
    );
    gw.stop().await;
}

/// `max_loaded_models = 1`: loading the second evicts the first (LRU); a pinned model is
/// never evicted, so the next load gets `503 max_loaded_models`.
#[tokio::test]
#[ignore]
async fn e2e_max_loaded_evicts_lru_and_respects_pinned() {
    let _serial = serial().await;
    let (Some(bin), Some(model), Some(embed)) = (
        env_path("HS_LLAMA_SERVER"),
        env_path("HS_TEST_MODEL"),
        env_path("HS_TEST_EMBED_MODEL"),
    ) else {
        return;
    };
    data_root(&bin);
    let chat = define("gw-max-chat", &model, 4096, &[]);
    define("gw-max-embed", &embed, 2048, &[]);
    let gw = Gw::start(&bin, 18891, |s| s.max_loaded_models = 1).await;
    let (status, _, _) = json_of(
        gw.post(
            "/v1/chat/completions",
            json!({"model": "gw-max-chat", "max_tokens": 4,
        "messages": [{"role": "user", "content": "hi"}]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200);
    let (status, headers, body) = json_of(
        gw.post(
            "/v1/embeddings",
            json!({"model": "gw-max-embed", "input": "x"}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(headers["x-harness-evicted"], "gw-max-chat");
    assert_eq!(gw.running(), ["gw-gw-max-embed"]);

    // Pin the embedding model: now the chat model cannot get a place.
    let mut e = harness_core::domain::store::Domain::global()
        .definitions()
        .unwrap()
        .into_iter()
        .find(|d| d.alias == "gw-max-embed")
        .unwrap();
    e.pinned = true;
    save(e);
    tokio::time::sleep(Duration::from_millis(1100)).await; // definition cache
    let (status, headers, body) = json_of(
        gw.post(
            "/v1/chat/completions",
            json!({"model": "gw-max-chat",
        "messages": [{"role": "user", "content": "hi"}]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["error"]["code"], "max_loaded_models");
    assert!(headers.get("retry-after").is_some());
    eprintln!("pinned blocks eviction: {}", body["error"]["message"]);
    let _ = chat;
    // Leave nothing pinned for the other tests.
    let mut e = harness_core::domain::store::Domain::global()
        .definitions()
        .unwrap()
        .into_iter()
        .find(|d| d.alias == "gw-max-embed")
        .unwrap();
    e.pinned = false;
    save(e);
    gw.stop().await;
}

/// Per-definition queue (1 slot, depth 1): the third concurrent request gets 503 +
/// Retry-After; a client that disconnects cancels the llama-server request.
#[tokio::test]
#[ignore]
async fn e2e_queue_overflow_and_disconnect_cancel() {
    let _serial = serial().await;
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL")) else {
        return;
    };
    data_root(&bin);
    define(
        "gw-queue",
        &model,
        4096,
        &[("parallel_slots", ParamValue::Int(1))],
    );
    let mut events = harness_core::events::subscribe();
    let gw = Gw::start(&bin, 18892, |s| s.max_queue = 1).await;
    let long = json!({"model": "gw-queue", "stream": true, "max_tokens": 400,
        "messages": [{"role": "user", "content": "Write a long story about a lighthouse keeper."}]});
    // Warm up (JIT load) first.
    let (status, _, _) = json_of(
        gw.post(
            "/v1/chat/completions",
            json!({"model": "gw-queue", "max_tokens": 2,
        "messages": [{"role": "user", "content": "hi"}]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200);

    let a = gw.post("/v1/chat/completions", long.clone()).await; // runs (headers came)
    assert_eq!(a.status(), 200);
    let b = tokio::spawn({
        let (http, url, body) = (
            gw.http.clone(),
            format!("{}/v1/chat/completions", gw.base),
            long.clone(),
        );
        async move { http.post(url).json(&body).send().await.unwrap() }
    });
    tokio::time::sleep(Duration::from_millis(300)).await; // b is waiting in the queue
    let (status, headers, body) =
        json_of(gw.post("/v1/chat/completions", long.clone()).await).await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["error"]["code"], "queue_full");
    let retry: u64 = headers["retry-after"].to_str().unwrap().parse().unwrap();
    eprintln!("queue full → 503, Retry-After {retry} s");
    assert!((1..=60).contains(&retry));

    // Disconnect a: read a few chunks, then drop it. b gets the slot.
    let mut stream = a.bytes_stream();
    for _ in 0..5 {
        stream.next().await.unwrap().unwrap();
    }
    let dropped_at = Instant::now();
    drop(stream);
    let b = b.await.unwrap();
    assert_eq!(b.status(), 200);
    let waited: u64 = b.headers()["x-harness-queue-ms"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    eprintln!(
        "b waited {waited} ms in the queue, got the slot {:?} after a disconnected",
        dropped_at.elapsed()
    );
    // While b streams, the single slot must be b's: a's generation was cancelled upstream.
    let port = gw
        .registry
        .get("gw-gw-queue")
        .unwrap()
        .active_server()
        .unwrap()
        .port;
    let slots: Value = reqwest::get(format!("http://127.0.0.1:{port}/slots"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    eprintln!(
        "slots while b runs: {}",
        serde_json::to_string(&slots)
            .unwrap()
            .chars()
            .take(300)
            .collect::<String>()
    );
    drop(b);
    // After b is dropped too, the server goes idle quickly (its request was cancelled).
    let t = Instant::now();
    loop {
        let slots: Value = reqwest::get(format!("http://127.0.0.1:{port}/slots"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if slots
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["is_processing"] == false)
        {
            break;
        }
        assert!(
            t.elapsed() < Duration::from_secs(5),
            "upstream still processing after disconnect"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    eprintln!(
        "upstream idle {:?} after the last client disconnected",
        t.elapsed()
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    let recs = records(&mut events);
    let cancelled: Vec<&RequestRecord> = recs.iter().filter(|r| r.cancelled).collect();
    assert!(cancelled.len() >= 2, "{recs:?}");
    assert!(cancelled
        .iter()
        .all(|r| r.usage.completion_tokens.unwrap_or(0) < 400));
    assert!(recs
        .iter()
        .any(|r| r.error_code.as_deref() == Some("queue_full") && r.status == 503));
    gw.stop().await;
}

/// Codex-style Responses: a `namespace` tool is flattened and the call comes back with its
/// namespace (JSON and SSE); `web_search` is refused with an explicit 400; with the user's
/// consent it is ignored and reported.
#[tokio::test]
#[ignore]
async fn e2e_responses_namespace_and_web_search() {
    let _serial = serial().await;
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL")) else {
        return;
    };
    data_root(&bin);
    let def = define("gw-codex", &model, 8192, &[]);
    let gw = Gw::start(&bin, 18893, |_| {}).await;
    let tools = json!([
        {"type": "function", "name": "shell", "description": "Run a shell command",
         "parameters": {"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}},
        {"type": "namespace", "name": "mcp__github", "description": "GitHub MCP server",
         "tools": [{"type": "function", "name": "get_issue", "description": "Get a GitHub issue by repository and number",
                    "defer_loading": true,
                    "parameters": {"type": "object", "properties": {"repo": {"type": "string"}, "number": {"type": "integer"}},
                                   "required": ["repo", "number"]}}]}
    ]);
    let input = "Get GitHub issue number 42 of the repository acme/web using the tool.";
    let (status, headers, body) = json_of(
        gw.post(
            "/v1/responses",
            json!({"model": "gw-codex", "input": input,
        "tools": tools, "tool_choice": "required"}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(headers["x-harness-compat"], "namespace_flattened=1");
    let call = body["output"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["type"] == "function_call")
        .expect("a call");
    eprintln!("call: {call}");
    assert_eq!(call["name"], "get_issue");
    assert_eq!(call["namespace"], "mcp__github");

    // Streaming: the output item events carry the original name + namespace.
    let resp = gw
        .post(
            "/v1/responses",
            json!({"model": "gw-codex", "input": input, "stream": true,
        "tools": tools, "tool_choice": "required"}),
        )
        .await;
    let (evs, _) = sse(resp).await;
    let done = evs
        .iter()
        .find(|e| e["type"] == "response.output_item.done" && e["item"]["type"] == "function_call")
        .expect("function call item");
    assert_eq!(done["item"]["name"], "get_issue");
    assert_eq!(done["item"]["namespace"], "mcp__github");
    assert!(
        !serde_json::to_string(&evs)
            .unwrap()
            .contains("mcp__github__get_issue"),
        "no flattened name leaks"
    );

    // Follow-up turn with the namespaced call in the history.
    let (status, _, body) = json_of(gw.post("/v1/responses", json!({"model": "gw-codex", "tools": tools, "input": [
        {"type": "message", "role": "user", "content": input},
        {"type": "function_call", "name": "get_issue", "namespace": "mcp__github", "call_id": "call_1",
         "arguments": "{\"repo\":\"acme/web\",\"number\":42}"},
        {"type": "function_call_output", "call_id": "call_1", "output": "{\"title\":\"Login button broken\",\"state\":\"open\"}"}
    ]})).await).await;
    assert_eq!(status, 200, "{body}");
    eprintln!("follow-up: {}", body["output"]);

    // web_search: explicit 400, nothing dropped.
    let mut with_search = tools.clone();
    with_search
        .as_array_mut()
        .unwrap()
        .push(json!({"type": "web_search"}));
    let (status, _, body) = json_of(
        gw.post(
            "/v1/responses",
            json!({"model": "gw-codex", "input": input,
        "tools": with_search}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(body["error"]["code"], "unsupported_tool_type");
    assert_eq!(body["error"]["param"], "tools[2]");
    assert_eq!(body["error"]["harness"]["tool_type"], "web_search");
    eprintln!("web_search → {}", body["error"]["message"]);

    // The user's explicit consent for this definition: ignored and reported.
    let sched = harness_core::gateway::scheduler().unwrap();
    let mut s = sched.settings();
    s.tool_ignores
        .insert(def.id.clone(), vec!["web_search".into()]);
    sched.set_settings(s);
    let (status, headers, body) = json_of(
        gw.post(
            "/v1/responses",
            json!({"model": "gw-codex", "input": input,
        "tools": with_search, "tool_choice": "required"}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        headers["x-harness-compat"],
        "namespace_flattened=1; ignored=web_search"
    );
    gw.stop().await;
}

/// Anthropic `/v1/messages` with tools, streaming.
#[tokio::test]
#[ignore]
async fn e2e_anthropic_messages_tools_stream() {
    let _serial = serial().await;
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL")) else {
        return;
    };
    data_root(&bin);
    define("gw-claude", &model, 8192, &[]);
    let gw = Gw::start(&bin, 18894, |_| {}).await;
    let resp = gw.http.post(format!("{}/v1/messages", gw.base))
        .header("anthropic-version", "2023-06-01")
        .header("x-api-key", "placeholder")
        .header("user-agent", "claude-cli/2.1.0 (external, cli)")
        .json(&json!({"model": "gw-claude", "max_tokens": 256, "stream": true,
            "tools": [{"name": "get_weather", "description": "Current weather of a city",
                       "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]}}],
            "tool_choice": {"type": "any"},
            "messages": [{"role": "user", "content": "What is the weather in Ankara?"}]}))
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let (evs, times) = sse(resp).await;
    let start = evs
        .iter()
        .find(|e| e["type"] == "content_block_start" && e["content_block"]["type"] == "tool_use")
        .expect("tool_use block");
    assert_eq!(start["content_block"]["name"], "get_weather");
    let args: String = evs
        .iter()
        .filter(|e| e["delta"]["type"] == "input_json_delta")
        .map(|e| e["delta"]["partial_json"].as_str().unwrap_or(""))
        .collect();
    let args: Value = serde_json::from_str(&args).unwrap();
    assert!(args["city"].as_str().unwrap().contains("Ankara"));
    assert!(evs
        .iter()
        .any(|e| e["type"] == "message_delta" && e["delta"]["stop_reason"] == "tool_use"));
    eprintln!(
        "anthropic stream: {} events / {} chunks, args {args}",
        evs.len(),
        times.len()
    );

    // A server tool of Anthropic is refused in Anthropic's error shape.
    let (status, _, body) = json_of(
        gw.post(
            "/v1/messages",
            json!({"model": "gw-claude", "max_tokens": 16,
        "tools": [{"type": "web_search_20250305", "name": "web_search", "max_uses": 2}],
        "messages": [{"role": "user", "content": "hi"}]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(body["type"], "error");
    assert_eq!(body["error"]["code"], "unsupported_tool_type");
    gw.stop().await;
}

/// Memory-aware eviction with two big models (e.g. gpt-oss-20b + a 14B on a 16 GB GPU):
/// the second does not fit next to the first, so the first (idle, LRU) is evicted.
#[tokio::test]
#[ignore]
async fn e2e_memory_pressure_eviction() {
    let _serial = serial().await;
    let (Some(bin), Some(big_a), Some(big_b)) = (
        env_path("HS_LLAMA_SERVER"),
        env_path("HS_TEST_MODEL_B"),
        env_path("HS_TEST_MODEL_C"),
    ) else {
        return;
    };
    data_root(&bin);
    define("gw-big-a", &big_a, 8192, &[]);
    define("gw-big-b", &big_b, 8192, &[]);
    let gw = Gw::start(&bin, 18895, |_| {}).await;
    let ask = |m: &str| json!({"model": m, "max_tokens": 8, "messages": [{"role": "user", "content": "Say OK."}]});
    let (status, _, body) = json_of(gw.post("/v1/chat/completions", ask("gw-big-a")).await).await;
    assert_eq!(status, 200, "{body}");
    let (status, headers, body) =
        json_of(gw.post("/v1/chat/completions", ask("gw-big-b")).await).await;
    assert_eq!(status, 200, "{body}");
    eprintln!(
        "load of b: {} ms, evicted {:?}",
        headers["x-harness-load-ms"].to_str().unwrap(),
        headers.get("x-harness-evicted")
    );
    assert_eq!(headers["x-harness-evicted"], "gw-big-a");
    assert_eq!(gw.running(), ["gw-gw-big-b"]);
    gw.stop().await;
}

/// A request that cannot fit even in the best case (everything evictable gone, pinned and busy
/// models stay) is refused with `503 insufficient_memory` WITHOUT evicting anything: the idle
/// model must still be loaded afterwards. When the request can fit, the least recently used
/// model leaves first as before.
///
/// `HS_TEST_MODEL` (pinned, e.g. Qwen2.5-3B), `HS_TEST_MODEL_SMALL` (idle, e.g. Qwen2.5-0.5B),
/// `HS_TEST_MODEL_B` (too big next to the pinned one, e.g. Llama-3.2-3B Q6_K) on a 4 GB card.
#[tokio::test]
#[ignore]
async fn e2e_unfittable_request_evicts_nothing() {
    let _serial = serial().await;
    let (Some(bin), Some(pinned), Some(small), Some(big)) = (
        env_path("HS_LLAMA_SERVER"),
        env_path("HS_TEST_MODEL"),
        env_path("HS_TEST_MODEL_SMALL"),
        env_path("HS_TEST_MODEL_B"),
    ) else {
        return;
    };
    data_root(&bin);
    let mut p = define("gw-nf-pinned", &pinned, 4096, &[]);
    p.pinned = true;
    let p = save(p);
    define("gw-nf-small", &small, 2048, &[]);
    define("gw-nf-big", &big, 8192, &[]);
    // Never fits, whatever leaves: the KV cache alone is far beyond any card.
    define("gw-nf-huge", &big, 1_048_576, &[]);
    // The same model with a small context: needs less, so it fits once one idle model is gone.
    define("gw-nf-mid", &big, 2048, &[]);
    tokio::time::sleep(Duration::from_millis(1100)).await; // definition cache
    let gw = Gw::start(&bin, 18897, |_| {}).await;
    let ask = |m: &str| json!({"model": m, "max_tokens": 8, "messages": [{"role": "user", "content": "Say OK."}]});
    for m in ["gw-nf-pinned", "gw-nf-small"] {
        let (status, _, body) = json_of(gw.post("/v1/chat/completions", ask(m)).await).await;
        assert_eq!(status, 200, "{body}");
    }
    let before = gw.running();
    eprintln!("loaded before: {before:?}");
    assert_eq!(before.len(), 2);

    // 1. Pinned + idle loaded; the big model cannot fit even with the idle one gone.
    let (status, headers, body) =
        json_of(gw.post("/v1/chat/completions", ask("gw-nf-big")).await).await;
    eprintln!("big while pinned: {status} {body}");
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["error"]["code"], "insufficient_memory");
    assert!(headers.get("x-harness-evicted").is_none());
    // Only what really cannot leave is listed: the pinned model, not the idle one.
    assert_eq!(
        body["error"]["harness"]["blocking_models"],
        json!(["gw-nf-pinned"]),
        "{body}"
    );
    let mut after = gw.running();
    let mut before_sorted = before.clone();
    after.sort();
    before_sorted.sort();
    assert_eq!(after, before_sorted, "the idle model must stay loaded");

    // 1b. Unpinned: every loaded model is idle and evictable, but a request that cannot fit even
    // on an empty card is still refused with 503 and NOTHING leaves.
    let mut p = p;
    p.pinned = false;
    let p = save(p);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let (status, headers, body) =
        json_of(gw.post("/v1/chat/completions", ask("gw-nf-huge")).await).await;
    eprintln!("huge after unpin: {status} {body}");
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["error"]["code"], "insufficient_memory");
    assert!(headers.get("x-harness-evicted").is_none());
    // Nothing is pinned or busy now: every loaded model counted as gone, none blocks.
    assert_eq!(
        body["error"]["harness"]["blocking_models"],
        json!([]),
        "{body}"
    );
    let mut after = gw.running();
    after.sort();
    assert_eq!(
        after, before_sorted,
        "nothing may be evicted for a request that never fits"
    );

    // 2. Unpinned: a request that can fit (small context) evicts only the least recently used
    // model, which was loaded before the small one
    let _ = p;
    let (status, headers, body) =
        json_of(gw.post("/v1/chat/completions", ask("gw-nf-mid")).await).await;
    eprintln!(
        "mid after unpin: {status}, evicted {:?}, running {:?}",
        headers.get("x-harness-evicted"),
        gw.running()
    );
    assert_eq!(status, 200, "{body}");
    let evicted = headers["x-harness-evicted"].to_str().unwrap().to_string();
    assert_eq!(evicted, "gw-nf-pinned");
    assert_eq!(
        gw.running().len(),
        2,
        "only one model leaves: {:?}",
        gw.running()
    );
    gw.stop().await;
}

/// OOM recovery hook, for real: with the memory check off, a JIT load that runs out of GPU memory
/// (Qwen3-4B with a 262144 context: the KV cache allocation fails on b11236/Vulkan) makes the
/// scheduler evict the idle model and retry once. Here the retry fails too (the KV cache
/// alone is too big), so the client gets `503 insufficient_memory` with the recovery
/// recovery suggestions. `fit` is switched off for the oversized model: with llama-server's own
/// `--fit` (default on CUDA) the context is shrunk to the free memory and nothing fails. (On
/// Windows/Vulkan an ordinary two-model overcommit spills to shared
/// memory instead of failing, so a retry that succeeds could not be provoked on this GPU.)
#[tokio::test]
#[ignore]
async fn e2e_oom_during_jit_load_evicts_and_retries_once() {
    let _serial = serial().await;
    let (Some(bin), Some(big)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL_B")) else {
        return;
    };
    data_root(&bin);
    define("gw-oom-a", &big, 8192, &[]);
    // `--fit off`: llama-server (b11456) otherwise shrinks the context / layers to the free
    // memory itself on CUDA, the load succeeds and the OOM path is never taken. The same model
    // as the idle one: the context asked for is only capped to what the file was trained for
    // (Qwen2.5-3B: 32768, a 1.2 GB KV cache that fits), so a model trained for 131072 or more
    // (Llama-3.2-3B, Qwen3-4B) is needed for a KV cache that cannot fit on a 4 GB card.
    define(
        "gw-oom-b",
        &big,
        262144,
        &[("fit", ParamValue::Text("off".into()))],
    );
    let mut events = harness_core::events::subscribe();
    let gw = Gw::start(&bin, 18896, |s| s.memory_aware = false).await;
    let ask = |m: &str| json!({"model": m, "max_tokens": 8, "messages": [{"role": "user", "content": "Say OK."}]});
    let (status, _, body) = json_of(gw.post("/v1/chat/completions", ask("gw-oom-a")).await).await;
    assert_eq!(status, 200, "{body}");
    let (status, headers, body) =
        json_of(gw.post("/v1/chat/completions", ask("gw-oom-b")).await).await;
    let mut actions = Vec::new();
    while let Ok(ev) = events.try_recv() {
        if let CoreEvent::GatewayModel {
            alias,
            action,
            reason,
            ..
        } = ev
        {
            actions.push(format!("{alias}:{action}:{}", reason.unwrap_or_default()));
        }
    }
    eprintln!(
        "status {status}, events {actions:?}
body {}",
        serde_json::to_string(&body).unwrap()
    );
    assert!(
        actions
            .iter()
            .any(|a| a.starts_with("gw-oom-b:oom_retry:servers.out_of_memory")),
        "{actions:?}"
    );
    assert!(
        actions.iter().any(|a| a == "gw-oom-a:evicted:gw-oom-b"),
        "{actions:?}"
    );
    assert!(!gw.running().contains(&"gw-gw-oom-a".to_string()));
    if status == 200 {
        assert_eq!(headers["x-harness-evicted"], "gw-oom-a");
    } else {
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "insufficient_memory");
        assert!(!body["error"]["harness"]["suggestions"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(headers.get("retry-after").is_some());
    }
    gw.stop().await;
}

/// Several allowed origins: llama-server (b11299 too) sends the whole comma-joined list in
/// one `Access-Control-Allow-Origin` header, which browsers reject. The gateway answers with
/// the request's own `Origin` when it is on the list, and with no CORS header otherwise.
#[tokio::test]
#[ignore]
async fn e2e_cors_reflects_the_matching_origin() {
    let Some(bin) = env_path("HS_LLAMA_SERVER") else {
        return;
    };
    let _serial = serial().await;
    let gw = Gw::start(&bin, 18897, |s| {
        s.cors_origins = vec!["http://a.test".into(), "https://b.test:8443".into()];
    })
    .await;
    let allow = |r: &reqwest::Response| {
        r.headers()
            .get_all("access-control-allow-origin")
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    for origin in ["http://a.test", "https://b.test:8443"] {
        let r = gw
            .http
            .get(format!("{}/health", gw.base))
            .header("Origin", origin)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(allow(&r), [origin], "simple request from {origin}");
        let pre = gw
            .http
            .request(
                reqwest::Method::OPTIONS,
                format!("{}/v1/chat/completions", gw.base),
            )
            .header("Origin", origin)
            .header("Access-Control-Request-Method", "POST")
            .header(
                "Access-Control-Request-Headers",
                "authorization,content-type",
            )
            .send()
            .await
            .unwrap();
        assert!(pre.status().is_success(), "{}", pre.status());
        assert_eq!(allow(&pre), [origin], "preflight from {origin}");
    }
    let other = gw
        .http
        .get(format!("{}/health", gw.base))
        .header("Origin", "http://evil.test")
        .send()
        .await
        .unwrap();
    assert!(allow(&other).is_empty(), "{:?}", allow(&other));
    gw.server.stop().await;
}
