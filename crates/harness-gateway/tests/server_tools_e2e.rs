//! Server-side tools (T1c) through the real gateway with a real llama-server model (no mocks).
//!
//! The gateway runs the tool loop itself for a model the user enabled it for: the client sends
//! no tool (or the hosted `web_search`, or tools of its own) and gets one finished answer.
//!
//! ```text
//! HS_LLAMA_SERVER=<llama-server>
//! HS_TEST_MODEL=<a model with native tool calls, e.g. Qwen3-4B-Instruct-2507 Q4_K_M>
//! HS_TEST_SEARXNG=<base URL of a real SearXNG with the JSON format on, for the web_search test>
//! cargo test -p harness-gateway --test server_tools_e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Temporary data root (never the user's), in-memory secrets, the gateway on 19810 and models on
//! 19850+. Everything started here is stopped here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use harness_core::analytics::ToolCallQuery;
use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::events::CoreEvent;
use harness_core::gateway::record::RequestRecord;
use harness_core::gateway::{GatewaySettings, ServerTools};
use harness_core::params::ParamValue;
use harness_core::servers::ServerRegistry;
use harness_core::tools::{SearchProviderKind, ToolPolicy};
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
            .prefix(&harness_core::paths::test_dir_prefix("servertools-e2e"))
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

/// The tool policy of the user's settings (what the gateway obeys).
fn set_policy(policy: ToolPolicy, trusted: &[&str], searxng: Option<&str>) {
    let mut s = harness_core::config::AppSettings::load();
    s.tools.policy = policy;
    s.tools.trusted = trusted.iter().map(|t| t.to_string()).collect();
    if let Some(url) = searxng {
        s.tools.web_search.provider = SearchProviderKind::Searxng;
        s.tools.web_search.searxng_url = url.to_string();
    }
    s.save().unwrap();
}

/// Defines the model; returns its definition id.
fn define(alias: &str, model: &Path, ctx: i64) -> String {
    let mut d = ModelDefinition::new(alias, ModelRef::Local { path: model.into() });
    d.load.insert("gpu_layers".into(), ParamValue::Int(99));
    d.load.insert("context_size".into(), ParamValue::Int(ctx));
    harness_core::domain::store::Domain::global()
        .save_definition(d)
        .unwrap()
        .id
}

struct Gw {
    server: GatewayServer,
    ctx: GatewayContext,
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
                ctx.clone(),
                &GatewaySettings {
                    enabled: true,
                    port: 19810,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        Gw {
            base: status.endpoint.unwrap().host_url,
            server,
            ctx,
            registry,
            http: reqwest::Client::new(),
            rx,
        }
    }

