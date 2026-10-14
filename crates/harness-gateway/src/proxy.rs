//! The streaming reverse proxy: `model` → scheduler lease → llama-server, response bytes
//! relayed as they arrive. Opt-in text-call recovery buffers only a possible call suffix.
//!
//! * The request body is parsed only to read `model`/`stream` and, for the tool APIs, to check
//!   the tools (`compat`); unchanged bodies are forwarded as received.
//! * Client disconnect: axum drops the response body (or the handler future before the
//!   headers arrive), which drops the upstream request; llama-server sees the closed
//!   connection and cancels the generation.
//! * Timeouts: `request_timeout_secs` bounds the wait for a slot, the wait for the upstream
//!   headers and every pause inside a stream.
//! * Every request ends with one `RequestRecord` on the event bus (content never included)
//!   and in the analytics recorder (`harness_core::analytics::submit`, non-blocking). Content
//!   is captured only for definitions with `capture_content` on and goes to the recorder
//!   alone.

use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use crate::text_tool_calls::{self, RecoveryStream};
use axum::body::Body;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures_util::Stream;
use harness_core::analytics::{self, ContentCapture};
use harness_core::gateway::limits::{Permit, Snapshot};
use harness_core::gateway::record::{ClientInfo, RequestRecord, UsageObserver};
use harness_core::gateway::scheduler::Lease;
use harness_core::gateway::tool_diag::{CallIndex, ToolRound};
use harness_core::token_policy::GatewayEndpoint;
use harness_core::tools::text_call::RecoveredCall;
use http_body::Frame;
use http_body_util::StreamBody;

use crate::compat::{self, Api, SseRewriter};
use crate::errors::GatewayError;
use crate::GwState;

/// Publishes the record once: when the response ended, or when it is dropped early.
pub struct RecordGuard {
    record: Option<RequestRecord>,
    pub(crate) trace_context: Option<harness_core::otel::RequestTraceContext>,
    pub(crate) started: Instant,
    /// Request/response content, only while the definition captures content.
    pub(crate) capture: Option<ContentCapture>,
    /// Rate-limit admission of the caller's token: holds its concurrency slot until the
    /// request ends, then charges the tokens used.
    pub(crate) permit: Option<Permit>,
    /// Rate-limit state after admission (`x-ratelimit-*` response headers).
    pub(crate) rate: Option<Snapshot>,
    /// The tool round of a request that involves tools (diagnosis, `tool_diag`): read from the
    /// request body, fed the response, written into the record when the request ends.
    pub(crate) tool: Option<ToolRound>,
    recovered: Vec<RecoveredCall>,
}

impl RecordGuard {
    pub(crate) fn new(
        record: RequestRecord,
        trace_context: Option<harness_core::otel::RequestTraceContext>,
    ) -> Self {
        Self {
            record: Some(record),
            trace_context,
            started: Instant::now(),
            capture: None,
            permit: None,
            rate: None,
            tool: None,
            recovered: Vec::new(),
        }
    }

    /// Moves the record out (into a response body that ends later) and leaves a guard that
    /// publishes nothing (it keeps a copy of the rate-limit state for the headers).
    pub(crate) fn take(&mut self) -> Self {
        let rate = self.rate;
        std::mem::replace(
            self,
            Self {
                record: None,
                trace_context: None,
                started: self.started,
                capture: None,
                permit: None,
                rate,
                tool: None,
                recovered: Vec::new(),
            },
        )
    }

    /// Records the caller's admission (see [`crate::auth::Caller::admit`]).
    pub(crate) fn admitted(&mut self, permit: Option<Permit>) {
        if let Some(p) = permit {
            self.rate = Some(p.snapshot);
            self.permit = Some(p);
        }
    }

    pub(crate) fn get(&mut self) -> &mut RequestRecord {
        self.record.get_or_insert_with(RequestRecord::default)
    }

    pub(crate) fn finish(&mut self, status: u16, error: Option<&str>) {
        let started = self.started;
        let r = self.get();
        r.status = status;
        if let Some(e) = error {
            r.error_code = Some(e.to_string());
        }
        r.total_ms = started.elapsed().as_millis() as u64;
        if let Some(mut r) = self.record.take() {
            if let Some(t) = self.tool.take() {
                t.finalize(&mut r, CallIndex::global());
            }
            if !self.recovered.is_empty() {
                let count = self.recovered.len();
                r.compat.push(format!("text_tool_calls={count}"));
                let mut detail: serde_json::Value = r
                    .tool_diag_detail
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                if r.tool_diag_code.as_deref() != Some("call_left_in_content") {
                    if let Some(code) = r.tool_diag_code.take() {
                        detail["upstream_finding"] = serde_json::json!(code);
                    }
                    r.tool_diag_code = Some("call_left_in_content".into());
                    r.tool_diag_category = Some("runtime".into());
                }
                detail["recovered"] = serde_json::json!(true);
                detail["recovered_calls"] = serde_json::json!(count);
                r.tool_diag_detail = Some(detail.to_string());
                // The verdict observes upstream bytes; recovered calls stay distinct from native counts.
                for c in &self.recovered {
                    CallIndex::global().record(&c.id, &r.id);
                }
            }
            if let Some(p) = self.permit.take() {
                let u = &r.usage;
                p.complete(u.prompt_tokens.unwrap_or(0) + u.completion_tokens.unwrap_or(0));
            }
            log_record(&r);
            let content = self.capture.take().map(ContentCapture::finish);
            harness_core::otel::record(&r, content.as_ref(), self.trace_context.as_ref());
            analytics::submit(r.clone(), content);
            r.publish();
        }
    }
}

