//! Server-side tools: the gateway completes a tool request itself.
//!
//! A client such as Codex sends the hosted `web_search` tool, which llama-server cannot run, and
//! the gateway answers `400 unsupported_tool_type`. With the user's opt-in for a model
//! (`GatewaySettings::server_tools`) the gateway instead runs the core streaming agent loop
//! (`harness_core::tools::agent_stream`) for that request:
//!
//! 1. [`plan`] reads the request: hosted `web_search` tools are removed (Harness's own
//!    `web_search` takes over), the Harness tools enabled for the model are added, the client's
//!    own function tools stay. The request becomes a chat-completions conversation
//!    ([`convert`]).
//! 2. [`serve`] checks that the user's tool policy lets every offered tool run **without
//!    asking** (the gateway has nobody to ask) and answers a clear `403` otherwise.
//! 3. The loop talks to the model through the scheduler (one lease per round, so the slot is
//!    free while a tool runs), runs the calls through the tool engine (policy, working
//!    directory, limits, tool log) and feeds the results back.
//! 4. The client gets one answer in its own wire format ([`wire`]): JSON or SSE, Chat
//!    Completions, Responses or Anthropic Messages. Calls the model makes to the *client's* tools
//!    end the loop and are handed back like any model answer would.
//!
//! Visible in `X-Harness-Compat` (`server_tools=…`), in the request record's `compat` notes
//! (with the number of server-side calls and rounds) and, per call, in the tool log (client
//! `gateway`, conversation = the request id).

mod convert;
mod wire;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use harness_core::domain::ModelDefinition;
use harness_core::gateway::admission::Demand;
use harness_core::gateway::record::RequestRecord;
use harness_core::gateway::ServerTools;
use harness_core::tools::agent::{DenyAll, StopReason};
use harness_core::tools::agent_stream::{
    run_agent_stream, AgentError, AgentEvent, ModelEndpoint, OpenedRound, StreamOutcome,
    StreamRequest, ToolSelection,
};
use harness_core::tools::{
    CancelToken, Decision, ToolLimits, ToolOrigin, ToolService, ToolSettings,
};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

use crate::compat::{self, Api, Prepared, Unsupported};
use crate::errors::GatewayError;
use crate::proxy::RecordGuard;
use crate::responses::Pending;
use crate::GwState;
use wire::{ChatWire, Finish, Meta, MsgWire, RespWire, Wire};

/// The Harness tool the hosted `web_search` request is fulfilled with.
pub const WEB_SEARCH_TOOL: &str = "web_search";

/// Why a request cannot be served (the normal path then answers it, or rejects it).
#[derive(Debug)]
pub(crate) enum PlanError {
    /// A tool the gateway can neither run nor drop (same rejection as without server tools).
    Unsupported(Unsupported),
    Invalid(String),
}

/// A request the gateway completes itself.
#[derive(Debug)]
pub(crate) struct Plan {
    /// Namespace flattening and ignored tools of the normal compatibility pass.
    pub prepared: Prepared,
    /// Harness tools offered to the model (and run by the gateway).
    pub offered: Vec<String>,
    /// Hosted tool types of the request that Harness fulfils (`web_search`).
    pub hosted: Vec<String>,
    job: convert::Job,
    /// Fields of a Responses request echoed in the response object.
    echo: Value,
}

/// Removes the hosted `web_search` request from a request document; returns what it found.
fn strip_hosted(api: Api, obj: &mut serde_json::Map<String, Value>) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(Value::Array(tools)) = obj.get_mut("tools") {
        let before = tools.len();
        tools.retain(|t| {
            !t.get("type")
                .and_then(Value::as_str)
                .is_some_and(|ty| ty.starts_with("web_search"))
        });
        if tools.len() != before {
            found.push(WEB_SEARCH_TOOL.to_string());
        }
    }
    if api == Api::Chat && obj.remove("web_search_options").is_some() {
        found.push(WEB_SEARCH_TOOL.to_string());
    }
    // A client that forces the hosted tool forces Harness's for the first round.
    if let Some(choice) = obj.get_mut("tool_choice") {
        let hosted = choice
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|t| t.starts_with("web_search"));
        if hosted {
            *choice = json!({"type": "function", "name": WEB_SEARCH_TOOL});
            if !found.iter().any(|f| f == WEB_SEARCH_TOOL) {
                found.push(WEB_SEARCH_TOOL.to_string());
            }
        }
    }
    found.dedup();
    found
}

