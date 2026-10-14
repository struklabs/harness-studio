//! Token policy, rate limits and OTLP export end to end with a real llama-server and a real
//! model (no mocks):
//!
//! ```text
//! HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<chat gguf>
//! cargo test -p harness-gateway --test policy_e2e -- --ignored --nocapture
//! ```
//!
//! Temporary data root, in-memory secrets, the gateway and an OTLP/HTTP receiver on free
//! ports, the model on 19140. Everything started here is stopped here.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::routing::post;
use futures_util::StreamExt;
use harness_core::control_api::{RateLimits, Scope, TokenPolicy, TokenStore};
use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::otel::{self, OtelProtocol, OtelSettings};
use harness_core::params::ParamValue;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use opentelemetry_proto::tonic::collector::trace::v1::{
    ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use opentelemetry_proto::tonic::common::v1::{any_value, KeyValue};
use opentelemetry_proto::tonic::trace::v1::Span;
use prost::Message;
use serde_json::{json, Value};

const ALIAS: &str = "policy-e2e-qwen";

fn env_path(name: &str) -> Option<PathBuf> {
    let p = std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_file());
    if p.is_none() {
        eprintln!("{name} not set, skipping");
    }
    p
}

fn data_root(bin: &Path) -> PathBuf {
    harness_core::secrets::use_memory_store();
    let dir = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("policy-e2e"))
        .tempdir()
        .unwrap()
        .keep();
    harness_core::paths::init(dir.clone());
    let mut s = harness_core::config::AppSettings::load();
    s.runtime_mode = harness_core::config::RuntimeMode::Custom;
    s.custom_binary.binary_path = Some(bin.to_path_buf());
    s.custom_binary.is_valid = true;
    s.server.port = 19140;
    s.control_api.port = 18080;
    s.save().unwrap();
    dir
}

fn attr(attrs: &[KeyValue], key: &str) -> Option<String> {
    let v = attrs.iter().find(|kv| kv.key == key)?.value.as_ref()?;
    Some(match v.value.as_ref()? {
        any_value::Value::StringValue(s) => s.clone(),
        any_value::Value::IntValue(i) => i.to_string(),
        any_value::Value::BoolValue(b) => b.to_string(),
        any_value::Value::DoubleValue(d) => d.to_string(),
        other => format!("{other:?}"),
    })
}

/// Decoded trace requests with their raw bodies.
type Traces = Arc<Mutex<Vec<(ExportTraceServiceRequest, Vec<u8>)>>>;