impl Drop for RecordGuard {
    fn drop(&mut self) {
        if self.record.is_some() {
            let status = self.get().status;
            self.get().cancelled = true;
            self.finish(if status == 0 { 499 } else { status }, None);
        }
    }
}

fn log_record(r: &RequestRecord) {
    // Metadata only (never content); compat changes are always logged.
    if !r.compat.is_empty() || r.error_code.is_some() || r.cancelled {
        eprintln!(
            "[Harness Gateway] {} {} model={} status={}{}{}{}",
            r.id,
            r.endpoint,
            r.alias.as_deref().or(r.model.as_deref()).unwrap_or("-"),
            r.status,
            r.error_code
                .as_deref()
                .map(|e| format!(" error={}", e))
                .unwrap_or_default(),
            if r.cancelled { " cancelled" } else { "" },
            if r.compat.is_empty() {
                String::new()
            } else {
                format!(" compat=[{}]", r.compat.join("; "))
            }
        );
    }
}

pub(crate) fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn request_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let ms = unix_ms();
    format!(
        "req_{:x}{:04x}",
        ms,
        NEXT.fetch_add(1, Ordering::Relaxed) & 0xffff
    )
}

/// The body a request sends upstream: a mistral.rs server gets `"model": "default"` (it serves
/// the one model it was started with and does not know the Harness alias); every other server
/// gets the body as it is.
pub(crate) fn upstream_body(def: &harness_core::domain::ModelDefinition, body: Bytes) -> Bytes {
    if !def
        .base
        .local_path()
        .is_some_and(harness_core::engine::is_mistralrs_dir)
    {
        return body;
    }
    harness_core::engine::mistralrs_upstream_body(&body)
        .map(Bytes::from)
        .unwrap_or(body)
}

/// A whole JSON answer with the alias as its model when the server does not know it.
fn renamed_json(rename: bool, alias: &str, body: Vec<u8>) -> Vec<u8> {
    if !rename {
        return body;
    }
    crate::model_name::ModelRenamer::new(alias, false)
        .rewrite_json(&body)
        .unwrap_or(body)
}

pub(crate) fn header_str(h: &HeaderMap, name: &str) -> Option<String> {
    h.get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.chars().take(256).collect())
}

pub(crate) fn trace_context(
    headers: &HeaderMap,
) -> Option<harness_core::otel::RequestTraceContext> {
    let traceparent = headers.get("traceparent")?.to_str().ok()?;
    let tracestate = headers.get("tracestate").and_then(|v| v.to_str().ok());
    harness_core::otel::parse_trace_context(traceparent, tracestate)
}

#[derive(serde::Deserialize)]
struct Peek<'a> {
    #[serde(borrow, default)]
    model: Option<std::borrow::Cow<'a, str>>,
    #[serde(default)]
    stream: Option<bool>,
    #[serde(default)]
    max_tokens: Option<u64>,
}

/// A prompt-cache warm-up of the Harness Playground: `x-harness-purpose: warmup` from the
/// Harness client token, not streamed, at most one output token. Only Harness itself can keep
/// a request out of the statistics, and only one that generates nothing.
fn is_warmup(
    headers: &HeaderMap,
    caller: &crate::auth::Caller,
    stream: bool,
    max_tokens: Option<u64>,
) -> bool {
    header_str(headers, "x-harness-purpose").as_deref() == Some("warmup")
        && caller.token_name.as_deref() == Some("harness")
        && caller.token_id.is_none()
        && !stream
        && max_tokens.is_some_and(|n| n <= 1)
}

/// Handles one inference request.
pub async fn handle(state: GwState, api: Api, headers: HeaderMap, body: Bytes) -> Response {
    // `background: true` on `/v1/responses` answers at once and runs the request in a task.
    if api == Api::Responses {
        if let Some(resp) = crate::responses::start_background(&state, &headers, &body).await {
            return resp;
        }
    }
    handle_with(state, api, headers, body, None).await
}

