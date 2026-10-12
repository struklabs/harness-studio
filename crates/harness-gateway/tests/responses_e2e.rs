//! Stateful Responses (`store`, `previous_response_id`, `GET`/`DELETE /v1/responses/{id}`,
//! `input_items`) end to end with a real llama-server and a real model (no mocks):
//!
//! ```text
//! HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<chat gguf>
//! cargo test -p harness-gateway --test responses_e2e -- --ignored --nocapture
//! ```
//!
//! The llama-server directory is copied to a temporary folder first (the binary is never run
//! in place). Temporary data root, in-memory secrets, the gateway on a free port, the model on
//! 19150. When Python with the official `openai` package is available, the same flow is also
//! driven by the OpenAI Python SDK. Everything started here is stopped here.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_util::StreamExt;
use harness_core::control_api::{Scope, TokenPolicy, TokenStore};
use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::params::ParamValue;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

const ALIAS: &str = "responses-e2e-qwen";

fn env_path(name: &str) -> Option<PathBuf> {
    let p = std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_file());
    if p.is_none() {
        eprintln!("{name} not set, skipping");
    }
    p
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Copies the llama-server folder (binary and its DLLs) into `dest`; returns the copy.
fn copy_runtime(bin: &Path, dest: &Path) -> PathBuf {
    let src = bin.parent().unwrap();
    std::fs::create_dir_all(dest).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            std::fs::copy(entry.path(), dest.join(entry.file_name())).unwrap();
        }
    }
    dest.join(bin.file_name().unwrap())
}

fn data_root(bin: &Path) -> PathBuf {
    harness_core::secrets::use_memory_store();
    let dir = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("responses-e2e"))
        .tempdir()
        .unwrap()
        .keep();
    let bin = copy_runtime(bin, &dir.join("runtime"));
    harness_core::paths::init(dir.clone());
    let mut s = harness_core::config::AppSettings::load();
    s.runtime_mode = harness_core::config::RuntimeMode::Custom;
    s.custom_binary.binary_path = Some(bin);
    s.custom_binary.is_valid = true;
    s.server.port = 19150;
    s.control_api.port = free_port();
    s.save().unwrap();
    dir
}

struct Resp {
    status: u16,
    body: Value,
}

struct Client {
    http: reqwest::Client,
    base: String,
}

fn drain_events(buffer: &mut String) -> Vec<Value> {
    let mut events = Vec::new();
    while let Some(pos) = buffer.find("\n\n") {
        let event: String = buffer.drain(..pos + 2).collect();
        for line in event.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(value) = serde_json::from_str(data) {
                    events.push(value);
                }
            }
        }
    }
    events
}

async fn collect_sse(response: reqwest::Response) -> Vec<Value> {
    assert_eq!(response.status().as_u16(), 200);
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut events = Vec::new();
    while let Some(chunk) = stream.next().await {
        buffer.push_str(&String::from_utf8_lossy(&chunk.unwrap()).replace('\r', ""));
        events.extend(drain_events(&mut buffer));
    }
    events
}

impl Client {
    async fn send(&self, req: reqwest::RequestBuilder, token: &str) -> Resp {
        let r = req.bearer_auth(token).send().await.unwrap();
        let status = r.status().as_u16();
        let text = r.text().await.unwrap();
        Resp {
            status,
            body: serde_json::from_str(&text).unwrap_or(Value::String(text)),
        }
    }

    async fn create(&self, token: &str, body: Value) -> Resp {
        let req = self
            .http
            .post(format!("{}/v1/responses", self.base))
            .json(&body);
        self.send(req, token).await
    }

    async fn get(&self, token: &str, path: &str) -> Resp {
        let req = self.http.get(format!("{}{}", self.base, path));
        self.send(req, token).await
    }

    async fn post(&self, token: &str, path: &str) -> Resp {
        let req = self.http.post(format!("{}{}", self.base, path));
        self.send(req, token).await
    }

    async fn delete(&self, token: &str, path: &str) -> Resp {
        let req = self.http.delete(format!("{}{}", self.base, path));
        self.send(req, token).await
    }