/// Decides whether the gateway serves this request itself.
///
/// * `Ok(None)`: no (nothing enabled applies, or the request has a hosted tool the user did not
///   allow Harness to fulfil: the normal path rejects it as before).
/// * `Ok(Some)`: yes, with the converted conversation.
pub(crate) fn plan(
    api: Api,
    body: &[u8],
    cfg: &ServerTools,
    ignore: &[String],
) -> Result<Option<Plan>, PlanError> {
    if !matches!(api, Api::Chat | Api::Responses | Api::Messages) {
        return Ok(None);
    }
    let Ok(mut doc) = serde_json::from_slice::<Value>(body) else {
        return Ok(None);
    };
    let Some(obj) = doc.as_object_mut() else {
        return Ok(None);
    };
    let hosted = strip_hosted(api, obj);
    if !hosted.is_empty() && !cfg.web_search {
        return Ok(None);
    }
    if hosted.is_empty() && cfg.tools.is_empty() {
        return Ok(None);
    }
    let stripped = serde_json::to_vec(&doc).unwrap_or_default();
    let prepared = compat::prepare(api, &stripped, ignore).map_err(PlanError::Unsupported)?;
    let final_body: Value = match &prepared.body {
        Some(b) => serde_json::from_slice(b).unwrap_or(doc),
        None => doc,
    };
    let Some(obj) = final_body.as_object() else {
        return Ok(None);
    };
    let job = convert::job(api, obj).map_err(PlanError::Invalid)?;
    let mut offered = cfg.tools.clone();
    if !hosted.is_empty() && !offered.iter().any(|t| t == WEB_SEARCH_TOOL) {
        offered.push(WEB_SEARCH_TOOL.to_string());
    }
    let mut echo = serde_json::Map::new();
    for k in [
        "instructions",
        "temperature",
        "top_p",
        "max_output_tokens",
        "parallel_tool_calls",
        "tool_choice",
    ] {
        if let Some(v) = obj.get(k).filter(|v| !v.is_null()) {
            echo.insert(k.to_string(), v.clone());
        }
    }
    if let Some(t) = obj.get("tools").filter(|t| t.is_array()) {
        echo.insert("tools".into(), t.clone());
    }
    Ok(Some(Plan {
        prepared,
        offered,
        hosted,
        job,
        echo: Value::Object(echo),
    }))
}