/// [`handle`] without the background check. `response_id` is the id a background Responses
/// request already answered with (its stored response keeps that id).
pub(crate) async fn handle_with(
    state: GwState,
    api: Api,
    headers: HeaderMap,
    body: Bytes,
    response_id: Option<String>,
) -> Response {
    let anthropic = api.anthropic();
    let mut guard = RecordGuard::new(
        RequestRecord {
            id: request_id(),
            started_at_ms: unix_ms(),
            endpoint: api.path().to_string(),
            ..Default::default()
        },
        trace_context(&headers),
    );
    let id = guard.get().id.clone();
    let result = run(
        &state,
        api,
        &headers,
        body,
        response_id.as_deref(),
        &mut guard,
    )
    .await;
    let mut resp = match result {
        Ok(resp) => resp,
        Err(err) => {
            let err = err.anthropic(anthropic);
            guard.finish(err.status, Some(&err.code));
            err.into_response()
        }
    };
    if let Ok(v) = HeaderValue::from_str(&id) {
        resp.headers_mut().insert("x-harness-request-id", v);
    }
    if let Some(rate) = guard.rate {
        crate::auth::rate_headers(resp.headers_mut(), &rate, anthropic);
    }
    resp
}

/// Endpoint group of an inference API (token policy).
pub fn endpoint_of(api: Api) -> GatewayEndpoint {
    GatewayEndpoint::of_path(api.path()).unwrap_or(GatewayEndpoint::Chat)
}

