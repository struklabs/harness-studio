//! Ollama-compatible API on the gateway (ARCHITECTURE §13.4).
//!
//! Ollama clients (the `ollama` CLI with `OLLAMA_HOST`, the Python/JS libraries, Open WebUI,
//! editors) talk to the gateway's port (and, when turned on, to the Ollama port 11434); every
//! request is translated to llama-server's OpenAI-compatible API and goes through the same
//! scheduler (JIT load, TTL, queue, eviction) and the same per-request records as `/v1/*`.
//!
//! * `POST /api/chat`, `/api/generate` → `/v1/chat/completions` (`raw` generate →
//!   `/v1/completions`); NDJSON streaming (Ollama streams unless `"stream": false`),
//!   durations in nanoseconds, `thinking`, tool calls (arguments as objects), images
//!   (base64 → `image_url` data URLs), `format` (JSON mode / schema), sampling `options`.
//! * `POST /api/embed`, legacy `/api/embeddings` → `/v1/embeddings`.
//! * `GET /api/tags`, `/api/ps`, `POST /api/show`, `GET /api/version`, `GET|HEAD /`.
//! * `POST /api/pull`: Hugging Face references only (`hf.co/<owner>/<repo>[:<quant>]`),
//!   and only when `GatewaySettings::ollama_allow_pull` is on.
//! * `POST /api/create`, `/api/copy`, `DELETE /api/delete`, `HEAD|POST /api/blobs/:digest`:
//!   model definitions ([`manage`], [`blobs`]); `POST /api/push` answers with an explicit error.
//! * `keep_alive` → the scheduler's TTL override ([`KeepAlive`]); an empty request loads the
//!   model, an empty request with `keep_alive: 0` unloads it.
//! * Names: [`convert::parse_name`] (`name:tag` = alias, `:latest` = the bare alias).
//!
//! Errors use Ollama's shape `{"error": "..."}` (mid-stream: one such NDJSON line).

pub mod blobs;
pub mod convert;
pub mod info;
pub mod manage;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant, SystemTime};

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, delete, get, head, post};
use axum::Router;
use bytes::Bytes;
use futures_util::Stream;
use harness_core::analytics::{self, ContentCapture};
use harness_core::domain::ModelDefinition;
use harness_core::gateway::record::{ClientInfo, RequestRecord, UsageObserver};
use harness_core::gateway::scheduler::{KeepAlive, Lease, SchedError, SlotState, EXTERNAL_PREFIX};
use serde_json::{json, Map, Value};

use crate::compat::{self, Api};
use crate::errors::GatewayError;
use crate::proxy::{header_str, request_id, unix_ms, RecordGuard};
use crate::GwState;
use convert::{Action, NameRef, Shape, Stats, StreamTranslator, Upstream as UpReq};
use harness_core::token_policy::GatewayEndpoint;

/// Ollama API version the layer follows (the release whose API reference it was checked
/// against); `/api/version` reports it so version-gated clients work.
pub const API_VERSION: &str = "0.34.4";

const NDJSON: &str = "application/x-ndjson";

/// The Ollama routes (merged into the gateway router).
pub fn routes() -> Router<GwState> {
    Router::new()
        .route("/", get(root))
        .route("/api/version", get(version))
        .route(
            "/api/tags",
            get(|State(s): State<GwState>, h: HeaderMap| tags(s, h)),
        )
        .route(
            "/api/ps",
            get(|State(s): State<GwState>, h: HeaderMap| ps(s, h)),
        )
        .route(
            "/api/show",
            post(|State(s): State<GwState>, h: HeaderMap, b: Bytes| show(s, h, b)),
        )
        .route(
            "/api/chat",
            post(|State(s): State<GwState>, h: HeaderMap, b: Bytes| handle(s, Kind::Chat, h, b)),
        )
        .route(
            "/api/generate",
            post(|State(s): State<GwState>, h: HeaderMap, b: Bytes| {
                handle(s, Kind::Generate, h, b)
            }),
        )
        .route(
            "/api/embed",
            post(|State(s): State<GwState>, h: HeaderMap, b: Bytes| handle(s, Kind::Embed, h, b)),
        )
        .route(
            "/api/embeddings",
            post(|State(s): State<GwState>, h: HeaderMap, b: Bytes| {
                handle(s, Kind::Embeddings, h, b)
            }),
        )
        .route(
            "/api/pull",
            post(|State(s): State<GwState>, h: HeaderMap, b: Bytes| pull(s, h, b)),
        )
        .route(
            "/api/create",
            post(|State(s): State<GwState>, h: HeaderMap, b: Bytes| manage::create(s, h, b)),
        )
        .route(
            "/api/copy",
            post(|State(s): State<GwState>, h: HeaderMap, b: Bytes| manage::copy(s, h, b)),
        )
        .route(
            "/api/delete",
            delete(|State(s): State<GwState>, h: HeaderMap, b: Bytes| manage::delete(s, h, b)),
        )
        .route("/api/push", post(manage::push))
        // `ollama push` asks who is signed in first; answer with the same explicit error.
        .route("/api/me", any(manage::push))
        .route("/api/blobs/{digest}", head(blobs::head).post(blobs::upload))
        .route("/api/{*rest}", any(unsupported))
}