/// The user's policy against the tools the gateway would run: only what runs without asking.
async fn check_policy(
    service: &ToolService,
    settings: &ToolSettings,
    offered: &[String],
    loopback: bool,
) -> Result<(), GatewayError> {
    if offered.iter().any(|n| service.registry().get(n).is_none()) {
        // An MCP server starts on first use (`npx` may take a while): wait for it, but not
        // forever; the start goes on in the background for the next request.
        service
            .sync_mcp_bounded(false, Duration::from_secs(30))
            .await;
    }
    let policy = serde_json::to_value(settings.policy).unwrap_or(Value::Null);
    for name in offered {
        let Some(tool) = service.registry().get(name) else {
            return Err(GatewayError::new(
                503,
                "server_tool_unavailable",
                format!(
                    "The server-side tool \"{name}\" is enabled for this model but Harness has no such tool (is its MCP server connected? Settings > Tools / Connections)."
                ),
            )
            .details(json!({"tool": name})));
        };
        let class = settings.class_for(tool.def());
        // Writing files, shells and processes are never run for whoever can reach a network
        // bind, whatever the policy says: the policy is the owner's choice for their own
        // clients on this machine.
        if class == harness_core::tools::PermissionClass::Dangerous && !loopback {
            return Err(GatewayError::new(
                403,
                "server_tool_not_allowed",
                format!(
                    "The server-side tool \"{name}\" is a dangerous tool (it writes files or runs commands) and the gateway is bound to a network address: such tools only run server-side while the gateway listens on this machine only."
                ),
            )
            .param("tools")
            .details(json!({"tool": name, "class": class.as_str(), "loopback": false})));
        }
        match settings.decide(name, class) {
            Decision::Run => {}
            decision => {
                let why = if decision == Decision::Refuse {
                    "the tool policy is \"off\"".to_string()
                } else {
                    format!(
                        "the tool policy ({}) asks the user before running {} tools and the gateway cannot ask anyone",
                        policy.as_str().unwrap_or("?"),
                        class.as_str()
                    )
                };
                return Err(GatewayError::new(
                    403,
                    "server_tool_not_allowed",
                    format!(
                        "The server-side tool \"{name}\" cannot run: {why}. Mark it trusted (Settings > Tools, policy \"allow trusted\"), choose \"allow all\", or remove it from this model's server-side tools (Settings > Gateway)."
                    ),
                )
                .param("tools")
                .details(json!({
                    "tool": name, "class": class.as_str(), "policy": policy,
                    "decision": decision,
                })));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The model side: the scheduler, one lease per round
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
struct Stats {
    server_id: Option<String>,
    queue_wait_ms: u64,
    load_ms: Option<u64>,
    evicted: Vec<String>,
    oom: bool,
    ctx_size: Option<u64>,
    backend: Option<String>,
    slots: Option<u32>,
    waited: Option<String>,
}

impl Stats {
    fn apply(&self, r: &mut RequestRecord) {
        r.server_id = self.server_id.clone();
        r.queue_wait_ms = self.queue_wait_ms;
        r.load_ms = self.load_ms;
        r.evicted = self.evicted.clone();
        r.oom = self.oom;
        r.ctx_size = self.ctx_size;
        r.backend = self.backend.clone();
        r.slots = self.slots;
        r.waited = self.waited.clone();
    }
}

type Ready = oneshot::Sender<Result<(), GatewayError>>;

struct GatewayEndpoint {
    state: GwState,
    def: ModelDefinition,
    timeout: Option<Duration>,
    trace: Option<harness_core::otel::RequestTraceContext>,
    stats: Arc<Mutex<Stats>>,
    /// The scheduler's or llama-server's own error of a failed round (the loop only sees text).
    failure: Arc<Mutex<Option<GatewayError>>>,
    /// Told once whether the first round could start (a stream answers its HTTP status then).
    ready: Mutex<Option<Ready>>,
}

impl GatewayEndpoint {
    fn signal(&self, result: Result<(), GatewayError>) {
        if let Some(tx) = self.ready.lock().ok().and_then(|mut r| r.take()) {
            let _ = tx.send(result);
        }
    }

    fn fail(&self, err: GatewayError) -> AgentError {
        let out = AgentError::new(err.code.clone(), err.message.clone());
        if let Ok(mut f) = self.failure.lock() {
            f.get_or_insert_with(|| err.clone());
        }
        self.signal(Err(err));
        out
    }

    async fn open_round(&self, body: Value) -> Result<OpenedRound, GatewayError> {
        let bytes = serde_json::to_vec(&body).unwrap_or_default();
        let demand = Demand::from_bytes(&bytes);
        let lease = match self
            .state
            .scheduler
            .acquire_tokens(&self.def, demand.as_ref())
            .await
        {
            Ok(l) => l,
            Err(e) => {
                if let Ok(mut s) = self.stats.lock() {
                    s.oom |= e.is_oom();
                }
                return Err(GatewayError::from_sched(&e));
            }
        };
        if let Ok(mut s) = self.stats.lock() {
            s.server_id = Some(lease.server_id.clone());
            s.queue_wait_ms += lease.queue_wait.as_millis() as u64;
            s.slots = Some(lease.slots);
            s.waited = lease.waited.map(str::to_string);
            s.ctx_size = lease.ctx_size;
            s.backend = lease.backend.clone();
            if let Some(load) = &lease.load {
                s.load_ms = Some(s.load_ms.unwrap_or(0) + load.ms);
                s.evicted.extend(load.evicted.iter().cloned());
                s.oom |= load.recovered;
            }
        }
        let bytes = crate::reasoning::translate(&bytes, lease.reasoning_control).unwrap_or(bytes);
        let mut req = self
            .state
            .http
            .post(format!("{}{}", lease.upstream, Api::Chat.path()))
            .header(header::CONTENT_TYPE, "application/json")
            .body(crate::proxy::upstream_body(&self.def, bytes.into()));
        if let Some(context) = &self.trace {
            req = req.header("traceparent", context.traceparent());
            if let Some(state) = context.tracestate() {
                req = req.header("tracestate", state);
            }
        }
        if let Some(key) = &lease.api_key {
            req = req.bearer_auth(key);
        }
        let send = req.send();
        let upstream = match self.timeout {
            Some(t) => tokio::time::timeout(t, send).await.map_err(|_| {
                GatewayError::new(
                    504,
                    "upstream_timeout",
                    format!("llama-server did not answer within {} s.", t.as_secs()),
                )
            })?,
            None => send.await,
        }
        .map_err(|e| {
            GatewayError::new(502, "upstream_unreachable", format!("llama-server: {e}"))
        })?;
        let status = upstream.status();
        if !status.is_success() {
            let text = upstream.text().await.unwrap_or_default();
            return Err(GatewayError::new(
                status.as_u16(),
                "upstream_error",
                format!("llama-server answered {status}: {text}"),
            ));
        }
        Ok(OpenedRound {
            stream: upstream
                .bytes_stream()
                .map(|r| r.map_err(|e| e.to_string()))
                .boxed(),
            hold: Some(Box::new(lease)),
        })
    }
}

impl ModelEndpoint for GatewayEndpoint {
    fn open<'a>(
        &'a self,
        _round: u32,
        body: Value,
    ) -> futures_util::future::BoxFuture<'a, Result<OpenedRound, AgentError>> {
        Box::pin(async move {
            match self.open_round(body).await {
                Ok(opened) => {
                    self.signal(Ok(()));
                    Ok(opened)
                }
                Err(e) => Err(self.fail(e)),
            }
        })
    }
}

fn agent_error(e: &AgentError) -> GatewayError {
    match e.code.as_str() {
        "idle_timeout" => GatewayError::new(504, "upstream_idle_timeout", e.message.clone()),
        "request" => GatewayError::new(502, "upstream_unreachable", e.message.clone()),
        _ => GatewayError::new(502, "upstream_error", e.message.clone()),
    }
}

// ---------------------------------------------------------------------------------------------
// Serving
// ---------------------------------------------------------------------------------------------

pub(crate) struct ServeArgs<'a> {
    // (`stateful` is the Responses state of the request: `None` for every other API.)
    pub state: &'a GwState,
    pub api: Api,
    pub def: &'a ModelDefinition,
    pub plan: Plan,
    pub stateful: Option<Pending>,
    pub compaction_note: Option<String>,
    pub guard: &'a mut RecordGuard,
    pub timeout: Option<Duration>,
}

/// Cancels the loop when dropped (the client went away).
struct CancelOnDrop(CancelToken);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// The body of a stream: what the loop task sends, until it ends.
struct ChannelBody {
    rx: mpsc::UnboundedReceiver<Bytes>,
    _cancel: CancelOnDrop,
}

impl Stream for ChannelBody {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx).map(|o| o.map(Ok))
    }
}