async fn run(
    state: &GwState,
    api: Api,
    headers: &HeaderMap,
    body: Bytes,
    response_id: Option<&str>,
    guard: &mut RecordGuard,
) -> Result<Response, GatewayError> {
    let caller = crate::auth::check(state, headers, api.anthropic(), endpoint_of(api))?;
    guard.get().client = ClientInfo::new(
        header_str(headers, "user-agent"),
        header_str(headers, "x-harness-client"),
        caller.token_name.clone(),
    );
    let peek: Peek = serde_json::from_slice(&body).map_err(|e| {
        GatewayError::new(
            400,
            "invalid_json",
            format!("The request body is not valid JSON: {}", e),
        )
    })?;
    let stream = peek.stream.unwrap_or(false);
    if is_warmup(headers, &caller, stream, peek.max_tokens) {
        guard.get().warmup = true;
    }
    let mut model = peek.model.as_deref().map(str::to_string);
    drop(peek);
    {
        let r = guard.get();
        r.model = model.clone();
        r.stream = stream;
    }
    // Rate limits before any model work (resolve, load), after the cheap peek so the record
    // names the model that was asked for.
    guard.admitted(caller.admit(state)?);
    let scheduler = state.scheduler.clone();
    let settings = scheduler.settings();
    // Stateful Responses: the stored conversation of `previous_response_id` goes in front of
    // the input (content capture records the client's own body).
    let mut stateful = None;
    let mut chained = None;
    let mut compaction_note = None;
    let client_body = body.clone();
    let mut body = body;
    if api == Api::Responses {
        let prep = crate::responses::prepare(&caller, &settings, &body, response_id).await?;
        if let Some(b) = prep.body {
            body = b;
        }
        if model.is_none() && prep.model.is_some() {
            model = prep.model;
            guard.get().model = model.clone();
        }
        stateful = prep.pending;
        chained = prep.chained;
    }
    let def = scheduler
        .resolve(model.as_deref())
        .await
        .map_err(|e| caller.sched_error(&e))?;
    {
        let r = guard.get();
        r.alias = Some(def.alias.clone());
        r.definition_id = Some(def.id.clone());
    }
    caller.check_model(&def.alias)?;
    if crate::speech::is_audio_cpp_model(&def) {
        return Err(GatewayError::new(
            400,
            "invalid_request",
            "audio.cpp only accepts POST /v1/audio/speech through the gateway.",
        ));
    }
    if crate::audio::is_speech_model(&def) {
        return Err(GatewayError::new(
            400,
            "model_is_speech",
            format!(
                "`{}` is a speech recognition model: send audio to /v1/audio/transcriptions, not {}.",
                def.alias,
                api.path()
            ),
        )
        .param("model"));
    }
    if crate::images::is_image_model(&def) {
        return Err(GatewayError::new(
            400,
            "model_is_image",
            format!(
                "`{}` is an image model: send the prompt to /v1/images/generations, not {}.",
                def.alias,
                api.path()
            ),
        )
        .param("model"));
    }
    if let Some(p) = stateful.as_mut() {
        p.model = def.alias.clone();
    }
    if let Some(c) = chained {
        let built = crate::responses::build_chained(state, &def, c, stateful.as_mut()).await?;
        body = built.body;
        compaction_note = built.note;
    }
    if def.capture_content {
        guard.capture = Some(ContentCapture::new(
            &client_body,
            def.content_retention_days
                .unwrap_or(analytics::DEFAULT_CONTENT_RETENTION_DAYS),
        ));
    }
    drop(client_body);
    // Server-side tools (T1c): for a model the user enabled them for, the gateway runs the
    // tool loop itself instead of rejecting hosted tools.
    if let Some(cfg) = settings.server_tools_of(&def.id) {
        let planned = crate::server_tools::plan(api, &body, cfg, settings.ignored_tools(&def.id))
            .map_err(|e| match e {
            crate::server_tools::PlanError::Unsupported(u) => {
                GatewayError::new(400, "unsupported_tool_type", u.message())
                    .param(u.param.clone())
                    .details(serde_json::json!({
                        "tool_type": u.tool_type, "tool_name": u.tool_name,
                        "reason": u.reason, "model": def.alias,
                    }))
            }
            crate::server_tools::PlanError::Invalid(m) => {
                GatewayError::new(400, "invalid_request", m)
            }
        })?;
        if let Some(plan) = planned {
            let timeout = match settings.request_timeout_secs {
                0 => None,
                n => Some(Duration::from_secs(u64::from(n))),
            };
            return crate::server_tools::serve(crate::server_tools::ServeArgs {
                state,
                api,
                def: &def,
                plan,
                stateful,
                compaction_note,
                guard,
                timeout,
            })
            .await;
        }
    }
    let mut prepared =
        compat::prepare(api, &body, settings.ignored_tools(&def.id)).map_err(|u| {
            GatewayError::new(400, "unsupported_tool_type", u.message())
                .param(u.param.clone())
                .details(serde_json::json!({
                    "tool_type": u.tool_type, "tool_name": u.tool_name, "reason": u.reason,
                    "model": def.alias,
                }))
        })?;
    guard.get().compat = prepared.notes();
    if !prepared.ignored.is_empty() {
        eprintln!(
            "[Harness Gateway] {} {}: ignored tools with the user's consent: {}",
            guard.get().id,
            def.alias,
            prepared.ignored.join(", ")
        );
    }
    // Token-budget admission: the request waits until its estimated prompt + generation
    // fits the server's shared KV pool next to the requests that already run.
    let demand = match api {
        Api::Embeddings | Api::Rerank | Api::CountTokens | Api::Hf => None,
        _ => harness_core::gateway::admission::Demand::from_bytes(
            prepared.body.as_deref().unwrap_or(&body),
        ),
    };
    let lease = match scheduler.acquire_tokens(&def, demand.as_ref()).await {
        Ok(lease) => lease,
        Err(e) => {
            guard.get().oom = e.is_oom();
            return Err(GatewayError::from_sched(&e));
        }
    };
    {
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
            if let Some(a) = &load.adapted {
                r.compat.push(format!("memory_fit={}", a.summary()));
            }
        }
    }
    // Tool-call diagnosis: the round is read from the request that goes upstream (after the
    // compat changes, so the names match what the model answers with).
    if matches!(api, Api::Chat | Api::Messages | Api::Responses) {
        guard.tool = ToolRound::start(
            prepared.body.as_deref().unwrap_or(&body),
            lease.template_tools,
        );
    }
    let timeout = match settings.request_timeout_secs {
        0 => None,
        n => Some(Duration::from_secs(u64::from(n))),
    };
    let compat_header = prepared.header();
    let recovery_names = if api == Api::Chat && settings.recover_text_tools(&def.id) {
        text_tool_calls::offered(prepared.body.as_deref().unwrap_or(&body))
    } else {
        Vec::new()
    };
    let loop_names = if api == Api::Chat {
        text_tool_calls::offered(prepared.body.as_deref().unwrap_or(&body))
    } else {
        vec![]
    };
    let names = std::mem::take(&mut prepared.names);
    let renames_model = def
        .base
        .local_path()
        .is_some_and(harness_core::engine::is_mistralrs_dir);
    let body = upstream_body(
        &def,
        match prepared.body.take() {
            Some(b) => Bytes::from(b),
            None => body,
        },
    );
    let mut req = state
        .http
        .post(format!("{}{}", lease.upstream, api.upstream_path()))
        .header(header::CONTENT_TYPE, "application/json")
        .body(body);
    for name in ["accept", "anthropic-version", "anthropic-beta"] {
        if let Some(v) = headers.get(name) {
            req = req.header(name, v.clone());
        }
    }
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
    let content_type = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"));
    let sse = content_type
        .to_str()
        .is_ok_and(|c| c.starts_with("text/event-stream"));
    guard.get().status = status;
    if let Some(t) = guard.tool.as_mut() {
        t.begin_response(sse);
    }

    let mut builder = Response::builder()
        .status(StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY))
        .header(header::CONTENT_TYPE, content_type)
        .header("x-harness-model", &def.alias)
        .header("x-harness-server", &lease.server_id)
        .header(
            "x-harness-queue-ms",
            lease.queue_wait.as_millis().to_string(),
        );
    if sse {
        builder = builder
            .header(header::CACHE_CONTROL, "no-cache")
            .header("x-accel-buffering", "no");
    }
    if let Some(load) = &lease.load {
        builder = builder.header("x-harness-load-ms", load.ms.to_string());
        if !load.evicted.is_empty() {
            builder = builder.header("x-harness-evicted", load.evicted.join(","));
        }
    }
    if !compat_header.is_empty() {
        builder = builder.header("x-harness-compat", &compat_header);
    }
    if let Some(n) = compaction_note
        .as_deref()
        .and_then(|n| HeaderValue::from_str(n).ok())
    {
        builder = builder.header("x-harness-compaction", n);
    }

    if (!recovery_names.is_empty() || !loop_names.is_empty()) && status == 200 && !sse {
        let read = upstream.bytes();
        let bytes = match timeout {
            Some(t) => tokio::time::timeout(t, read).await.map_err(|_| {
                GatewayError::new(504, "upstream_timeout", "Upstream JSON body stalled.")
            })?,
            None => read.await,
        }
        .map_err(|e| GatewayError::new(502, "upstream_error", e.to_string()))?;
        let mut observer = UsageObserver::new(false);
        observer.feed(&bytes);
        observer.finish();
        if let Some(t) = guard.tool.as_mut() {
            t.feed(&bytes);
        }
        guard.get().usage = observer.usage;
        guard.get().ttft_ms = Some(guard.started.elapsed().as_millis() as u64);
        let (out, mut calls) = text_tool_calls::recover_json(&bytes, &recovery_names);
        let (out, cut) = crate::tool_loop_guard::trim_json(&out, loop_names.len());
        if let Some(n) = cut {
            let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
            calls.retain(|c| {
                value["choices"][0]["message"]["tool_calls"]
                    .as_array()
                    .is_some_and(|list| list.iter().any(|v| v["id"] == c.id))
            });
            eprintln!("tool-call döngüsü kesildi: {n} çağrı");
            guard.get().compat.push(format!("tool_loop_cut={n}"));
            guard.get().usage.finish_reason = Some("tool_calls".into());
        }
        if !calls.is_empty() {
            let note = format!("text_tool_calls={}", calls.len());
            let header = if compat_header.is_empty() {
                note
            } else {
                format!("{compat_header}; {note}")
            };
            if let Some(headers) = builder.headers_mut() {
                if let Ok(value) = HeaderValue::from_str(&header) {
                    headers.insert("x-harness-compat", value);
                }
            }
            guard.recovered = calls;
        }
        let out = renamed_json(renames_model, &def.alias, out);
        if let Some(c) = guard.capture.as_mut() {
            c.push_response(&out);
        }
        guard.finish(status, None);
        drop(lease);
        return builder
            .body(Body::from(out))
            .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()));
    }
    let recovery = (!recovery_names.is_empty() && status == 200 && sse)
        .then(|| RecoveryStream::new(recovery_names));
    if recovery.is_some() {
        builder = builder.header(header::TRAILER, "x-harness-compat");
    }

    // Non-streaming Responses with flattened namespaces or state: the (small) JSON is mapped
    // back, finished (`previous_response_id`, `store`) and stored before it is sent.
    if api == Api::Responses && !sse && (!names.is_empty() || stateful.is_some()) {
        let bytes = upstream
            .bytes()
            .await
            .map_err(|e| GatewayError::new(502, "upstream_error", e.to_string()))?;
        let mut observer = UsageObserver::new(false);
        observer.feed(&bytes);
        observer.finish();
        if let Some(t) = guard.tool.as_mut() {
            t.feed(&bytes);
        }
        guard.get().usage = observer.usage;
        guard.get().ttft_ms = Some(guard.started.elapsed().as_millis() as u64);
        let mut out = if names.is_empty() {
            None
        } else {
            compat::rewrite_json(&bytes, &names)
        }
        .unwrap_or_else(|| bytes.to_vec());
        out = renamed_json(renames_model, &def.alias, out);
        if let Some(p) = stateful.as_ref().filter(|_| status == 200) {
            if let Some(done) = p.complete(&out).await {
                out = done;
            }
        }
        if let Some(c) = guard.capture.as_mut() {
            c.push_response(&out);
        }
        guard.finish(status, None);
        drop(lease);
        return builder
            .body(Body::from(out))
            .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()));
    }

    let relay = Relay {
        loop_guard: (api == Api::Chat && status == 200 && sse)
            .then(|| crate::tool_loop_guard::Guard::new(loop_names.len())),
        inner: Box::pin(upstream.bytes_stream()),
        recovery,
        trailer: None,
        compat_header,
        rewriter: (!names.is_empty() && sse).then(|| SseRewriter::new(names)),
        renamer: renames_model.then(|| crate::model_name::ModelRenamer::new(&def.alias, sse)),
        observer: UsageObserver::new(sse),
        guard: Some(guard.take()),
        idle: timeout.map(|t| (t, Box::pin(tokio::time::sleep(t)))),
        lease: Some(lease),
        first: true,
        status,
        stateful: stateful
            .filter(|_| sse && status == 200)
            .map(|p| (p, crate::responses::SseScan::default(), Vec::new())),
        saving: None,
        held: None,
    };
    builder
        .body(Body::new(StreamBody::new(relay)))
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))
}