/// An error in Ollama's shape.
pub fn error_response(e: GatewayError) -> Response {
    let status = StatusCode::from_u16(e.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut resp = (status, axum::Json(json!({"error": e.message}))).into_response();
    if let Some(s) = e.retry_after {
        if let Ok(v) = HeaderValue::from_str(&s.to_string()) {
            resp.headers_mut().insert(header::RETRY_AFTER, v);
        }
    }
    if let Ok(v) = HeaderValue::from_str(&e.code) {
        resp.headers_mut().insert("x-harness-error", v);
    }
    if let Some(r) = &e.rate {
        crate::auth::rate_headers(resp.headers_mut(), r, false);
    }
    resp
}

fn bad_request(msg: impl Into<String>) -> GatewayError {
    GatewayError::new(400, "invalid_request", msg)
}

/// Ollama answers `GET /` with this text (clients probe it; the CLI sends `HEAD /`).
async fn root() -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "Ollama is running (Harness Studio gateway, Ollama-compatible API)",
    )
        .into_response()
}

async fn version() -> Response {
    let mut resp = axum::Json(json!({"version": API_VERSION})).into_response();
    resp.headers_mut()
        .insert("x-harness-compat", HeaderValue::from_static("ollama"));
    resp
}

async fn unsupported(uri: axum::http::Uri) -> Response {
    let path = uri.path();
    error_response(GatewayError::new(
        404,
        "not_found",
        format!(
            "{} is not an endpoint of Harness's Ollama-compatible API.",
            path
        ),
    ))
}

fn parse_body(body: &[u8]) -> Result<Map<String, Value>, GatewayError> {
    if body.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(Map::new());
    }
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(o)) => Ok(o),
        Ok(_) => Err(bad_request("the request body must be a JSON object")),
        Err(e) => Err(bad_request(format!("invalid JSON: {}", e))),
    }
}

/// `model`, else the deprecated `name` (the `ollama show` CLI still sends only `name`, with
/// an empty `model`).
fn model_field(req: &Map<String, Value>) -> Option<String> {
    ["model", "name"].iter().find_map(|k| {
        req.get(*k)
            .and_then(|m| m.as_str())
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_string)
    })
}

fn not_found(name: &str) -> GatewayError {
    GatewayError::new(
        404,
        "model_not_found",
        format!("model \"{}\" not found", name),
    )
}

/// The definition an Ollama name refers to (see [`convert::parse_name`]).
pub async fn resolve(state: &GwState, name: &str) -> Result<ModelDefinition, GatewayError> {
    match convert::parse_name(name) {
        NameRef::Alias(candidates) => {
            for c in &candidates {
                match state.scheduler.resolve(Some(c)).await {
                    Ok(d) => return Ok(d),
                    Err(SchedError::ModelNotFound { .. }) => continue,
                    Err(e) => return Err(GatewayError::from_sched(&e)),
                }
            }
            Err(not_found(name))
        }
        NameRef::Hf { repo, quant } => match hf_definition(state, &repo, quant.as_deref()).await? {
            Some(d) => Ok(d),
            None => Err(GatewayError::new(
                404,
                "model_not_found",
                format!(
                    "model \"{}\" not found: no model definition uses a GGUF from {}{} (pull it with /api/pull or download it in Harness Studio)",
                    name,
                    repo,
                    quant.map(|q| format!(" with quantization {}", q)).unwrap_or_default()
                ),
            )),
        },
    }
}

/// A servable definition whose GGUF lies in the `<owner>/<repo>` folder (Harness downloads
/// into `<library>/<owner>/<repo>/`) with quantization `quant` (Ollama's default for Hugging
/// Face names is `Q4_K_M` when several match).
async fn hf_definition(
    state: &GwState,
    repo: &str,
    quant: Option<&str>,
) -> Result<Option<ModelDefinition>, GatewayError> {
    let defs = state
        .scheduler
        .servable()
        .await
        .map_err(|e| GatewayError::from_sched(&e))?;
    let needle = format!("/{}/", repo.to_lowercase());
    let quant_of = |d: &ModelDefinition| -> String {
        d.base
            .local_path()
            .and_then(|p| p.file_name())
            .map(|f| harness_core::gguf::extract_quant_from_filename(&f.to_string_lossy()))
            .unwrap_or_default()
    };
    let matches: Vec<&ModelDefinition> = defs
        .iter()
        .filter(|d| {
            d.base.local_path().is_some_and(|p| {
                p.to_string_lossy()
                    .replace('\\', "/")
                    .to_lowercase()
                    .contains(&needle)
            })
        })
        .filter(|d| quant.is_none_or(|q| quant_of(d).eq_ignore_ascii_case(q)))
        .collect();
    let pick = match matches.as_slice() {
        [] => None,
        [one] => Some(*one),
        many => many
            .iter()
            .find(|d| quant_of(d).eq_ignore_ascii_case("Q4_K_M"))
            .or_else(|| many.first())
            .copied(),
    };
    Ok(pick.cloned())
}

// --- inference ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Chat,
    Generate,
    Embed,
    Embeddings,
}

impl Kind {
    fn path(self) -> &'static str {
        match self {
            Kind::Chat => "/api/chat",
            Kind::Generate => "/api/generate",
            Kind::Embed => "/api/embed",
            Kind::Embeddings => "/api/embeddings",
        }
    }

    fn embeds(self) -> bool {
        matches!(self, Kind::Embed | Kind::Embeddings)
    }
}

