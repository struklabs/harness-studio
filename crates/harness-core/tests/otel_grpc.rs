//! OTLP/gRPC export against a real gRPC receiver built from the official OTLP service
//! definitions (feature `otel-grpc`: `cargo test -p harness-core --features otel-grpc`).
//! "Test export" and the running exporter (spans from a finished gateway request record,
//! metrics) both go over gRPC with the export headers as metadata.
//!
//! Temporary data root (never the user's) and in-memory secrets.
#![cfg(feature = "otel-grpc")]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::gateway::record::RequestRecord;
use harness_core::otel::{self, OtelProtocol, OtelSettings};
use opentelemetry_proto::tonic::collector::metrics::v1::metrics_service_server::{
    MetricsService, MetricsServiceServer,
};
use opentelemetry_proto::tonic::collector::metrics::v1::{
    ExportMetricsServiceRequest, ExportMetricsServiceResponse,
};
use opentelemetry_proto::tonic::collector::trace::v1::trace_service_server::{
    TraceService, TraceServiceServer,
};
use opentelemetry_proto::tonic::collector::trace::v1::{
    ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use opentelemetry_proto::tonic::common::v1::{any_value, KeyValue};
use opentelemetry_proto::tonic::trace::v1::Span;

const KEY_HEADER: &str = "x-otel-key";
const KEY: &str = "grpc-receiver-secret-17";

#[derive(Default)]
struct Received {
    traces: Vec<ExportTraceServiceRequest>,
    metrics: Vec<ExportMetricsServiceRequest>,
    refused: usize,
}

type Store = Arc<Mutex<Received>>;

impl Received {
    /// Spans with their resource's `service.name`.
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
}

fn attr(attrs: &[KeyValue], key: &str) -> Option<String> {
    let v = attrs.iter().find(|kv| kv.key == key)?.value.as_ref()?;
    Some(match v.value.as_ref()? {
        any_value::Value::StringValue(s) => s.clone(),
        any_value::Value::IntValue(i) => i.to_string(),
        any_value::Value::BoolValue(b) => b.to_string(),
        other => format!("{other:?}"),
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

struct Grpc(Store);

fn authorized<T>(req: &tonic::Request<T>) -> bool {
    req.metadata().get(KEY_HEADER).and_then(|v| v.to_str().ok()) == Some(KEY)
}

#[tonic::async_trait]
impl TraceService for Grpc {
    async fn export(
        &self,
        request: tonic::Request<ExportTraceServiceRequest>,
    ) -> Result<tonic::Response<ExportTraceServiceResponse>, tonic::Status> {
        if !authorized(&request) {
            self.0.lock().unwrap().refused += 1;
            return Err(tonic::Status::unauthenticated("missing key"));
        }
        self.0.lock().unwrap().traces.push(request.into_inner());
        Ok(tonic::Response::new(ExportTraceServiceResponse::default()))
    }
}

#[tonic::async_trait]
impl MetricsService for Grpc {
    async fn export(
        &self,
        request: tonic::Request<ExportMetricsServiceRequest>,
    ) -> Result<tonic::Response<ExportMetricsServiceResponse>, tonic::Status> {
        if !authorized(&request) {
            self.0.lock().unwrap().refused += 1;
            return Err(tonic::Status::unauthenticated("missing key"));
        }
        self.0.lock().unwrap().metrics.push(request.into_inner());
        Ok(tonic::Response::new(ExportMetricsServiceResponse::default()))
    }
}

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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn grpc_test_export_and_pipeline_reach_a_real_receiver() {
    harness_core::secrets::use_memory_store();
    let dir = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("core-otel-grpc"))
        .tempdir()
        .unwrap();
    harness_core::paths::init(dir.path().to_path_buf());
    assert!(otel::status().built && otel::status().grpc);

    let (url, store) = grpc_receiver().await;
    let settings = OtelSettings {
        enabled: true,
        endpoint: url.clone(),
        protocol: OtelProtocol::Grpc,
        service_name: "harness-core-grpc".into(),
        metrics_interval_secs: 3600,
        ..Default::default()
    };

    // "Test export": refused without the key (the receiver's answer), accepted with it.
    let r = otel::test_export(&settings, Some("")).await.unwrap();
    assert!(!r.ok && r.error.is_some(), "{r:?}");
    assert_eq!(r.protocol, OtelProtocol::Grpc);
    let r = otel::test_export(&settings, Some(&format!("{KEY_HEADER}={KEY}")))
        .await
        .unwrap();
    assert!(r.ok, "{r:?}");
    assert_eq!(r.url, url);
    {
        let got = store.lock().unwrap();
        assert_eq!(got.refused, 1);
        let spans = got.spans();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].0.name, "harness.test_export");
        assert_eq!(hex(&spans[0].0.trace_id), r.trace_id);
        assert_eq!(spans[0].1, "harness-core-grpc");
    }

    // The running exporter: headers from the secret store, spans and metrics of a record.
    let saved = otel::save_settings(&settings, Some(&format!("{KEY_HEADER}={KEY}"))).unwrap();
    otel::apply(&saved).await.unwrap();
    assert!(otel::status().active);
    let record = RequestRecord {
        id: "req_grpc_1".into(),
        started_at_ms: 1_700_000_000_000,
        endpoint: "/v1/chat/completions".into(),
        model: Some("no-such-model".into()),
        status: 404,
        error_code: Some("model_not_found".into()),
        ..Default::default()
    };
    otel::record(&record, None, None);
    assert!(otel::flush(Duration::from_secs(15)).await);
    {
        let got = store.lock().unwrap();
        let spans: Vec<Span> = got
            .spans()
            .into_iter()
            .filter(|(s, svc)| {
                svc == "harness-core-grpc"
                    && attr(&s.attributes, "harness.request_id").as_deref() == Some("req_grpc_1")
            })
            .map(|(s, _)| s)
            .collect();
        assert_eq!(spans.len(), 1, "one server span");
        assert_eq!(spans[0].name, "chat no-such-model");
        assert_eq!(
            attr(&spans[0].attributes, "error.type").as_deref(),
            Some("model_not_found")
        );
        assert!(got
            .metric_names()
            .contains(&"harness.gateway.requests".to_string()));
        assert_eq!(got.refused, 1, "the pipeline sent the saved key");
    }
    otel::shutdown().await;
    assert!(!otel::status().active);
}
