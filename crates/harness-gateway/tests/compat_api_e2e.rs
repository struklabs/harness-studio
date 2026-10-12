//! Ollama-compatible API end to end with a real llama-server, real models and (optionally)
//! the real `ollama` CLI as the client. No mocks:
//!
//! ```text
//! HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<chat gguf> HS_TEST_EMBED_MODEL=<embedding gguf>
//! [HS_OLLAMA_CLI=<ollama executable>]
//! cargo test -p harness-gateway --test compat_api_e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `HS_TEST_MODEL` is any instruct GGUF with a tool-calling template that answers without a
//! thinking block, kept as Harness downloads it (`<owner>/<repo>/<file>`, for its `hf.co/` name);
//! family, quantization, size and that name are read from the file. Checked with
//! Qwen3-4B-Instruct-2507 and Qwen2.5-0.5B-Instruct (Q4_K_M).
//!
//! (The file is not called `ollama_e2e`: the port-listener test checks who holds the Ollama
//! port by process name, and a test binary named `ollama_e2e-….exe` looked like Ollama itself.)
//!
//! Temporary data root, gateway on 19500 (models on the ports after it), Ollama-port
//! listener tests on 19520+. The `ollama` CLI only ever talks to the test port
//! (`OLLAMA_HOST`); a real Ollama on 11434 is never touched. Every test stops what it started.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::events::CoreEvent;
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
            .prefix(&harness_core::paths::test_dir_prefix("ollama-e2e"))
            .tempdir()
            .unwrap()
            .keep();
        harness_core::paths::init(dir.clone());
        let mut s = harness_core::config::AppSettings::load();
        s.runtime_mode = harness_core::config::RuntimeMode::Custom;
        s.custom_binary.binary_path = Some(bin.to_path_buf());
        s.custom_binary.is_valid = true;
        s.server.port = 19590;
        s.control_api.port = 19599;
        s.save().unwrap();
        dir
    })
}