async fn handle(state: GwState, kind: Kind, headers: HeaderMap, body: Bytes) -> Response {
    let mut guard = RecordGuard::new(
        RequestRecord {
            id: request_id(),
            started_at_ms: unix_ms(),
            endpoint: kind.path().to_string(),
            ..Default::default()
        },
        super::proxy::trace_context(&headers),
    );
    let id = guard.get().id.clone();
    let mut resp = match run(&state, kind, &headers, body, &mut guard).await {
        Ok(resp) => resp,
        Err(err) => {
            guard.finish(err.status, Some(&err.code));
            error_response(err)
        }
    };
    if let Ok(v) = HeaderValue::from_str(&id) {
        resp.headers_mut().insert("x-harness-request-id", v);
    }
    if let Some(rate) = guard.rate {
        crate::auth::rate_headers(resp.headers_mut(), &rate, false);
    }
    resp
}

fn record_lease(guard: &mut RecordGuard, lease: &Lease) {
    let r = guard.get();
    r.server_id = Some(lease.server_id.clone());
    r.queue_wait_ms = lease.queue_wait.as_millis() as u64;
    r.slots = Some(lease.slots);
    r.waited = lease.waited.map(str::to_string);
    r.ctx_size = lease.ctx_size;
    r.backend = lease.backend.clone();
    if let Some(load) = &lease.load {
        r.load_ms = Some(load.ms);
        r.evicted = load.evicted.clone();
        r.oom = load.recovered;
    }
}

async fn acquire(
    state: &GwState,
    def: &ModelDefinition,
    demand: Option<&harness_core::gateway::admission::Demand>,
    keep_alive: Option<KeepAlive>,
    guard: &mut RecordGuard,
) -> Result<Lease, GatewayError> {
    match state
        .scheduler
        .acquire_keep_alive(def, demand, keep_alive)
        .await
    {
        Ok(lease) => {
            record_lease(guard, &lease);
            Ok(lease)
        }
        Err(e) => {
            guard.get().oom = e.is_oom();
            Err(GatewayError::from_sched(&e))
        }
    }
}

fn load_ns(lease: &Lease) -> u64 {
    lease
        .load
        .as_ref()
        .map(|l| l.ms.saturating_mul(1_000_000))
        .unwrap_or(0)
}

fn json_response(v: &Value, extra: &[(&str, String)]) -> Response {
    let mut resp = axum::Json(v).into_response();
    for (k, val) in extra {
        if let (Ok(name), Ok(val)) = (
            axum::http::HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(val),
        ) {
            resp.headers_mut().insert(name, val);
        }
    }
    resp
}