type Upstream = Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>;

/// The response body: upstream chunks as they come, observed for usage, optionally rewritten
/// (namespaces), bounded by the idle timeout. Holds the scheduler lease until it ends or the
/// client goes away.
struct Relay {
    loop_guard: Option<crate::tool_loop_guard::Guard>,
    inner: Upstream,
    recovery: Option<RecoveryStream>,
    trailer: Option<HeaderMap>,
    compat_header: String,
    rewriter: Option<SseRewriter>,
    /// mistral.rs answers with its own model id: renamed to the alias the client asked for.
    renamer: Option<crate::model_name::ModelRenamer>,
    observer: UsageObserver,
    guard: Option<RecordGuard>,
    idle: Option<(Duration, Pin<Box<tokio::time::Sleep>>)>,
    lease: Option<Lease>,
    first: bool,
    status: u16,
    /// Stateful Responses stream: the final response is stored when it passes by.
    stateful: Option<(
        crate::responses::Pending,
        crate::responses::SseScan,
        Vec<u8>,
    )>,
    /// The store of the final response; `held` (the chunk with `response.completed`) is sent
    /// only after it, so the response can be read back as soon as the client sees the event.
    saving: Option<tokio::task::JoinHandle<()>>,
    held: Option<Bytes>,
}

impl Relay {
    fn end(&mut self, error: Option<&str>) {
        self.observer.finish();
        if let Some(g) = self.loop_guard.as_ref().filter(|g| g.stopped) {
            let ids = g.accepted_ids();
            if let Some(r) = self.recovery.as_mut() {
                r.calls.retain(|c| ids.contains(&c.id));
            }
            if let Some(r) = self.guard.as_mut() {
                r.recovered.retain(|c| ids.contains(&c.id));
            }
        }
        if let Some(recovery) = self.recovery.as_mut() {
            if !recovery.calls.is_empty() {
                let note = format!("text_tool_calls={}", recovery.calls.len());
                let value = if self.compat_header.is_empty() {
                    note
                } else {
                    format!("{}; {note}", self.compat_header)
                };
                let mut trailers = HeaderMap::new();
                trailers.insert("x-harness-compat", HeaderValue::from_str(&value).unwrap());
                self.trailer = Some(trailers);
                if let Some(g) = self.guard.as_mut() {
                    g.recovered = std::mem::take(&mut recovery.calls);
                }
            }
        }
        if let Some(mut g) = self.guard.take() {
            g.get().usage = std::mem::take(&mut self.observer.usage);
            g.finish(self.status, error);
        }
        self.lease.take();
    }
}