/// OTLP/HTTP trace receiver keeping the decoded requests and the raw bodies.
async fn receiver() -> (String, Traces) {
    let store: Traces = Arc::default();
    let s = store.clone();
    let app = axum::Router::new().route(
        "/v1/traces",
        post(move |body: Bytes| {
            let s = s.clone();
            async move {
                let req = ExportTraceServiceRequest::decode(body.clone()).expect("valid OTLP");
                s.lock().unwrap().push((req, body.to_vec()));
                (
                    [("content-type", "application/x-protobuf")],
                    ExportTraceServiceResponse::default().encode_to_vec(),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, store)
}

fn spans(store: &Mutex<Vec<(ExportTraceServiceRequest, Vec<u8>)>>) -> Vec<Span> {
    store
        .lock()
        .unwrap()
        .iter()
        .flat_map(|(r, _)| r.resource_spans.clone())
        .flat_map(|rs| rs.scope_spans)
        .flat_map(|ss| ss.spans)
        .collect()
}

struct Resp {
    status: u16,
    headers: reqwest::header::HeaderMap,
    body: Value,
}

async fn chat(http: &reqwest::Client, base: &str, token: &str, body: Value) -> Resp {
    let r = http
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = r.status().as_u16();
    let headers = r.headers().clone();
    let text = r.text().await.unwrap();
    Resp {
        status,
        headers,
        body: serde_json::from_str(&text).unwrap_or(Value::String(text)),
    }
}

fn token(name: &str, policy: TokenPolicy) -> String {
    TokenStore::at(&harness_core::paths::config_dir())
        .create_with_policy(name, &[Scope::Inference], &policy)
        .unwrap()
        .token
}

/// Tokens-per-minute charged with the real usage, concurrency cap while a real stream runs,
/// a model allowlist against a real alias, and the exported span of a real generation (load,
/// first token, token counts; no prompt text while content capture is off).
#[tokio::test]
#[ignore]
async fn e2e_limits_policy_and_otel_span_timing() {
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL")) else {
        return;
    };
    let dir = data_root(&bin);
    let mut def = ModelDefinition::new(ALIAS, ModelRef::Local { path: model });
    def.load.insert("gpu_layers".into(), ParamValue::Int(99));
    def.load
        .insert("context_size".into(), ParamValue::Int(4096));
    let def = harness_core::domain::store::Domain::global()
        .save_definition(def)
        .unwrap();
    assert!(!def.capture_content);

    let (otlp, store) = receiver().await;
    let saved = otel::save_settings(
        &OtelSettings {
            enabled: true,
            endpoint: otlp.clone(),
            protocol: OtelProtocol::HttpProtobuf,
            metrics: false,
            // Asked for, but the definition does not capture content: nothing is sent.
            include_content: true,
            ..Default::default()
        },
        Some(""),
    )
    .unwrap();
    otel::apply(&saved).await.unwrap();

    let registry = Arc::new(ServerRegistry::default());
    let server = GatewayServer::new();
    let status = server
        .start_ephemeral(
            GatewayContext {
                registry: registry.clone(),
                runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
                gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
                data_dir: dir.clone(),
                downloads: None,
            },
            &GatewaySettings {
                enabled: true,
                port: 0,
                require_token: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();

    // --- Model allowlist: the real alias is not covered, the listing hides it. ---
    let other = token(
        "e2e-other-models",
        TokenPolicy {
            models: vec!["llama-*".into()],
            ..Default::default()
        },
    );
    let marker = "HSOTELPRIV7731";
    let r = chat(
        &http,
        &base,
        &other,
        json!({"model": ALIAS, "messages": [{"role": "user", "content": marker}]}),
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.body);
    assert_eq!(r.body["error"]["code"], "model_not_allowed");

    // --- Tokens per minute: charged with the real usage when the request ends. ---
    let tpm = token(
        "e2e-tpm",
        TokenPolicy {
            limits: RateLimits {
                tokens_per_minute: Some(20),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let t0 = std::time::Instant::now();
    let r = chat(
        &http,
        &base,
        &tpm,
        json!({"model": ALIAS, "max_tokens": 40, "temperature": 0,
               "messages": [{"role": "user", "content": format!("Repeat exactly: {marker}. Then count from 1 to 30.")}]}),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let first_load = t0.elapsed();
    let used = r.body["usage"]["total_tokens"].as_u64().unwrap();
    assert!(used > 20, "the reply uses more than the budget: {used}");
    assert_eq!(
        r.headers
            .get("x-ratelimit-limit-tokens")
            .and_then(|v| v.to_str().ok()),
        Some("20")
    );
    let r = chat(
        &http,
        &base,
        &tpm,
        json!({"model": ALIAS, "max_tokens": 4, "messages": [{"role": "user", "content": "hi"}]}),
    )
    .await;
    assert_eq!(r.status, 429, "{}", r.body);
    assert_eq!(r.body["error"]["type"], "rate_limit_error");
    assert_eq!(r.body["error"]["harness"]["limit"], "tokens_per_minute");
    let retry: u64 = r
        .headers
        .get("retry-after")
        .unwrap()
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    // The deficit (used - 20 tokens) refills at 20 per minute: about 3 s per token.
    let expected = ((used - 20) as f64 * 3.0).ceil() as u64;
    assert!(
        retry + 5 >= expected && retry <= expected + 5,
        "retry {retry}, expected about {expected} (used {used})"
    );

    // --- Concurrency: one stream at a time. ---
    let conc = token(
        "e2e-conc",
        TokenPolicy {
            limits: RateLimits {
                max_concurrent: Some(1),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let stream = http
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth(&conc)
        .json(
            &json!({"model": ALIAS, "max_tokens": 200, "stream": true, "temperature": 0,
                      "messages": [{"role": "user", "content": "Count from 1 to 100."}]}),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(stream.status(), 200);
    assert_eq!(
        stream
            .headers()
            .get("x-harness-ratelimit-concurrency")
            .and_then(|v| v.to_str().ok()),
        Some("1")
    );
    let mut chunks = stream.bytes_stream();
    let first = chunks.next().await.unwrap().unwrap();
    assert!(!first.is_empty());
    let r = chat(
        &http,
        &base,
        &conc,
        json!({"model": ALIAS, "max_tokens": 4, "messages": [{"role": "user", "content": "hi"}]}),
    )
    .await;
    assert_eq!(r.status, 429, "{}", r.body);
    assert_eq!(r.body["error"]["harness"]["limit"], "max_concurrent");
    while let Some(c) = chunks.next().await {
        c.unwrap();
    }
    drop(chunks);
    // The permit is released when the stream ends.
    let mut ok = false;
    for _ in 0..20 {
        let r = chat(
            &http,
            &base,
            &conc,
            json!({"model": ALIAS, "max_tokens": 4, "messages": [{"role": "user", "content": "hi"}]}),
        )
        .await;
        if r.status == 200 {
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ok, "the concurrency slot comes back after the stream");

    // --- The exported spans. ---
    otel::flush(Duration::from_secs(5)).await;
    let mut found = None;
    for _ in 0..50 {
        let all = spans(&store);
        if let Some(root) = all.iter().find(|s| {
            attr(&s.attributes, "harness.token_name").as_deref() == Some("e2e-tpm")
                && attr(&s.attributes, "http.response.status_code").as_deref() == Some("200")
        }) {
            found = Some((root.clone(), all));
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        otel::flush(Duration::from_secs(5)).await;
    }
    let (root, all) = found.expect("the span of the first generation");
    assert_eq!(root.name, format!("chat {ALIAS}"));
    let total_ms: u64 = attr(&root.attributes, "harness.total_ms")
        .unwrap()
        .parse()
        .unwrap();
    let ttft_ms: u64 = attr(&root.attributes, "harness.ttft_ms")
        .unwrap()
        .parse()
        .unwrap();
    let load_ms: u64 = attr(&root.attributes, "harness.load_ms")
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        load_ms > 0 && ttft_ms > 0 && ttft_ms <= total_ms,
        "{load_ms} {ttft_ms} {total_ms}"
    );
    assert!(
        total_ms as u128 <= first_load.as_millis() + 50,
        "the span covers at most the client-side time"
    );
    let out: u64 = attr(&root.attributes, "gen_ai.usage.output_tokens")
        .unwrap()
        .parse()
        .unwrap();
    let input: u64 = attr(&root.attributes, "gen_ai.usage.input_tokens")
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(input + out, used, "span tokens match the reply's usage");
    let span_ms = (root.end_time_unix_nano - root.start_time_unix_nano) / 1_000_000;
    assert!(span_ms.abs_diff(total_ms) <= 2, "{span_ms} vs {total_ms}");
    let children: Vec<&Span> = all
        .iter()
        .filter(|s| s.parent_span_id == root.span_id && s.trace_id == root.trace_id)
        .collect();
    let names: Vec<&str> = children.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"gateway.load"), "{names:?}");
    let generate = children
        .iter()
        .find(|s| s.name == "llama-server.generate")
        .unwrap_or_else(|| panic!("{names:?}"));
    assert!(generate
        .events
        .iter()
        .any(|e| e.name == "gen_ai.first_token"));

    // Rate-limited and forbidden requests are spans too.
    assert!(all.iter().any(|s| {
        attr(&s.attributes, "harness.token_name").as_deref() == Some("e2e-conc")
            && attr(&s.attributes, "error.type").as_deref() == Some("rate_limited")
    }));
    // Content capture is off for the definition: the prompt marker never left the machine.
    let raw: Vec<u8> = store
        .lock()
        .unwrap()
        .iter()
        .flat_map(|(_, b)| b.clone())
        .collect();
    assert!(!raw.windows(marker.len()).any(|w| w == marker.as_bytes()));
    assert!(!all
        .iter()
        .any(|s| attr(&s.attributes, "harness.request.body").is_some()));

    otel::shutdown().await;
    server.stop().await;
    registry.stop_all().await;
}
