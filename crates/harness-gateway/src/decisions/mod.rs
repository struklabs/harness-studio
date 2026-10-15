//! Decision APIs: `POST /v1/systemone` (Ollama / LM Studio / llama.cpp) and `POST /v1/decisions`
//! (OpenAI / LM Studio). A decision model reads a state and answers typed questions with
//! probabilities instead of text.
//!
//! * A native decision model (`<arch>.decision.type` in its GGUF header) is served by
//!   llama-server's own `/v1/systemone`; both APIs are translated to it ([`native`]).
//! * Any other model is answered by [`emulate`] (one grammar-constrained token per question,
//!   probabilities from logprobs) while `GatewaySettings::decision_emulation` is on (default).
//!   Off, the request goes to llama-server as it is, and its `501` (not a decision model)
//!   becomes the `400` Ollama and OpenAI clients expect.
//!
//! Same gates as every inference route: origin guard (router layer), token and its model and
//! endpoint policy, rate limits, JIT load with token admission, request records. Errors use
//! each API's own shape: Ollama's `{"error": "..."}` on `/v1/systemone`, OpenAI's on
//! `/v1/decisions`. The mode that answered is in `X-Harness-Decision-Mode: native|emulated`.

pub mod emulate;
pub mod native;
pub mod schema;

use std::time::Duration;

use axum::http::{HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use harness_core::analytics::{self, ContentCapture};
use harness_core::domain::ModelDefinition;
use harness_core::gateway::record::{ClientInfo, RequestRecord};
use harness_core::gateway::scheduler::Lease;
use harness_core::token_policy::GatewayEndpoint;
use serde_json::{json, Value};

use crate::errors::GatewayError;
use crate::proxy::{header_str, request_id, unix_ms, RecordGuard};
use crate::GwState;
use schema::{Answer, DecisionRequest, Invalid, Kind, Outcome};

/// Which API a request came in on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    SystemOne,
    Decisions,
}

impl Flavor {
    pub fn path(self) -> &'static str {
        match self {
            Flavor::SystemOne => "/v1/systemone",
            Flavor::Decisions => "/v1/decisions",
        }
    }
}

/// How a request was answered (`X-Harness-Decision-Mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Native,
    Emulated,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Native => "native",
            Mode::Emulated => "emulated",
        }
    }
}

/// Error key of the "not a decision model" refusal (localized in the app).
pub const ERR_DECISION_MODEL_REQUIRED: &str = "gateway.decision_model_required";
/// Error key of a choice question with more options than emulation can code.
pub const ERR_EMULATION_TOO_MANY_CHOICES: &str = "gateway.decision_emulation_choices";

fn bad(e: Invalid) -> GatewayError {
    let err = GatewayError::new(400, "invalid_request", e.message);
    match e.param {
        Some(p) => err.param(p),
        None => err,
    }
}

/// `400 model_not_decision` with its localizable key.
pub fn not_a_decision_model(alias: &str) -> GatewayError {
    use harness_core::error_key::err;
    GatewayError::new(
        400,
        "model_not_decision",
        format!(
            "\"{alias}\" is not a decision model. Use a decision model, or turn on chat model emulation for decision endpoints in the gateway settings."
        ),
    )
    .param("model")
    .details(json!({
        "error_key": err(ERR_DECISION_MODEL_REQUIRED).with("model", alias).to_string(),
    }))
}

/// An upstream error answer as a gateway error: llama-server's `501` (not a decision model, or
/// no image support) is the `400` of Ollama and OpenAI; its `400` (invalid request, prompt over
/// the context) keeps its message; anything else is a failed decision (`500`).
pub fn upstream_error(status: u16, body: &[u8], alias: &str) -> GatewayError {
    let message = crate::ollama::convert::upstream_error_message(body);
    match status {
        501 if message
            .to_ascii_lowercase()
            .contains("not a decision model") =>
        {
            not_a_decision_model(alias)
        }
        501 | 400 => GatewayError::new(400, "invalid_request", message),
        404 => not_a_decision_model(alias),
        _ => GatewayError::new(500, "decision_failed", message),
    }
}