impl Relay {
    /// Feeds relayed bytes to the stateful scan; `true` when the final response was found
    /// and is being stored (the caller holds the bytes until that ends).
    fn save_final(&mut self, out: &[u8]) -> bool {
        let Some((pending, scan, _)) = self.stateful.as_mut() else {
            return false;
        };
        let Some(response) = scan.push(out) else {
            return false;
        };
        let handle = pending.complete_stream(response);
        self.stateful = None;
        match handle {
            Some(h) => {
                self.saving = Some(h);
                true
            }
            None => false,
        }
    }
}

impl Drop for Relay {
    /// The client went away before the stream ended (or right after the last event, before
    /// the connection saw the end, e.g. a client that stops at `[DONE]`). The usage observed
    /// so far goes into the record; a response whose finish reason was already relayed is
    /// complete, not cancelled.
    fn drop(&mut self) {
        if let Some(mut g) = self.guard.take() {
            self.observer.finish();
            let usage = std::mem::take(&mut self.observer.usage);
            let complete = usage.finish_reason.is_some();
            g.get().usage = usage;
            if complete {
                g.finish(self.status, None);
            }
            // Otherwise the guard's own drop records it as cancelled.
        }
        self.lease.take();
    }
}

impl Stream for Relay {
    type Item = Result<Frame<Bytes>, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        loop {
            if let Some(saving) = this.saving.as_mut() {
                if Pin::new(saving).poll(cx).is_pending() {
                    return Poll::Pending;
                }
                this.saving = None;
            }
            if let Some(held) = this.held.take() {
                return Poll::Ready(Some(Ok(Frame::data(held))));
            }
            if this.guard.is_none() {
                return Poll::Ready(this.trailer.take().map(|t| Ok(Frame::trailers(t))));
            }
            if let Some((_, sleep)) = this.idle.as_mut() {
                if sleep.as_mut().poll(cx).is_ready() {
                    this.end(Some("upstream_idle_timeout"));
                    return Poll::Ready(Some(Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "llama-server stream stalled",
                    ))));
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
                    if let Some(t) = this.guard.as_mut().and_then(|g| g.tool.as_mut()) {
                        t.feed(&chunk);
                    }
                    let chunk = match this.recovery.as_mut() {
                        Some(r) => {
                            let out = r.push(&chunk);
                            if let Some(g) = this.guard.as_mut().filter(|_| !r.calls.is_empty()) {
                                g.recovered = r.calls.clone();
                            }
                            Bytes::from(out)
                        }
                        None => chunk,
                    };
                    let chunk = if let Some(g) = this.loop_guard.as_mut() {
                        let out = g.push(&chunk);
                        if g.stopped {
                            let n = g.count;
                            eprintln!("tool-call döngüsü kesildi: {n} çağrı");
                            if let Some(record) = this.guard.as_mut() {
                                record.get().compat.push(format!("tool_loop_cut={n}"));
                            }
                            // Drop the reqwest body now: cancelling upstream closes its connection.
                            this.inner = Box::pin(futures_util::stream::empty());
                            this.observer.usage.finish_reason = Some("tool_calls".into());
                        }
                        Bytes::from(out)
                    } else {
                        chunk
                    };
                    let mut out = match this.rewriter.as_mut() {
                        Some(r) => Bytes::from(r.push(&chunk)),
                        None => chunk,
                    };
                    if let Some(r) = this.renamer.as_mut() {
                        out = Bytes::from(r.push(&out));
                    }
                    if let Some((pending, _, buffer)) = this.stateful.as_mut() {
                        out = Bytes::from(pending.decorate_stream(buffer, &out));
                    }
                    if out.is_empty() {
                        continue;
                    }
                    if let Some(c) = this.guard.as_mut().and_then(|g| g.capture.as_mut()) {
                        c.push_response(&out);
                    }
                    if this.save_final(&out) {
                        this.held = Some(out);
                        continue;
                    }
                    if this.loop_guard.as_ref().is_some_and(|g| g.stopped) {
                        this.end(None);
                    }
                    Poll::Ready(Some(Ok(Frame::data(out))))
                }
                Poll::Ready(Some(Err(e))) => {
                    this.end(Some("upstream_error"));
                    Poll::Ready(Some(Err(std::io::Error::other(e))))
                }
                Poll::Ready(None) => {
                    let recovered_rest = this
                        .recovery
                        .as_mut()
                        .map(|r| r.finish())
                        .unwrap_or_default();
                    let mut rest = this
                        .rewriter
                        .as_mut()
                        .map(|r| r.finish())
                        .unwrap_or_default();
                    rest.extend(recovered_rest);
                    if let Some(r) = this.renamer.as_mut() {
                        let mut renamed = r.push(&rest);
                        renamed.extend(r.finish());
                        rest = renamed;
                    }
                    if let Some(g) = this.loop_guard.as_mut() {
                        let mut filtered = g.push(&rest);
                        filtered.extend(g.finish());
                        if g.stopped {
                            let n = g.count;
                            eprintln!("tool-call döngüsü kesildi: {n} çağrı");
                            if let Some(record) = this.guard.as_mut() {
                                record.get().compat.push(format!("tool_loop_cut={n}"));
                            }
                            this.observer.usage.finish_reason = Some("tool_calls".into());
                        }
                        rest = filtered;
                    }
                    if let Some((pending, _, buffer)) = this.stateful.as_mut() {
                        let mut decorated = pending.decorate_stream(buffer, &rest);
                        decorated.append(buffer);
                        rest = decorated;
                    }
                    if let Some(c) = this.guard.as_mut().and_then(|g| g.capture.as_mut()) {
                        c.push_response(&rest);
                    }
                    this.end(None);
                    if this.save_final(&rest) {
                        if !rest.is_empty() {
                            this.held = Some(Bytes::from(rest));
                        }
                        continue;
                    }
                    if rest.is_empty() {
                        Poll::Ready(this.trailer.take().map(|t| Ok(Frame::trailers(t))))
                    } else {
                        Poll::Ready(Some(Ok(Frame::data(Bytes::from(rest)))))
                    }
                }
            };
        }
    }
}

