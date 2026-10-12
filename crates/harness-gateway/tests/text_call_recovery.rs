//! Real gateway + scheduler + managed loopback bridge + fake HTTP upstream.
//! Isolated data and memory secrets; no installed settings or GPU processes are touched.
use axum::{
    body::Body,
    http::header,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use harness_core::{
    domain::{ModelDefinition, ModelRef},
    gateway::GatewaySettings,
    process::LaunchConfig,
    servers::ServerRegistry,
};
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};
use std::sync::Arc;

const CALL: &str = r#"<tools>{"name":"read","arguments":{"path":"README.md"}}</tools>"#;
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

static RUNAWAY_DROPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
struct DropNotice;
impl Drop for DropNotice {
    fn drop(&mut self) {
        RUNAWAY_DROPPED.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

async fn fake(Json(req): Json<Value>) -> axum::response::Response {
    let scenario = req["messages"][0]["content"].as_str().unwrap_or("call");
    if scenario == "runaway" {
        RUNAWAY_DROPPED.store(false, std::sync::atomic::Ordering::SeqCst);
        // Never sends finish_reason or DONE. The relay must cancel it itself.
        let stream =
            futures_util::stream::unfold((true, DropNotice), |(first, notice)| async move {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                let bytes: &'static [u8] = if first {
                    include_bytes!("fixtures/runaway-stream.txt")
                } else {
                    b": heartbeat\n\n"
                };
                Some((Ok::<_, std::io::Error>(bytes), (false, notice)))
            });
        return (
            [(header::CONTENT_TYPE, "text/event-stream")],
            Body::from_stream(stream),
        )
            .into_response();
    }
    let content = match scenario {
        "unknown" => CALL.replace("\"read\"", "\"missing\""),
        "example" => {
            "Example:\n```json\n{\"name\":\"read\",\"arguments\":{}}\n```\nUse this format.".into()
        }
        "multiple" => format!(
            "Açıklama {CALL}{} son",
            CALL.replace("README.md", "OTHER.md")
        ),
        _ => CALL.into(),
    };
    let native = scenario == "native";
    let reason = if native {
        "tool_calls"
    } else if scenario == "length" {
        "length"
    } else {
        "stop"
    };
    let native_calls =
        json!([{"id":"native","type":"function","function":{"name":"read","arguments":"{}"}}]);
    if req["stream"] == true {
        let mut events = Vec::new();
        for text in content.chars().collect::<Vec<_>>().chunks(7) {
            events.push(json!({"choices":[{"index":0,"delta":{"content":text.iter().collect::<String>()},"finish_reason":null}]}));
        }
        if native {
            let mut calls = native_calls.clone();
            calls[0]["index"] = json!(0);
            events.push(
                json!({"choices":[{"index":0,"delta":{"tool_calls":calls},"finish_reason":null}]}),
            );
        }
        events.push(json!({"choices":[{"index":0,"delta":{},"finish_reason":reason}]}));
        events.push(json!({"choices":[],"usage":{"prompt_tokens":42,"completion_tokens":20}}));
        let mut wire = events
            .into_iter()
            .map(|v| format!("data: {v}\r\n\r\n"))
            .collect::<String>();
        wire.push_str("data: [DONE]\r\n\r\n");
        return (
            [(header::CONTENT_TYPE, "text/event-stream")],
            Body::from(wire),
        )
            .into_response();
    }
    let mut message = json!({"role":"assistant","content":content});
    if native {
        message["tool_calls"] = native_calls;
    }
    Json(json!({"choices":[{"index":0,"message":message,"finish_reason":reason}],"usage":{"prompt_tokens":42,"completion_tokens":20}})).into_response()
}

#[tokio::test]
async fn recovery_is_opt_in_observable_and_preserves_non_calls() {
    harness_core::secrets::use_memory_store();
    let dir = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("text-call-recovery"))
        .tempdir()
        .unwrap();
    harness_core::paths::init(dir.path().to_path_buf());
    let bridge = dir
        .path()
        .join(format!("bridge{}", std::env::consts::EXE_SUFFIX));
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/upstream-bridge.rs");
    assert!(harness_core::process::quiet_command("rustc")
        .arg(source)
        .arg("-o")
        .arg(&bridge)
        .status()
        .unwrap()
        .success());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_port = listener.local_addr().unwrap().port();
    let upstream = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/health", get(|| async { Json(json!({"status":"ok"})) }))
            .route("/props", get(|| async { Json(json!({"total_slots":1,"default_generation_settings":{"n_ctx":8192},"chat_template_caps":{"supports_tools":true}})) }))
            .route("/v1/chat/completions", post(fake))).await.unwrap();
    });
    let model = dir.path().join("model.gguf");
    std::fs::write(&model, b"GGUF").unwrap();
    let def = ModelDefinition::new(
        "recovery-test",
        ModelRef::Local {
            path: model.clone(),
        },
    );
    let def = harness_core::domain::store::Domain::global()
        .save_definition(def)
        .unwrap();
    let id = def.id.clone();
    let registry = Arc::new(ServerRegistry::default());
    let pm = registry.get_or_create("bridge");
    pm.start(LaunchConfig {
        binary_path: bridge,
        model_path: model,
        definition_id: Some(id.clone()),
        port: free_port(),
        host: "127.0.0.1".into(),
        context_size: 8192,
        custom_args: Some(format!("--bridge-target {upstream_port}")),
        ..Default::default()
    })
    .await
    .unwrap();
    for _ in 0..200 {
        if pm.get_status() == harness_core::process::ServerStatus::Running {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    if pm.get_status() != harness_core::process::ServerStatus::Running {
        let logs = pm.get_logs(Some(30));
        registry.stop_all().await;
        panic!("validation bridge did not become ready: {logs:?}");
    }
    let ctx = GatewayContext {
        registry: registry.clone(),
        runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
        gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
        data_dir: dir.path().to_path_buf(),
        downloads: None,
    };
    let server = GatewayServer::new();
    let port = free_port();
    let mut settings = GatewaySettings {
        enabled: true,
        port,
        ..Default::default()
    };
    use futures_util::FutureExt;
    let checks = std::panic::AssertUnwindSafe(async {
    // Recovery is on by default: the "off" half of the test opts out explicitly.
    settings.text_tool_calls.insert(id.clone(), false);
    server.start(ctx.clone(), &settings).await.unwrap();
    let http = reqwest::Client::new();
    let mut rx = harness_core::events::subscribe();
    for enabled in [false, true] {
        if enabled {
            settings.text_tool_calls.insert(id.clone(), true);
            server.start(ctx.clone(), &settings).await.unwrap(); // settings update; no restart
        }
        for stream in [false, true] {
            for (scenario, count) in [
                ("call", 1),
                ("multiple", 2),
                ("native", 0),
                ("unknown", 0),
                ("example", 0),
                ("length", 0),
                ("no-tools", 0),
            ] {
                let mut body = json!({"model":"recovery-test","stream":stream,"messages":[{"role":"user","content":scenario}],
                    "tools":[{"type":"function","function":{"name":"read","parameters":{"type":"object"}}}]});
                if scenario == "no-tools" {
                    body.as_object_mut().unwrap().remove("tools");
                }
                let r = http
                    .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
                    .json(&body)
                    .send()
                    .await
                    .unwrap();
                assert_eq!(r.status(), 200);
                let request_id = r.headers()["x-harness-request-id"]
                    .to_str()
                    .unwrap()
                    .to_string();
                let compat = r
                    .headers()
                    .get("x-harness-compat")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                let text = r.text().await.unwrap();
                let events: Vec<Value> = if stream {
                    text.lines()
                        .filter_map(|l| {
                            l.strip_prefix("data:")
                                .and_then(|s| serde_json::from_str(s.trim()).ok())
                        })
                        .collect()
                } else {
                    vec![serde_json::from_str(&text).unwrap()]
                };
                let calls = events
                    .iter()
                    .map(|v| {
                        let m = if stream {
                            &v["choices"][0]["delta"]
                        } else {
                            &v["choices"][0]["message"]
                        };
                        m["tool_calls"].as_array().map_or(0, Vec::len)
                    })
                    .sum::<usize>();
                let expected = if scenario == "native" {
                    1
                } else if enabled {
                    count
                } else {
                    0
                };
                assert_eq!(
                    calls, expected,
                    "enabled={enabled} stream={stream} {scenario}: {text}"
                );
                if enabled && count > 0 && !stream {
                    assert_eq!(compat, Some(format!("text_tool_calls={count}")));
                }
                let record = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    loop {
                        if let Ok(harness_core::events::CoreEvent::GatewayRequest { record }) =
                            rx.recv().await
                        {
                            if record.id == request_id {
                                break record;
                            }
                        }
                    }
                })
                .await
                .unwrap();
                assert_eq!(record.usage.completion_tokens, Some(20));
                if enabled && count > 0 {
                    assert_eq!(
                        record.tool_diag_code.as_deref(),
                        Some("call_left_in_content")
                    );
                    let detail: Value =
                        serde_json::from_str(record.tool_diag_detail.as_deref().unwrap()).unwrap();
                    assert_eq!(detail["recovered"], true);
                    assert_eq!(detail["recovered_calls"], count);
                    assert!(record.compat.contains(&format!("text_tool_calls={count}")));
                    assert_eq!(record.tool_calls, 0); // keep native evidence honest
                } else {
                    assert!(!record
                        .compat
                        .iter()
                        .any(|s| s.starts_with("text_tool_calls=")));
                }
            }
        }
    }
    // The recorded runaway closes without waiting for the fake model; the upstream
    // body is dropped too (the managed bridge sees the disconnect on its heartbeat).
    let r = http.post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .json(&json!({"model":"recovery-test","stream":true,"messages":[{"role":"user","content":"runaway"}],
            "tools":(0..16).map(|i|json!({"type":"function","function":{"name":format!("tool{i}"),"parameters":{"type":"object"}}})).collect::<Vec<_>>() }))
        .send().await.unwrap();
    let request_id = r.headers()["x-harness-request-id"].to_str().unwrap().to_string();
    let text = tokio::time::timeout(std::time::Duration::from_secs(5), r.text()).await.unwrap().unwrap();
    assert!(text.ends_with("data: [DONE]\n\n"));
    let events: Vec<Value> = text.lines().filter_map(|l|l.strip_prefix("data:")).filter_map(|s|serde_json::from_str(s.trim()).ok()).collect();
    assert_eq!(events.iter().map(|v|v["choices"][0]["delta"]["tool_calls"].as_array().map_or(0,Vec::len)).sum::<usize>(),1);
    let retained: Vec<_> = events.iter().flat_map(|v| v["choices"][0]["delta"]["tool_calls"].as_array().cloned().unwrap_or_default()).collect();
    assert_eq!(retained[0]["index"], 0);
    assert_eq!(retained[0]["function"]["name"], "web_search");
    assert_eq!(serde_json::from_str::<Value>(retained[0]["function"]["arguments"].as_str().unwrap()).unwrap()["query"], "llama.cpp son s\u{00fc}r\u{00fc}m\u{00fc}");
    assert!(events.iter().any(|v|v["choices"][0]["finish_reason"]=="tool_calls"));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !RUNAWAY_DROPPED.load(std::sync::atomic::Ordering::SeqCst) { tokio::time::sleep(std::time::Duration::from_millis(10)).await; }
    }).await.expect("upstream body was not cancelled");
    let record = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop { if let Ok(harness_core::events::CoreEvent::GatewayRequest {record})=rx.recv().await { if record.id==request_id {break record;} } }
    }).await.unwrap();
    assert!(record.compat.contains(&"tool_loop_cut=9".into()));
    assert_eq!(record.usage.finish_reason.as_deref(),Some("tool_calls"));

    // HTTP/1 trailer goes after the SSE terminal events (ordinary initial headers cannot
    // know the count). TE explicitly permits hyper to send response trailers.
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let body =
        json!({"model":"recovery-test","stream":true,"messages":[{"role":"user","content":"call"}],
        "tools":[{"type":"function","function":{"name":"read"}}]})
        .to_string();
    let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    socket.write_all(format!("POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nTE: trailers\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    socket.read_to_end(&mut raw).await.unwrap();
    let raw = String::from_utf8(raw).unwrap();
    assert!(raw.contains("trailer: x-harness-compat"), "{raw}");
    assert!(
        raw.rfind("x-harness-compat: text_tool_calls=1").unwrap() > raw.find("[DONE]").unwrap(),
        "{raw}"
    );
    }).catch_unwind().await;
    server.stop().await;
    registry.stop_all().await;
    upstream.abort();
    if let Err(panic) = checks {
        std::panic::resume_unwind(panic);
    }
}