fn now_secs() -> u64 {
    crate::proxy::unix_ms() / 1000
}

/// Shown to the client when the round limit stopped a model that still wanted tools.
fn round_limit_note(limits: &ToolLimits) -> String {
    format!(
        "[Harness: the server-side tool round limit ({}) was reached before the model gave an answer.]",
        limits.max_rounds
    )
}

fn call_counts(outcome: &StreamOutcome) -> (usize, usize) {
    let failed = outcome
        .calls
        .iter()
        .filter(|c| c.result.as_ref().is_none_or(|r| r.is_error()))
        .count();
    (outcome.calls.len(), failed)
}

/// The tool round of the request (what `tool_diag` records for a normal request): offered
/// tools, calls made by the model (server-side and the client's).
fn record_tools(r: &mut RequestRecord, offered: u32, outcome: &StreamOutcome) {
    r.tool_involved = true;
    r.tools_offered = offered;
    r.tool_calls = (outcome.calls.len() + outcome.client_calls.len()) as u32;
}

/// Writes what the loop did into the request record.
fn finish_notes(notes: &mut Vec<String>, outcome: &StreamOutcome) {
    let (n, failed) = call_counts(outcome);
    notes.push(format!("server_tool_calls={n}"));
    if failed > 0 {
        notes.push(format!("server_tool_errors={failed}"));
    }
    notes.push(format!("server_tool_rounds={}", outcome.rounds));
    if outcome.dropped_calls > 0 {
        notes.push(format!(
            "server_calls_dropped_for_client_calls={}",
            outcome.dropped_calls
        ));
    }
    if outcome.stop == StopReason::RoundLimit {
        notes.push("server_tool_round_limit".into());
    }
}