use std::future::Future;

/// `GET /v1/models`: every definition whose file is on disk (loaded or not).
pub async fn models(state: GwState, headers: HeaderMap) -> Response {
    let caller = match crate::auth::check(&state, &headers, false, GatewayEndpoint::Models) {
        Ok(c) => c,
        Err(e) => return e.into_response(),
    };
    let defs = match state.scheduler.servable().await {
        Ok(d) => d,
        Err(e) => return GatewayError::from_sched(&e).into_response(),
    };
    let slots = state.scheduler.snapshot().await;
    let mut data: Vec<serde_json::Value> = defs
        .iter()
        .filter(|d| caller.allows_model(&d.alias))
        .map(|d| model_object(d, slots.iter().find(|s| s.definition_id == d.id)))
        .collect();
    // Running servers the user started without a definition are reachable by their served id.
    if let Ok(external) = state.scheduler.external().await {
        data.extend(
            external
                .iter()
                .filter(|d| caller.allows_model(&d.alias))
                .map(|d| {
                    let mut o = model_object(d, None);
                    o["harness"]["state"] =
                        serde_json::json!(harness_core::gateway::scheduler::SlotState::Ready);
                    o
                }),
        );
    }
    axum::Json(serde_json::json!({"object": "list", "data": data})).into_response()
}