async fn run(
    state: &GwState,
    kind: Kind,
    headers: &HeaderMap,
    body: Bytes,
    guard: &mut RecordGuard,
) -> Result<Response, GatewayError> {
    let caller = crate::auth::check(state, headers, false, GatewayEndpoint::Ollama)?;
    guard.get().client = ClientInfo::new(
        header_str(headers, "user-agent"),
        header_str(headers, "x-harness-client"),
        caller.token_name.clone(),
    );
    let req = parse_body(&body)?;
    let name = model_field(&req).ok_or_else(|| bad_request("model is required"))?;
    guard.get().model = Some(name.clone());
    guard.admitted(caller.admit(state)?);
    let def = resolve(state, &name).await?;
    if crate::speech::is_audio_cpp_model(&def) {
        return Err(bad_request("audio.cpp only accepts POST /v1/audio/speech"));
    }
    {
        let r = guard.get();
        r.alias = Some(def.alias.clone());
        r.definition_id = Some(def.id.clone());
    }
    caller.check_model(&def.alias)?;
    if crate::audio::is_crispasr_model(&def) {
        return Err(GatewayError::new(
            400,
            "model_is_speech",
            "CrispASR only accepts /v1/audio/transcriptions and /v1/audio/translations",
        ));
    }
    if def.capture_content {
        guard.capture = Some(ContentCapture::new(
            &body,
            def.content_retention_days
                .unwrap_or(analytics::DEFAULT_CONTENT_RETENTION_DAYS),
        ));
    }
    let keep_alive = convert::parse_keep_alive(req.get("keep_alive")).map_err(bad_request)?;
    let (action, shape) = match kind {
        Kind::Chat => (
            convert::chat_request(&req, &def.alias).map_err(bad_request)?,
            Shape::Chat,
        ),
        Kind::Generate => convert::generate_request(&req, &def.alias).map_err(bad_request)?,
        Kind::Embed | Kind::Embeddings => {
            let up = convert::embed_request(&req, &def.alias, kind == Kind::Embeddings)
                .map_err(bad_request)?;
            let action = if convert::embed_is_empty(&up) {
                if keep_alive == Some(KeepAlive::Secs(0)) {
                    Action::Unload
                } else {
                    Action::Load
                }
            } else {
                Action::Forward(up)
            };
            (action, Shape::Generate)
        }
    };
    let lifecycle = |reason: &str, load_ns: u64, total: Instant| -> Value {
        if kind.embeds() {
            if kind == Kind::Embeddings {
                json!({"embedding": []})
            } else {
                json!({"model": name, "embeddings": [],
                    "total_duration": total.elapsed().as_nanos() as u64, "load_duration": load_ns})
            }
        } else {
            let mut o = convert::lifecycle_object(shape, &name, reason);
            if reason == "load" {
                o["total_duration"] = json!(total.elapsed().as_nanos() as u64);
                o["load_duration"] = json!(load_ns);
            }
            o
        }
    };
    let started = guard.started;
    match action {
        Action::Unload => {
            // Explicit unload (like "Unload" in the app): servers the user started are
            // stopped too.
            state
                .scheduler
                .unload(&def.id)
                .await
                .map_err(|e| GatewayError::new(500, "unload_failed", e))?;
            guard.finish(200, None);
            Ok(json_response(&lifecycle("unload", 0, started), &[]))
        }
        Action::Load => {
            let lease = acquire(state, &def, None, keep_alive, guard).await?;
            let load = load_ns(&lease);
            let server = lease.server_id.clone();
            drop(lease);
            guard.finish(200, None);
            Ok(json_response(
                &lifecycle("load", load, started),
                &[
                    ("x-harness-model", def.alias.clone()),
                    ("x-harness-server", server),
                ],
            ))
        }
        Action::Forward(up) => {
            forward(state, kind, shape, &name, &def, up, keep_alive, guard).await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn forward(
    state: &GwState,
    kind: Kind,
    shape: Shape,
    name: &str,
    def: &ModelDefinition,
    mut up: UpReq,
    keep_alive: Option<KeepAlive>,
    guard: &mut RecordGuard,
) -> Result<Response, GatewayError> {
    let settings = state.scheduler.settings();
    let mut body = serde_json::to_vec(&up.body).unwrap_or_default();
    if kind == Kind::Chat {
        let prepared =
            compat::prepare(Api::Chat, &body, settings.ignored_tools(&def.id)).map_err(|u| {
                GatewayError::new(400, "unsupported_tool_type", u.message()).param(u.param.clone())
            })?;
        up.notes.extend(prepared.notes());
        if let Some(b) = prepared.body {
            body = b;
        }
    }
    if up.has_images && def.mmproj.is_none() && !def.id.starts_with(EXTERNAL_PREFIX) {
        return Err(bad_request(format!(
            "model \"{}\" does not support images: its model definition has no multimodal projector (mmproj)",
            name
        )));
    }
    // The header always says the request was translated; the record (and the log line it
    // triggers) carries only what could not be applied.
    let mut notes = vec!["ollama".to_string()];
    notes.extend(up.notes.iter().cloned());
    {
        let r = guard.get();
        r.compat = up.notes.clone();
        r.stream = up.stream;
    }
    let demand = if kind.embeds() {
        None
    } else {
        harness_core::gateway::admission::Demand::from_bytes(&body)
    };
    let lease = acquire(state, def, demand.as_ref(), keep_alive, guard).await?;
    let timeout = match settings.request_timeout_secs {
        0 => None,
        n => Some(Duration::from_secs(u64::from(n))),
    };
    let mut req = state
        .http
        .post(format!("{}{}", lease.upstream, up.path))
        .header(header::CONTENT_TYPE, "application/json")
        .body(crate::proxy::upstream_body(def, body.into()));
    if let Some(context) = &guard.trace_context {
        req = req.header("traceparent", context.traceparent());
        if let Some(state) = context.tracestate() {
            req = req.header("tracestate", state);
        }
    }
    if let Some(key) = &lease.api_key {
        req = req.bearer_auth(key);
    }
    let send = req.send();
    let upstream = match timeout {
        Some(t) => tokio::time::timeout(t, send).await.map_err(|_| {
            GatewayError::new(
                504,
                "upstream_timeout",
                format!("llama-server did not answer within {} s.", t.as_secs()),
            )
        })?,
        None => send.await,
    }
    .map_err(|e| GatewayError::new(502, "upstream_unreachable", format!("llama-server: {}", e)))?;
    let status = upstream.status().as_u16();
    let sse = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|c| c.to_str().ok())
        .is_some_and(|c| c.starts_with("text/event-stream"));
    guard.get().status = status;
    if !(200..300).contains(&status) {
        let bytes = upstream.bytes().await.unwrap_or_default();
        return Err(GatewayError::new(
            status,
            "upstream_error",
            convert::upstream_error_message(&bytes),
        ));
    }
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            if up.stream {
                NDJSON
            } else {
                "application/json"
            },
        )
        .header("x-harness-model", &def.alias)
        .header("x-harness-server", &lease.server_id)
        .header(
            "x-harness-queue-ms",
            lease.queue_wait.as_millis().to_string(),
        )
        .header("x-harness-compat", notes.join("; "));
    if let Some(load) = &lease.load {
        builder = builder.header("x-harness-load-ms", load.ms.to_string());
        if !load.evicted.is_empty() {
            builder = builder.header("x-harness-evicted", load.evicted.join(","));
        }
    }
    let load_ns = load_ns(&lease);

    if up.stream && sse {
        builder = builder
            .header(header::CACHE_CONTROL, "no-cache")
            .header("x-accel-buffering", "no");
        let relay = NdjsonRelay {
            inner: Box::pin(upstream.bytes_stream()),
            tr: StreamTranslator::new(shape, name),
            observer: UsageObserver::new(true),
            guard: Some(guard.take()),
            idle: timeout.map(|t| (t, Box::pin(tokio::time::sleep(t)))),
            lease: Some(lease),
            first: true,
            status,
            load_ns,
            ended: false,
        };
        return builder
            .body(Body::from_stream(relay))
            .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()));
    }

    // Non-streaming (or an upstream that answered JSON to a stream request).
    let bytes = upstream
        .bytes()
        .await
        .map_err(|e| GatewayError::new(502, "upstream_error", e.to_string()))?;
    let mut observer = UsageObserver::new(false);
    observer.feed(&bytes);
    observer.finish();
    guard.get().usage = observer.usage;
    guard.get().ttft_ms = Some(guard.started.elapsed().as_millis() as u64);
    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let stats = Stats {
        total_ns: guard.started.elapsed().as_nanos() as u64,
        load_ns,
    };
    let out = if kind.embeds() {
        let dims = if kind == Kind::Embed {
            up.dimensions
        } else {
            None
        };
        convert::embed_response(name, &v, kind == Kind::Embeddings, dims, stats)
    } else {
        convert::from_response(shape, name, &v, stats)
    };
    let bytes = if up.stream {
        convert::line(&out)
    } else {
        serde_json::to_vec(&out).unwrap_or_default()
    };
    if let Some(c) = guard.capture.as_mut() {
        c.push_response(&bytes);
    }
    guard.finish(status, None);
    drop(lease);
    builder
        .body(Body::from(bytes))
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))
}