pub(crate) async fn serve(a: ServeArgs<'_>) -> Result<Response, GatewayError> {
    let ServeArgs {
        state,
        api,
        def,
        plan,
        stateful,
        compaction_note,
        guard,
        timeout,
    } = a;
    let service = ToolService::shared();
    let settings = harness_core::config::AppSettings::load().tools;
    check_policy(service, &settings, &plan.offered, state.loopback).await?;

    let mut notes = plan.prepared.notes();
    notes.push(format!("server_tools={}", plan.offered.join(",")));
    if !plan.hosted.is_empty() {
        notes.push(format!("hosted_fulfilled={}", plan.hosted.join(",")));
    }
    guard.get().compat = notes.clone();
    let compat_header = notes.join("; ");

    let request_id = guard.get().id.clone();
    let meta = Meta {
        id: request_id.clone(),
        model: def.alias.clone(),
        created: now_secs(),
        include_usage: plan.job.include_usage,
        names: plan.prepared.names.clone(),
    };
    let wire = match api {
        Api::Chat => Wire::Chat(ChatWire::new(meta)),
        Api::Responses => Wire::Responses(Box::new(RespWire::new(meta, stateful, plan.echo))),
        _ => Wire::Messages(MsgWire::new(meta)),
    };
    let tools_offered = (plan.offered.len() + plan.job.client_tools.len()) as u32;
    let limits = ToolLimits::default();
    let cancel = CancelToken::new();
    let mut req = StreamRequest::new(def.alias.clone(), plan.job.messages);
    req.params = plan.job.params;
    req.client_tools = plan.job.client_tools;
    req.tools = ToolSelection::Only(plan.offered.clone());
    req.origin = ToolOrigin::new("gateway")
        .conversation(request_id)
        .model(def.alias.clone());
    req.settings = Some(settings);
    req.limits = limits;
    req.stream_idle = timeout;
    req.cancel = cancel.clone();

    let stats = Arc::new(Mutex::new(Stats::default()));
    let failure = Arc::new(Mutex::new(None));
    let (ready_tx, ready_rx) = oneshot::channel();
    let endpoint = GatewayEndpoint {
        state: state.clone(),
        def: def.clone(),
        timeout,
        trace: guard.trace_context.clone(),
        stats: stats.clone(),
        failure: failure.clone(),
        ready: Mutex::new(Some(ready_tx)),
    };
    let stream = plan.job.stream;
    let started = guard.started;

    let mut builder = Response::builder()
        .header("x-harness-model", &def.alias)
        .header("x-harness-compat", compat_header);
    if let Some(n) = compaction_note
        .as_deref()
        .and_then(|n| HeaderValue::from_str(n).ok())
    {
        builder = builder.header("x-harness-compaction", n);
    }

    if !stream {
        // One JSON body: run the loop to its end here.
        let _stop = CancelOnDrop(cancel.clone());
        let mut wire = wire;
        let mut first_text = None;
        let mut emit = |ev: AgentEvent| {
            if first_text.is_none() && matches!(ev, AgentEvent::Text(_)) {
                first_text = Some(started.elapsed().as_millis() as u64);
            }
            let _ = wire.event(&ev);
        };
        let result = run_agent_stream(service, &endpoint, req, &DenyAll, &mut emit, None).await;
        let outcome = match result {
            Ok(o) => o,
            Err(e) => {
                let err = failure
                    .lock()
                    .ok()
                    .and_then(|mut f| f.take())
                    .unwrap_or_else(|| agent_error(&e));
                return Err(err);
            }
        };
        if outcome.stop == StopReason::RoundLimit {
            emit(AgentEvent::Text(round_limit_note(&limits)));
        }
        let fin = Finish::of(&outcome);
        let mut bytes = serde_json::to_vec(&wire.json(&fin)).unwrap_or_default();
        // Stateful Responses: decorated and stored before the client sees the answer.
        if let Wire::Responses(w) = &wire {
            if let Some(p) = &w.pending {
                if let Some(done) = p.complete(&bytes).await {
                    bytes = done;
                }
            }
        }
        if let Some(c) = guard.capture.as_mut() {
            c.push_response(&bytes);
        }
        let s = stats.lock().map(|s| s.clone()).unwrap_or_default();
        let mut notes = notes;
        finish_notes(&mut notes, &outcome);
        {
            let r = guard.get();
            s.apply(r);
            record_tools(r, tools_offered, &outcome);
            r.usage = outcome.usage.clone();
            r.compat = notes;
            r.ttft_ms = first_text.or(Some(started.elapsed().as_millis() as u64));
        }
        guard.finish(200, None);
        let b = builder
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json");
        let b = with_server_headers(b, &s);
        return b
            .body(Body::from(bytes))
            .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()));
    }

    // A stream: the loop runs in a task that feeds the body. The HTTP status waits for the
    // first round to start, so "no memory", "queue full" and the like are real HTTP errors.
    let (tx, rx) = mpsc::unbounded_channel::<Bytes>();
    let stop = CancelOnDrop(cancel.clone());
    let mut owned = guard.take();
    let anthropic = api.anthropic();
    let limits_note = round_limit_note(&limits);
    let task_stats = stats.clone();
    let task_failure = failure.clone();
    tokio::spawn(async move {
        let mut wire = wire;
        let mut notes = notes;
        let push = |guard: &mut RecordGuard, b: Bytes, tx: &mpsc::UnboundedSender<Bytes>| -> bool {
            if let Some(c) = guard.capture.as_mut() {
                c.push_response(&b);
            }
            tx.send(b).is_ok()
        };
        for b in wire.begin() {
            push(&mut owned, b, &tx);
        }
        let result = {
            let mut emit = |ev: AgentEvent| {
                if matches!(ev, AgentEvent::Text(_)) && owned.get().ttft_ms.is_none() {
                    owned.get().ttft_ms = Some(started.elapsed().as_millis() as u64);
                }
                for b in wire.event(&ev) {
                    if !push(&mut owned, b, &tx) {
                        cancel.cancel();
                    }
                }
            };
            let r = run_agent_stream(service, &endpoint, req, &DenyAll, &mut emit, None).await;
            if let Ok(o) = &r {
                if o.stop == StopReason::RoundLimit {
                    emit(AgentEvent::Text(limits_note));
                }
            }
            r
        };
        let s = task_stats.lock().map(|s| s.clone()).unwrap_or_default();
        match result {
            Ok(outcome) => {
                let fin = Finish::of(&outcome);
                let out = match &mut wire {
                    Wire::Responses(w) => {
                        let mut out = w.close_events();
                        let mut r = w.response(&fin);
                        if let Some(p) = &w.pending {
                            if let Some(done) = p
                                .complete(&serde_json::to_vec(&r).unwrap_or_default())
                                .await
                            {
                                r = serde_json::from_slice(&done).unwrap_or(r);
                            }
                        }
                        out.extend(w.end_with(r));
                        out
                    }
                    other => other.end(&fin),
                };
                for b in out {
                    push(&mut owned, b, &tx);
                }
                finish_notes(&mut notes, &outcome);
                {
                    let r = owned.get();
                    s.apply(r);
                    record_tools(r, tools_offered, &outcome);
                    r.usage = outcome.usage.clone();
                    r.compat = notes;
                    r.cancelled = outcome.stop == StopReason::Cancelled;
                }
                let status = if outcome.stop == StopReason::Cancelled {
                    499
                } else {
                    200
                };
                owned.finish(status, None);
            }
            Err(e) => {
                let err = task_failure
                    .lock()
                    .ok()
                    .and_then(|mut f| f.take())
                    .unwrap_or_else(|| agent_error(&e));
                for b in wire.fail(&e) {
                    push(&mut owned, b, &tx);
                }
                s.apply(owned.get());
                owned.get().compat = notes;
                owned.finish(err.status, Some(&err.code));
            }
        }
        drop(tx);
    });

    match ready_rx.await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            // The task already recorded the failure; the client gets the plain HTTP error.
            return Ok(axum::response::IntoResponse::into_response(
                err.anthropic(anthropic),
            ));
        }
        Err(_) => {
            return Err(GatewayError::new(
                500,
                "gateway_error",
                "the server-side tool loop ended before it started",
            ))
        }
    }
    let s = stats.lock().map(|s| s.clone()).unwrap_or_default();
    let b = builder
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no");
    let b = with_server_headers(b, &s);
    b.body(Body::from_stream(ChannelBody { rx, _cancel: stop }))
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))
}

