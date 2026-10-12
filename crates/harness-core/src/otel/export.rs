//! The exporter (cargo feature `otel`): OTLP span, log and metric exporters of
//! `opentelemetry-otlp`, fed from finished gateway requests.
//!
//! * Spans are built from the request record after the fact (explicit start/end times from
//!   the record), queued (bounded; a full queue drops and counts) and exported in batches by
//!   one task on the caller's Tokio runtime.
//! * Metrics go through the SDK meter provider and its periodic reader (own thread). That
//!   thread is not a Tokio worker, so the OTLP metric exporter runs on a small runtime of its
//!   own ([`Bridged`]), where it was also built.
//! * [`test_export`] builds a fresh span exporter and sends one span, so the result is the
//!   collector's real answer (not a queued "maybe later").
//! * OTLP/HTTP is sent with the workspace's reqwest 0.12 ([`ReqwestHttp`]; OS TLS like every
//!   other request of the app); OTLP/gRPC (tonic) only with the cargo feature `otel-grpc`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use opentelemetry::logs::{AnyValue, LogRecord as _, Logger as _, LoggerProvider as _, Severity};
use opentelemetry::metrics::{Counter, Histogram, MeterProvider as _};
use opentelemetry::trace::{
    Event, SpanContext, SpanId, SpanKind, Status, TraceFlags, TraceId, TraceState,
};
use opentelemetry::{InstrumentationScope, KeyValue};
use opentelemetry_http::{Bytes, HttpError, Request, Response, ResponseBodyTooLarge};
#[cfg(feature = "otel-grpc")]
use opentelemetry_otlp::tonic_types::{metadata::MetadataMap, transport::ClientTlsConfig};
#[cfg(feature = "otel-grpc")]
use opentelemetry_otlp::WithTonicConfig;
use opentelemetry_otlp::{
    LogExporter, MetricExporter, Protocol, RetryPolicy, SpanExporter, WithExportConfig,
    WithHttpConfig,
};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::logs::{
    LogBatch, LogExporter as _, SdkLogRecord, SdkLogger, SdkLoggerProvider,
};
use opentelemetry_sdk::metrics::data::ResourceMetrics;
use opentelemetry_sdk::metrics::exporter::PushMetricExporter;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider, Temporality};
use opentelemetry_sdk::trace::{SpanData, SpanEvents, SpanExporter as _, SpanLinks};
use opentelemetry_sdk::Resource;
use tokio::sync::{mpsc, oneshot};

use super::{
    OtelProtocol, OtelSettings, OtelStatus, RequestTraceContext, TestExportResult, CONTENT_LIMIT,
};
use crate::analytics::CapturedContent;
use crate::gateway::record::RequestRecord;

/// Spans waiting for export; more are dropped (and counted).
const QUEUE: usize = 4096;
/// Largest span batch per export call.
const BATCH: usize = 512;
/// Longest time a finished request waits before its span is sent.
const BATCH_DELAY: Duration = Duration::from_secs(2);
const EXPORT_TIMEOUT: Duration = Duration::from_secs(10);
/// Largest collector answer read into memory (the limit of opentelemetry-http's own clients).
const MAX_RESPONSE_BODY: usize = 4 * 1024 * 1024;
const SCOPE: &str = "harness-gateway";

enum SpanMsg {
    Span(Box<SpanData>),
    Flush(oneshot::Sender<()>),
}

enum LogMsg {
    Record(Box<SdkLogRecord>),
    Flush(oneshot::Sender<()>),
}

struct Meters {
    provider: SdkMeterProvider,
    requests: Counter<u64>,
    duration: Histogram<f64>,
    tokens: Histogram<u64>,
    ttft: Histogram<f64>,
    queue: Histogram<f64>,
}

struct Pipeline {
    settings: OtelSettings,
    spans: Option<mpsc::Sender<SpanMsg>>,
    span_task: Option<tokio::task::JoinHandle<()>>,
    logs: Option<mpsc::Sender<LogMsg>>,
    log_task: Option<tokio::task::JoinHandle<()>>,
    meters: Option<Meters>,
}

#[derive(Default)]
struct Counters {
    exported: AtomicU64,
    dropped: AtomicU64,
    logs_exported: AtomicU64,
    logs_dropped: AtomicU64,
    errors: AtomicU64,
    last_export_ms: AtomicU64,
    last_error: Mutex<Option<String>>,
}

fn counters() -> &'static Counters {
    static C: OnceLock<Counters> = OnceLock::new();
    C.get_or_init(Counters::default)
}

fn pipeline() -> &'static RwLock<Option<Pipeline>> {
    static P: RwLock<Option<Pipeline>> = RwLock::new(None);
    &P
}

/// One apply/shutdown at a time.
fn apply_lock() -> &'static tokio::sync::Mutex<()> {
    static L: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    &L
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn set_error(e: String) {
    counters().errors.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut l) = counters().last_error.lock() {
        *l = Some(e);
    }
}

// The SDK reduces HTTP client failures to "network error". Keep the cause per exporter.
#[derive(Debug, Clone, Default)]
struct TransportErrors(Arc<Mutex<Option<String>>>);

impl TransportErrors {
    fn text(&self, error: impl std::fmt::Display) -> String {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .unwrap_or_else(|| error.to_string())
    }
}

struct TrackedExporter<T> {
    exporter: T,
    errors: TransportErrors,
}

impl<T> std::ops::Deref for TrackedExporter<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.exporter
    }
}

fn resource(settings: &OtelSettings) -> Resource {
    Resource::builder_empty()
        .with_service_name(settings.service_name.clone())
        .with_attributes([
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            KeyValue::new("telemetry.sdk.language", "rust"),
            KeyValue::new("host.arch", std::env::consts::ARCH),
            KeyValue::new("os.type", std::env::consts::OS),
        ])
        .build()
}

fn scope() -> InstrumentationScope {
    InstrumentationScope::builder(SCOPE)
        .with_version(env!("CARGO_PKG_VERSION"))
        .build()
}