pub async fn handle(state: GwState, flavor: Flavor, headers: HeaderMap, body: Bytes) -> Response {
    let mut guard = RecordGuard::new(
        RequestRecord {
            id: request_id(),
            started_at_ms: unix_ms(),
            endpoint: flavor.path().to_string(),
            ..Default::default()
        },
        crate::proxy::trace_context(&headers),
    );
    let id = guard.get().id.clone();
    let mut resp = match run(&state, flavor, &headers, body, &mut guard).await {
        Ok(resp) => resp,
        Err(err) => {
            guard.finish(err.status, Some(&err.code));
            match flavor {
                Flavor::SystemOne => crate::ollama::error_response(err),
                Flavor::Decisions => err.into_response(),
            }
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

/// Models that never answer decisions (speech, image, audio.cpp): refused before a load.
fn never_decides(def: &ModelDefinition) -> bool {
    crate::speech::is_audio_cpp_model(def)
        || crate::speech::is_crispasr_tts(def)
        || crate::audio::is_speech_model(def)
        || crate::images::is_image_model(def)
}

/// Text of the whole request, for the token admission estimate.
fn demand(req: &DecisionRequest) -> Option<harness_core::gateway::admission::Demand> {
    let mut text = emulate::text(&req.state);
    for q in &req.questions {
        text.push('\n');
        text.push_str(&emulate::text(&q.instructions));
        match &q.kind {
            Kind::Choice(c) => c.iter().for_each(|o| {
                text.push('\n');
                text.push_str(&o.key);
                if let Some(d) = &o.description {
                    text.push_str(d);
                }
            }),
            Kind::Score(l) => l.iter().for_each(|l| {
                text.push('\n');
                text.push_str(&l.text());
            }),
            Kind::Noul { yes, no } => {
                for d in [yes, no].into_iter().flatten() {
                    text.push('\n');
                    text.push_str(d);
                }
            }
        }
    }
    let mut content = vec![json!({"type": "text", "text": text})];
    for u in &req.images {
        content.push(json!({"type": "image_url", "image_url": {"url": u}}));
    }
    let body = json!({"messages": [{"role": "user", "content": content}], "max_tokens": 1});
    harness_core::gateway::admission::Demand::from_bytes(&serde_json::to_vec(&body).ok()?)
}

async fn run(
    state: &GwState,
    flavor: Flavor,
    headers: &HeaderMap,
    body: Bytes,
    guard: &mut RecordGuard,
) -> Result<Response, GatewayError> {
    let endpoint = GatewayEndpoint::of_path(flavor.path()).unwrap_or(GatewayEndpoint::Chat);
    let caller = crate::auth::check(state, headers, false, endpoint)?;
    guard.get().client = ClientInfo::new(
        header_str(headers, "user-agent"),
        header_str(headers, "x-harness-client"),
        caller.token_name.clone(),
    );
    let req = match flavor {
        Flavor::SystemOne => schema::parse_systemone(&body),
        Flavor::Decisions => schema::parse_decisions(&body),
    }
    .map_err(bad)?;
    guard.get().model = Some(req.model.clone());
    guard.admitted(caller.admit(state)?);
    let def = match flavor {
        Flavor::SystemOne => crate::ollama::resolve(state, &req.model).await?,
        Flavor::Decisions => state
            .scheduler
            .resolve(Some(&req.model))
            .await
            .map_err(|e| caller.sched_error(&e))?,
    };
    {
        let r = guard.get();
        r.alias = Some(def.alias.clone());
        r.definition_id = Some(def.id.clone());
    }
    caller.check_model(&def.alias)?;
    if never_decides(&def) {
        return Err(not_a_decision_model(&def.alias));
    }
    let settings = state.scheduler.settings();
    let native = def
        .base
        .local_path()
        .is_some_and(harness_core::models::is_decision_model);
    // Emulation off: llama-server decides; its 501 for a chat model is mapped below.
    let mode = if native || !settings.decision_emulation {
        Mode::Native
    } else {
        Mode::Emulated
    };
    if mode == Mode::Emulated {
        for (i, q) in req.questions.iter().enumerate() {
            if let Kind::Choice(c) = &q.kind {
                if c.len() > emulate::MAX_CHOICES {
                    use harness_core::error_key::err;
                    return Err(GatewayError::new(
                        400,
                        "invalid_request",
                        format!(
                            "\"{}\" is not a decision model: chat model emulation supports up to {} options per question.",
                            def.alias,
                            emulate::MAX_CHOICES
                        ),
                    )
                    .param(match flavor {
                        Flavor::SystemOne => format!(
                            "questions.{}.criteria",
                            q.name.as_deref().unwrap_or_default()
                        ),
                        Flavor::Decisions => format!("questions[{i}].choices"),
                    })
                    .details(json!({
                        "error_key": err(ERR_EMULATION_TOO_MANY_CHOICES)
                            .with("max", emulate::MAX_CHOICES)
                            .to_string(),
                    })));
                }
            }
        }
    }
    if def.capture_content {
        guard.capture = Some(ContentCapture::new(
            &body,
            def.content_retention_days
                .unwrap_or(analytics::DEFAULT_CONTENT_RETENTION_DAYS),
        ));
    }
    let keep_alive = crate::ollama::convert::parse_keep_alive(req.keep_alive.as_ref())
        .map_err(|m| GatewayError::new(400, "invalid_request", m).param("keep_alive"))?;
    let demand = demand(&req);
    let lease = match state
        .scheduler
        .acquire_keep_alive(&def, demand.as_ref(), keep_alive)
        .await
    {
        Ok(lease) => lease,
        Err(e) => {
            guard.get().oom = e.is_oom();
            return Err(GatewayError::from_sched(&e));
        }
    };
    crate::ollama::record_lease(guard, &lease);
    guard
        .get()
        .compat
        .push(format!("decision_mode={}", mode.as_str()));
    let timeout = match settings.request_timeout_secs {
        0 => None,
        n => Some(Duration::from_secs(u64::from(n))),
    };
    let outcome = match mode {
        Mode::Native => decide_native(state, &def, &lease, &req, timeout).await?,
        Mode::Emulated => decide_emulated(state, &def, &lease, &req, timeout).await?,
    };
    let elapsed = guard.started.elapsed().as_millis() as u64;
    {
        let r = guard.get();
        r.status = 200;
        r.usage.prompt_tokens = Some(outcome.input_tokens);
        r.usage.completion_tokens = Some(outcome.output_tokens);
        r.ttft_ms = Some(elapsed);
    }
    let out = match flavor {
        Flavor::SystemOne => schema::render_systemone(&req.model, &req, &outcome),
        Flavor::Decisions => schema::render_decisions(&req.model, &req, &outcome),
    };
    let bytes = serde_json::to_vec(&out).unwrap_or_default();
    if let Some(c) = guard.capture.as_mut() {
        c.push_response(&bytes);
    }
    guard.finish(200, None);
    let mut resp = (
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        bytes,
    )
        .into_response();
    let h = resp.headers_mut();
    for (k, v) in [
        ("x-harness-decision-mode", mode.as_str().to_string()),
        ("x-harness-model", def.alias.clone()),
        ("x-harness-server", lease.server_id.clone()),
        (
            "x-harness-queue-ms",
            lease.queue_wait.as_millis().to_string(),
        ),
    ] {
        if let Ok(v) = HeaderValue::from_str(&v) {
            h.insert(k, v);
        }
    }
    if let Some(load) = &lease.load {
        if let Ok(v) = HeaderValue::from_str(&load.ms.to_string()) {
            h.insert("x-harness-load-ms", v);
        }
    }
    drop(lease);
    Ok(resp)
}

/// One upstream JSON call: status and parsed body (the raw bytes when they are not JSON).
async fn post(
    state: &GwState,
    lease: &Lease,
    path: &str,
    body: Bytes,
    timeout: Option<Duration>,
) -> Result<(u16, Bytes), GatewayError> {
    let mut req = state
        .http
        .post(format!("{}{}", lease.upstream, path))
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(body);
    if let Some(key) = &lease.api_key {
        req = req.bearer_auth(key);
    }
    let call = async {
        let resp = req.send().await?;
        let status = resp.status().as_u16();
        Ok::<_, reqwest::Error>((status, resp.bytes().await?))
    };
    match timeout {
        Some(t) => tokio::time::timeout(t, call).await.map_err(|_| {
            GatewayError::new(
                504,
                "upstream_timeout",
                format!("the model server did not answer within {} s", t.as_secs()),
            )
        })?,
        None => call.await,
    }
    .map_err(|e| GatewayError::new(502, "upstream_unreachable", format!("model server: {e}")))
}

/// Questions a model need not see: a choice with a single option.
fn local_answer(q: &schema::Question) -> Option<Answer> {
    match &q.kind {
        Kind::Choice(c) if c.len() == 1 => Some(Answer::Choice(vec![1.0])),
        _ => None,
    }
}

async fn decide_native(
    state: &GwState,
    def: &ModelDefinition,
    lease: &Lease,
    req: &DecisionRequest,
    timeout: Option<Duration>,
) -> Result<Outcome, GatewayError> {
    let mut parts = Vec::new();
    let mut ask = Vec::new();
    for (i, q) in req.questions.iter().enumerate() {
        match local_answer(q) {
            Some(a) => parts.push((i, a)),
            None => ask.push(i),
        }
    }
    let mut outcome = Outcome::default();
    if !ask.is_empty() {
        let body = serde_json::to_vec(&native::request(req, &ask)).unwrap_or_default();
        let (status, bytes) =
            post(state, lease, "/v1/systemone", Bytes::from(body), timeout).await?;
        if status != 200 {
            return Err(upstream_error(status, &bytes, &def.alias));
        }
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|e| GatewayError::new(500, "decision_failed", e.to_string()))?;
        let (answers, input, output) = native::answers(req, &ask, &value)
            .map_err(|m| GatewayError::new(500, "decision_failed", m))?;
        parts.extend(ask.into_iter().zip(answers));
        outcome.add_usage(input, output);
    }
    outcome.answers = native::merge(parts, req.questions.len());
    Ok(outcome)
}

async fn decide_emulated(
    state: &GwState,
    def: &ModelDefinition,
    lease: &Lease,
    req: &DecisionRequest,
    timeout: Option<Duration>,
) -> Result<Outcome, GatewayError> {
    let mut outcome = Outcome::default();
    for q in &req.questions {
        if let Some(a) = local_answer(q) {
            outcome.answers.push(a);
            continue;
        }
        let request = emulate::request(&def.alias, &req.state, &req.images, q);
        let mut bytes = serde_json::to_vec(&request).unwrap_or_default();
        bytes = crate::reasoning::translate(&bytes, lease.reasoning_control).unwrap_or_else(|| {
            let mut v = request.clone();
            if let Some(o) = v.as_object_mut() {
                o.remove("think");
            }
            serde_json::to_vec(&v).unwrap_or_default()
        });
        let body = crate::proxy::upstream_body(def, Bytes::from(bytes));
        let (status, resp) = post(state, lease, "/v1/chat/completions", body, timeout).await?;
        if status != 200 {
            let message = crate::ollama::convert::upstream_error_message(&resp);
            return Err(if status == 400 {
                GatewayError::new(400, "invalid_request", message)
            } else {
                GatewayError::new(500, "decision_failed", message)
            });
        }
        let value: Value = serde_json::from_slice(&resp)
            .map_err(|e| GatewayError::new(500, "decision_failed", e.to_string()))?;
        let codes = emulate::codes(&q.kind);
        let p = emulate::probabilities(&value, &codes).ok_or_else(|| {
            GatewayError::new(
                500,
                "decision_failed",
                format!(
                    "\"{}\" returned no token probabilities for the options; emulation needs a llama.cpp model server",
                    def.alias
                ),
            )
        })?;
        outcome.answers.push(emulate::answer(&q.kind, p));
        let usage = |k: &str| value["usage"][k].as_u64().unwrap_or(0);
        outcome.add_usage(usage("prompt_tokens"), usage("completion_tokens"));
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests;
