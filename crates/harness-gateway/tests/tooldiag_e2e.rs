//! Tool-call diagnosis through the real gateway with real llama-server models (no mocks).
//!
//! Each test sends real tool requests through the gateway and reads the diagnosis the request
//! record carries (what the analytics store and the request inspector show).
//!
//! ```text
//! HS_LLAMA_SERVER=<llama-server>
//! HS_TEST_MODEL=<a model with native tool calls, e.g. Qwen3-4B-Instruct-2507 Q4_K_M>
//! HS_TEST_TEXTCALL_MODEL=<a model that writes calls as text, e.g. Qwen2.5-Coder-14B Q4_K_M>
//! HS_TEST_NOTOOLS_MODEL=<a model whose template has no tools, e.g. gemma-3-4b-it Q4_K_M>
//! cargo test -p harness-gateway --test tooldiag_e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The last two are optional (their tests are skipped when unset). Temporary data root (never
//! the user's), in-memory secrets, the gateway on 19700 and models on 19750+. Everything started
//! here is stopped here.

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
            .prefix(&harness_core::paths::test_dir_prefix("tooldiag-e2e"))
            .tempdir()
            .unwrap()
            .keep();
        harness_core::paths::init(dir.clone());
        let mut s = harness_core::config::AppSettings::load();
        s.runtime_mode = harness_core::config::RuntimeMode::Custom;
        s.custom_binary.binary_path = Some(bin.to_path_buf());
        s.custom_binary.is_valid = true;
        s.server.port = 19750;
        s.control_api.port = 19799;
        s.save().unwrap();
        dir
    })
}

fn define(alias: &str, model: &Path, ctx: i64) {
    let mut d = ModelDefinition::new(alias, ModelRef::Local { path: model.into() });
    d.load.insert("gpu_layers".into(), ParamValue::Int(99));
    d.load.insert("context_size".into(), ParamValue::Int(ctx));
    harness_core::domain::store::Domain::global()
        .save_definition(d)
        .unwrap();
}

struct Gw {
    server: GatewayServer,
    registry: Arc<ServerRegistry>,
    base: String,
    http: reqwest::Client,
    rx: tokio::sync::broadcast::Receiver<CoreEvent>,
}

