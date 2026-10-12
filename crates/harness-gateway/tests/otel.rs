//! OpenTelemetry export against real OTLP receivers: an HTTP server that decodes the
//! protobuf bodies the exporter sends (`/v1/traces`, `/v1/logs`, `/v1/metrics`) and a gRPC server built
//! from the official OTLP service definitions. The gateway is the real in-process listener;
//! its requests end before a model would load (403/404/429), so no llama-server is needed.
//!
//! The gRPC test needs the cargo feature `otel-grpc` (`cargo test -p harness-gateway
//! --features otel-grpc`); without it the build refuses gRPC with a keyed error (tested too).
//!
//! Temporary data root (never the user's) and in-memory secrets (the export headers go to
//! the secret store, never to settings.json).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use harness_core::control_api::{RateLimits, Scope, TokenPolicy, TokenStore};
use harness_core::gateway::GatewaySettings;
use harness_core::otel::{self, OtelProtocol, OtelSettings};
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use opentelemetry_proto::tonic::collector::logs::v1::{
    ExportLogsServiceRequest, ExportLogsServiceResponse,
};
#[cfg(feature = "otel-grpc")]
use opentelemetry_proto::tonic::collector::metrics::v1::metrics_service_server::{
    MetricsService, MetricsServiceServer,
};
use opentelemetry_proto::tonic::collector::metrics::v1::{
    ExportMetricsServiceRequest, ExportMetricsServiceResponse,
};
#[cfg(feature = "otel-grpc")]
use opentelemetry_proto::tonic::collector::trace::v1::trace_service_server::{
    TraceService, TraceServiceServer,
};
use opentelemetry_proto::tonic::collector::trace::v1::{
    ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use opentelemetry_proto::tonic::common::v1::{any_value, KeyValue};
use opentelemetry_proto::tonic::logs::v1::LogRecord;
use opentelemetry_proto::tonic::trace::v1::Span;
use prost::Message;
use serde_json::json;

const KEY_HEADER: &str = "x-otel-key";
const KEY: &str = "receiver-secret-4711";

fn data_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        harness_core::secrets::use_memory_store();
        let dir = tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("gw-otel"))
            .tempdir()
            .unwrap()
            .keep();
        harness_core::paths::init(dir.clone());
        let mut s = harness_core::config::AppSettings::load();
        s.control_api.port = free_port();
        s.save().unwrap();
        dir
    })
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// The exporter and the gateway status are process-wide: one test at a time.
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    LOCK.lock().await
}

/// What a receiver got.
#[derive(Default)]
struct Received {
    traces: Vec<ExportTraceServiceRequest>,
    metrics: Vec<ExportMetricsServiceRequest>,
    logs: Vec<ExportLogsServiceRequest>,
    /// Requests refused for a missing/wrong key.
    refused: usize,
}

type Store = Arc<Mutex<Received>>;

impl Received {
    fn spans(&self) -> Vec<(Span, String)> {
        let mut out = Vec::new();
        for req in &self.traces {
            for rs in &req.resource_spans {
                let service = rs
                    .resource
                    .as_ref()
                    .and_then(|r| attr(&r.attributes, "service.name"))
                    .unwrap_or_default();
                for ss in &rs.scope_spans {
                    for s in &ss.spans {
                        out.push((s.clone(), service.clone()));
                    }
                }
            }
        }
        out
    }

    fn metric_names(&self) -> Vec<String> {
        self.metrics
            .iter()
            .flat_map(|r| &r.resource_metrics)
            .flat_map(|rm| &rm.scope_metrics)
            .flat_map(|sm| &sm.metrics)
            .map(|m| m.name.clone())
            .collect()
    }

    fn log_records(&self) -> Vec<LogRecord> {
        self.logs
            .iter()
            .flat_map(|request| &request.resource_logs)
            .flat_map(|resource| &resource.scope_logs)
            .flat_map(|scope| &scope.log_records)
            .cloned()
            .collect()
    }