/// The OTLP/HTTP transport: the app's reqwest 0.12 behind `opentelemetry_http::HttpClient`
/// (opentelemetry-http only implements it for reqwest 0.13, a second copy of reqwest and
/// hyper-util in the binary). TLS is reqwest's default here (the OS: SChannel / Security
/// framework / OpenSSL), like the rest of the app. Mirrors the built-in client: the whole
/// answer in memory, at most [`MAX_RESPONSE_BODY`].
#[derive(Debug)]
struct ReqwestHttp(reqwest::Client, TransportErrors);

#[async_trait::async_trait]
impl opentelemetry_http::HttpClient for ReqwestHttp {
    async fn send_bytes(&self, request: Request<Bytes>) -> Result<Response<Bytes>, HttpError> {
        let url = request.uri().to_string();
        self.1 .0.lock().unwrap_or_else(|e| e.into_inner()).take();
        let failed = |e: reqwest::Error| {
            let error = crate::http_error::transport(&e, &url);
            *self.1 .0.lock().unwrap_or_else(|e| e.into_inner()) = Some(error.clone());
            std::io::Error::other(error)
        };
        let request = reqwest::Request::try_from(request).map_err(failed)?;
        let mut response = self.0.execute(request).await.map_err(failed)?;
        let status = response.status();
        let headers = std::mem::take(response.headers_mut());
        let capacity = response
            .content_length()
            .unwrap_or(0)
            .min(MAX_RESPONSE_BODY as u64) as usize;
        let mut body = Vec::with_capacity(capacity);
        while let Some(chunk) = response.chunk().await.map_err(failed)? {
            if body.len() + chunk.len() > MAX_RESPONSE_BODY {
                return Err(Box::new(ResponseBodyTooLarge::new()));
            }
            body.extend_from_slice(&chunk);
        }
        let mut out = Response::builder().status(status).body(Bytes::from(body))?;
        *out.headers_mut() = headers;
        Ok(out)
    }
}

fn http_client(url: &str, errors: TransportErrors) -> Result<ReqwestHttp, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(EXPORT_TIMEOUT)
        .build()
        .map(|client| ReqwestHttp(client, errors))
        .map_err(|e| crate::http_error::transport(&e, url))
}

#[cfg(feature = "otel-grpc")]
fn metadata(headers: &[(String, String)]) -> MetadataMap {
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
    let mut map = HeaderMap::new();
    for (n, v) in headers {
        if let (Ok(k), Ok(v)) = (
            HeaderName::from_bytes(n.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            map.insert(k, v);
        }
    }
    MetadataMap::from_headers(map)
}

#[cfg(feature = "otel-grpc")]
fn tls(endpoint: &str) -> Option<ClientTlsConfig> {
    endpoint
        .to_ascii_lowercase()
        .starts_with("https://")
        .then(|| ClientTlsConfig::new().with_native_roots())
}

/// A span exporter for `settings` (gRPC needs a Tokio runtime context: the channel's worker
/// is spawned there). gRPC without `otel-grpc` is `otel.grpc_not_built` (normally already
/// refused by `validate_settings`).
fn span_exporter(
    settings: &OtelSettings,
    headers: &[(String, String)],
    retry: RetryPolicy,
) -> Result<TrackedExporter<SpanExporter>, String> {
    let errors = TransportErrors::default();
    let map: HashMap<String, String> = headers.iter().cloned().collect();
    let mut exporter = match settings.protocol {
        OtelProtocol::HttpProtobuf => SpanExporter::builder()
            .with_http()
            .with_protocol(Protocol::HttpBinary)
            .with_endpoint(settings.traces_url())
            .with_timeout(EXPORT_TIMEOUT)
            .with_http_client(http_client(&settings.traces_url(), errors.clone())?)
            .with_headers(map)
            .with_retry_policy(retry)
            .build(),
        #[cfg(feature = "otel-grpc")]
        OtelProtocol::Grpc => {
            let mut b = SpanExporter::builder()
                .with_tonic()
                .with_endpoint(settings.traces_url())
                .with_timeout(EXPORT_TIMEOUT)
                .with_metadata(metadata(headers))
                .with_retry_policy(retry);
            if let Some(t) = tls(&settings.endpoint) {
                b = b.with_tls_config(t);
            }
            b.build()
        }
        #[cfg(not(feature = "otel-grpc"))]
        OtelProtocol::Grpc => return Err(super::ERR_GRPC_NOT_BUILT.to_string()),
    }
    .map_err(|e| e.to_string())?;
    exporter.set_resource(&resource(settings));
    Ok(TrackedExporter { exporter, errors })
}

fn metric_exporter(
    settings: &OtelSettings,
    headers: &[(String, String)],
) -> Result<TrackedExporter<MetricExporter>, String> {
    let errors = TransportErrors::default();
    let map: HashMap<String, String> = headers.iter().cloned().collect();
    match settings.protocol {
        OtelProtocol::HttpProtobuf => MetricExporter::builder()
            .with_http()
            .with_protocol(Protocol::HttpBinary)
            .with_endpoint(settings.metrics_url())
            .with_timeout(EXPORT_TIMEOUT)
            .with_http_client(http_client(&settings.metrics_url(), errors.clone())?)
            .with_headers(map)
            .with_temporality(Temporality::Cumulative)
            .build(),
        #[cfg(feature = "otel-grpc")]
        OtelProtocol::Grpc => {
            let mut b = MetricExporter::builder()
                .with_tonic()
                .with_endpoint(settings.metrics_url())
                .with_timeout(EXPORT_TIMEOUT)
                .with_metadata(metadata(headers))
                .with_temporality(Temporality::Cumulative);
            if let Some(t) = tls(&settings.endpoint) {
                b = b.with_tls_config(t);
            }
            b.build()
        }
        #[cfg(not(feature = "otel-grpc"))]
        OtelProtocol::Grpc => return Err(super::ERR_GRPC_NOT_BUILT.to_string()),
    }
    .map(|exporter| TrackedExporter { exporter, errors })
    .map_err(|e| e.to_string())
}

fn log_exporter(
    settings: &OtelSettings,
    headers: &[(String, String)],
) -> Result<TrackedExporter<LogExporter>, String> {
    let errors = TransportErrors::default();
    let map: HashMap<String, String> = headers.iter().cloned().collect();
    let mut exporter = match settings.protocol {
        OtelProtocol::HttpProtobuf => LogExporter::builder()
            .with_http()
            .with_protocol(Protocol::HttpBinary)
            .with_endpoint(settings.logs_url())
            .with_timeout(EXPORT_TIMEOUT)
            .with_http_client(http_client(&settings.logs_url(), errors.clone())?)
            .with_headers(map)
            .build(),
        #[cfg(feature = "otel-grpc")]
        OtelProtocol::Grpc => {
            let mut b = LogExporter::builder()
                .with_tonic()
                .with_endpoint(settings.logs_url())
                .with_timeout(EXPORT_TIMEOUT)
                .with_metadata(metadata(headers));
            if let Some(t) = tls(&settings.endpoint) {
                b = b.with_tls_config(t);
            }
            b.build()
        }
        #[cfg(not(feature = "otel-grpc"))]
        OtelProtocol::Grpc => return Err(super::ERR_GRPC_NOT_BUILT.to_string()),
    }
    .map_err(|e| e.to_string())?;
    exporter.set_resource(&resource(settings));
    Ok(TrackedExporter { exporter, errors })
}

/// The OTLP metric exporter on a runtime of its own, driven by the periodic reader's thread.
struct Bridged {
    rt: Option<tokio::runtime::Runtime>,
    inner: TrackedExporter<MetricExporter>,
}

impl Bridged {
    fn new(settings: &OtelSettings, headers: &[(String, String)]) -> Result<Self, String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .thread_name("harness-otel-metrics")
            .build()
            .map_err(|e| e.to_string())?;
        let inner = {
            let _enter = rt.enter();
            metric_exporter(settings, headers)?
        };
        Ok(Self {
            rt: Some(rt),
            inner,
        })
    }
}