/// `GET /v1/models/{model}`.
pub async fn model(state: GwState, headers: HeaderMap, id: String) -> Response {
    let caller = match crate::auth::check(&state, &headers, false, GatewayEndpoint::Models) {
        Ok(c) => c,
        Err(e) => return e.into_response(),
    };
    match state.scheduler.resolve(Some(&id)).await {
        Ok(d) => {
            if let Err(e) = caller.check_model(&d.alias) {
                return e.into_response();
            }
            let slots = state.scheduler.snapshot().await;
            axum::Json(model_object(
                &d,
                slots.iter().find(|s| s.definition_id == d.id),
            ))
            .into_response()
        }
        Err(e) => caller.sched_error(&e).into_response(),
    }
}

fn model_object(
    d: &harness_core::domain::ModelDefinition,
    slot: Option<&harness_core::gateway::scheduler::ModelSlot>,
) -> serde_json::Value {
    serde_json::json!({
        "id": d.alias,
        "object": "model",
        "created": d.created_at,
        "owned_by": "harness",
        "harness": {
            "definition_id": d.id,
            "state": slot.map(|s| s.state).unwrap_or(harness_core::gateway::scheduler::SlotState::Unloaded),
            "pinned": d.pinned,
            "ttl_seconds": d.ttl_seconds,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::domain::{ModelDefinition, ModelRef};

    #[test]
    fn only_harness_itself_can_mark_a_one_token_warmup() {
        let mut h = HeaderMap::new();
        h.insert("x-harness-purpose", "warmup".parse().unwrap());
        let app = crate::auth::Caller {
            token_name: Some("harness".into()),
            ..Default::default()
        };
        assert!(is_warmup(&h, &app, false, Some(1)));
        assert!(!is_warmup(&h, &app, true, Some(1)));
        assert!(!is_warmup(&h, &app, false, Some(2)));
        assert!(!is_warmup(&h, &app, false, None));
        assert!(!is_warmup(&HeaderMap::new(), &app, false, Some(1)));
        let named = crate::auth::Caller {
            token_name: Some("harness".into()),
            token_id: Some("t1".into()),
            ..Default::default()
        };
        assert!(!is_warmup(&h, &named, false, Some(1)));
        assert!(!is_warmup(
            &h,
            &crate::auth::Caller::default(),
            false,
            Some(1)
        ));
    }

    #[test]
    fn a_warmup_record_is_not_published() {
        let mut rx = harness_core::events::subscribe();
        let mut r = harness_core::gateway::record::RequestRecord {
            id: "warm".into(),
            warmup: true,
            ..Default::default()
        };
        r.clone().publish();
        r.warmup = false;
        r.id = "real".into();
        r.publish();
        let mut ids = Vec::new();
        while let Ok(e) = rx.try_recv() {
            if let harness_core::events::CoreEvent::GatewayRequest { record } = e {
                ids.push(record.id);
            }
        }
        assert!(ids.contains(&"real".to_string()));
        assert!(!ids.contains(&"warm".to_string()));
    }

    #[test]
    fn only_a_mistralrs_upstream_gets_the_default_model_id() {
        let root = tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("gw-mrs"))
            .tempdir()
            .unwrap();
        let dir = root.path().join("Qwen2.5-0.5B-Instruct");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.json"),
            r#"{"architectures":["Qwen2ForCausalLM"]}"#,
        )
        .unwrap();
        std::fs::write(dir.join("model.safetensors"), [0u8; 8]).unwrap();
        let body = Bytes::from_static(br#"{"model":"qwen-small","messages":[]}"#);
        let mistral = ModelDefinition::new("qwen-small", ModelRef::Local { path: dir.clone() });
        let sent: serde_json::Value =
            serde_json::from_slice(&upstream_body(&mistral, body.clone())).unwrap();
        assert_eq!(sent["model"], "default");
        assert_eq!(sent["messages"], serde_json::json!([]));
        let gguf = ModelDefinition::new(
            "qwen-small",
            ModelRef::Local {
                path: dir.join("x.gguf"),
            },
        );
        assert_eq!(upstream_body(&gguf, body.clone()), body);
    }
}