impl Gw {
    async fn start(bin: &Path) -> Self {
        let registry = Arc::new(ServerRegistry::default());
        let ctx = GatewayContext {
            registry: registry.clone(),
            runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
            gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
            data_dir: data_root(bin).to_path_buf(),
            downloads: None,
        };
        let rx = harness_core::events::subscribe();
        let server = GatewayServer::new();
        let status = server
            .start(
                ctx,
                &GatewaySettings {
                    enabled: true,
                    port: 19700,
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
            rx,
        }
    }

    async fn stop(self) {
        self.server.stop().await;
        self.registry.stop_all().await;
    }

    /// The record of the request with this id (waits for it: a record ends with its response).
    async fn record(&mut self, id: &str) -> RequestRecord {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            while let Ok(ev) = self.rx.try_recv() {
                if let CoreEvent::GatewayRequest { record } = ev {
                    if record.id == id {
                        return *record;
                    }
                }
            }
            assert!(Instant::now() < end, "no record for {id}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Sends a request, returns (request id, body text). Streams are read to the end.
    async fn send(&self, path: &str, body: Value, headers: &[(&str, &str)]) -> (String, String) {
        let mut req = self.http.post(format!("{}{}", self.base, path)).json(&body);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req.send().await.unwrap();
        assert_eq!(resp.status(), 200, "{path}");
        let id = resp
            .headers()
            .get("x-harness-request-id")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        (id, resp.text().await.unwrap())
    }
}

fn weather_tool() -> Value {
    json!({"type": "function", "function": {
        "name": "get_weather", "description": "Current weather of a city",
        "parameters": {"type": "object", "properties": {
            "city": {"type": "string"},
            "unit": {"type": "string", "enum": ["celsius", "fahrenheit"]}}, "required": ["city"]}}})
}

fn detail(r: &RequestRecord) -> Value {
    serde_json::from_str(r.tool_diag_detail.as_deref().unwrap_or("null")).unwrap_or(Value::Null)
}

/// A model with native tool calls: clean calls in all three wire formats, then the client's
/// error result linked to the call, then a call cut off by `max_tokens`.
#[tokio::test]
#[ignore]
async fn e2e_native_calls_client_error_and_truncation() {
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL")) else {
        return;
    };
    data_root(&bin);
    define("td-native", &model, 8192);
    let mut gw = Gw::start(&bin).await;
    let prompt = json!([{"role": "user", "content": "What is the weather in Ankara?"}]);

    // Chat Completions, JSON.
    let (id1, text) = gw
        .send(
            "/v1/chat/completions",
            json!({"model": "td-native", "temperature": 0, "max_tokens": 200,
                   "messages": prompt, "tools": [weather_tool()]}),
            &[],
        )
        .await;
    let body: Value = serde_json::from_str(&text).unwrap();
    let call = body["choices"][0]["message"]["tool_calls"][0].clone();
    assert_eq!(call["function"]["name"], "get_weather");
    let r = gw.record(&id1).await;
    assert!(r.tool_involved);
    assert_eq!((r.tools_offered, r.tool_calls), (1, 1));
    assert_eq!(r.tool_diag_category, None, "{:?}", r.tool_diag_detail);
    eprintln!("chat json: clean call, {} tool(s) offered", r.tools_offered);

    // Chat Completions, streaming.
    let (id, _) = gw
        .send(
            "/v1/chat/completions",
            json!({"model": "td-native", "temperature": 0, "max_tokens": 200, "stream": true,
                   "messages": prompt, "tools": [weather_tool()]}),
            &[],
        )
        .await;
    let r = gw.record(&id).await;
    assert_eq!((r.tools_offered, r.tool_calls), (1, 1));
    assert_eq!(r.tool_diag_category, None, "{:?}", r.tool_diag_detail);
    eprintln!("chat stream: clean call");

    // Anthropic Messages, streaming.
    let (id, _) = gw
        .send(
            "/v1/messages",
            json!({"model": "td-native", "temperature": 0, "max_tokens": 200, "stream": true,
                   "tools": [{"name": "get_weather", "description": "Current weather of a city",
                              "input_schema": {"type": "object",
                                  "properties": {"city": {"type": "string"}}, "required": ["city"]}}],
                   "messages": prompt}),
            &[("anthropic-version", "2023-06-01")],
        )
        .await;
    let r = gw.record(&id).await;
    assert_eq!((r.tools_offered, r.tool_calls), (1, 1));
    assert_eq!(r.tool_diag_category, None, "{:?}", r.tool_diag_detail);
    eprintln!("anthropic stream: clean call");

    // Responses, JSON.
    let (id, _) = gw
        .send(
            "/v1/responses",
            json!({"model": "td-native", "temperature": 0, "max_output_tokens": 200,
                   "input": "What is the weather in Ankara?",
                   "tools": [{"type": "function", "name": "get_weather",
                              "description": "Current weather of a city",
                              "parameters": {"type": "object",
                                  "properties": {"city": {"type": "string"}}, "required": ["city"]}}]}),
            &[],
        )
        .await;
    let r = gw.record(&id).await;
    assert_eq!((r.tools_offered, r.tool_calls), (1, 1));
    assert_eq!(r.tool_diag_category, None, "{:?}", r.tool_diag_detail);
    eprintln!("responses json: clean call");

    // The client's next request: the tool failed. The error is the client's, linked to round 1.
    let (id2, _) = gw
        .send(
            "/v1/chat/completions",
            json!({"model": "td-native", "temperature": 0, "max_tokens": 100,
                   "messages": [prompt[0].clone(),
                       {"role": "assistant", "content": "", "tool_calls": [call.clone()]},
                       {"role": "tool", "tool_call_id": call["id"],
                        "content": "Error: city not found: Ankara"}],
                   "tools": [weather_tool()]}),
            &[],
        )
        .await;
    let r = gw.record(&id2).await;
    assert_eq!(r.tool_diag_category.as_deref(), Some("client"));
    assert_eq!(r.tool_diag_code.as_deref(), Some("tool_result_error"));
    let d = detail(&r);
    assert_eq!(d["tool"], "get_weather");
    assert_eq!(d["linked_request"], id1.as_str());
    eprintln!("client error: {d}");

    // The output limit cuts the call off (a real model response, not a staged one).
    let (id3, text) = gw
        .send(
            "/v1/chat/completions",
            json!({"model": "td-native", "temperature": 0, "max_tokens": 8,
                   "messages": prompt, "tools": [weather_tool()]}),
            &[],
        )
        .await;
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["choices"][0]["finish_reason"], "length");
    let r = gw.record(&id3).await;
    assert_eq!(r.tool_diag_category.as_deref(), Some("context"));
    assert_eq!(r.tool_diag_code.as_deref(), Some("output_truncated"));
    eprintln!("truncated: {}", detail(&r));

    // A request without tools is not a tool round at all.
    let (id4, _) = gw
        .send(
            "/v1/chat/completions",
            json!({"model": "td-native", "max_tokens": 8,
                   "messages": [{"role": "user", "content": "Say hi."}]}),
            &[],
        )
        .await;
    let r = gw.record(&id4).await;
    assert!(!r.tool_involved);
    assert_eq!((r.tools_offered, r.tool_calls), (0, 0));
    gw.stop().await;
}

/// A model that writes its call as text (`<tools>{...}</tools>`): llama-server returns it as
/// content, and the gateway says so, in JSON and in a stream.
#[tokio::test]
#[ignore]
async fn e2e_call_written_as_text() {
    let (Some(bin), Some(model)) = (
        env_path("HS_LLAMA_SERVER"),
        env_path("HS_TEST_TEXTCALL_MODEL"),
    ) else {
        return;
    };
    data_root(&bin);
    define("td-text", &model, 8192);
    let mut gw = Gw::start(&bin).await;
    for stream in [false, true] {
        let (id, _) = gw
            .send(
                "/v1/chat/completions",
                json!({"model": "td-text", "temperature": 0, "max_tokens": 200, "stream": stream,
                       "messages": [{"role": "user", "content": "What is the weather in Ankara?"}],
                       "tools": [weather_tool()]}),
                &[],
            )
            .await;
        let r = gw.record(&id).await;
        assert_eq!(r.tool_calls, 0);
        assert_eq!(
            r.tool_diag_category.as_deref(),
            Some("runtime"),
            "stream={stream}"
        );
        assert_eq!(r.tool_diag_code.as_deref(), Some("call_left_in_content"));
        eprintln!("stream={stream}: {}", detail(&r));
    }
    gw.stop().await;
}

/// A model whose chat template has no tool handling: the gateway reads
/// `chat_template_caps.supports_tools = false` from `/props` and names the template.
#[tokio::test]
#[ignore]
async fn e2e_template_without_tools() {
    let (Some(bin), Some(model)) = (
        env_path("HS_LLAMA_SERVER"),
        env_path("HS_TEST_NOTOOLS_MODEL"),
    ) else {
        return;
    };
    data_root(&bin);
    define("td-notools", &model, 8192);
    let mut gw = Gw::start(&bin).await;
    let (id, _) = gw
        .send(
            "/v1/chat/completions",
            json!({"model": "td-notools", "temperature": 0, "max_tokens": 60,
                   "messages": [{"role": "user", "content": "What is the weather in Ankara?"}],
                   "tools": [weather_tool()]}),
            &[],
        )
        .await;
    let r = gw.record(&id).await;
    assert_eq!(r.tool_calls, 0);
    assert_eq!(r.tool_diag_category.as_deref(), Some("runtime"));
    assert_eq!(r.tool_diag_code.as_deref(), Some("template_no_tools"));
    eprintln!("{}", detail(&r));
    gw.stop().await;
}