    /// Changes the server-side tools of a running gateway (limits-style change: no restart).
    async fn set_server_tools(&self, map: BTreeMap<String, ServerTools>) {
        self.server
            .start(
                self.ctx.clone(),
                &GatewaySettings {
                    enabled: true,
                    port: 19810,
                    server_tools: map,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }

    async fn stop(self) {
        self.server.stop().await;
        self.registry.stop_all().await;
    }

    async fn record(&mut self, id: &str) -> RequestRecord {
        let end = Instant::now() + Duration::from_secs(15);
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

    async fn post(&self, path: &str, body: Value, headers: &[(&str, &str)]) -> reqwest::Response {
        let mut req = self.http.post(format!("{}{}", self.base, path)).json(&body);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        req.send().await.unwrap()
    }
}

struct Reply {
    id: String,
    status: u16,
    compat: String,
    text: String,
}

async fn reply(r: reqwest::Response) -> Reply {
    let h = |n: &str| {
        r.headers()
            .get(n)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    };
    let (id, compat, status) = (h("x-harness-request-id"), h("x-harness-compat"), r.status());
    Reply {
        id,
        status: status.as_u16(),
        compat,
        text: r.text().await.unwrap(),
    }
}

fn json_of(r: &Reply) -> Value {
    serde_json::from_str(&r.text).unwrap_or_else(|e| panic!("not JSON ({e}): {}", r.text))
}

/// `(event name, data)` of an SSE body.
fn sse(text: &str) -> Vec<(String, Value)> {
    text.split("\n\n")
        .filter(|b| !b.trim().is_empty())
        .filter_map(|block| {
            let mut ev = String::new();
            let mut data = String::new();
            for line in block.lines() {
                if let Some(e) = line.strip_prefix("event: ") {
                    ev = e.to_string();
                } else if let Some(d) = line.strip_prefix("data: ") {
                    data.push_str(d);
                }
            }
            if data == "[DONE]" {
                return Some(("[DONE]".to_string(), Value::Null));
            }
            serde_json::from_str(&data).ok().map(|v| (ev, v))
        })
        .collect()
}

fn tool_rows() -> Vec<harness_core::analytics::ToolCallRow> {
    harness_core::analytics::shared()
        .unwrap()
        .tool_calls(&ToolCallQuery::default())
        .unwrap()
}

const CALC_PROMPT: &str =
    "Use the calculator tool to compute 1234 + 4321 and tell me the result in one sentence.";

fn weather_tool_chat() -> Value {
    json!({"type": "function", "function": {
        "name": "get_weather", "description": "Current weather of a city",
        "parameters": {"type": "object", "properties": {"city": {"type": "string"}},
                       "required": ["city"]}}})
}

fn enable(id: &str, web: bool, tools: &[&str]) -> BTreeMap<String, ServerTools> {
    BTreeMap::from([(
        id.to_string(),
        ServerTools {
            web_search: web,
            tools: tools.iter().map(|t| t.to_string()).collect(),
        },
    )])
}

/// Off by default, and the policy decides: nothing is run silently, nothing is skipped silently.
#[tokio::test]
#[ignore]
async fn e2e_default_off_and_policy_errors() {
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL")) else {
        return;
    };
    data_root(&bin);
    set_policy(ToolPolicy::AskEveryTime, &[], None);
    let id = define("st-off", &model, 8192);
    let gw = Gw::start(&bin).await;
    let hosted = json!({"model": "st-off", "input": "news?", "max_output_tokens": 20,
                        "tools": [{"type": "web_search"}]});

    // Default: the hosted tool is rejected as it always was.
    let r = reply(gw.post("/v1/responses", hosted.clone(), &[]).await).await;
    assert_eq!(r.status, 400, "{}", r.text);
    assert_eq!(json_of(&r)["error"]["code"], "unsupported_tool_type");
    eprintln!("default off: 400 unsupported_tool_type");

    // Enabled for tools only (web_search not allowed): still rejected.
    gw.set_server_tools(enable(&id, false, &["calculator"]))
        .await;
    let r = reply(gw.post("/v1/responses", hosted.clone(), &[]).await).await;
    assert_eq!(r.status, 400, "{}", r.text);

    // web_search allowed but the policy asks the user (restricted tool): a clear 403.
    gw.set_server_tools(enable(&id, true, &[])).await;
    let r = reply(gw.post("/v1/responses", hosted.clone(), &[]).await).await;
    assert_eq!(r.status, 403, "{}", r.text);
    let v = json_of(&r);
    assert_eq!(v["error"]["code"], "server_tool_not_allowed");
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("cannot ask"));
    assert_eq!(v["error"]["harness"]["tool"], "web_search");
    eprintln!("policy asks: {}", v["error"]["message"]);

    // Anthropic format gets the Anthropic error shape.
    let r = reply(
        gw.post(
            "/v1/messages",
            json!({"model": "st-off", "max_tokens": 20,
                   "messages": [{"role": "user", "content": "news?"}],
                   "tools": [{"type": "web_search_20250305", "name": "web_search"}]}),
            &[("anthropic-version", "2023-06-01")],
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 403);
    assert_eq!(json_of(&r)["type"], "error");

    // Policy off: refused as well.
    set_policy(ToolPolicy::Off, &[], None);
    let r = reply(gw.post("/v1/responses", hosted.clone(), &[]).await).await;
    assert_eq!(r.status, 403);
    assert!(json_of(&r)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("\"off\""));

    // A tool the user listed that Harness does not have.
    set_policy(ToolPolicy::AskEveryTime, &[], None);
    gw.set_server_tools(enable(&id, false, &["no_such_tool"]))
        .await;
    let r = reply(
        gw.post(
            "/v1/chat/completions",
            json!({"model": "st-off", "messages": [{"role": "user", "content": "hi"}]}),
            &[],
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 503, "{}", r.text);
    assert_eq!(json_of(&r)["error"]["code"], "server_tool_unavailable");

    // "Ask every time" lets safe tools run (no one has to be asked), so a safe tool works.
    gw.set_server_tools(enable(&id, false, &["calculator"]))
        .await;
    let r = reply(
        gw.post(
            "/v1/chat/completions",
            json!({"model": "st-off", "temperature": 0, "max_tokens": 200,
                   "messages": [{"role": "user", "content": CALC_PROMPT}]}),
            &[],
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
    assert!(json_of(&r)["choices"][0]["message"]["content"]
        .as_str()
        .unwrap()
        .contains("5555"));
    gw.stop().await;
}

/// The enabled tools on every wire format, streaming and not, plus the request record and the
/// tool log.
#[tokio::test]
#[ignore]
async fn e2e_enabled_tools_on_every_wire_format() {
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL")) else {
        return;
    };
    data_root(&bin);
    set_policy(ToolPolicy::AskEveryTime, &[], None);
    let id = define("st-tools", &model, 8192);
    let mut gw = Gw::start(&bin).await;
    gw.set_server_tools(enable(&id, false, &["calculator", "current_datetime"]))
        .await;
    let ua = [("user-agent", "server-tools-e2e/1.0")];

    // --- Chat Completions, JSON.
    let r = reply(
        gw.post(
            "/v1/chat/completions",
            json!({"model": "st-tools", "temperature": 0, "max_tokens": 300,
                   "messages": [{"role": "user", "content": CALC_PROMPT}]}),
            &ua,
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
    let v = json_of(&r);
    let msg = &v["choices"][0]["message"];
    eprintln!("chat json: {} | compat: {}", msg["content"], r.compat);
    assert!(msg["content"].as_str().unwrap().contains("5555"));
    assert!(
        msg.get("tool_calls").is_none(),
        "the client sees no tool call"
    );
    assert_eq!(v["choices"][0]["finish_reason"], "stop");
    assert!(v["usage"]["total_tokens"].as_u64().unwrap() > 0);
    assert_eq!(v["model"], "st-tools");
    assert!(
        r.compat
            .contains("server_tools=calculator,current_datetime"),
        "{}",
        r.compat
    );
    let rec = gw.record(&r.id).await;
    assert_eq!(rec.status, 200);
    assert!(
        rec.compat.iter().any(|c| c == "server_tool_calls=1"),
        "{:?}",
        rec.compat
    );
    assert!(
        rec.compat.iter().any(|c| c == "server_tool_rounds=2"),
        "{:?}",
        rec.compat
    );
    assert!(rec.usage.prompt_tokens.unwrap_or(0) > 0);
    assert!(rec.server_id.is_some() && rec.definition_id.as_deref() == Some(id.as_str()));
    assert_eq!(rec.client.label.as_deref(), Some("server-tools-e2e"));
    assert!(rec.tool_involved);
    assert_eq!((rec.tools_offered, rec.tool_calls), (2, 1));
    assert!(rec.tool_diag_category.is_none());

    // The tool log: the call, tied to this request.
    let rows = tool_rows();
    let row = rows
        .iter()
        .find(|t| t.conversation.as_deref() == Some(r.id.as_str()))
        .expect("the call is in the tool log");
    assert_eq!(
        (row.tool.as_str(), row.client.as_str()),
        ("calculator", "gateway")
    );
    assert_eq!(row.model.as_deref(), Some("st-tools"));
    assert!(row.error_code.is_none(), "{:?}", row.error_code);
    assert!(!row.approved, "a safe tool needs no approval");
    eprintln!("tool log: {} {} -> ok", row.client, row.tool);

    // --- Chat Completions, SSE with usage.
    let r = reply(
        gw.post(
            "/v1/chat/completions",
            json!({"model": "st-tools", "temperature": 0, "max_tokens": 300, "stream": true,
                   "stream_options": {"include_usage": true},
                   "messages": [{"role": "user", "content": CALC_PROMPT}]}),
            &ua,
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200);
    let events = sse(&r.text);
    assert_eq!(events.last().unwrap().0, "[DONE]", "{}", r.text);
    let mut content = String::new();
    let mut finish = None;
    let mut usage = None;
    for (_, e) in &events {
        if let Some(t) = e["choices"][0]["delta"]["content"].as_str() {
            content.push_str(t);
        }
        if let Some(f) = e["choices"][0]["finish_reason"].as_str() {
            finish = Some(f.to_string());
        }
        if e.get("usage").is_some() {
            usage = Some(e["usage"].clone());
        }
        assert!(e["choices"][0]["delta"].get("tool_calls").is_none());
    }
    eprintln!("chat sse: {content:?} finish={finish:?}");
    assert!(content.contains("5555"));
    assert_eq!(finish.as_deref(), Some("stop"));
    assert!(usage.unwrap()["total_tokens"].as_u64().unwrap() > 0);
    assert_eq!(events[0].1["choices"][0]["delta"]["role"], "assistant");
    let rec = gw.record(&r.id).await;
    assert!(rec.stream && rec.ttft_ms.is_some());
    assert!(rec.compat.iter().any(|c| c == "server_tool_calls=1"));

    // --- Responses, JSON.
    let r = reply(
        gw.post(
            "/v1/responses",
            json!({"model": "st-tools", "temperature": 0, "max_output_tokens": 300,
                   "instructions": "Answer briefly.",
                   "input": [{"type": "message", "role": "user",
                              "content": [{"type": "input_text", "text": CALC_PROMPT}]}]}),
            &ua,
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
    let v = json_of(&r);
    eprintln!("responses json: {}", v["output"]);
    assert_eq!(v["object"], "response");
    assert_eq!(v["status"], "completed");
    assert_eq!(v["instructions"], "Answer briefly.");
    let out = v["output"].as_array().unwrap();
    assert!(out.iter().all(|i| i["type"] != "function_call"), "{out:?}");
    let text = out.iter().find(|i| i["type"] == "message").unwrap()["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(text.contains("5555"), "{text}");
    assert!(v["usage"]["total_tokens"].as_u64().unwrap() > 0);
    let resp_id = v["id"].as_str().unwrap().to_string();

    // GET: the stateful part works for a response the gateway produced itself.
    let got = gw
        .http
        .get(format!("{}/v1/responses/{}", gw.base, resp_id))
        .send()
        .await
        .unwrap();
    assert_eq!(got.status(), 200);

    // ... and continues the conversation with previous_response_id.
    let r2 = reply(
        gw.post(
            "/v1/responses",
            json!({"model": "st-tools", "temperature": 0, "max_output_tokens": 200,
                   "previous_response_id": resp_id,
                   "input": "Now add 1 to that number using the calculator tool and say only the new number."}),
            &ua,
        )
        .await,
    )
    .await;
    assert_eq!(r2.status, 200, "{}", r2.text);
    let v2 = json_of(&r2);
    assert_eq!(v2["previous_response_id"], resp_id);
    let text2 = v2["output"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["type"] == "message")
        .unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    eprintln!("responses chained: {text2:?}");
    assert!(text2.contains("5556"), "{text2}");

    // --- Responses, SSE.
    let r = reply(
        gw.post(
            "/v1/responses",
            json!({"model": "st-tools", "temperature": 0, "max_output_tokens": 300, "stream": true,
                   "input": CALC_PROMPT}),
            &ua,
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200);
    let events = sse(&r.text);
    let kinds: Vec<&str> = events.iter().map(|(e, _)| e.as_str()).collect();
    eprintln!(
        "responses sse: {} events, first {:?}, last {:?}",
        kinds.len(),
        kinds.first(),
        kinds.last()
    );
    assert_eq!(kinds[0], "response.created");
    assert_eq!(*kinds.last().unwrap(), "response.completed");
    assert!(kinds.contains(&"response.output_text.delta"));
    let seqs: Vec<u64> = events
        .iter()
        .map(|(_, e)| e["sequence_number"].as_u64().unwrap())
        .collect();
    assert!(seqs.windows(2).all(|p| p[1] == p[0] + 1), "{seqs:?}");
    let deltas: String = events
        .iter()
        .filter(|(k, _)| k == "response.output_text.delta")
        .map(|(_, e)| e["delta"].as_str().unwrap())
        .collect();
    let done = &events.last().unwrap().1["response"];
    assert!(deltas.contains("5555"), "{deltas}");
    assert_eq!(
        done["output"].as_array().unwrap().last().unwrap()["content"][0]["text"],
        json!(deltas)
    );
    assert!(done["usage"]["total_tokens"].as_u64().unwrap() > 0);
    gw.record(&r.id).await;

    // --- Responses with `background: true`: answered at once, finished in the background.
    let r = reply(
        gw.post(
            "/v1/responses",
            json!({"model": "st-tools", "temperature": 0, "max_output_tokens": 300,
                   "background": true, "input": CALC_PROMPT}),
            &ua,
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
    let queued = json_of(&r);
    assert_eq!(queued["background"], true);
    let bg_id = queued["id"].as_str().unwrap().to_string();
    let end = Instant::now() + Duration::from_secs(90);
    let finished = loop {
        let got: Value = gw
            .http
            .get(format!("{}/v1/responses/{}", gw.base, bg_id))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if got["status"] == "completed" {
            break got;
        }
        assert!(
            Instant::now() < end,
            "background response never finished: {got}"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    let text = finished["output"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["type"] == "message")
        .unwrap()["content"][0]["text"]
        .as_str()
        .unwrap();
    eprintln!("responses background: {text:?}");
    assert!(text.contains("5555"), "{text}");

    // --- Anthropic Messages, JSON and SSE.
    let am = [
        ("anthropic-version", "2023-06-01"),
        ("user-agent", "server-tools-e2e/1.0"),
    ];
    let r = reply(
        gw.post(
            "/v1/messages",
            json!({"model": "st-tools", "temperature": 0, "max_tokens": 300,
                   "system": "Answer briefly.",
                   "messages": [{"role": "user", "content": CALC_PROMPT}]}),
            &am,
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
    let v = json_of(&r);
    eprintln!("messages json: {}", v["content"]);
    assert_eq!(v["type"], "message");
    assert_eq!(v["stop_reason"], "end_turn");
    assert!(v["content"]
        .as_array()
        .unwrap()
        .iter()
        .all(|b| b["type"] == "text"));
    assert!(v["content"][0]["text"].as_str().unwrap().contains("5555"));
    assert!(v["usage"]["output_tokens"].as_u64().unwrap() > 0);

    let r = reply(
        gw.post(
            "/v1/messages",
            json!({"model": "st-tools", "temperature": 0, "max_tokens": 300, "stream": true,
                   "messages": [{"role": "user", "content": CALC_PROMPT}]}),
            &am,
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200);
    let events = sse(&r.text);
    let kinds: Vec<&str> = events.iter().map(|(e, _)| e.as_str()).collect();
    assert_eq!(kinds[0], "message_start");
    assert_eq!(*kinds.last().unwrap(), "message_stop");
    assert!(kinds.contains(&"content_block_delta") && kinds.contains(&"message_delta"));
    let text: String = events
        .iter()
        .filter(|(k, _)| k == "content_block_delta")
        .map(|(_, e)| e["delta"]["text"].as_str().unwrap_or(""))
        .collect();
    eprintln!("messages sse: {text:?}");
    assert!(text.contains("5555"));
    gw.record(&r.id).await;
    gw.stop().await;
}

/// The client's own tools next to the server's: its calls come back to it, on every format.
#[tokio::test]
#[ignore]
async fn e2e_client_tools_come_back_to_the_client() {
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL")) else {
        return;
    };
    data_root(&bin);
    set_policy(ToolPolicy::AskEveryTime, &[], None);
    let id = define("st-mixed", &model, 8192);
    let mut gw = Gw::start(&bin).await;
    gw.set_server_tools(enable(&id, false, &["calculator"]))
        .await;
    let prompt = "What is the weather in Ankara right now? Use the get_weather tool.";

    // Chat: JSON, then the client's tool result continues the conversation.
    let r = reply(
        gw.post(
            "/v1/chat/completions",
            json!({"model": "st-mixed", "temperature": 0, "max_tokens": 200,
                   "messages": [{"role": "user", "content": prompt}],
                   "tools": [weather_tool_chat()]}),
            &[],
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
    let v = json_of(&r);
    let call = v["choices"][0]["message"]["tool_calls"][0].clone();
    eprintln!("chat json: client call {call}");
    assert_eq!(v["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(call["function"]["name"], "get_weather");
    assert!(call["function"]["arguments"]
        .as_str()
        .unwrap()
        .contains("Ankara"));
    let rec = gw.record(&r.id).await;
    assert!(
        rec.compat.iter().any(|c| c == "server_tool_calls=0"),
        "{:?}",
        rec.compat
    );

    let r = reply(
        gw.post(
            "/v1/chat/completions",
            json!({"model": "st-mixed", "temperature": 0, "max_tokens": 200,
                   "messages": [
                       {"role": "user", "content": prompt},
                       {"role": "assistant", "content": null, "tool_calls": [call.clone()]},
                       {"role": "tool", "tool_call_id": call["id"], "content": "12 degrees, cloudy"}],
                   "tools": [weather_tool_chat()]}),
            &[],
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
    let v = json_of(&r);
    let text = v["choices"][0]["message"]["content"].as_str().unwrap();
    eprintln!("chat json: after the tool result: {text:?}");
    assert!(text.contains("12"), "{text}");

    // Chat SSE: the client call arrives as tool_calls deltas.
    let r = reply(
        gw.post(
            "/v1/chat/completions",
            json!({"model": "st-mixed", "temperature": 0, "max_tokens": 200, "stream": true,
                   "messages": [{"role": "user", "content": prompt}],
                   "tools": [weather_tool_chat()]}),
            &[],
        )
        .await,
    )
    .await;
    let events = sse(&r.text);
    let calls: Vec<&Value> = events
        .iter()
        .filter_map(|(_, e)| e["choices"][0]["delta"].get("tool_calls"))
        .collect();
    assert_eq!(calls.len(), 1, "{}", r.text);
    assert_eq!(calls[0][0]["function"]["name"], "get_weather");
    let finish = events.iter().find_map(|(_, e)| {
        e["choices"][0]["finish_reason"]
            .as_str()
            .map(str::to_string)
    });
    assert_eq!(finish.as_deref(), Some("tool_calls"));

    // Responses (Codex-style function tool), JSON and SSE.
    let tool = json!({"type": "function", "name": "get_weather",
                      "description": "Current weather of a city",
                      "parameters": {"type": "object", "properties": {"city": {"type": "string"}},
                                     "required": ["city"]}});
    let r = reply(
        gw.post(
            "/v1/responses",
            json!({"model": "st-mixed", "temperature": 0, "max_output_tokens": 200,
                   "input": prompt, "tools": [tool.clone()]}),
            &[],
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
    let v = json_of(&r);
    let item = v["output"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["type"] == "function_call")
        .unwrap_or_else(|| panic!("{}", r.text))
        .clone();
    eprintln!("responses json: function_call {item}");
    assert_eq!(item["name"], "get_weather");
    assert!(item["call_id"].as_str().is_some_and(|c| !c.is_empty()));
    let call_id = item["call_id"].clone();

    // The client answers with a function_call_output; the gateway continues.
    let r = reply(
        gw.post(
            "/v1/responses",
            json!({"model": "st-mixed", "temperature": 0, "max_output_tokens": 200,
                   "input": [
                       {"type": "message", "role": "user", "content": [{"type": "input_text", "text": prompt}]},
                       {"type": "function_call", "call_id": call_id, "name": "get_weather",
                        "arguments": item["arguments"]},
                       {"type": "function_call_output", "call_id": call_id, "output": "12 degrees, cloudy"}],
                   "tools": [tool.clone()]}),
            &[],
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
    let text = json_of(&r)["output"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["type"] == "message")
        .unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    eprintln!("responses json: after the output: {text:?}");
    assert!(text.contains("12"), "{text}");

    let r = reply(
        gw.post(
            "/v1/responses",
            json!({"model": "st-mixed", "temperature": 0, "max_output_tokens": 200, "stream": true,
                   "input": prompt, "tools": [tool]}),
            &[],
        )
        .await,
    )
    .await;
    let events = sse(&r.text);
    let kinds: Vec<&str> = events.iter().map(|(e, _)| e.as_str()).collect();
    assert!(
        kinds.contains(&"response.function_call_arguments.done"),
        "{kinds:?}"
    );
    assert_eq!(*kinds.last().unwrap(), "response.completed");
    let done = &events.last().unwrap().1["response"];
    assert!(done["output"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["type"] == "function_call" && i["name"] == "get_weather"));

    // Anthropic: tool_use block and stop_reason tool_use, JSON and SSE.
    let am = [("anthropic-version", "2023-06-01")];
    let tool = json!({"name": "get_weather", "description": "Current weather of a city",
                      "input_schema": {"type": "object",
                          "properties": {"city": {"type": "string"}}, "required": ["city"]}});
    let r = reply(
        gw.post(
            "/v1/messages",
            json!({"model": "st-mixed", "temperature": 0, "max_tokens": 200,
                   "messages": [{"role": "user", "content": prompt}], "tools": [tool.clone()]}),
            &am,
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
    let v = json_of(&r);
    assert_eq!(v["stop_reason"], "tool_use");
    let block = v["content"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["type"] == "tool_use")
        .unwrap()
        .clone();
    eprintln!("messages json: tool_use {block}");
    assert_eq!(block["name"], "get_weather");
    assert_eq!(block["input"]["city"], "Ankara");

    let r = reply(
        gw.post(
            "/v1/messages",
            json!({"model": "st-mixed", "temperature": 0, "max_tokens": 200,
                   "messages": [
                       {"role": "user", "content": prompt},
                       {"role": "assistant", "content": [block.clone()]},
                       {"role": "user", "content": [{"type": "tool_result", "tool_use_id": block["id"],
                                                     "content": "12 degrees, cloudy"}]}],
                   "tools": [tool.clone()]}),
            &am,
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
    let text = json_of(&r)["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    eprintln!("messages json: after the tool_result: {text:?}");
    assert!(text.contains("12"), "{text}");

    let r = reply(
        gw.post(
            "/v1/messages",
            json!({"model": "st-mixed", "temperature": 0, "max_tokens": 200, "stream": true,
                   "messages": [{"role": "user", "content": prompt}], "tools": [tool]}),
            &am,
        )
        .await,
    )
    .await;
    let events = sse(&r.text);
    let kinds: Vec<&str> = events.iter().map(|(e, _)| e.as_str()).collect();
    assert_eq!(*kinds.last().unwrap(), "message_stop");
    let delta = events.iter().find(|(k, _)| k == "message_delta").unwrap();
    assert_eq!(delta.1["delta"]["stop_reason"], "tool_use");
    let partial: String = events
        .iter()
        .filter_map(|(_, e)| e["delta"]["partial_json"].as_str())
        .collect();
    assert!(partial.contains("Ankara"), "{partial}");
    gw.record(&r.id).await;
    gw.stop().await;
}

/// Codex's hosted `web_search`, fulfilled with a real search provider (a real SearXNG).
#[tokio::test]
#[ignore]
async fn e2e_hosted_web_search_is_fulfilled() {
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL")) else {
        return;
    };
    let Ok(searxng) = std::env::var("HS_TEST_SEARXNG") else {
        eprintln!("HS_TEST_SEARXNG not set, skipping");
        return;
    };
    data_root(&bin);
    set_policy(ToolPolicy::AllowTrusted, &["web_search"], Some(&searxng));
    let id = define("st-web", &model, 8192);
    let mut gw = Gw::start(&bin).await;
    gw.set_server_tools(enable(&id, true, &[])).await;
    let prompt = "Search the web for \"SearXNG metasearch engine\" and tell me in one sentence \
                  what the first result is about.";
    let shell = json!({"type": "function", "name": "shell",
                       "description": "Run a shell command",
                       "parameters": {"type": "object", "properties": {"command": {"type": "string"}}}});

    // Responses, as Codex sends it (hosted tool next to its own function tool), streaming.
    let r = reply(
        gw.post(
            "/v1/responses",
            json!({"model": "st-web", "temperature": 0, "max_output_tokens": 400, "stream": true,
                   "input": [{"type": "message", "role": "user",
                              "content": [{"type": "input_text", "text": prompt}]}],
                   "tools": [shell.clone(), {"type": "web_search", "external_web_access": true}]}),
            &[("user-agent", "codex_cli_rs/0.1")],
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
    eprintln!("compat: {}", r.compat);
    assert!(r.compat.contains("server_tools=web_search"), "{}", r.compat);
    assert!(
        r.compat.contains("hosted_fulfilled=web_search"),
        "{}",
        r.compat
    );
    let events = sse(&r.text);
    let kinds: Vec<&str> = events.iter().map(|(e, _)| e.as_str()).collect();
    assert!(
        kinds.contains(&"response.web_search_call.searching"),
        "{kinds:?}"
    );
    let done = &events.last().unwrap().1["response"];
    let types: Vec<&str> = done["output"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["type"].as_str().unwrap())
        .collect();
    eprintln!("output items: {types:?}");
    assert_eq!(types.first(), Some(&"web_search_call"));
    assert_eq!(types.last(), Some(&"message"));
    let query = done["output"][0]["action"]["query"].as_str().unwrap();
    eprintln!("search query: {query:?}");
    assert!(!query.is_empty());
    let answer = done["output"].as_array().unwrap().last().unwrap()["content"][0]["text"]
        .as_str()
        .unwrap();
    eprintln!("answer: {answer:?}");
    assert!(!answer.trim().is_empty());
    let rec = gw.record(&r.id).await;
    assert!(rec
        .compat
        .iter()
        .any(|c| c.starts_with("server_tool_calls=")));
    let row = tool_rows()
        .into_iter()
        .find(|t| t.conversation.as_deref() == Some(r.id.as_str()) && t.tool == "web_search")
        .expect("web_search is in the tool log");
    eprintln!(
        "tool log: {} {} target={:?} error={:?}",
        row.client, row.tool, row.network_target, row.error_code
    );
    assert!(row.error_code.is_none(), "{:?}", row.error_code);
    assert!(row.network_target.is_some());

    // The same through Chat (`web_search_options`) and Anthropic (`web_search_20250305`).
    let r = reply(
        gw.post(
            "/v1/chat/completions",
            json!({"model": "st-web", "temperature": 0, "max_tokens": 400,
                   "web_search_options": {"search_context_size": "low"},
                   "messages": [{"role": "user", "content": prompt}]}),
            &[],
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
    let text = json_of(&r)["choices"][0]["message"]["content"]
        .as_str()
        .unwrap()
        .to_string();
    eprintln!("chat web_search_options: {text:?}");
    assert!(!text.trim().is_empty());
    assert!(r.compat.contains("hosted_fulfilled=web_search"));

    let r = reply(
        gw.post(
            "/v1/messages",
            json!({"model": "st-web", "temperature": 0, "max_tokens": 400,
                   "tools": [{"type": "web_search_20250305", "name": "web_search", "max_uses": 2}],
                   "messages": [{"role": "user", "content": prompt}]}),
            &[("anthropic-version", "2023-06-01")],
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
    let v = json_of(&r);
    eprintln!("anthropic web_search: {}", v["content"]);
    assert_eq!(v["usage"]["server_tool_use"]["web_search_requests"], 1);
    gw.stop().await;
}

/// The real Codex CLI (`codex exec --search`) against the gateway: Codex sends its hosted
/// `web_search` tool, Harness fulfils it with the real SearXNG, Codex prints the answer.
/// Needs `HS_TEST_CODEX` (the codex executable) and `HS_TEST_SEARXNG`.
#[tokio::test]
#[ignore]
async fn e2e_real_codex_cli_with_hosted_web_search() {
    let (Some(bin), Some(model), Some(codex)) = (
        env_path("HS_LLAMA_SERVER"),
        env_path("HS_TEST_MODEL"),
        env_path("HS_TEST_CODEX"),
    ) else {
        return;
    };
    let Ok(searxng) = std::env::var("HS_TEST_SEARXNG") else {
        eprintln!("HS_TEST_SEARXNG not set, skipping");
        return;
    };
    data_root(&bin);
    set_policy(ToolPolicy::AllowTrusted, &["web_search"], Some(&searxng));
    // Codex's own system prompt and tool list are long: a roomy context.
    let id = define("st-codex", &model, 32768);
    let gw = Gw::start(&bin).await;
    gw.set_server_tools(enable(&id, true, &[])).await;

    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let last = work.path().join("last.txt");
    let prompt = "Search the web for \"SearXNG metasearch engine\" and tell me in one sentence \
                  what the first result is about.";
    let out = tokio::process::Command::new(&codex)
        .env("CODEX_HOME", home.path())
        // The `-c` arguments `harness launch codex` passes, with `--search` instead of
        // `web_search=disabled` (the gateway fulfils the hosted tool for this model). Straight to
        // the gateway: Codex's `include: ["reasoning.encrypted_content"]` is accepted.
        .args([
            "-c",
            "model_provider=harness",
            "-c",
            "model_providers.harness.name=Harness Studio",
            "-c",
        ])
        .arg(format!("model_providers.harness.base_url={}/v1", gw.base))
        .args([
            "-c",
            "model_providers.harness.wire_api=responses",
            "-c",
            "model_context_window=32768",
            "-m",
            "st-codex",
            "--search",
            "exec",
            "--skip-git-repo-check",
            "--ephemeral",
            "--json",
            "-C",
        ])
        .arg(work.path())
        .arg("-o")
        .arg(&last)
        .arg(prompt)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    eprintln!(
        "codex exit: {:?}\n--- stdout ---\n{stdout}\n--- stderr (tail) ---\n{}",
        out.status.code(),
        stderr
            .chars()
            .rev()
            .take(1500)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
    );
    let answer = std::fs::read_to_string(&last).unwrap_or_default();
    eprintln!("codex final message: {answer:?}");
    assert!(out.status.success());
    assert!(answer.to_lowercase().contains("search"), "{answer}");
    // The gateway fulfilled the search and Codex saw it as its own web_search item.
    assert!(stdout.contains("web_search"), "{stdout}");
    let rows = tool_rows();
    assert!(
        rows.iter()
            .any(|t| t.tool == "web_search" && t.client == "gateway" && t.error_code.is_none()),
        "{rows:?}"
    );
    gw.stop().await;
}

/// An MCP server's tool as a server-side tool: Harness's own MCP client connects the real
/// `@modelcontextprotocol/server-everything` (needs Node and the network for `npx`), the
/// gateway offers its `get-sum` tool and runs the call.
#[tokio::test]
#[ignore = "needs Node and the network (npx)"]
async fn e2e_mcp_tool_runs_server_side() {
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL")) else {
        return;
    };
    data_root(&bin);
    let mut cfg = harness_core::mcp::McpConfig::default();
    cfg.mcp_servers.insert(
        "everything".into(),
        harness_core::mcp::McpServerDef {
            command: Some("npx".into()),
            args: vec![
                "-y".into(),
                "@modelcontextprotocol/server-everything".into(),
            ],
            ..Default::default()
        },
    );
    harness_core::mcp::save(&cfg).unwrap();
    // `get-sum` has a read-only hint: a restricted tool, so the user trusts it by name.
    set_policy(ToolPolicy::AllowTrusted, &["everything_get-sum"], None);
    let id = define("st-mcp", &model, 8192);
    // The first `npx` run may download the package: connect once up front so the request
    // below does not depend on the network speed.
    let service = harness_core::tools::ToolService::shared();
    service.sync_mcp(true).await;
    assert!(
        service.registry().get("everything_get-sum").is_some(),
        "{:?}",
        service.mcp().status()
    );
    let mut gw = Gw::start(&bin).await;
    gw.set_server_tools(enable(&id, false, &["everything_get-sum"]))
        .await;
    let r = reply(
        gw.post(
            "/v1/chat/completions",
            json!({"model": "st-mcp", "temperature": 0, "max_tokens": 300,
                   "messages": [{"role": "user", "content":
                       "Use the get-sum tool to add 1234 and 4321 and tell me the result."}]}),
            &[],
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
    let text = json_of(&r)["choices"][0]["message"]["content"]
        .as_str()
        .unwrap()
        .to_string();
    eprintln!("mcp server-side: {text:?} | compat: {}", r.compat);
    assert!(text.contains("5555"), "{text}");
    let row = tool_rows()
        .into_iter()
        .find(|t| t.conversation.as_deref() == Some(r.id.as_str()))
        .expect("the MCP call is in the tool log");
    assert_eq!(
        (row.tool.as_str(), row.source.as_str()),
        ("everything_get-sum", "mcp")
    );
    assert!(row.error_code.is_none());
    gw.record(&r.id).await;
    // Not trusted any more: a clear error, never a silent skip.
    set_policy(ToolPolicy::AllowTrusted, &[], None);
    let r = reply(
        gw.post(
            "/v1/chat/completions",
            json!({"model": "st-mcp", "messages": [{"role": "user", "content": "hi"}]}),
            &[],
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.text);
    gw.stop().await;
    // Harness's MCP client closes its servers with the process; leave none behind now.
    harness_core::tools::ToolService::shared()
        .mcp()
        .shutdown(Duration::from_secs(3))
        .await;
}

/// The client goes away in the middle of a stream: the loop stops, the record says so and the
/// model's slot is free again.
#[tokio::test]
#[ignore]
async fn e2e_client_disconnect_cancels_the_loop() {
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL")) else {
        return;
    };
    data_root(&bin);
    set_policy(ToolPolicy::AskEveryTime, &[], None);
    let id = define("st-cancel", &model, 8192);
    let mut gw = Gw::start(&bin).await;
    gw.set_server_tools(enable(&id, false, &["current_datetime"]))
        .await;
    let resp = gw
        .post(
            "/v1/chat/completions",
            json!({"model": "st-cancel", "temperature": 0.7, "max_tokens": 1500, "stream": true,
                   "messages": [{"role": "user", "content":
                       "Write a very long story about a lighthouse keeper, at least 800 words."}]}),
            &[],
        )
        .await;
    assert_eq!(resp.status(), 200);
    let rid = resp
        .headers()
        .get("x-harness-request-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let mut body = resp.bytes_stream();
    use futures_util::StreamExt;
    let mut got = 0;
    while got < 3 {
        let chunk = body.next().await.expect("stream ended early").unwrap();
        got += String::from_utf8_lossy(&chunk)
            .matches("\"content\":\"")
            .count();
    }
    drop(body);
    let rec = gw.record(&rid).await;
    eprintln!(
        "record after the disconnect: status={} cancelled={}",
        rec.status, rec.cancelled
    );
    assert!(rec.cancelled);
    // The slot is free: the next request answers promptly.
    let started = Instant::now();
    let r = reply(
        gw.post(
            "/v1/chat/completions",
            json!({"model": "st-cancel", "temperature": 0, "max_tokens": 20,
                   "messages": [{"role": "user", "content": "Say hi."}]}),
            &[],
        )
        .await,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
    assert!(started.elapsed() < Duration::from_secs(30));
    gw.stop().await;
}