impl Drop for Bridged {
    fn drop(&mut self) {
        // May be dropped inside an async context, where a blocking runtime drop panics.
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}

impl PushMetricExporter for Bridged {
    async fn export(&self, metrics: &ResourceMetrics) -> OTelSdkResult {
        let result = match self.rt.as_ref() {
            Some(rt) => rt.block_on(self.inner.export(metrics)),
            None => Ok(()),
        };
        if let Err(e) = &result {
            set_error(self.inner.errors.text(format!("metrics: {}", e)));
        }
        result
    }

    fn force_flush(&self) -> OTelSdkResult {
        self.inner.force_flush()
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        self.inner.shutdown_with_timeout(timeout)
    }

    fn temporality(&self) -> Temporality {
        self.inner.temporality()
    }
}

fn meters(settings: &OtelSettings, headers: &[(String, String)]) -> Result<Meters, String> {
    let reader = PeriodicReader::builder(Bridged::new(settings, headers)?)
        .with_interval(Duration::from_secs(u64::from(
            settings.metrics_interval_secs,
        )))
        .build();
    let provider = SdkMeterProvider::builder()
        .with_resource(resource(settings))
        .with_reader(reader)
        .build();
    let meter = provider.meter_with_scope(scope());
    Ok(Meters {
        requests: meter
            .u64_counter("harness.gateway.requests")
            .with_description("Gateway requests")
            .with_unit("{request}")
            .build(),
        duration: meter
            .f64_histogram("gen_ai.client.operation.duration")
            .with_description("Gateway request duration (arrival to last byte)")
            .with_unit("s")
            .build(),
        tokens: meter
            .u64_histogram("gen_ai.client.token.usage")
            .with_description("Prompt and completion tokens per request")
            .with_unit("{token}")
            .build(),
        ttft: meter
            .f64_histogram("harness.gateway.time_to_first_token")
            .with_description("Arrival to first response byte")
            .with_unit("s")
            .build(),
        queue: meter
            .f64_histogram("harness.gateway.queue_wait")
            .with_description("Wait for a slot, including a JIT load")
            .with_unit("s")
            .build(),
        provider,
    })
}

async fn span_task(exporter: TrackedExporter<SpanExporter>, mut rx: mpsc::Receiver<SpanMsg>) {
    let mut batch: Vec<SpanData> = Vec::new();
    let mut waiters: Vec<oneshot::Sender<()>> = Vec::new();
    let mut tick = tokio::time::interval(BATCH_DELAY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        let open = tokio::select! {
            msg = rx.recv() => match msg {
                Some(SpanMsg::Span(s)) => { batch.push(*s); true }
                Some(SpanMsg::Flush(w)) => { waiters.push(w); true }
                None => false,
            },
            _ = tick.tick() => true,
        };
        let due = !open || !waiters.is_empty() || batch.len() >= BATCH || {
            // The tick fired (or nothing else happened): send what is there.
            rx.is_empty()
        };
        if due && !batch.is_empty() {
            while !batch.is_empty() {
                let rest = batch.split_off(batch.len().min(BATCH));
                let chunk = std::mem::replace(&mut batch, rest);
                let n = chunk.len() as u64;
                match exporter.export(chunk).await {
                    Ok(()) => {
                        counters().exported.fetch_add(n, Ordering::Relaxed);
                        counters().last_export_ms.store(now_ms(), Ordering::Relaxed);
                    }
                    Err(e) => set_error(exporter.errors.text(format!("traces: {}", e))),
                }
            }
        }
        for w in waiters.drain(..) {
            let _ = w.send(());
        }
        if !open {
            break;
        }
    }
    let _ = exporter.shutdown();
}

async fn log_task(exporter: TrackedExporter<LogExporter>, mut rx: mpsc::Receiver<LogMsg>) {
    let mut batch: Vec<SdkLogRecord> = Vec::new();
    let mut waiters: Vec<oneshot::Sender<()>> = Vec::new();
    let mut tick = tokio::time::interval(BATCH_DELAY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        let open = tokio::select! {
            msg = rx.recv() => match msg {
                Some(LogMsg::Record(record)) => { batch.push(*record); true }
                Some(LogMsg::Flush(w)) => { waiters.push(w); true }
                None => false,
            },
            _ = tick.tick() => true,
        };
        let due = !open || !waiters.is_empty() || batch.len() >= BATCH || rx.is_empty();
        if due && !batch.is_empty() {
            while !batch.is_empty() {
                let rest = batch.split_off(batch.len().min(BATCH));
                let chunk = std::mem::replace(&mut batch, rest);
                let instrumentation = scope();
                let scoped: Vec<_> = chunk
                    .iter()
                    .map(|record| (record, &instrumentation))
                    .collect();
                let n = chunk.len() as u64;
                match exporter.export(LogBatch::new(&scoped)).await {
                    Ok(()) => {
                        counters().logs_exported.fetch_add(n, Ordering::Relaxed);
                        counters().last_export_ms.store(now_ms(), Ordering::Relaxed);
                    }
                    Err(e) => set_error(exporter.errors.text(format!("logs: {}", e))),
                }
            }
        }
        for waiter in waiters.drain(..) {
            let _ = waiter.send(());
        }
        if !open {
            break;
        }
    }
    let _ = exporter.shutdown();
}

pub(super) async fn apply(settings: &OtelSettings) -> Result<(), String> {
    let _serial = apply_lock().lock().await;
    stop_locked().await;
    if !settings.enabled || (!settings.traces && !settings.logs && !settings.metrics) {
        return Ok(());
    }
    let settings = super::validate_settings(settings)?;
    let headers = {
        let s = tokio::task::spawn_blocking(super::saved_headers)
            .await
            .map_err(|e| e.to_string())?;
        s?
    };
    let (spans, span_task) = if settings.traces {
        let exporter = span_exporter(&settings, &headers, RetryPolicy::recommended())?;
        let (tx, rx) = mpsc::channel(QUEUE);
        (Some(tx), Some(tokio::spawn(span_task(exporter, rx))))
    } else {
        (None, None)
    };
    let (logs, log_task) = if settings.logs {
        let exporter = log_exporter(&settings, &headers)?;
        let (tx, rx) = mpsc::channel(QUEUE);
        (Some(tx), Some(tokio::spawn(log_task(exporter, rx))))
    } else {
        (None, None)
    };
    let meters = if settings.metrics {
        let s = settings.clone();
        let h = headers.clone();
        // The periodic reader starts a thread and the bridge a runtime: off the async worker.
        Some(
            tokio::task::spawn_blocking(move || meters(&s, &h))
                .await
                .map_err(|e| e.to_string())??,
        )
    } else {
        None
    };
    if let Ok(mut l) = counters().last_error.lock() {
        *l = None;
    }
    eprintln!(
        "[Harness OTel] OTLP dışa aktarma açık: {} ({:?}).",
        settings.endpoint, settings.protocol
    );
    *pipeline().write().unwrap_or_else(|e| e.into_inner()) = Some(Pipeline {
        settings,
        spans,
        span_task,
        logs,
        log_task,
        meters,
    });
    Ok(())
}

/// Stops the running pipeline after exporting what it holds (caller holds the apply lock).
async fn stop_locked() {
    let old = pipeline().write().unwrap_or_else(|e| e.into_inner()).take();
    let Some(mut p) = old else {
        return;
    };
    // Closing the channel ends the task after a last export.
    drop(p.spans.take());
    drop(p.logs.take());
    if let Some(task) = p.span_task.take() {
        let _ = tokio::time::timeout(EXPORT_TIMEOUT + Duration::from_secs(2), task).await;
    }
    if let Some(task) = p.log_task.take() {
        let _ = tokio::time::timeout(EXPORT_TIMEOUT + Duration::from_secs(2), task).await;
    }
    if let Some(m) = p.meters.take() {
        let _ = tokio::task::spawn_blocking(move || {
            let _ = m.provider.shutdown();
        })
        .await;
    }
}

pub(super) async fn shutdown() {
    let _serial = apply_lock().lock().await;
    stop_locked().await;
}

pub(super) async fn flush(timeout: Duration) -> bool {
    let (spans, logs, provider) = {
        let g = pipeline().read().unwrap_or_else(|e| e.into_inner());
        match g.as_ref() {
            Some(p) => (
                p.spans.clone(),
                p.logs.clone(),
                p.meters.as_ref().map(|m| m.provider.clone()),
            ),
            None => return true,
        }
    };
    let work = async move {
        if let Some(tx) = spans {
            let (w, done) = oneshot::channel();
            if tx.send(SpanMsg::Flush(w)).await.is_ok() {
                let _ = done.await;
            }
        }
        if let Some(tx) = logs {
            let (w, done) = oneshot::channel();
            if tx.send(LogMsg::Flush(w)).await.is_ok() {
                let _ = done.await;
            }
        }
        if let Some(p) = provider {
            let _ = tokio::task::spawn_blocking(move || p.force_flush()).await;
        }
    };
    tokio::time::timeout(timeout, work).await.is_ok()
}

pub(super) fn status() -> OtelStatus {
    let g = pipeline().read().unwrap_or_else(|e| e.into_inner());
    let c = counters();
    let last = c.last_export_ms.load(Ordering::Relaxed);
    OtelStatus {
        built: true,
        grpc: super::grpc_built(),
        active: g.is_some(),
        traces_url: g
            .as_ref()
            .filter(|p| p.spans.is_some())
            .map(|p| p.settings.traces_url()),
        metrics_url: g
            .as_ref()
            .filter(|p| p.meters.is_some())
            .map(|p| p.settings.metrics_url()),
        logs_url: g
            .as_ref()
            .filter(|p| p.logs.is_some())
            .map(|p| p.settings.logs_url()),
        spans_exported: c.exported.load(Ordering::Relaxed),
        spans_dropped: c.dropped.load(Ordering::Relaxed),
        logs_exported: c.logs_exported.load(Ordering::Relaxed),
        logs_dropped: c.logs_dropped.load(Ordering::Relaxed),
        export_errors: c.errors.load(Ordering::Relaxed),
        last_error: c.last_error.lock().ok().and_then(|l| l.clone()),
        last_export_ms: (last > 0).then_some(last),
    }
}

pub(super) fn record(
    r: &RequestRecord,
    content: Option<&CapturedContent>,
    incoming: Option<&RequestTraceContext>,
) {
    let g = pipeline().read().unwrap_or_else(|e| e.into_inner());
    let Some(p) = g.as_ref() else {
        return;
    };
    if let Some(m) = &p.meters {
        observe(m, r);
    }
    let (span_context, parent, parent_remote) = request_context(incoming);
    let content = content.filter(|_| p.settings.include_content);
    if let Some(tx) = &p.spans {
        for span in spans_of(r, content, span_context.clone(), parent, parent_remote) {
            if tx.try_send(SpanMsg::Span(Box::new(span))).is_err() {
                counters().dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    if let Some(tx) = &p.logs {
        if tx
            .try_send(LogMsg::Record(Box::new(log_of(r, content, &span_context))))
            .is_err()
        {
            counters().logs_dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn observe(m: &Meters, r: &RequestRecord) {
    let model = r
        .alias
        .clone()
        .or_else(|| r.model.clone())
        .unwrap_or_default();
    let mut attrs = vec![
        KeyValue::new("gen_ai.operation.name", operation(&r.endpoint)),
        KeyValue::new("gen_ai.response.model", model),
        KeyValue::new("http.route", r.endpoint.clone()),
        KeyValue::new("http.response.status_code", i64::from(r.status)),
    ];
    if let Some(e) = &r.error_code {
        attrs.push(KeyValue::new("error.type", e.clone()));
    }
    let mut with_token = attrs.clone();
    if let Some(t) = &r.client.token_name {
        with_token.push(KeyValue::new("harness.token_name", t.clone()));
    }
    m.requests.add(1, &with_token);
    m.duration.record(r.total_ms as f64 / 1000.0, &attrs);
    if let Some(t) = r.ttft_ms {
        m.ttft.record(t as f64 / 1000.0, &attrs);
    }
    if r.queue_wait_ms > 0 || r.server_id.is_some() {
        m.queue.record(r.queue_wait_ms as f64 / 1000.0, &attrs);
    }
    for (kind, n) in [
        ("input", r.usage.prompt_tokens),
        ("output", r.usage.completion_tokens),
    ] {
        if let Some(n) = n {
            let mut a = attrs.clone();
            a.push(KeyValue::new("gen_ai.token.type", kind));
            m.tokens.record(n, &a);
        }
    }
}

/// `gen_ai.operation.name` of a gateway route.
fn operation(endpoint: &str) -> &'static str {
    match endpoint {
        "/v1/embeddings" | "/api/embed" | "/api/embeddings" => "embeddings",
        "/v1/rerank" => "rerank",
        "/v1/completions" | "/api/generate" => "text_completion",
        "/v1/messages/count_tokens" => "count_tokens",
        _ => "chat",
    }
}

fn random_ids() -> (TraceId, SpanId) {
    let mut buf = [0u8; 24];
    if getrandom::fill(&mut buf).is_err() || buf.iter().all(|b| *b == 0) {
        // Never all-zero (invalid ids); time-based fallback.
        let n = now_ms().to_le_bytes();
        buf[..8].copy_from_slice(&n);
        buf[16..].copy_from_slice(&n);
        buf[23] |= 1;
        buf[15] |= 1;
    }
    let mut t = [0u8; 16];
    let mut s = [0u8; 8];
    t.copy_from_slice(&buf[..16]);
    s.copy_from_slice(&buf[16..]);
    (TraceId::from_bytes(t), SpanId::from_bytes(s))
}

fn new_span_id() -> SpanId {
    random_ids().1
}

fn request_context(incoming: Option<&RequestTraceContext>) -> (SpanContext, SpanId, bool) {
    let (trace_id, parent, flags, state, remote) = match incoming {
        Some(context) => (
            TraceId::from_bytes(context.trace_id),
            SpanId::from_bytes(context.parent_span_id),
            TraceFlags::new(context.flags),
            context
                .tracestate
                .as_deref()
                .and_then(|s| s.parse::<TraceState>().ok())
                .unwrap_or_default(),
            true,
        ),
        None => {
            let (trace_id, _) = random_ids();
            (
                trace_id,
                SpanId::INVALID,
                TraceFlags::SAMPLED,
                TraceState::default(),
                false,
            )
        }
    };
    (
        SpanContext::new(trace_id, new_span_id(), flags, false, state),
        parent,
        remote,
    )
}

fn log_record_factory() -> &'static SdkLogger {
    static LOGGER: OnceLock<SdkLogger> = OnceLock::new();
    LOGGER.get_or_init(|| {
        SdkLoggerProvider::builder()
            .build()
            .logger_with_scope(scope())
    })
}

fn log_of(
    r: &RequestRecord,
    content: Option<&CapturedContent>,
    context: &SpanContext,
) -> SdkLogRecord {
    let mut log = log_record_factory().create_log_record();
    log.set_event_name("gateway_request");
    log.set_target(SCOPE);
    log.set_timestamp(at(r.started_at_ms + r.total_ms));
    log.set_observed_timestamp(SystemTime::now());
    log.set_trace_context(
        context.trace_id(),
        context.span_id(),
        Some(context.trace_flags()),
    );
    let (severity, text) = if r.status >= 500 {
        (Severity::Error, "ERROR")
    } else if r.status >= 400 || r.cancelled {
        (Severity::Warn, "WARN")
    } else {
        (Severity::Info, "INFO")
    };
    log.set_severity_number(severity);
    log.set_severity_text(text);
    log.set_body(AnyValue::from(
        serde_json::to_string(r).unwrap_or_else(|_| "{}".to_string()),
    ));
    log.add_attribute("harness.request_id", r.id.clone());
    log.add_attribute("http.route", r.endpoint.clone());
    log.add_attribute("http.response.status_code", i64::from(r.status));
    log.add_attribute("harness.cancelled", r.cancelled);
    log.add_attribute("harness.total_ms", r.total_ms as i64);
    if let Some(model) = r.alias.clone().or_else(|| r.model.clone()) {
        log.add_attribute("gen_ai.response.model", model);
    }
    if let Some(error) = &r.error_code {
        log.add_attribute("error.type", error.clone());
    }
    if let Some(c) = content {
        log.add_attribute("harness.request.body", truncated(&c.request));
        log.add_attribute("harness.response.body", truncated(&c.response));
    }
    log
}

fn at(ms: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(ms)
}

fn truncated(text: &str) -> String {
    if text.len() <= CONTENT_LIMIT {
        return text.to_string();
    }
    let mut end = CONTENT_LIMIT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[allow(clippy::too_many_arguments)]
fn span(
    ctx: SpanContext,
    parent: SpanId,
    kind: SpanKind,
    name: String,
    start: SystemTime,
    end: SystemTime,
    attributes: Vec<KeyValue>,
    events: Vec<Event>,
    status: Status,
    parent_remote: bool,
) -> SpanData {
    SpanData {
        span_context: ctx,
        parent_span_id: parent,
        parent_span_is_remote: parent_remote,
        span_kind: kind,
        name: name.into(),
        start_time: start,
        end_time: end.max(start),
        attributes,
        dropped_attributes_count: 0,
        events: {
            let mut e = SpanEvents::default();
            e.events = events;
            e
        },
        links: SpanLinks::default(),
        status,
        instrumentation_scope: scope(),
    }
}

/// The spans of one request: the server span, and child spans for the queue wait, the JIT
/// load and generation (first token as an event).
pub(crate) fn spans_of(
    r: &RequestRecord,
    content: Option<&CapturedContent>,
    root_context: SpanContext,
    parent: SpanId,
    parent_remote: bool,
) -> Vec<SpanData> {
    let trace_id = root_context.trace_id();
    let root_id = root_context.span_id();
    let flags = root_context.trace_flags();
    let state = root_context.trace_state().clone();
    let ctx = |id| SpanContext::new(trace_id, id, flags, false, state.clone());
    let start_ms = r.started_at_ms;
    let end_ms = start_ms + r.total_ms;
    let op = operation(&r.endpoint);
    let model = r.alias.clone().or_else(|| r.model.clone());
    let mut a = vec![
        KeyValue::new("gen_ai.operation.name", op),
        KeyValue::new("gen_ai.provider.name", "llama.cpp"),
        KeyValue::new("http.request.method", "POST"),
        KeyValue::new("http.route", r.endpoint.clone()),
        KeyValue::new("url.path", r.endpoint.clone()),
        KeyValue::new("http.response.status_code", i64::from(r.status)),
        KeyValue::new("harness.request_id", r.id.clone()),
        KeyValue::new("harness.stream", r.stream),
        KeyValue::new("harness.cancelled", r.cancelled),
        KeyValue::new("harness.queue_wait_ms", r.queue_wait_ms as i64),
        KeyValue::new("harness.total_ms", r.total_ms as i64),
    ];
    let mut opt = |k: &'static str, v: Option<opentelemetry::Value>| {
        if let Some(v) = v {
            a.push(KeyValue::new(k, v));
        }
    };
    opt("gen_ai.request.model", r.model.clone().map(Into::into));
    opt("gen_ai.response.model", r.alias.clone().map(Into::into));
    opt(
        "gen_ai.usage.input_tokens",
        r.usage.prompt_tokens.map(|n| (n as i64).into()),
    );
    opt(
        "gen_ai.usage.output_tokens",
        r.usage.completion_tokens.map(|n| (n as i64).into()),
    );
    opt(
        "gen_ai.response.finish_reasons",
        r.usage
            .finish_reason
            .clone()
            .map(|f| opentelemetry::Value::Array(opentelemetry::Array::String(vec![f.into()]))),
    );
    opt(
        "harness.cached_tokens",
        r.usage.cached_tokens.map(|n| (n as i64).into()),
    );
    opt(
        "harness.tokens_per_second",
        r.usage.predicted_per_second.map(Into::into),
    );
    opt("harness.load_ms", r.load_ms.map(|n| (n as i64).into()));
    opt("harness.ttft_ms", r.ttft_ms.map(|n| (n as i64).into()));
    opt(
        "harness.definition_id",
        r.definition_id.clone().map(Into::into),
    );
    opt("harness.server_id", r.server_id.clone().map(Into::into));
    opt(
        "harness.token_name",
        r.client.token_name.clone().map(Into::into),
    );
    opt("harness.client", r.client.label.clone().map(Into::into));
    opt(
        "user_agent.original",
        r.client.user_agent.clone().map(Into::into),
    );
    opt("harness.backend", r.backend.clone().map(Into::into));
    opt("harness.ctx_size", r.ctx_size.map(|n| (n as i64).into()));
    opt("error.type", r.error_code.clone().map(Into::into));
    if !r.evicted.is_empty() {
        a.push(KeyValue::new("harness.evicted", r.evicted.join(",")));
    }
    if r.oom {
        a.push(KeyValue::new("harness.oom", true));
    }
    if let Some(c) = content {
        a.push(KeyValue::new("harness.request.body", truncated(&c.request)));
        a.push(KeyValue::new(
            "harness.response.body",
            truncated(&c.response),
        ));
    }
    let status = if r.status >= 500 {
        Status::error(
            r.error_code
                .clone()
                .unwrap_or_else(|| format!("HTTP {}", r.status)),
        )
    } else {
        Status::Unset
    };
    let name = match &model {
        Some(m) => format!("{} {}", op, m),
        None => op.to_string(),
    };
    let mut out = vec![span(
        root_context,
        parent,
        SpanKind::Server,
        name,
        at(start_ms),
        at(end_ms),
        a,
        Vec::new(),
        status,
        parent_remote,
    )];

    // Children only for requests that reached a server.
    if r.server_id.is_some() {
        let queue_end = start_ms + r.queue_wait_ms;
        if r.queue_wait_ms > 0 {
            out.push(span(
                ctx(new_span_id()),
                root_id,
                SpanKind::Internal,
                "gateway.queue".into(),
                at(start_ms),
                at(queue_end),
                vec![KeyValue::new(
                    "harness.queue_wait_ms",
                    r.queue_wait_ms as i64,
                )],
                Vec::new(),
                Status::Unset,
                false,
            ));
        }
        if let Some(load) = r.load_ms {
            let load_start = queue_end.saturating_sub(load).max(start_ms);
            let mut attrs = vec![KeyValue::new("harness.load_ms", load as i64)];
            if !r.evicted.is_empty() {
                attrs.push(KeyValue::new("harness.evicted", r.evicted.join(",")));
            }
            out.push(span(
                ctx(new_span_id()),
                root_id,
                SpanKind::Internal,
                "gateway.load".into(),
                at(load_start),
                at(queue_end),
                attrs,
                Vec::new(),
                Status::Unset,
                false,
            ));
        }
        let mut events = Vec::new();
        if let Some(t) = r.ttft_ms {
            events.push(Event::new(
                "gen_ai.first_token",
                at(start_ms + t),
                vec![KeyValue::new("harness.ttft_ms", t as i64)],
                0,
            ));
        }
        let mut attrs = Vec::new();
        if let Some(p) = r.usage.prompt_ms {
            attrs.push(KeyValue::new("harness.prompt_ms", p));
        }
        if let Some(p) = r.usage.predicted_ms {
            attrs.push(KeyValue::new("harness.predicted_ms", p));
        }
        out.push(span(
            ctx(new_span_id()),
            root_id,
            SpanKind::Client,
            "llama-server.generate".into(),
            at(queue_end),
            at(end_ms),
            attrs,
            events,
            Status::Unset,
            false,
        ));
    }
    out
}

pub(super) async fn test_export(
    settings: &OtelSettings,
    headers: &[(String, String)],
) -> TestExportResult {
    let started = Instant::now();
    let (trace_id, span_id) = random_ids();
    let base = TestExportResult {
        ok: false,
        url: settings.traces_url(),
        protocol: settings.protocol,
        elapsed_ms: 0,
        trace_id: trace_id.to_string(),
        error: None,
    };
    // One attempt, the collector's own answer.
    let exporter = match span_exporter(settings, headers, RetryPolicy::disabled()) {
        Ok(e) => e,
        Err(e) => {
            return TestExportResult {
                error: Some(e),
                ..base
            }
        }
    };
    let now = SystemTime::now();
    let test = span(
        SpanContext::new(
            trace_id,
            span_id,
            TraceFlags::SAMPLED,
            false,
            TraceState::default(),
        ),
        SpanId::INVALID,
        SpanKind::Internal,
        "harness.test_export".into(),
        now - Duration::from_millis(1),
        now,
        vec![
            KeyValue::new("harness.test", true),
            KeyValue::new("harness.version", env!("CARGO_PKG_VERSION")),
        ],
        Vec::new(),
        Status::Ok,
        false,
    );
    let result = tokio::time::timeout(
        EXPORT_TIMEOUT + Duration::from_secs(2),
        exporter.export(vec![test]),
    )
    .await;
    let _ = exporter.shutdown();
    let error = match result {
        Ok(Ok(())) => None,
        Ok(Err(e)) => Some(exporter.errors.text(e)),
        Err(_) => Some(
            crate::error_key::err(crate::http_error::timeout_key(&settings.traces_url()))
                .with("url", settings.traces_url())
                .to_string(),
        ),
    };
    TestExportResult {
        ok: error.is_none(),
        elapsed_ms: started.elapsed().as_millis() as u64,
        error,
        ..base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn collector_connection_error_keeps_the_transport_envelope() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let settings = OtelSettings {
            endpoint: format!("http://{}", listener.local_addr().unwrap()),
            ..Default::default()
        };
        drop(listener);
        let errors = TransportErrors::default();
        let http = ReqwestHttp(
            reqwest::Client::builder().no_proxy().build().unwrap(),
            errors.clone(),
        );
        let exporter = SpanExporter::builder()
            .with_http()
            .with_endpoint(settings.traces_url())
            .with_http_client(http)
            .with_retry_policy(RetryPolicy::disabled())
            .build()
            .unwrap();
        let failure = exporter
            .export(request_spans(&record(), None))
            .await
            .unwrap_err();
        let text = errors.text(failure);
        let (key, params) = crate::error_key::parse(&text).unwrap_or_else(|| panic!("{text}"));
        assert_eq!(key, "http.connect_failed_local");
        assert_eq!(params["url"], settings.traces_url());
        assert!(params.contains_key("transport_detail"));
    }

    #[test]
    fn sdk_wrappers_preserve_only_the_transport_error() {
        let keyed = crate::error_key::err("http.timeout_local")
            .with("url", "http://127.0.0.1:12345/v1/traces")
            .with("transport_detail", "raw transport source")
            .to_string();
        let errors = TransportErrors::default();
        *errors.0.lock().unwrap() = Some(keyed.clone());
        assert_eq!(
            errors.text("Operation failed: HTTP export failed: network error"),
            keyed
        );
        assert_eq!(
            errors.text("export refused: HTTP 401"),
            "export refused: HTTP 401"
        );
    }
    use crate::gateway::record::{ClientInfo, Usage};

    fn record() -> RequestRecord {
        RequestRecord {
            id: "req_1".into(),
            started_at_ms: 1_700_000_000_000,
            endpoint: "/v1/chat/completions".into(),
            model: Some("qwen".into()),
            alias: Some("qwen3-4b".into()),
            server_id: Some("s1".into()),
            client: ClientInfo::new(None, None, Some("ci".into())),
            status: 200,
            queue_wait_ms: 900,
            load_ms: Some(800),
            ttft_ms: Some(1000),
            total_ms: 2000,
            usage: Usage {
                prompt_tokens: Some(12),
                completion_tokens: Some(30),
                finish_reason: Some("stop".into()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn attr<'a>(s: &'a SpanData, k: &str) -> Option<&'a opentelemetry::Value> {
        s.attributes
            .iter()
            .find(|kv| kv.key.as_str() == k)
            .map(|kv| &kv.value)
    }

    fn request_spans(r: &RequestRecord, content: Option<&CapturedContent>) -> Vec<SpanData> {
        let (context, parent, remote) = request_context(None);
        spans_of(r, content, context, parent, remote)
    }

    #[test]
    fn spans_follow_the_record_timeline() {
        let spans = request_spans(&record(), None);
        assert_eq!(spans.len(), 4, "server, queue, load, generate");
        let root = &spans[0];
        assert_eq!(root.name, "chat qwen3-4b");
        assert_eq!(root.span_kind, SpanKind::Server);
        assert_eq!(root.parent_span_id, SpanId::INVALID);
        assert_eq!(
            root.end_time.duration_since(root.start_time).unwrap(),
            Duration::from_millis(2000)
        );
        assert_eq!(
            attr(root, "gen_ai.usage.output_tokens"),
            Some(&opentelemetry::Value::I64(30))
        );
        assert_eq!(
            attr(root, "harness.token_name").map(|v| v.to_string()),
            Some("ci".into())
        );
        assert!(attr(root, "harness.request.body").is_none(), "no content");
        let trace = root.span_context.trace_id();
        for child in &spans[1..] {
            assert_eq!(child.span_context.trace_id(), trace);
            assert_eq!(child.parent_span_id, root.span_context.span_id());
            assert!(child.start_time >= root.start_time && child.end_time <= root.end_time);
        }
        let load = spans.iter().find(|s| s.name == "gateway.load").unwrap();
        assert_eq!(
            load.end_time.duration_since(load.start_time).unwrap(),
            Duration::from_millis(800)
        );
        let generate = spans
            .iter()
            .find(|s| s.name == "llama-server.generate")
            .unwrap();
        assert_eq!(generate.events.events[0].name, "gen_ai.first_token");
        assert_eq!(
            generate.events.events[0].timestamp,
            at(1_700_000_000_000 + 1000)
        );
    }

    #[test]
    fn rejected_requests_are_one_span_with_the_error() {
        let r = RequestRecord {
            id: "req_2".into(),
            started_at_ms: 1,
            endpoint: "/v1/messages".into(),
            status: 429,
            error_code: Some("rate_limited".into()),
            ..Default::default()
        };
        let spans = request_spans(&r, None);
        assert_eq!(spans.len(), 1);
        assert_eq!(
            attr(&spans[0], "error.type").map(|v| v.to_string()),
            Some("rate_limited".into())
        );
        assert_eq!(spans[0].status, Status::Unset, "4xx is not a server error");
        let r5 = RequestRecord {
            status: 502,
            error_code: Some("upstream_unreachable".into()),
            ..r
        };
        assert!(matches!(
            request_spans(&r5, None)[0].status,
            Status::Error { .. }
        ));
    }

    #[test]
    fn content_only_when_given_and_truncated() {
        let big = "ü".repeat(CONTENT_LIMIT);
        let c = CapturedContent {
            request: "{\"messages\":[]}".into(),
            response: big,
            request_truncated: false,
            response_truncated: false,
            retention_days: 1,
        };
        let spans = request_spans(&record(), Some(&c));
        let body = attr(&spans[0], "harness.response.body")
            .unwrap()
            .to_string();
        assert!(body.len() <= CONTENT_LIMIT + "…".len());
        assert!(body.ends_with('…'));
        assert_eq!(
            attr(&spans[0], "harness.request.body").map(|v| v.to_string()),
            Some("{\"messages\":[]}".into())
        );
    }

    #[test]
    fn log_record_contains_analytics_metadata_and_trace_context() {
        let r = record();
        let (context, _, _) = request_context(None);
        let log = log_of(&r, None, &context);
        assert_eq!(log.event_name(), Some("gateway_request"));
        assert_eq!(log.severity_number(), Some(Severity::Info));
        assert_eq!(log.trace_context().unwrap().trace_id, context.trace_id());
        assert_eq!(log.trace_context().unwrap().span_id, context.span_id());
        let body = match log.body().unwrap() {
            AnyValue::String(value) => value.as_str(),
            other => panic!("unexpected body: {other:?}"),
        };
        let decoded: RequestRecord = serde_json::from_str(body).unwrap();
        assert_eq!(decoded, r);
        assert!(log
            .attributes_iter()
            .all(|(key, _)| key.as_str() != "harness.request.body"));

        let content = CapturedContent {
            request: "allowed prompt".into(),
            response: "allowed answer".into(),
            request_truncated: false,
            response_truncated: false,
            retention_days: 1,
        };
        let with_content = log_of(&r, Some(&content), &context);
        assert!(with_content.attributes_iter().any(|(key, value)| {
            key.as_str() == "harness.request.body" && value == &AnyValue::from("allowed prompt")
        }));
    }

    #[test]
    fn incoming_context_becomes_remote_parent() {
        let incoming = super::super::parse_trace_context(
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            Some("vendor=value"),
        )
        .unwrap();
        let (context, parent, remote) = request_context(Some(&incoming));
        let spans = spans_of(&record(), None, context, parent, remote);
        assert_eq!(
            spans[0].span_context.trace_id().to_string(),
            "4bf92f3577b34da6a3ce929d0e0e4736"
        );
        assert_eq!(spans[0].parent_span_id.to_string(), "00f067aa0ba902b7");
        assert!(spans[0].parent_span_is_remote);
        assert_eq!(spans[0].span_context.trace_state().header(), "vendor=value");
    }
}