type Upstream = Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>;

/// NDJSON response body: llama-server SSE translated as it arrives. Holds the scheduler
/// lease until it ends or the client goes away (then the upstream request is dropped and
/// llama-server stops generating).
struct NdjsonRelay {
    inner: Upstream,
    tr: StreamTranslator,
    observer: UsageObserver,
    guard: Option<RecordGuard>,
    idle: Option<(Duration, Pin<Box<tokio::time::Sleep>>)>,
    lease: Option<Lease>,
    first: bool,
    status: u16,
    load_ns: u64,
    ended: bool,
}

impl NdjsonRelay {
    fn end(&mut self, error: Option<&str>) {
        self.ended = true;
        self.observer.finish();
        if let Some(mut g) = self.guard.take() {
            g.get().usage = std::mem::take(&mut self.observer.usage);
            g.finish(self.status, error);
        }
        self.lease.take();
    }

    fn capture(&mut self, bytes: &[u8]) {
        if let Some(c) = self.guard.as_mut().and_then(|g| g.capture.as_mut()) {
            c.push_response(bytes);
        }
    }
}

impl Drop for NdjsonRelay {
    fn drop(&mut self) {
        if let Some(mut g) = self.guard.take() {
            self.observer.finish();
            let usage = std::mem::take(&mut self.observer.usage);
            let complete = usage.finish_reason.is_some();
            g.get().usage = usage;
            if complete {
                g.finish(self.status, None);
            }
        }
        self.lease.take();
    }
}

impl Stream for NdjsonRelay {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        if this.ended {
            return Poll::Ready(None);
        }
        loop {
            if let Some((_, sleep)) = this.idle.as_mut() {
                if sleep.as_mut().poll(cx).is_ready() {
                    let out = convert::line(
                        &json!({"error": "llama-server stream stalled (request timeout)"}),
                    );
                    this.capture(&out);
                    this.end(Some("upstream_idle_timeout"));
                    return Poll::Ready(Some(Ok(Bytes::from(out))));
                }
            }
            return match this.inner.as_mut().poll_next(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Some(Ok(chunk))) => {
                    if this.first {
                        this.first = false;
                        if let Some(g) = this.guard.as_mut() {
                            g.get().ttft_ms = Some(g.started.elapsed().as_millis() as u64);
                        }
                    }
                    if let Some((t, sleep)) = this.idle.as_mut() {
                        sleep.as_mut().reset(tokio::time::Instant::now() + *t);
                    }
                    this.observer.feed(&chunk);
                    let out = this.tr.push(&chunk);
                    if out.is_empty() {
                        continue;
                    }
                    this.capture(&out);
                    if this.tr.errored() {
                        this.end(Some("upstream_error"));
                    }
                    Poll::Ready(Some(Ok(Bytes::from(out))))
                }
                Poll::Ready(Some(Err(e))) => {
                    let out = convert::line(&json!({"error": format!("llama-server: {}", e)}));
                    this.capture(&out);
                    this.end(Some("upstream_error"));
                    Poll::Ready(Some(Ok(Bytes::from(out))))
                }
                Poll::Ready(None) => {
                    let total_ns = this
                        .guard
                        .as_ref()
                        .map(|g| g.started.elapsed().as_nanos() as u64)
                        .unwrap_or(0);
                    let out = this.tr.finish(Stats {
                        total_ns,
                        load_ns: this.load_ns,
                    });
                    this.capture(&out);
                    let err = this.tr.errored().then_some("upstream_error");
                    this.end(err);
                    if out.is_empty() {
                        Poll::Ready(None)
                    } else {
                        Poll::Ready(Some(Ok(Bytes::from(out))))
                    }
                }
            };
        }
    }
}

// --- listings ---

fn auth(state: &GwState, headers: &HeaderMap) -> Result<crate::auth::Caller, GatewayError> {
    crate::auth::check(state, headers, false, GatewayEndpoint::Ollama)
}