fn define(alias: &str, model: &Path, ctx: i64) -> ModelDefinition {
    let mut d = ModelDefinition::new(alias, ModelRef::Local { path: model.into() });
    d.load.insert("gpu_layers".into(), ParamValue::Int(99));
    d.load.insert("context_size".into(), ParamValue::Int(ctx));
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
        let registry = Arc::new(ServerRegistry::default());
        let ctx = GatewayContext {
            registry: registry.clone(),
            runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
            gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
            data_dir: data_root(bin).to_path_buf(),
            downloads: Some(Arc::new(
                harness_core::downloader::DownloadManager::default(),
            )),
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

    async fn get(&self, path: &str) -> Value {
        let r = self
            .http
            .get(format!("{}{}", self.base, path))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        r.json().await.unwrap()
    }

    async fn stop(self) {
        self.server.stop().await;
        self.registry.stop_all().await;
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

/// NDJSON objects and the arrival time of each chunk.
async fn ndjson(resp: reqwest::Response) -> (Vec<Value>, Vec<Duration>) {
    let t = Instant::now();
    let mut stream = resp.bytes_stream();
    let (mut buf, mut out, mut times) = (String::new(), Vec::new(), Vec::new());
    while let Some(Ok(chunk)) = stream.next().await {
        times.push(t.elapsed());
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(end) = buf.find('\n') {
            let l: String = buf.drain(..end + 1).collect();
            if !l.trim().is_empty() {
                out.push(serde_json::from_str(l.trim()).expect("each line is JSON"));
            }
        }
    }
    assert!(buf.trim().is_empty(), "trailing partial line: {buf}");
    (out, times)
}

fn norm(v: &Value) -> f64 {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap().powi(2))
        .sum::<f64>()
        .sqrt()
}

async fn ps_names(gw: &Gw) -> Vec<String> {
    gw.get("/api/ps").await["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap().to_string())
        .collect()
}

const WEATHER_TOOL: &str = r#"{"type": "function", "function": {"name": "get_weather",
  "description": "Get the current weather in a given city",
  "parameters": {"type": "object", "properties": {"city": {"type": "string", "description": "City name"}},
  "required": ["city"]}}}"#;

#[tokio::test]
#[ignore]
async fn e2e_ollama_api() {
    let _serial = serial().await;
    let (Some(bin), Some(model), Some(embed)) = (
        env_path("HS_LLAMA_SERVER"),
        env_path("HS_TEST_MODEL"),
        env_path("HS_TEST_EMBED_MODEL"),
    ) else {
        return;
    };
    data_root(&bin);
    // Expectations come from the file itself, so any chat GGUF works (not only Qwen3-4B).
    let meta = harness_core::gguf::read_gguf_metadata(&model).unwrap();
    let arch = meta.architecture.clone().expect("general.architecture");
    let file_size = std::fs::metadata(&model).unwrap().len();
    // Hugging Face name of the same file: `<owner>/<repo>/<file>` as Harness downloads it.
    let repo: Vec<String> = model
        .parent()
        .unwrap()
        .iter()
        .rev()
        .take(2)
        .map(|c| c.to_string_lossy().into_owned())
        .collect();
    let hf_name = format!("hf.co/{}/{}:{}", repo[1], repo[0], meta.quantization);
    define("qwen3-4b", &model, 4096);
    define("nomic-embed:v1.5", &embed, 2048);
    let mut events = harness_core::events::subscribe();
    let gw = Gw::start(&bin, 19500, |_| {}).await;

    // Version, root probe (the CLI sends HEAD /).
    let v = gw.get("/api/version").await;
    assert!(v["version"].as_str().unwrap().starts_with("0."), "{v}");
    let head = gw.http.head(format!("{}/", gw.base)).send().await.unwrap();
    assert_eq!(head.status(), 200);

    // Tags: aliases without a tag get `:latest`; details from the GGUF header.
    let tags = gw.get("/api/tags").await;
    eprintln!("tags: {}", serde_json::to_string_pretty(&tags).unwrap());
    let models = tags["models"].as_array().unwrap();
    let qwen = models
        .iter()
        .find(|m| m["name"] == "qwen3-4b:latest")
        .unwrap();
    assert_eq!(qwen["model"], "qwen3-4b:latest");
    assert_eq!(qwen["details"]["format"], "gguf");
    assert_eq!(qwen["details"]["family"], arch.as_str());
    assert_eq!(
        qwen["details"]["quantization_level"],
        meta.quantization.as_str()
    );
    // "4.0B", or "596M" below a billion (as Ollama prints it).
    let params = qwen["details"]["parameter_size"].as_str().unwrap();
    assert!(params.ends_with('B') || params.ends_with('M'), "{params}");
    assert_eq!(qwen["size"].as_u64().unwrap(), file_size);
    assert_eq!(qwen["digest"].as_str().unwrap().len(), 64);
    assert!(models.iter().any(|m| m["name"] == "nomic-embed:v1.5"));

    // Show: capabilities, model_info (GGUF keys), the Jinja template.
    let (status, _, show) = json_of(gw.post("/api/show", json!({"model": "qwen3-4b"})).await).await;
    assert_eq!(status, 200, "{show}");
    let caps: Vec<&str> = show["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    assert!(caps.contains(&"completion"), "{caps:?}");
    assert_eq!(
        caps.contains(&"tools"),
        meta.template_supports_tools,
        "{caps:?}"
    );
    assert_eq!(show["model_info"]["general.architecture"], arch.as_str());
    assert!(
        show["model_info"][format!("{arch}.context_length")]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        show["model_info"]["tokenizer.ggml.tokens"],
        json!([]),
        "large arrays only with verbose"
    );
    assert!(!show["template"].as_str().unwrap().is_empty());
    let (_, _, show_e) = json_of(
        gw.post("/api/show", json!({"model": "nomic-embed:v1.5"}))
            .await,
    )
    .await;
    assert_eq!(show_e["capabilities"], json!(["embedding"]));

    // Unknown model: Ollama's error shape and 404.
    let (status, _, err) = json_of(
        gw.post(
            "/api/chat",
            json!({"model": "llama3.2", "messages": [{"role": "user", "content": "hi"}]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 404);
    assert!(
        err["error"].as_str().unwrap().contains("not found"),
        "{err}"
    );

    // Preload with an empty request.
    let t = Instant::now();
    let (status, _, load) =
        json_of(gw.post("/api/generate", json!({"model": "qwen3-4b"})).await).await;
    assert_eq!(status, 200, "{load}");
    assert_eq!(load["done"], true);
    assert_eq!(load["done_reason"], "load");
    assert!(load["load_duration"].as_u64().unwrap() > 0);
    eprintln!("preload {:?}: {}", t.elapsed(), load);
    let ps = gw.get("/api/ps").await;
    eprintln!("ps: {ps}");
    let m = &ps["models"][0];
    assert_eq!(m["name"], "qwen3-4b:latest");
    assert_eq!(m["context_length"], 4096);
    assert!(
        m["expires_at"].as_str().unwrap() > "2200",
        "no TTL: far future"
    );

    // Streaming chat: NDJSON, chunks while the model generates, final stats in ns.
    let resp = gw
        .post(
            "/api/chat",
            json!({"model": "qwen3-4b:latest", "options": {"temperature": 0, "num_predict": 120},
            "messages": [{"role": "user", "content": "Count from 1 to 40 separated by spaces."}]}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["content-type"], "application/x-ndjson");
    assert!(resp.headers()["x-harness-compat"]
        .to_str()
        .unwrap()
        .starts_with("ollama"));
    let (lines, times) = ndjson(resp).await;
    assert!(times.len() > 10, "streamed in many chunks: {}", times.len());
    assert!(times[0] * 4 < *times.last().unwrap());
    let text: String = lines
        .iter()
        .filter_map(|l| l["message"]["content"].as_str())
        .collect();
    assert!(text.contains("1 2 3"), "{text}");
    let last = lines.last().unwrap();
    eprintln!("chat final: {last}");
    assert_eq!(last["done"], true);
    assert!(lines[..lines.len() - 1].iter().all(|l| l["done"] == false));
    assert!(["stop", "length"].contains(&last["done_reason"].as_str().unwrap()));
    assert!(last["eval_count"].as_u64().unwrap() > 10);
    let eval_ns = last["eval_duration"].as_u64().unwrap();
    assert!(eval_ns > 10_000_000, "nanoseconds: {eval_ns}");
    assert!(last["total_duration"].as_u64().unwrap() >= eval_ns);
    assert!(last["prompt_eval_count"].as_u64().unwrap() > 5);
    assert!(last["created_at"].as_str().unwrap().ends_with('Z'));

    // Tool calling (non-streaming): arguments as an object, then the tool result.
    let tool: Value = serde_json::from_str(WEATHER_TOOL).unwrap();
    let (status, _, r) = json_of(
        gw.post(
            "/api/chat",
            json!({"model": "qwen3-4b", "stream": false, "tools": [tool],
            "messages": [{"role": "user", "content": "What is the weather in Tokyo right now?"}]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{r}");
    eprintln!("tool call: {r}");
    let call = &r["message"]["tool_calls"][0]["function"];
    assert_eq!(call["name"], "get_weather");
    assert!(call["arguments"]["city"]
        .as_str()
        .unwrap()
        .contains("Tokyo"));
    assert_eq!(r["done_reason"], "stop");
    let assistant = r["message"].clone();
    let (status, _, r) = json_of(
        gw.post(
            "/api/chat",
            json!({"model": "qwen3-4b", "stream": false, "tools": [tool], "options": {"temperature": 0},
            "messages": [
                {"role": "user", "content": "What is the weather in Tokyo right now?"},
                assistant,
                {"role": "tool", "tool_name": "get_weather", "content": "{\"temperature_c\": 17, \"sky\": \"rain\"}"}
            ]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{r}");
    let answer = r["message"]["content"].as_str().unwrap();
    eprintln!("after tool: {answer}");
    assert!(answer.contains("17"), "{answer}");

    // Streaming tool call: one object with the whole call.
    let resp = gw
        .post(
            "/api/chat",
            json!({"model": "qwen3-4b", "tools": [tool],
            "messages": [{"role": "user", "content": "Weather in Paris?"}]}),
        )
        .await;
    let (lines, _) = ndjson(resp).await;
    let with_calls: Vec<&Value> = lines
        .iter()
        .filter(|l| l["message"].get("tool_calls").is_some())
        .collect();
    assert_eq!(with_calls.len(), 1, "{lines:?}");
    assert_eq!(
        with_calls[0]["message"]["tool_calls"][0]["function"]["arguments"]["city"],
        "Paris"
    );

    // Generate, structured output, Hugging Face name of the same file.
    let (status, _, r) = json_of(
        gw.post(
            "/api/generate",
            json!({"model": hf_name, "stream": false,
            "prompt": "Ollama is 22 years old and busy saving the world. Respond using JSON.",
            "format": {"type": "object", "properties": {"age": {"type": "integer"}, "available": {"type": "boolean"}},
                       "required": ["age", "available"]},
            "options": {"temperature": 0, "num_ctx": 8192}}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{r}");
    assert_eq!(r["model"], hf_name.as_str());
    let parsed: Value = serde_json::from_str(r["response"].as_str().unwrap()).unwrap();
    assert_eq!(parsed["age"], 22);
    assert!(parsed["available"].is_boolean());

    // Raw generate over /v1/completions.
    let (status, _, r) = json_of(
        gw.post(
            "/api/generate",
            json!({"model": "qwen3-4b", "stream": false, "raw": true, "options": {"num_predict": 8, "temperature": 0},
            "prompt": "<|im_start|>user\nSay OK<|im_end|>\n<|im_start|>assistant\n"}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{r}");
    assert!(!r["response"].as_str().unwrap().is_empty());

    // Images for a model without a projector: a clear error, nothing sent upstream.
    let (status, _, r) = json_of(
        gw.post(
            "/api/chat",
            json!({"model": "qwen3-4b", "messages": [{"role": "user", "content": "what is this",
            "images": ["iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="]}]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 400);
    assert!(
        r["error"]
            .as_str()
            .unwrap()
            .contains("does not support images"),
        "{r}"
    );

    // keep_alive → TTL: "3s" unloads the gateway-loaded model after 3 idle seconds.
    let (status, _, _) = json_of(
        gw.post(
            "/api/generate",
            json!({"model": "qwen3-4b", "stream": false, "keep_alive": "3s",
            "prompt": "Say hi", "options": {"num_predict": 4}}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200);
    let ps = gw.get("/api/ps").await;
    let exp = ps["models"][0]["expires_at"].as_str().unwrap().to_string();
    assert!(exp.as_str() < "2200", "TTL from keep_alive: {exp}");
    let t = Instant::now();
    while !ps_names(&gw).await.is_empty() {
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "keep_alive 3s did not unload"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    eprintln!("unloaded {:?} after keep_alive 3s", t.elapsed());

    // keep_alive -1 = forever; then an explicit unload with keep_alive 0.
    let (status, _, _) = json_of(
        gw.post(
            "/api/chat",
            json!({"model": "qwen3-4b", "messages": [], "keep_alive": -1}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200);
    let slot = gw.server.scheduler().await.unwrap().snapshot().await;
    assert_eq!(
        slot.iter()
            .find(|s| s.alias == "qwen3-4b")
            .unwrap()
            .keep_alive,
        Some(harness_core::gateway::scheduler::KeepAlive::Forever)
    );
    let (status, _, r) = json_of(
        gw.post(
            "/api/generate",
            json!({"model": "qwen3-4b", "keep_alive": 0}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(r["done_reason"], "unload");
    assert!(ps_names(&gw).await.is_empty());

    // Embeddings: normalized vectors, dimensions, the legacy endpoint.
    let (status, _, r) = json_of(
        gw.post(
            "/api/embed",
            json!({"model": "nomic-embed:v1.5", "input": ["search_query: a panda", "search_query: Paris"]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{r}");
    let e = r["embeddings"].as_array().unwrap();
    assert_eq!(e.len(), 2);
    assert_eq!(e[0].as_array().unwrap().len(), 768);
    assert!((norm(&e[0]) - 1.0).abs() < 1e-3);
    assert!(r["prompt_eval_count"].as_u64().unwrap() > 0);
    assert!(
        r["load_duration"].as_u64().unwrap() > 0,
        "JIT load reported"
    );
    let (_, _, r) = json_of(
        gw.post(
            "/api/embed",
            json!({"model": "nomic-embed:v1.5", "input": "search_query: a panda", "dimensions": 256}),
        )
        .await,
    )
    .await;
    assert_eq!(r["embeddings"][0].as_array().unwrap().len(), 256);
    assert!((norm(&r["embeddings"][0]) - 1.0).abs() < 1e-3);
    let (status, _, r) = json_of(
        gw.post(
            "/api/embeddings",
            json!({"model": "nomic-embed:v1.5", "prompt": "search_query: a panda"}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{r}");
    let legacy = &r["embedding"];
    assert_eq!(legacy.as_array().unwrap().len(), 768);
    eprintln!("legacy /api/embeddings norm {:.3}", norm(legacy));
    assert!(
        (norm(legacy) - 1.0).abs() > 1e-2,
        "legacy endpoint is not normalized"
    );

    // Pull: registry names are refused, a present HF file succeeds without downloading,
    // other HF repos need the setting.
    let (status, _, r) = json_of(gw.post("/api/pull", json!({"model": "llama3.2"})).await).await;
    assert_eq!(status, 400);
    assert!(r["error"].as_str().unwrap().contains("hf.co/"), "{r}");
    let resp = gw.post("/api/pull", json!({"model": hf_name})).await;
    let (lines, _) = ndjson(resp).await;
    assert_eq!(lines.last().unwrap()["status"], "success");
    let (status, _, r) = json_of(
        gw.post(
            "/api/pull",
            json!({"model": "hf.co/some-owner/some-repo-GGUF:Q8_0", "stream": false}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 403, "{r}");

    // Records: every Ollama request went through the analytics hook.
    let mut endpoints = Vec::new();
    while let Ok(ev) = events.try_recv() {
        if let CoreEvent::GatewayRequest { record } = ev {
            endpoints.push((
                record.endpoint.clone(),
                record.status,
                record.usage.completion_tokens,
            ));
        }
    }
    eprintln!("records: {endpoints:?}");
    assert!(endpoints
        .iter()
        .any(|(e, s, t)| e == "/api/chat" && *s == 200 && t.unwrap_or(0) > 10));
    assert!(endpoints.iter().any(|(e, _, _)| e == "/api/embed"));
    assert!(endpoints
        .iter()
        .any(|(e, s, _)| e == "/api/chat" && *s == 404));

    cli(&gw).await;

    gw.stop().await;
}

/// The real `ollama` CLI against the test port (`HS_OLLAMA_CLI`).
async fn cli(gw: &Gw) {
    let Some(exe) = env_path("HS_OLLAMA_CLI") else {
        return;
    };
    let host = gw.base.trim_start_matches("http://").to_string();
    let run = |args: Vec<&'static str>| {
        let exe = exe.clone();
        let host = host.clone();
        async move {
            tokio::task::spawn_blocking(move || {
                let out = std::process::Command::new(&exe)
                    .args(&args)
                    .env("OLLAMA_HOST", &host)
                    .env("NO_COLOR", "1")
                    .stdin(std::process::Stdio::null())
                    .output()
                    .unwrap();
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                eprintln!(
                    "$ OLLAMA_HOST={} ollama {}\n[exit {:?}]\n{}{}",
                    host,
                    args.join(" "),
                    out.status.code(),
                    stdout,
                    stderr
                );
                (out.status.success(), stdout, stderr)
            })
            .await
            .unwrap()
        }
    };
    let (ok, out, err) = run(vec!["--version"]).await;
    assert!(ok, "{err}");
    assert!(!out.contains("could not connect") && !err.contains("could not connect"));
    let (ok, out, err) = run(vec!["list"]).await;
    assert!(ok, "{err}");
    assert!(
        out.contains("qwen3-4b:latest") && out.contains("nomic-embed:v1.5"),
        "{out}"
    );
    let (ok, out, err) = run(vec!["show", "qwen3-4b"]).await;
    assert!(ok, "{err}");
    assert!(out.contains("qwen3") && out.contains("tools"), "{out}");
    let (ok, out, err) = run(vec![
        "run",
        "qwen3-4b",
        "--verbose",
        "Reply with exactly the word: banana",
    ])
    .await;
    assert!(ok, "{err}");
    assert!(out.to_lowercase().contains("banana"), "{out}");
    assert!(err.contains("eval rate"), "--verbose statistics: {err}");
    let (ok, out, err) = run(vec!["ps"]).await;
    assert!(ok, "{err}");
    assert!(
        out.contains("qwen3-4b:latest") && out.contains("4096"),
        "{out}"
    );
    let (ok, out, err) = run(vec!["run", "nomic-embed:v1.5", "search_query: hello"]).await;
    assert!(ok, "{err}");
    assert!(out.trim_start().starts_with('['), "embedding vector: {out}");
    let (ok, _, err) = run(vec!["stop", "qwen3-4b"]).await;
    assert!(ok, "{err}");
    assert!(!ps_names(gw).await.contains(&"qwen3-4b:latest".to_string()));
    let (ok, out, err) = run(vec!["pull", "llama3.2"]).await;
    assert!(!ok);
    assert!((out + &err).contains("hf.co/"));
}

/// The optional Ollama-port listener: serves the same API on a second port, and when that
/// port is taken (another gateway answering like Ollama, or any program) it warns instead of
/// failing.
#[tokio::test]
#[ignore]
async fn e2e_ollama_port_listener() {
    let _serial = serial().await;
    let Some(bin) = env_path("HS_LLAMA_SERVER") else {
        return;
    };
    data_root(&bin);
    let gw = Gw::start(&bin, 19510, |s| {
        s.ollama_port_enabled = true;
        s.ollama_port = 19520;
    })
    .await;
    let status = gw.server.status().await;
    assert_eq!(status.ollama_port, Some(19520), "{status:?}");
    assert_eq!(status.ollama_warning, None);
    let r = reqwest::get("http://127.0.0.1:19520/api/version")
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let r = reqwest::get("http://127.0.0.1:19520/api/tags")
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let r = reqwest::get("http://127.0.0.1:19520/health").await.unwrap();
    assert_eq!(r.status(), 200, "the whole gateway is on the Ollama port");

    // A second gateway that wants 19520 finds an Ollama-compatible server there.
    let other = Gw::start(&bin, 19530, |s| {
        s.ollama_port_enabled = true;
        s.ollama_port = 19520;
    })
    .await;
    let st = other.server.status().await;
    assert_eq!(st.ollama_port, None);
    assert!(
        st.ollama_warning
            .as_deref()
            .unwrap()
            .starts_with(harness_core::gateway::WARN_OLLAMA_HARNESS),
        "{st:?}"
    );
    assert!(st.running, "the gateway itself keeps running");
    other.stop().await;

    // A port held by a program that does not speak HTTP.
    let holder = std::net::TcpListener::bind("127.0.0.1:19540").unwrap();
    let busy = Gw::start(&bin, 19550, |s| {
        s.ollama_port_enabled = true;
        s.ollama_port = 19540;
    })
    .await;
    let st = busy.server.status().await;
    assert!(
        st.ollama_warning
            .as_deref()
            .unwrap()
            .starts_with(harness_core::gateway::WARN_OLLAMA_BUSY),
        "{st:?}"
    );
    busy.stop().await;
    drop(holder);

    // The same port as the gateway is refused by validation.
    let registry = Arc::new(ServerRegistry::default());
    let err = GatewayServer::new()
        .start(
            GatewayContext {
                registry,
                runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
                gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
                data_dir: data_root(&bin).to_path_buf(),
                downloads: None,
            },
            &GatewaySettings {
                enabled: true,
                port: 19560,
                ollama_port_enabled: true,
                ollama_port: 19560,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert_eq!(err, harness_core::gateway::ERR_OLLAMA_PORT_CONFLICT);
    gw.stop().await;
}