    /// Data point attribute sets of a sum metric.
    fn sum_points(&self, name: &str) -> Vec<(Vec<KeyValue>, u64)> {
        use opentelemetry_proto::tonic::metrics::v1::{metric, number_data_point};
        let mut out = Vec::new();
        for m in self
            .metrics
            .iter()
            .flat_map(|r| &r.resource_metrics)
            .flat_map(|rm| &rm.scope_metrics)
            .flat_map(|sm| &sm.metrics)
            .filter(|m| m.name == name)
        {
            if let Some(metric::Data::Sum(sum)) = &m.data {
                for p in &sum.data_points {
                    let v = match p.value {
                        Some(number_data_point::Value::AsInt(i)) => i as u64,
                        Some(number_data_point::Value::AsDouble(d)) => d as u64,
                        None => 0,
                    };
                    out.push((p.attributes.clone(), v));
                }
            }
        }
        out
    }
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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// OTLP/HTTP receiver: decodes the protobuf request bodies; answers 401 without the key.
async fn http_receiver() -> (String, Store) {
    let store: Store = Arc::default();
    let authorized = |h: &HeaderMap| h.get(KEY_HEADER).and_then(|v| v.to_str().ok()) == Some(KEY);
    let traces_store = store.clone();
    let metrics_store = store.clone();
    let logs_store = store.clone();
    let app = axum::Router::new()
        .route(
            "/v1/traces",
            post(move |h: HeaderMap, body: Bytes| {
                let store = traces_store.clone();
                async move {
                    if !authorized(&h) {
                        store.lock().unwrap().refused += 1;
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    assert_eq!(
                        h.get("content-type").unwrap(),
                        "application/x-protobuf",
                        "OTLP/HTTP protobuf"
                    );
                    let req = ExportTraceServiceRequest::decode(body).expect("valid OTLP");
                    store.lock().unwrap().traces.push(req);
                    (
                        [("content-type", "application/x-protobuf")],
                        ExportTraceServiceResponse::default().encode_to_vec(),
                    )
                        .into_response()
                }
            }),
        )
        .route(
            "/v1/logs",
            post(move |h: HeaderMap, body: Bytes| {
                let store = logs_store.clone();
                async move {
                    if !authorized(&h) {
                        store.lock().unwrap().refused += 1;
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    assert_eq!(h.get("content-type").unwrap(), "application/x-protobuf");
                    let request = ExportLogsServiceRequest::decode(body).expect("valid OTLP");
                    store.lock().unwrap().logs.push(request);
                    (
                        [("content-type", "application/x-protobuf")],
                        ExportLogsServiceResponse::default().encode_to_vec(),
                    )
                        .into_response()
                }
            }),
        )
        .route(
            "/v1/metrics",
            post(move |h: HeaderMap, body: Bytes| {
                let store = metrics_store.clone();
                async move {
                    if !authorized(&h) {
                        store.lock().unwrap().refused += 1;
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    let req = ExportMetricsServiceRequest::decode(body).expect("valid OTLP");
                    store.lock().unwrap().metrics.push(req);
                    (
                        [("content-type", "application/x-protobuf")],
                        ExportMetricsServiceResponse::default().encode_to_vec(),
                    )
                        .into_response()
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, store)
}

#[cfg(feature = "otel-grpc")]
struct Grpc(Store);

#[cfg(feature = "otel-grpc")]
fn grpc_authorized<T>(req: &tonic::Request<T>) -> bool {
    req.metadata().get(KEY_HEADER).and_then(|v| v.to_str().ok()) == Some(KEY)
}

#[cfg(feature = "otel-grpc")]
#[tonic::async_trait]
impl TraceService for Grpc {
    async fn export(
        &self,
        request: tonic::Request<ExportTraceServiceRequest>,
    ) -> Result<tonic::Response<ExportTraceServiceResponse>, tonic::Status> {
        if !grpc_authorized(&request) {
            self.0.lock().unwrap().refused += 1;
            return Err(tonic::Status::unauthenticated("missing key"));
        }
        self.0.lock().unwrap().traces.push(request.into_inner());
        Ok(tonic::Response::new(ExportTraceServiceResponse::default()))
    }
}

#[cfg(feature = "otel-grpc")]
#[tonic::async_trait]
impl MetricsService for Grpc {
    async fn export(
        &self,
        request: tonic::Request<ExportMetricsServiceRequest>,
    ) -> Result<tonic::Response<ExportMetricsServiceResponse>, tonic::Status> {
        if !grpc_authorized(&request) {
            self.0.lock().unwrap().refused += 1;
            return Err(tonic::Status::unauthenticated("missing key"));
        }
        self.0.lock().unwrap().metrics.push(request.into_inner());
        Ok(tonic::Response::new(ExportMetricsServiceResponse::default()))
    }
}

/// OTLP/gRPC receiver (the collector's trace and metrics services).
#[cfg(feature = "otel-grpc")]
async fn grpc_receiver() -> (String, Store) {
    let store: Store = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let incoming = tonic::transport::server::TcpIncoming::from(listener);
    let traces = TraceServiceServer::new(Grpc(store.clone()));
    let metrics = MetricsServiceServer::new(Grpc(store.clone()));
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(traces)
            .add_service(metrics)
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });
    (url, store)
}

async fn gateway() -> (GatewayServer, String) {
    let ctx = GatewayContext {
        registry: Arc::new(ServerRegistry::default()),
        runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
        gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
        data_dir: data_root().to_path_buf(),
        downloads: None,
    };
    let s = GatewaySettings {
        enabled: true,
        port: free_port(),
        require_token: true,
        ..Default::default()
    };
    let server = GatewayServer::new();
    let status = server.start(ctx, &s).await.unwrap();
    (server, status.endpoint.unwrap().host_url)
}

async fn chat(http: &reqwest::Client, base: &str, token: &str, model: &str) -> u16 {
    http.post(format!("{base}/v1/chat/completions"))
        .bearer_auth(token)
        .json(
            &json!({"model": model, "messages": [{"role": "user", "content": "SECRET-PROMPT-93"}]}),
        )
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

async fn chat_with_trace(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    model: &str,
    traceparent: &str,
) -> u16 {
    http.post(format!("{base}/v1/chat/completions"))
        .bearer_auth(token)
        .header("traceparent", traceparent)
        .header("tracestate", "vendor=value")
        .json(
            &json!({"model": model, "messages": [{"role": "user", "content": "SECRET-PROMPT-93"}]}),
        )
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

fn limited_token(name: &str) -> String {
    TokenStore::at(&data_root().join("config"))
        .create_with_policy(
            name,
            &[Scope::Inference],
            &TokenPolicy {
                limits: RateLimits {
                    requests_per_minute: Some(1),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap()
        .token
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn http_protobuf_export_reaches_a_real_receiver() {
    let _s = serial().await;
    data_root();
    let (url, store) = http_receiver().await;
    let settings = OtelSettings {
        enabled: true,
        endpoint: format!("{url}/v1/traces"), // a pasted signal URL is reduced to the base
        protocol: OtelProtocol::HttpProtobuf,
        service_name: "harness-otel-test".into(),
        logs: true,
        metrics_interval_secs: 3600,
        ..Default::default()
    };

    // "Test export" really sends: refused without the key, accepted with it, and an
    // endpoint nobody listens on fails.
    let r = otel::test_export(&settings, Some("")).await.unwrap();
    assert!(!r.ok);
    assert!(r.error.as_deref().unwrap().contains("401"), "{r:?}");
    assert_eq!(r.url, format!("{url}/v1/traces"));
    let r = otel::test_export(&settings, Some(&format!("X-Otel-Key: {KEY}")))
        .await
        .unwrap();
    assert!(r.ok, "{r:?}");
    {
        let got = store.lock().unwrap();
        assert_eq!(got.refused, 1);
        let spans = got.spans();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].0.name, "harness.test_export");
        assert_eq!(hex(&spans[0].0.trace_id), r.trace_id);
        assert_eq!(spans[0].1, "harness-otel-test");
    }
    let closed = OtelSettings {
        endpoint: format!("http://127.0.0.1:{}", free_port()),
        ..settings.clone()
    };
    let r = otel::test_export(&closed, Some("")).await.unwrap();
    assert!(!r.ok && r.error.is_some(), "{r:?}");

    // Save (headers to the secret store only) and turn export on.
    let saved = otel::save_settings(&settings, Some(&format!("{KEY_HEADER}={KEY}"))).unwrap();
    assert_eq!(saved.endpoint, url);
    assert_eq!(saved.header_names, vec![KEY_HEADER.to_string()]);
    let file = std::fs::read_to_string(harness_core::config::AppSettings::config_path()).unwrap();
    assert!(
        file.contains(KEY_HEADER) && !file.contains(KEY),
        "no header value on disk"
    );
    otel::apply(&saved).await.unwrap();
    assert!(otel::status().active);

    // Gateway requests: a 404 and a rate-limited 429 of one token.
    let (gw, base) = gateway().await;
    let http = reqwest::Client::new();
    let token = limited_token("otel-http-token");
    let incoming = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    assert_eq!(
        chat_with_trace(&http, &base, &token, "no-such-model", incoming).await,
        404
    );
    assert_eq!(chat(&http, &base, &token, "no-such-model").await, 429);
    assert!(otel::flush(Duration::from_secs(15)).await);

    {
        let got = store.lock().unwrap();
        let spans: Vec<Span> = got
            .spans()
            .into_iter()
            .filter(|(s, svc)| {
                svc == "harness-otel-test"
                    && attr(&s.attributes, "harness.token_name").as_deref()
                        == Some("otel-http-token")
            })
            .map(|(s, _)| s)
            .collect();
        assert_eq!(spans.len(), 2, "one server span per request");
        let limited = spans
            .iter()
            .find(|s| attr(&s.attributes, "http.response.status_code").as_deref() == Some("429"))
            .expect("429 span");
        assert_eq!(
            attr(&limited.attributes, "error.type").as_deref(),
            Some("rate_limited")
        );
        assert_eq!(
            attr(&limited.attributes, "http.route").as_deref(),
            Some("/v1/chat/completions")
        );
        assert_eq!(limited.kind, 2, "SPAN_KIND_SERVER");
        assert_eq!(limited.name, "chat no-such-model");
        assert!(limited.end_time_unix_nano >= limited.start_time_unix_nano);
        let missing = spans
            .iter()
            .find(|s| attr(&s.attributes, "error.type").as_deref() == Some("model_not_found"))
            .expect("404 span");
        assert_eq!(
            attr(&missing.attributes, "gen_ai.request.model").as_deref(),
            Some("no-such-model")
        );
        assert_eq!(hex(&missing.trace_id), "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(hex(&missing.parent_span_id), "00f067aa0ba902b7");
        // Content is never exported unless both switches are on.
        let raw = format!("{:?}", got.traces);
        assert!(!raw.contains("SECRET-PROMPT-93"));

        let logs = got.log_records();
        assert_eq!(logs.len(), 2, "one log record per gateway request");
        let correlated = logs
            .iter()
            .find(|log| log.trace_id == missing.trace_id)
            .expect("log correlated to propagated trace");
        assert_eq!(correlated.span_id, missing.span_id);
        assert_eq!(correlated.event_name, "gateway_request");
        assert!(!format!("{logs:?}").contains("SECRET-PROMPT-93"));
    }

    // Metrics: the request counter with the error type and token.
    assert!(otel::flush(Duration::from_secs(15)).await);
    {
        let got = store.lock().unwrap();
        assert!(
            got.metric_names()
                .contains(&"harness.gateway.requests".to_string()),
            "{:?}",
            got.metric_names()
        );
        assert!(got
            .metric_names()
            .contains(&"gen_ai.client.operation.duration".to_string()));
        let points = got.sum_points("harness.gateway.requests");
        // Cumulative sums: every export repeats the running total, the latest is the count.
        let rate_limited = points
            .iter()
            .filter(|(a, _)| {
                attr(a, "error.type").as_deref() == Some("rate_limited")
                    && attr(a, "harness.token_name").as_deref() == Some("otel-http-token")
            })
            .map(|(_, v)| *v)
            .next_back();
        assert_eq!(rate_limited, Some(1));
    }

    // Off: nothing more is sent.
    let off = otel::save_settings(
        &OtelSettings {
            enabled: false,
            ..saved.clone()
        },
        None,
    )
    .unwrap();
    otel::apply(&off).await.unwrap();
    assert!(!otel::status().active);
    let before = store.lock().unwrap().traces.len();
    assert_eq!(chat(&http, &base, &token, "no-such-model").await, 429);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(store.lock().unwrap().traces.len(), before);
    gw.stop().await;
}

#[cfg(feature = "otel-grpc")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn grpc_export_reaches_a_real_receiver() {
    assert!(otel::status().grpc, "built with otel-grpc");
    let _s = serial().await;
    data_root();
    let (url, store) = grpc_receiver().await;
    let settings = OtelSettings {
        enabled: true,
        endpoint: url.clone(),
        protocol: OtelProtocol::Grpc,
        service_name: "harness-otel-grpc".into(),
        metrics_interval_secs: 3600,
        ..Default::default()
    };
    let r = otel::test_export(&settings, Some("")).await.unwrap();
    assert!(!r.ok, "unauthenticated without the key: {r:?}");
    let r = otel::test_export(&settings, Some(&format!("{KEY_HEADER}={KEY}")))
        .await
        .unwrap();
    assert!(r.ok, "{r:?}");
    assert_eq!(r.url, url);
    assert_eq!(
        hex(&store.lock().unwrap().spans()[0].0.trace_id),
        r.trace_id
    );

    let saved = otel::save_settings(&settings, Some(&format!("{KEY_HEADER}={KEY}"))).unwrap();
    otel::apply(&saved).await.unwrap();
    let (gw, base) = gateway().await;
    let http = reqwest::Client::new();
    let token = limited_token("otel-grpc-token");
    assert_eq!(chat(&http, &base, &token, "no-such-model").await, 404);
    assert_eq!(chat(&http, &base, &token, "no-such-model").await, 429);
    assert!(otel::flush(Duration::from_secs(15)).await);
    {
        let got = store.lock().unwrap();
        let spans: Vec<_> = got
            .spans()
            .into_iter()
            .filter(|(s, svc)| {
                svc == "harness-otel-grpc"
                    && attr(&s.attributes, "harness.token_name").as_deref()
                        == Some("otel-grpc-token")
            })
            .collect();
        assert_eq!(spans.len(), 2);
        assert!(spans
            .iter()
            .any(|(s, _)| attr(&s.attributes, "error.type").as_deref() == Some("rate_limited")));
        assert!(got
            .metric_names()
            .contains(&"harness.gateway.requests".to_string()));
    }
    otel::shutdown().await;
    assert!(!otel::status().active);
    gw.stop().await;
}

/// Default builds have no gRPC exporter: testing, saving and starting gRPC export are the keyed
/// error `otel.grpc_not_built` (nothing is sent, nothing stored), and the status says so.
#[cfg(not(feature = "otel-grpc"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn grpc_is_refused_without_the_feature() {
    let _s = serial().await;
    data_root();
    assert!(otel::status().built);
    assert!(!otel::status().grpc);
    let settings = OtelSettings {
        enabled: true,
        endpoint: format!("http://127.0.0.1:{}", free_port()),
        protocol: OtelProtocol::Grpc,
        ..Default::default()
    };
    assert_eq!(
        otel::test_export(&settings, Some("")).await.unwrap_err(),
        otel::ERR_GRPC_NOT_BUILT
    );
    let before = harness_core::config::AppSettings::load().otel;
    assert_eq!(
        otel::save_settings(&settings, None).unwrap_err(),
        otel::ERR_GRPC_NOT_BUILT
    );
    assert_eq!(harness_core::config::AppSettings::load().otel, before);
    // A settings file from a gRPC build: starting export fails clearly, nothing runs.
    assert_eq!(
        otel::apply(&settings).await.unwrap_err(),
        otel::ERR_GRPC_NOT_BUILT
    );
    assert!(!otel::status().active);
}