/// A loaded model of `/api/ps`.
struct PsRow {
    def: ModelDefinition,
    /// Seconds until the idle TTL unloads it (`None` = never).
    remaining: Option<u64>,
    ttl: Option<u64>,
    busy: bool,
    pid: Option<u32>,
}

/// `GET /api/tags`: every definition whose file is on disk (like `/v1/models`), plus running
/// servers without a definition.
async fn tags(state: GwState, headers: HeaderMap) -> Response {
    let caller = match auth(&state, &headers) {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };
    let mut defs = match state.scheduler.servable().await {
        Ok(d) => d,
        Err(e) => return error_response(GatewayError::from_sched(&e)),
    };
    if let Ok(ext) = state.scheduler.external().await {
        defs.extend(ext);
    }
    defs.retain(|d| caller.allows_model(&d.alias));
    let models = tokio::task::spawn_blocking(move || {
        defs.iter()
            .filter_map(|d| {
                let p = d.base.local_path()?;
                let info = info::file_info(p)?;
                Some(info::tag_entry(d, &info))
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    axum::Json(json!({ "models": models })).into_response()
}

/// `GET /api/ps`: loaded models (ready), with the time they unload (`expires_at`: TTL /
/// keep_alive; far in the future when they never unload for idleness).
async fn ps(state: GwState, headers: HeaderMap) -> Response {
    let caller = match auth(&state, &headers) {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };
    let slots = state.scheduler.snapshot().await;
    let mut defs = state.scheduler.definitions().await.unwrap_or_default();
    if let Ok(ext) = state.scheduler.external().await {
        defs.extend(ext);
    }
    defs.retain(|d| caller.allows_model(&d.alias));
    let registry = state.scheduler.registry().clone();
    let mut rows: Vec<PsRow> = slots
        .iter()
        .filter(|s| s.state == SlotState::Ready)
        .filter_map(|s| {
            let d = defs.iter().find(|d| d.id == s.definition_id)?.clone();
            let pid = s
                .server_id
                .as_deref()
                .and_then(|id| registry.get(id))
                .and_then(|pm| pm.pid());
            let remaining = s
                .ttl_secs
                .map(|ttl| ttl.saturating_sub(s.idle_secs.unwrap_or(0)));
            Some(PsRow {
                def: d,
                remaining,
                ttl: s.ttl_secs,
                busy: s.in_flight > 0,
                pid,
            })
        })
        .collect();
    // Servers the user started without a definition are loaded too.
    for d in defs.iter().filter(|d| d.id.starts_with(EXTERNAL_PREFIX)) {
        if !rows.iter().any(|r| r.def.id == d.id) {
            let pid = registry
                .all()
                .into_iter()
                .find(|(_, pm)| {
                    pm.launch_config()
                        .is_some_and(|c| Some(c.model_path.as_path()) == d.base.local_path())
                })
                .and_then(|(_, pm)| pm.pid());
            rows.push(PsRow {
                def: d.clone(),
                remaining: None,
                ttl: None,
                busy: false,
                pid,
            });
        }
    }
    // Context window of each loaded server (`/props`).
    let props =
        futures_util::future::join_all(slots.iter().filter(|s| s.state == SlotState::Ready).map(
            |s| {
                let http = state.http.clone();
                let id = s.definition_id.clone();
                let active = s
                    .server_id
                    .as_deref()
                    .and_then(|sid| registry.get(sid))
                    .and_then(|pm| pm.active_server());
                async move {
                    let Some(a) = active else { return (id, None) };
                    let mut req = http
                        .get(format!(
                            "http://{}:{}/props",
                            harness_core::process::connect_host(&a.connect_host),
                            a.port
                        ))
                        .timeout(Duration::from_secs(2));
                    if let Some(k) = &a.api_key {
                        req = req.bearer_auth(k);
                    }
                    let v: Option<Value> = match req.send().await {
                        Ok(r) => r.json().await.ok(),
                        Err(_) => None,
                    };
                    let ctx = v.as_ref().and_then(|v| {
                        v.get("default_generation_settings")
                            .and_then(|g| g.get("n_ctx"))
                            .or_else(|| v.get("n_ctx"))
                            .and_then(|n| n.as_u64())
                    });
                    (id, ctx)
                }
            },
        ))
        .await;
    let models = tokio::task::spawn_blocking(move || {
        let now = SystemTime::now();
        rows.into_iter()
            .filter_map(
                |PsRow {
                     def: d,
                     remaining,
                     ttl,
                     busy,
                     pid,
                 }| {
                    let p = d.base.local_path()?;
                    let info = info::file_info(p)?;
                    let vram = pid
                        .and_then(harness_core::gpu::process_vram_bytes)
                        .unwrap_or(0);
                    let expires = match (ttl, busy) {
                        (None, _) => now + Duration::from_secs(290 * 365 * 86_400),
                        (Some(t), true) => now + Duration::from_secs(t),
                        (Some(_), false) => now + Duration::from_secs(remaining.unwrap_or(0)),
                    };
                    let name = convert::ollama_name(&d.alias);
                    let ctx = props
                        .iter()
                        .find(|(id, _)| *id == d.id)
                        .and_then(|(_, c)| *c)
                        .unwrap_or(0);
                    Some(json!({
                        "name": name,
                        "model": name,
                        "size": info.size.max(vram),
                        "digest": info.digest,
                        "details": info::details(&info),
                        "expires_at": convert::rfc3339(expires),
                        "size_vram": vram,
                        "context_length": ctx,
                    }))
                },
            )
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    axum::Json(json!({ "models": models })).into_response()
}

/// `POST /api/show`.
async fn show(state: GwState, headers: HeaderMap, body: Bytes) -> Response {
    let caller = match auth(&state, &headers) {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };
    let req = match parse_body(&body) {
        Ok(r) => r,
        Err(e) => return error_response(e),
    };
    let Some(name) = model_field(&req) else {
        return error_response(bad_request("model is required"));
    };
    let def = match resolve(&state, &name).await {
        Ok(d) => d,
        Err(e) => return error_response(e),
    };
    if let Err(e) = caller.check_model(&def.alias) {
        return error_response(e);
    }
    let verbose = req
        .get("verbose")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let Some(path) = def.base.local_path().map(|p| p.to_path_buf()) else {
        return error_response(not_found(&name));
    };
    let result = tokio::task::spawn_blocking(move || info::show(&def, &path, verbose)).await;
    match result {
        Ok(Ok(v)) => axum::Json(v).into_response(),
        Ok(Err(e)) => error_response(GatewayError::new(500, "gguf_unreadable", e)),
        Err(e) => error_response(GatewayError::new(500, "gateway_error", e.to_string())),
    }
}

// --- pull ---

fn progress_line(v: Value) -> Bytes {
    Bytes::from(convert::line(&v))
}

/// Creates (or finds) the model definition of a finished download so the name resolves.
fn ensure_definition(path: std::path::PathBuf, repo: &str, quant: &str) -> Result<String, String> {
    use harness_core::domain::store::Domain;
    use harness_core::domain::{unique_alias, ModelRef};
    let domain = Domain::global();
    let defs = domain.definitions()?;
    if let Some(d) = defs
        .iter()
        .find(|d| d.base.local_path().is_some_and(|p| p == path.as_path()))
    {
        return Ok(d.alias.clone());
    }
    let repo_name = repo.rsplit('/').next().unwrap_or(repo);
    let base = format!("{}:{}", repo_name, quant).to_lowercase();
    let alias = unique_alias(&base, defs.iter().map(|d| d.alias.as_str()));
    let saved = domain.save_definition(ModelDefinition::new(alias, ModelRef::Local { path }))?;
    Ok(saved.alias)
}

fn pull_error_text(error: &str, repo: &str, quant: Option<&str>) -> String {
    let key = harness_core::error_key::parse(error).map_or(error, |(k, _)| k);
    match key {
        harness_core::downloader::ERR_VARIANT_NOT_FOUND => format!(
            "{} has no GGUF{}",
            repo,
            quant
                .map(|q| format!(" with quantization {}", q))
                .unwrap_or_default()
        ),
        harness_core::downloader::ERR_VARIANT_AMBIGUOUS => format!(
            "{} has several GGUF variants: name one, e.g. hf.co/{}:Q4_K_M",
            repo, repo
        ),
        _ => error.to_string(),
    }
}

/// `POST /api/pull`: explicit Hugging Face references only; progress as Ollama's NDJSON
/// status objects. The download runs in Harness's download queue (it continues if the
/// client goes away, like Ollama's).
async fn pull(state: GwState, headers: HeaderMap, body: Bytes) -> Response {
    let caller = match auth(&state, &headers) {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };
    let req = match parse_body(&body) {
        Ok(r) => r,
        Err(e) => return error_response(e),
    };
    let Some(name) = model_field(&req) else {
        return error_response(bad_request("model is required"));
    };
    // A token limited to some models may pull only names its patterns allow.
    if let Err(e) = caller.check_model(&name) {
        return error_response(e);
    }
    let stream = req.get("stream").and_then(|s| s.as_bool()).unwrap_or(true);
    let NameRef::Hf { repo, quant } = convert::parse_name(&name) else {
        return error_response(GatewayError::new(
            400,
            "pull_requires_hf_reference",
            format!(
                "Harness pulls only from Hugging Face: use hf.co/<owner>/<repo>[:<quantization>] (e.g. hf.co/unsloth/Qwen3-4B-Instruct-2507-GGUF:Q4_K_M). \"{}\" is an Ollama registry name; Harness does not use the Ollama registry.",
                name
            ),
        ));
    };
    if let Err(e) = harness_core::downloader::DownloadManager::validate_repo_id(&repo) {
        return error_response(bad_request(e));
    }
    let done = |stream: bool| -> Response {
        if stream {
            let mut out = convert::line(&json!({"status": "pulling manifest"}));
            out.extend(convert::line(&json!({"status": "success"})));
            ([(header::CONTENT_TYPE, NDJSON)], out).into_response()
        } else {
            axum::Json(json!({"status": "success"})).into_response()
        }
    };
    // Already present: nothing to download.
    match hf_definition(&state, &repo, quant.as_deref()).await {
        Ok(Some(_)) => return done(stream),
        Ok(None) => {}
        Err(e) => return error_response(e),
    }
    if !state.scheduler.settings().ollama_allow_pull {
        return error_response(GatewayError::new(
            403,
            "pull_disabled",
            "Pulling through the Ollama API is turned off. Turn on \"Allow /api/pull\" in Harness Studio (Settings > Gateway > Ollama-compatible API), or download the model in the Model Hub.",
        ));
    }
    let Some(downloads) = state.downloads.clone() else {
        return error_response(GatewayError::new(
            503,
            "downloads_unavailable",
            "This gateway has no download queue.",
        ));
    };
    let token = harness_core::hf_auth::current_token().await;
    // Ollama's default for Hugging Face names without a tag is Q4_K_M.
    let started = match quant.as_deref() {
        Some(q) => downloads.pull(&repo, None, Some(q), token.as_deref()).await,
        None => match downloads
            .pull(&repo, None, Some("Q4_K_M"), token.as_deref())
            .await
        {
            Err(e) if e == harness_core::downloader::ERR_VARIANT_NOT_FOUND => {
                downloads.pull(&repo, None, None, token.as_deref()).await
            }
            other => other,
        },
    };
    let (job, variant) = match started {
        Ok(v) => v,
        Err(e) => {
            return error_response(GatewayError::new(
                404,
                "pull_failed",
                pull_error_text(&e, &repo, quant.as_deref()),
            ))
        }
    };
    let first = variant
        .shards
        .iter()
        .min_by_key(|s| s.shard_index)
        .map(|s| s.filename.clone())
        .unwrap_or_default();
    let path = harness_core::downloader::DownloadManager::models_dir()
        .join(&repo)
        .join(&first);
    let digest = variant
        .shards
        .iter()
        .find_map(|s| s.sha256.clone())
        .map(|h| format!("sha256:{}", h))
        .unwrap_or_else(|| variant.name.clone());
    let watch = PullWatch {
        downloads,
        job,
        repo,
        quant: variant.quantization.clone(),
        path,
        digest,
        stage: 0,
    };
    if stream {
        let body = futures_util::stream::unfold(watch, |mut w| async move {
            w.next().await.map(|b| (Ok::<Bytes, std::io::Error>(b), w))
        });
        return ([(header::CONTENT_TYPE, NDJSON)], Body::from_stream(body)).into_response();
    }
    let mut w = watch;
    let mut last = json!({"status": "success"});
    while let Some(b) = w.next().await {
        if let Ok(v) = serde_json::from_slice::<Value>(
            b.split(|c| *c == b'\n')
                .rfind(|l| !l.is_empty())
                .unwrap_or(&[]),
        ) {
            last = v;
        }
    }
    if let Some(e) = last.get("error").and_then(|e| e.as_str()) {
        return error_response(GatewayError::new(500, "pull_failed", e));
    }
    axum::Json(json!({"status": "success"})).into_response()
}

struct PullWatch {
    downloads: Arc<harness_core::downloader::DownloadManager>,
    job: u64,
    repo: String,
    quant: String,
    path: std::path::PathBuf,
    digest: String,
    /// 0 = manifest line not sent, 1 = downloading, 2 = finished.
    stage: u8,
}

impl PullWatch {
    /// The next NDJSON bytes (`None` = the stream is over).
    async fn next(&mut self) -> Option<Bytes> {
        use harness_core::downloader::DownloadStatus;
        match self.stage {
            0 => {
                self.stage = 1;
                return Some(progress_line(json!({"status": "pulling manifest"})));
            }
            2 => return None,
            _ => {}
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        let Some(p) = self.downloads.get(self.job) else {
            self.stage = 2;
            return Some(progress_line(
                json!({"error": "the download was removed from the queue"}),
            ));
        };
        match p.status {
            DownloadStatus::Completed => {
                self.stage = 2;
                let (path, repo, quant) =
                    (self.path.clone(), self.repo.clone(), self.quant.clone());
                let created =
                    tokio::task::spawn_blocking(move || ensure_definition(path, &repo, &quant))
                        .await
                        .map_err(|e| e.to_string())
                        .and_then(|r| r);
                let mut out = convert::line(&json!({
                    "status": format!("pulling {}", self.digest),
                    "digest": self.digest, "total": p.total_bytes, "completed": p.total_bytes
                }));
                out.extend(convert::line(&json!({"status": "verifying sha256 digest"})));
                match created {
                    Ok(alias) => {
                        out.extend(convert::line(&json!({"status": "writing manifest"})));
                        eprintln!(
                            "[Harness Gateway] /api/pull: {} indirildi, model tanımı {}",
                            self.repo, alias
                        );
                        out.extend(convert::line(&json!({"status": "success"})));
                    }
                    Err(e) => out.extend(convert::line(&json!({
                        "error": format!("downloaded, but the model definition could not be created: {}", e)
                    }))),
                }
                Some(Bytes::from(out))
            }
            DownloadStatus::Error(e) => {
                self.stage = 2;
                Some(progress_line(
                    json!({"error": format!("download failed: {}", e)}),
                ))
            }
            DownloadStatus::Paused => {
                self.stage = 2;
                Some(progress_line(
                    json!({"error": "the download was paused in Harness Studio"}),
                ))
            }
            _ => Some(progress_line(json!({
                "status": format!("pulling {}", self.digest),
                "digest": self.digest,
                "total": p.total_bytes,
                "completed": p.downloaded_bytes,
            }))),
        }
    }
}