fn with_server_headers(
    mut b: axum::http::response::Builder,
    s: &Stats,
) -> axum::http::response::Builder {
    if let Some(id) = &s.server_id {
        b = b.header("x-harness-server", id);
    }
    b = b.header("x-harness-queue-ms", s.queue_wait_ms.to_string());
    if let Some(ms) = s.load_ms {
        b = b.header("x-harness-load-ms", ms.to_string());
    }
    if !s.evicted.is_empty() {
        b = b.header("x-harness-evicted", s.evicted.join(","));
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(web: bool, tools: &[&str]) -> ServerTools {
        ServerTools {
            web_search: web,
            tools: tools.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn run(api: Api, body: Value, cfg: &ServerTools) -> Result<Option<Plan>, PlanError> {
        plan(api, &serde_json::to_vec(&body).unwrap(), cfg, &[])
    }

    /// What Codex sends: its own function tool, an MCP namespace and the hosted search.
    fn codex() -> Value {
        json!({
            "model": "qwen", "stream": true,
            "input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": "news?"}]}],
            "tools": [
                {"type": "function", "name": "shell", "parameters": {"type": "object"}},
                {"type": "namespace", "name": "mcp__gh", "tools": [
                    {"type": "function", "name": "issue", "parameters": {"type": "object"}}]},
                {"type": "web_search", "external_web_access": true}
            ]
        })
    }

    #[test]
    fn hosted_web_search_is_taken_over_only_when_allowed() {
        // Off: the normal path rejects the tool, as before.
        assert!(run(Api::Responses, codex(), &cfg(false, &[]))
            .unwrap()
            .is_none());
        // On: the tool is replaced by Harness's own, the client's tools stay (namespace
        // flattened for the model, mapped back for the client).
        let p = run(Api::Responses, codex(), &cfg(true, &[]))
            .unwrap()
            .unwrap();
        assert_eq!(p.offered, ["web_search"]);
        assert_eq!(p.hosted, ["web_search"]);
        let names: Vec<&str> = p
            .job
            .client_tools
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["shell", "mcp__gh__issue"]);
        assert_eq!(p.prepared.header(), "namespace_flattened=1");
        assert!(p.job.stream);
    }

    #[test]
    fn enabled_tools_are_offered_on_every_request() {
        let body = json!({"model": "m", "messages": [{"role": "user", "content": "2+2"}]});
        assert!(run(Api::Chat, body.clone(), &cfg(true, &[]))
            .unwrap()
            .is_none());
        let p = run(
            Api::Chat,
            body,
            &cfg(false, &["calculator", "current_datetime"]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(p.offered, ["calculator", "current_datetime"]);
        assert!(p.hosted.is_empty());
        assert!(p.job.client_tools.is_empty());
    }

    #[test]
    fn other_hosted_tools_still_reject_and_formats_are_found() {
        let body = json!({"model": "m", "input": "x", "tools": [
            {"type": "web_search"}, {"type": "image_generation"}]});
        let err = run(Api::Responses, body, &cfg(true, &[])).unwrap_err();
        assert!(matches!(err, PlanError::Unsupported(u) if u.tool_type == "image_generation"));
        // Chat: the `web_search_options` parameter; Anthropic: the versioned server tool.
        let chat = json!({"model": "m", "messages": [{"role": "user", "content": "x"}],
                          "web_search_options": {"search_context_size": "low"}});
        let p = run(Api::Chat, chat, &cfg(true, &[])).unwrap().unwrap();
        assert_eq!(p.hosted, ["web_search"]);
        let msg = json!({"model": "m", "max_tokens": 50, "messages": [{"role": "user", "content": "x"}],
            "tools": [{"type": "web_search_20250305", "name": "web_search", "max_uses": 3},
                      {"name": "mine", "input_schema": {"type": "object"}}]});
        let p = run(Api::Messages, msg, &cfg(true, &[])).unwrap().unwrap();
        assert_eq!(p.hosted, ["web_search"]);
        assert_eq!(p.job.client_tools.len(), 1);
        // A forced hosted tool becomes a forced Harness tool for the first round.
        let forced = json!({"model": "m", "input": "x", "tools": [{"type": "web_search_preview"}],
                            "tool_choice": {"type": "web_search_preview"}});
        let p = run(Api::Responses, forced, &cfg(true, &[]))
            .unwrap()
            .unwrap();
        assert_eq!(
            p.job.params["tool_choice"]["function"]["name"],
            "web_search"
        );
    }

    /// The policy gate: only what runs without asking, nothing dangerous for a network bind.
    #[tokio::test]
    async fn only_tools_the_policy_runs_unasked_are_served() {
        use harness_core::tools::{LogSink, ToolPolicy, ToolRegistry};
        let service = ToolService::new(ToolRegistry::with_builtin(), LogSink::None);
        let settings = |policy, trusted: &[&str]| ToolSettings {
            policy,
            trusted: trusted.iter().map(|t| t.to_string()).collect(),
            ..Default::default()
        };
        let names = |n: &[&str]| n.iter().map(|t| t.to_string()).collect::<Vec<_>>();
        let shell = names(&["exec_shell_command"]);
        let safe = names(&["calculator", "current_datetime"]);

        // Safe tools run under every policy but "off" (nobody has to be asked).
        let ask = settings(ToolPolicy::AskEveryTime, &[]);
        check_policy(&service, &ask, &safe, true).await.unwrap();
        let off = settings(ToolPolicy::Off, &[]);
        let e = check_policy(&service, &off, &safe, true).await.unwrap_err();
        assert_eq!(
            (e.status, e.code.as_str()),
            (403, "server_tool_not_allowed")
        );
        // Restricted and dangerous tools ask under "ask" and "allow trusted" unless trusted.
        let e = check_policy(&service, &ask, &names(&["web_search"]), true)
            .await
            .unwrap_err();
        assert!(e.message.contains("cannot ask anyone"), "{}", e.message);
        let trusted = settings(
            ToolPolicy::AllowTrusted,
            &["web_search", "exec_shell_command"],
        );
        check_policy(&service, &trusted, &names(&["web_search"]), true)
            .await
            .unwrap();
        check_policy(&service, &trusted, &shell, true)
            .await
            .unwrap();
        // ... but a dangerous tool is not served to a network bind, trusted or not.
        let all = settings(ToolPolicy::AllowAll, &[]);
        let e = check_policy(&service, &all, &shell, false)
            .await
            .unwrap_err();
        assert!(e.message.contains("network"), "{}", e.message);
        check_policy(&service, &all, &names(&["web_search", "read_file"]), false)
            .await
            .unwrap();
        // A tool Harness does not have.
        let e = check_policy(&service, &all, &names(&["nope"]), true)
            .await
            .unwrap_err();
        assert_eq!(e.status, 503);
    }
}