    /// A streamed response: every event in order and the final response object.
    async fn stream(&self, token: &str, mut body: Value) -> (Vec<String>, Value) {
        body["stream"] = json!(true);
        let r = self
            .http
            .post(format!("{}/v1/responses", self.base))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 200);
        let mut stream = r.bytes_stream();
        let mut buf = String::new();
        let mut kinds = Vec::new();
        let mut done = None;
        while let Some(chunk) = stream.next().await {
            buf.push_str(&String::from_utf8_lossy(&chunk.unwrap()).replace('\r', ""));
            for v in drain_events(&mut buf) {
                let kind = v["type"].as_str().unwrap_or_default().to_string();
                if kind == "response.completed" {
                    done = Some(v["response"].clone());
                }
                kinds.push(kind);
            }
        }
        (kinds, done.expect("response.completed"))
    }
}

/// Polls a background response until its status is final (at most 2 minutes).
async fn poll_final(c: &Client, token: &str, id: &str) -> Value {
    for _ in 0..240 {
        let r = c.get(token, &format!("/v1/responses/{id}")).await;
        assert_eq!(r.status, 200, "{}", r.body);
        if !matches!(r.body["status"].as_str(), Some("queued" | "in_progress")) {
            return r.body;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    panic!("background response {id} did not finish");
}

fn text_of(response: &Value) -> String {
    response["output"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|i| i["type"] == "message")
        .flat_map(|i| i["content"].as_array().cloned().unwrap_or_default())
        .filter_map(|c| c["text"].as_str().map(str::to_string))
        .collect::<Vec<_>>()
        .join("")
}

fn token(name: &str) -> String {
    TokenStore::at(&harness_core::paths::config_dir())
        .create_with_policy(name, &[Scope::Inference], &TokenPolicy::default())
        .unwrap()
        .token
}

#[tokio::test]
#[ignore]
async fn e2e_stateful_responses_remember_previous_turns() {
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL")) else {
        return;
    };
    let dir = data_root(&bin);
    let mut def = ModelDefinition::new(ALIAS, ModelRef::Local { path: model });
    def.load.insert("gpu_layers".into(), ParamValue::Int(99));
    def.load
        .insert("context_size".into(), ParamValue::Int(8192));
    harness_core::domain::store::Domain::global()
        .save_definition(def)
        .unwrap();

    let registry = Arc::new(ServerRegistry::default());
    let server = GatewayServer::new();
    let status = server
        .start(
            GatewayContext {
                registry: registry.clone(),
                runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
                gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
                data_dir: dir.clone(),
                downloads: None,
            },
            &GatewaySettings {
                enabled: true,
                port: free_port(),
                require_token: true,
                responses: harness_core::gateway::responses::ResponsesSettings {
                    compaction: harness_core::gateway::responses::CompactionSettings {
                        keep_links: 1,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let c = Client {
        http: reqwest::Client::new(),
        base: status.endpoint.unwrap().host_url,
    };
    let alice = token("e2e-alice");
    let bob = token("e2e-bob");

    // --- Non-streaming: a fact, then a question about it through previous_response_id. ---
    let r1 = c
        .create(
            &alice,
            json!({"model": ALIAS, "temperature": 0, "max_output_tokens": 20,
                   "instructions": "You are a terse assistant.",
                   "input": "Remember this: my secret code word is PERIWINKLE. Reply only with OK."}),
        )
        .await;
    assert_eq!(r1.status, 200, "{}", r1.body);
    let id1 = r1.body["id"].as_str().unwrap().to_string();
    assert_eq!(r1.body["store"], true);
    assert_eq!(r1.body["previous_response_id"], Value::Null);
    assert_eq!(r1.body["instructions"], "You are a terse assistant.");

    // No `model`: the previous response's model continues.
    let r2 = c
        .create(
            &alice,
            json!({"previous_response_id": id1, "temperature": 0, "max_output_tokens": 20,
                   "input": "What is my secret code word? Answer with the word only."}),
        )
        .await;
    assert_eq!(r2.status, 200, "{}", r2.body);
    let id2 = r2.body["id"].as_str().unwrap().to_string();
    let answer = text_of(&r2.body);
    eprintln!("non-streaming answer: {answer:?}");
    assert!(answer.to_uppercase().contains("PERIWINKLE"), "{answer}");
    assert_eq!(r2.body["previous_response_id"], id1.as_str());
    // Instructions are not carried over (OpenAI semantics).
    assert_eq!(r2.body["instructions"], Value::Null);
    // The usage proves the history went to the model: more input than the question alone.
    let in1 = r1.body["usage"]["input_tokens"].as_u64().unwrap();
    let in2 = r2.body["usage"]["input_tokens"].as_u64().unwrap();
    assert!(in2 > in1, "history prepended: {in1} → {in2}");

    // --- Retrieval, scoped to the token. ---
    let got = c.get(&alice, &format!("/v1/responses/{id1}")).await;
    assert_eq!(got.status, 200);
    assert_eq!(got.body["id"], id1.as_str());
    assert_eq!(got.body["output"], r1.body["output"]);
    let other = c.get(&bob, &format!("/v1/responses/{id1}")).await;
    assert_eq!(other.status, 404, "{}", other.body);
    assert_eq!(
        other.body["error"]["message"],
        format!("Response with id '{id1}' not found.")
    );
    assert_eq!(other.body["error"]["type"], "invalid_request_error");
    let steal = c
        .create(
            &bob,
            json!({"model": ALIAS, "previous_response_id": id1, "input": "What is my code word?"}),
        )
        .await;
    assert_eq!(steal.status, 404, "{}", steal.body);
    assert_eq!(steal.body["error"]["code"], "previous_response_not_found");
    assert_eq!(steal.body["error"]["param"], "previous_response_id");
    let unknown = c.get(&alice, "/v1/responses/resp_doesnotexist").await;
    assert_eq!(unknown.status, 404);

    // --- Input items: the request's own input, paged like OpenAI. ---
    let items = c
        .get(&alice, &format!("/v1/responses/{id2}/input_items"))
        .await;
    assert_eq!(items.status, 200, "{}", items.body);
    assert_eq!(items.body["object"], "list");
    assert_eq!(items.body["data"].as_array().unwrap().len(), 1);
    assert_eq!(items.body["data"][0]["role"], "user");
    assert_eq!(items.body["has_more"], false);
    assert!(items.body["first_id"].as_str().unwrap().starts_with("msg_"));

    // --- Streaming continuation: stored before response.completed reaches the client. ---
    let (kinds, r3) = c
        .stream(
            &alice,
            json!({"model": ALIAS, "previous_response_id": id2, "temperature": 0,
                   "max_output_tokens": 20,
                   "input": "Say the code word once more, in lowercase letters only."}),
        )
        .await;
    assert!(kinds.iter().any(|k| k == "response.output_text.delta"));
    let id3 = r3["id"].as_str().unwrap().to_string();
    let streamed = text_of(&r3);
    eprintln!("streaming answer: {streamed:?}");
    assert!(streamed.to_lowercase().contains("periwinkle"), "{streamed}");
    let got = c.get(&alice, &format!("/v1/responses/{id3}")).await;
    assert_eq!(got.status, 200, "{}", got.body);
    assert_eq!(got.body["previous_response_id"], id2.as_str());
    assert_eq!(text_of(&got.body), streamed);

    // A new streamed fact, continued by a stream.
    let (_, s1) = c
        .stream(
            &alice,
            json!({"model": ALIAS, "temperature": 0, "max_output_tokens": 10,
                   "input": "My cat is named Biscuit. Reply only with OK."}),
        )
        .await;
    let (_, s2) = c
        .stream(
            &alice,
            json!({"model": ALIAS, "temperature": 0, "max_output_tokens": 20,
                   "previous_response_id": s1["id"],
                   "input": "What is my cat's name? Answer with the name only."}),
        )
        .await;
    let cat = text_of(&s2);
    eprintln!("streamed chain answer: {cat:?}");
    assert!(cat.contains("Biscuit"), "{cat}");

    // --- store: false is answered but never retrievable or continuable. ---
    let nostore = c
        .create(
            &alice,
            json!({"model": ALIAS, "store": false, "max_output_tokens": 10,
                   "input": "Reply only with OK."}),
        )
        .await;
    assert_eq!(nostore.status, 200, "{}", nostore.body);
    assert_eq!(nostore.body["store"], false);
    let id_ns = nostore.body["id"].as_str().unwrap().to_string();
    assert_eq!(
        c.get(&alice, &format!("/v1/responses/{id_ns}"))
            .await
            .status,
        404
    );
    let (_, streamed_ns) = c
        .stream(
            &alice,
            json!({"model": ALIAS, "store": false, "max_output_tokens": 10, "input": "Reply only with OK."}),
        )
        .await;
    let sid = streamed_ns["id"].as_str().unwrap();
    assert_eq!(
        c.get(&alice, &format!("/v1/responses/{sid}")).await.status,
        404
    );
    let cont = c
        .create(
            &alice,
            json!({"model": ALIAS, "previous_response_id": id_ns, "input": "Again."}),
        )
        .await;
    assert_eq!(cont.status, 404);

    // --- Tool calls round-trip through the stored chain. ---
    let tools = json!([{"type": "function", "name": "get_weather",
        "description": "Current weather of a city",
        "parameters": {"type": "object", "properties": {"city": {"type": "string"}},
                       "required": ["city"]}}]);
    let t1 = c
        .create(
            &alice,
            json!({"model": ALIAS, "temperature": 0, "tools": tools, "tool_choice": "required",
                   "input": "What is the weather in Paris right now?"}),
        )
        .await;
    assert_eq!(t1.status, 200, "{}", t1.body);
    let call = t1.body["output"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["type"] == "function_call")
        .unwrap_or_else(|| panic!("no function call: {}", t1.body))
        .clone();
    assert_eq!(call["name"], "get_weather");
    let t2 = c
        .create(
            &alice,
            json!({"model": ALIAS, "temperature": 0, "tools": tools, "max_output_tokens": 60,
                   "previous_response_id": t1.body["id"],
                   "input": [{"type": "function_call_output", "call_id": call["call_id"],
                              "output": "{\"condition\": \"sunny\", \"temperature_c\": 23}"}]}),
        )
        .await;
    assert_eq!(t2.status, 200, "{}", t2.body);
    let weather = text_of(&t2.body);
    eprintln!("tool answer: {weather:?}");
    assert!(weather.contains("23"), "{weather}");
    let t2id = t2.body["id"].as_str().unwrap();
    let items = c
        .get(
            &alice,
            &format!("/v1/responses/{t2id}/input_items?order=asc&limit=5"),
        )
        .await;
    assert_eq!(items.body["data"][0]["type"], "function_call_output");
    assert_eq!(items.body["data"][0]["call_id"], call["call_id"]);
    let stored_t1 = c
        .get(
            &alice,
            &format!("/v1/responses/{}", t1.body["id"].as_str().unwrap()),
        )
        .await;
    assert!(stored_t1.body["output"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["type"] == "function_call" && i["call_id"] == call["call_id"]));

    // --- Delete: only by the owner; a deleted link breaks the chain. ---
    assert_eq!(
        c.delete(&bob, &format!("/v1/responses/{id1}")).await.status,
        404
    );
    let del = c.delete(&alice, &format!("/v1/responses/{id1}")).await;
    assert_eq!(del.status, 200);
    assert_eq!(
        del.body,
        json!({"id": id1, "object": "response", "deleted": true})
    );
    assert_eq!(
        c.get(&alice, &format!("/v1/responses/{id1}")).await.status,
        404
    );
    let broken = c
        .create(
            &alice,
            json!({"model": ALIAS, "previous_response_id": id3, "input": "Again."}),
        )
        .await;
    assert_eq!(broken.status, 404);
    assert!(broken.body["error"]["message"]
        .as_str()
        .unwrap()
        .contains(&id1));

    let stats = harness_core::state::shared()
        .unwrap()
        .responses_stats()
        .unwrap();
    eprintln!("stored: {} responses, {} bytes", stats.count, stats.bytes);
    // id2, id3, s1, s2, t1, t2 (id1 deleted, store:false never stored).
    assert_eq!(stats.count, 6);

    // --- The official OpenAI Python SDK against the same gateway (when installed). ---
    // (On a blocking thread: the gateway runs on this test's runtime.)
    let (base, key, d) = (c.base.clone(), alice.clone(), dir.clone());
    tokio::task::spawn_blocking(move || openai_sdk(&base, &key, &d))
        .await
        .unwrap();

    // --- Background responses: answered at once, polled, cancelled. ---
    let bg = |extra: Value| {
        let mut body = json!({"model": ALIAS, "temperature": 0, "background": true});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        body
    };
    let started = std::time::Instant::now();
    let b1 = c
        .create(
            &alice,
            bg(json!({"max_output_tokens": 20,
                "input": "Remember this: the vault code is 7421. Reply only with OK."})),
        )
        .await;
    assert_eq!(b1.status, 200, "{}", b1.body);
    // The answer came before the model did: nothing generated yet.
    assert!(matches!(
        b1.body["status"].as_str(),
        Some("queued" | "in_progress")
    ));
    assert_eq!(b1.body["background"], true);
    assert_eq!(b1.body["output"], json!([]));
    assert_eq!(b1.body["store"], true);
    let bid1 = b1.body["id"].as_str().unwrap().to_string();
    assert!(bid1.starts_with("resp_"));
    eprintln!("background create answered in {:?}", started.elapsed());
    let done1 = poll_final(&c, &alice, &bid1).await;
    assert_eq!(done1["status"], "completed", "{done1}");
    assert_eq!(
        done1["id"],
        bid1.as_str(),
        "the finished response keeps the id it answered with"
    );
    assert_eq!(done1["background"], true);
    assert!(done1["usage"]["output_tokens"].as_u64().unwrap() > 0);
    let items = c
        .get(&alice, &format!("/v1/responses/{bid1}/input_items"))
        .await;
    assert_eq!(items.status, 200);
    assert_eq!(items.body["data"].as_array().unwrap().len(), 1);
    // Encrypted reasoning is accepted (local reasoning is plain text); other values are not.
    for path in [
        format!("/v1/responses/{bid1}?include=reasoning.encrypted_content"),
        format!("/v1/responses/{bid1}/input_items?include=reasoning.encrypted_content"),
    ] {
        let accepted = c.get(&alice, &path).await;
        assert_eq!(accepted.status, 200, "{}", accepted.body);
    }
    for path in [
        format!("/v1/responses/{bid1}?include=file_search_call.results"),
        format!("/v1/responses/{bid1}/input_items?include=file_search_call.results"),
    ] {
        let unsupported = c.get(&alice, &path).await;
        assert_eq!(unsupported.status, 400, "{}", unsupported.body);
        assert_eq!(unsupported.body["error"]["param"], "include");
    }

    // It continues like any stored response, also in the background.
    let b2 = c
        .create(
            &alice,
            bg(
                json!({"previous_response_id": bid1, "max_output_tokens": 20,
                "input": "What was the vault code? Answer with the number only."}),
            ),
        )
        .await;
    assert_eq!(b2.status, 200, "{}", b2.body);
    let done2 = poll_final(&c, &alice, b2.body["id"].as_str().unwrap()).await;
    assert_eq!(done2["status"], "completed", "{done2}");
    assert!(text_of(&done2).contains("7421"), "{}", text_of(&done2));
    assert_eq!(done2["previous_response_id"], bid1.as_str());

    // A background stream survives its first client disconnect, persists sequence-numbered
    // events, then resumes strictly after the supplied cursor.
    let response = c
        .http
        .post(format!("{}/v1/responses", c.base))
        .bearer_auth(&alice)
        .json(&bg(json!({"stream": true, "max_output_tokens": 120,
            "input": "Write twelve short numbered facts about the Moon."})))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let mut live = response.bytes_stream();
    let mut buffer = String::new();
    let first = loop {
        let chunk = live.next().await.unwrap().unwrap();
        buffer.push_str(&String::from_utf8_lossy(&chunk).replace('\r', ""));
        if let Some(event) = drain_events(&mut buffer).into_iter().next() {
            break event;
        }
    };
    assert_eq!(first["type"], "response.queued");
    assert_eq!(first["sequence_number"], 0);
    let streamed_id = first["response"]["id"].as_str().unwrap().to_string();
    drop(live);
    let streamed_done = poll_final(&c, &alice, &streamed_id).await;
    assert_eq!(streamed_done["status"], "completed", "{streamed_done}");
    let resumed = c
        .http
        .get(format!(
            "{}/v1/responses/{streamed_id}?stream=true&starting_after=0",
            c.base
        ))
        .bearer_auth(&alice)
        .send()
        .await
        .unwrap();
    let resumed = collect_sse(resumed).await;
    assert!(!resumed.is_empty());
    assert!(resumed
        .iter()
        .all(|event| event["sequence_number"].as_i64().unwrap() > 0));
    for pair in resumed.windows(2) {
        assert_eq!(
            pair[1]["sequence_number"].as_i64().unwrap(),
            pair[0]["sequence_number"].as_i64().unwrap() + 1
        );
    }
    for kind in [
        "response.created",
        "response.in_progress",
        "response.completed",
    ] {
        let event = resumed
            .iter()
            .find(|event| event["type"] == kind)
            .unwrap_or_else(|| panic!("missing {kind}: {resumed:?}"));
        assert_eq!(event["response"]["id"], streamed_id);
        assert_eq!(event["response"]["background"], true);
        assert_eq!(event["response"]["store"], true);
        assert_eq!(event["response"]["previous_response_id"], Value::Null);
    }

    // llama-server accepts probability switches but its Responses adapter hard-codes an empty
    // output_text.logprobs array, so the gateway refuses to pretend the include was fulfilled.
    let logs = c
        .create(
            &alice,
            json!({"model": ALIAS, "max_output_tokens": 8, "top_logprobs": 2,
                "include": ["message.output_text.logprobs"], "input": "Reply with OK."}),
        )
        .await;
    assert_eq!(logs.status, 400, "{}", logs.body);
    assert_eq!(logs.body["error"]["code"], "unsupported_value");
    assert_eq!(logs.body["error"]["param"], "include");

    // Scoped like every stored response.
    let foreign = c.get(&bob, &format!("/v1/responses/{bid1}")).await;
    assert_eq!(foreign.status, 404);
    let foreign = c.post(&bob, &format!("/v1/responses/{bid1}/cancel")).await;
    assert_eq!(foreign.status, 404);

    // Refused combinations.
    let r = c
        .create(&alice, bg(json!({"store": false, "input": "hi"})))
        .await;
    assert_eq!(
        (r.status, r.body["error"]["param"].as_str()),
        (400, Some("background"))
    );
    let r = c
        .get(&alice, &format!("/v1/responses/{bid1}?stream=true"))
        .await;
    assert_eq!(
        (r.status, r.body["error"]["param"].as_str()),
        (400, Some("stream"))
    );
    let r = c
        .create(
            &alice,
            bg(json!({"previous_response_id": "resp_nope", "input": "hi"})),
        )
        .await;
    assert_eq!(r.status, 404, "{}", r.body);

    // Cancel: a long generation is stopped, the slot is free again, the status says so.
    let long = c
        .create(
            &alice,
            bg(json!({"max_output_tokens": 3000,
                "input": "Write a very long story about a lighthouse keeper, at least two thousand words."})),
        )
        .await;
    assert_eq!(long.status, 200, "{}", long.body);
    let lid = long.body["id"].as_str().unwrap().to_string();
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let mid = c.get(&alice, &format!("/v1/responses/{lid}")).await;
    assert_eq!(mid.body["status"], "in_progress", "{}", mid.body);
    let cancelled = c.post(&alice, &format!("/v1/responses/{lid}/cancel")).await;
    assert_eq!(cancelled.status, 200, "{}", cancelled.body);
    assert_eq!(cancelled.body["status"], "cancelled");
    let after = c.get(&alice, &format!("/v1/responses/{lid}")).await;
    assert_eq!(after.body["status"], "cancelled");
    // Cancelling again, or a finished response, returns it unchanged.
    let again = c.post(&alice, &format!("/v1/responses/{lid}/cancel")).await;
    assert_eq!(
        (again.status, again.body["status"].as_str()),
        (200, Some("cancelled"))
    );
    let finished = c
        .post(&alice, &format!("/v1/responses/{bid1}/cancel"))
        .await;
    assert_eq!(
        (finished.status, finished.body["status"].as_str()),
        (200, Some("completed"))
    );
    // A synchronous response cannot be cancelled.
    let sync = c.post(&alice, &format!("/v1/responses/{id2}/cancel")).await;
    assert_eq!(sync.status, 400, "{}", sync.body);
    // llama-server really stopped: the next request is not stuck behind the long one.
    let sched = harness_core::gateway::scheduler().unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let slot = sched.snapshot().await;
    assert_eq!(slot[0].in_flight, 0, "{slot:?}");
    let quick = std::time::Instant::now();
    let ok = c
        .create(
            &alice,
            json!({"model": ALIAS, "max_output_tokens": 5, "input": "Reply only with OK."}),
        )
        .await;
    assert_eq!(ok.status, 200);
    assert!(
        quick.elapsed() < std::time::Duration::from_secs(20),
        "{:?}",
        quick.elapsed()
    );
    // Deleting a response that is still running stops it too.
    let long2 = c
        .create(
            &alice,
            bg(json!({"max_output_tokens": 3000, "input": "Count from one to two thousand in words."})),
        )
        .await;
    let lid2 = long2.body["id"].as_str().unwrap().to_string();
    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    assert_eq!(
        c.delete(&alice, &format!("/v1/responses/{lid2}"))
            .await
            .status,
        200
    );
    assert_eq!(
        c.get(&alice, &format!("/v1/responses/{lid2}")).await.status,
        404
    );
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(sched.snapshot().await[0].in_flight, 0);

    // --- Chain compaction: `context_management` summarizes the older links of a long chain. ---
    let facts = [
        "The project code name is Bluefin Heron.",
        "The total budget is 48,200 euros.",
        "The team lead is Marisol Quinteros.",
        "The launch is on 17 March at 09:40.",
    ];
    let filler = "I am writing this down so that nothing gets lost between our meetings, because \
        last quarter several details were forgotten and we had to redo the planning twice, which \
        cost the team a lot of time and goodwill with the stakeholders, and I would very much like \
        to avoid a repeat of that situation this time around. ";
    let mut prev: Option<String> = None;
    for (i, fact) in facts.iter().enumerate() {
        let mut body = json!({"model": ALIAS, "temperature": 0, "max_output_tokens": 12,
            "input": format!("Note {}. {fact} {filler}{filler}{filler}{filler}Reply only with OK.", i + 1)});
        if let Some(p) = &prev {
            body["previous_response_id"] = json!(p);
        }
        let r = c.create(&alice, body).await;
        assert_eq!(r.status, 200, "{}", r.body);
        assert!(r.body.get("harness_compaction").is_none());
        prev = Some(r.body["id"].as_str().unwrap().to_string());
    }
    let last_plain = prev.clone().unwrap();
    // Not over the threshold: nothing happens, the header is absent.
    let raw = |body: Value| {
        let req = c
            .http
            .post(format!("{}/v1/responses", c.base))
            .bearer_auth(&alice)
            .json(&body);
        async move {
            let r = req.send().await.unwrap();
            let note = r
                .headers()
                .get("x-harness-compaction")
                .map(|v| v.to_str().unwrap().to_string());
            let status = r.status().as_u16();
            (status, note, r.json::<Value>().await.unwrap())
        }
    };
    let (status, note, body) = raw(json!({"previous_response_id": last_plain, "temperature": 0,
        "max_output_tokens": 12, "input": "Reply only with OK.",
        "context_management": [{"type": "compaction", "compact_threshold": 7000}]}))
    .await;
    assert_eq!((status, note), (200, None), "{body}");
    assert!(body.get("harness_compaction").is_none());
    // Over the threshold: the first links are summarized by the model, the question is still
    // answered from the summary.
    let (status, note, body) = raw(json!({"previous_response_id": last_plain, "temperature": 0,
        "max_output_tokens": 40,
        "input": "What are the project code name and the total budget? Answer briefly.",
        "context_management": [{"type": "compaction", "compact_threshold": 400}]}))
    .await;
    assert_eq!(status, 200, "{body}");
    eprintln!("compaction header: {note:?}");
    assert!(
        note.as_deref().is_some_and(|n| n.contains("tokens")),
        "{note:?}"
    );
    let mark = &body["harness_compaction"];
    eprintln!("compaction: {mark}\nanswer: {}", text_of(&body));
    assert_eq!(mark["links"], 3);
    let (before, after) = (
        mark["tokens_before"].as_u64().unwrap(),
        mark["tokens_after"].as_u64().unwrap(),
    );
    assert!(after < before, "{before} -> {after}");
    let sent = body["usage"]["input_tokens"].as_u64().unwrap();
    assert!(sent < before, "the model saw {sent} tokens, not {before}");
    let answer = text_of(&body).to_lowercase();
    assert!(answer.contains("bluefin"), "{answer}");
    assert!(
        answer.contains("48,200") || answer.contains("48200"),
        "{answer}"
    );
    let compacted = body["id"].as_str().unwrap().to_string();
    // The marker is stored; the next turn starts from the summary without asking again.
    let stored = c.get(&alice, &format!("/v1/responses/{compacted}")).await;
    assert_eq!(
        stored.body["harness_compaction"]["through"],
        mark["through"]
    );
    let (status, note, body) = raw(json!({"previous_response_id": compacted, "temperature": 0,
        "max_output_tokens": 40, "input": "Who is the team lead? Answer with the name only."}))
    .await;
    assert_eq!((status, note), (200, None), "{body}");
    let answer = text_of(&body).to_lowercase();
    eprintln!("later answer: {answer}");
    assert!(answer.contains("marisol"), "{answer}");
    assert!(body.get("harness_compaction").is_none());
    assert!(body["usage"]["input_tokens"].as_u64().unwrap() < before);
    // A threshold below what is needed to summarize nothing new, and a bad value, are told.
    let bad = c
        .create(
            &alice,
            json!({"model": ALIAS, "input": "hi", "context_management": [{"type": "x"}]}),
        )
        .await;
    assert_eq!(bad.status, 400, "{}", bad.body);
    assert_eq!(bad.body["error"]["param"], "context_management");

    server.stop().await;
    registry.stop_all().await;
    // The copied runtime is large; the rest of the temporary data root is small.
    let _ = std::fs::remove_dir_all(dir.join("runtime"));
}

const SDK_SCRIPT: &str = r#"
import os, sys
from openai import OpenAI, NotFoundError
c = OpenAI(base_url=os.environ["HS_BASE"] + "/v1", api_key=os.environ["HS_TOKEN"])
m = os.environ["HS_MODEL"]
r1 = c.responses.create(model=m, temperature=0, max_output_tokens=20,
    input="Remember: my favourite city is Reykjavik. Reply only with OK.")
r2 = c.responses.create(model=m, temperature=0, max_output_tokens=20, previous_response_id=r1.id,
    input="What is my favourite city? Answer with the city only.")
assert "reykjavik" in r2.output_text.lower(), r2.output_text
assert r2.previous_response_id == r1.id
assert c.responses.retrieve(r1.id).id == r1.id
items = c.responses.input_items.list(r2.id)
assert len(items.data) == 1 and items.data[0].role == "user", items
final = None
for ev in c.responses.create(model=m, temperature=0, max_output_tokens=20, stream=True,
        previous_response_id=r2.id, input="Name the city once more."):
    if ev.type == "response.completed":
        final = ev.response
assert final is not None and "reykjavik" in final.output_text.lower(), final
assert c.responses.retrieve(final.id).id == final.id
r3 = c.responses.create(model=m, store=False, max_output_tokens=5, input="Reply only with OK.")
try:
    c.responses.retrieve(r3.id)
    sys.exit("store=False response was retrievable")
except NotFoundError:
    pass
c.responses.delete(r1.id)
try:
    c.responses.retrieve(r1.id)
    sys.exit("deleted response was retrievable")
except NotFoundError:
    pass
import time
b = c.responses.create(model=m, background=True, max_output_tokens=20, input="Reply only with OK.")
assert b.status in ("queued", "in_progress"), b.status
for _ in range(240):
    b = c.responses.retrieve(b.id)
    if b.status not in ("queued", "in_progress"):
        break
    time.sleep(0.5)
assert b.status == "completed", b.status
lng = c.responses.create(model=m, background=True, max_output_tokens=3000,
    input="Write a very long story about a harbour, at least two thousand words.")
time.sleep(1.5)
cx = c.responses.cancel(lng.id)
assert cx.status == "cancelled", cx.status
assert c.responses.retrieve(lng.id).status == "cancelled"
print("SDK_OK", repr(r2.output_text), repr(final.output_text), b.status, cx.status)
"#;

fn openai_sdk(base: &str, token: &str, dir: &Path) {
    let python = std::env::var("HS_PYTHON").unwrap_or_else(|_| "python".into());
    let has_sdk = std::process::Command::new(&python)
        .args(["-c", "import openai"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !has_sdk {
        eprintln!("python with the openai package not found, skipping the SDK check");
        return;
    }
    let script = dir.join("sdk_check.py");
    std::fs::write(&script, SDK_SCRIPT).unwrap();
    let out = std::process::Command::new(&python)
        .arg(&script)
        .env("HS_BASE", base)
        .env("HS_TOKEN", token)
        .env("HS_MODEL", ALIAS)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    eprintln!(
        "openai SDK: {stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success() && stdout.contains("SDK_OK"));
}
