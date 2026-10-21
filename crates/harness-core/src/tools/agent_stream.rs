//! The streaming agent loop (T1c): a model that calls tools, driven without a UI.
//!
//! Same loop as the Playground's (`runAgentLoop`) and as [`super::agent::run_agent`], but the
//! model request **streams**: tokens go to an event sink as they arrive while the `tool_calls`
//! deltas are collected. Then, per round: policy check → run through [`ToolService`] → append the
//! `tool` result → continue, until the model answers without calls, the round limit
//! ([`ToolLimits::max_rounds`]) is reached or the run is cancelled.
//!
//! What it adds to the seed:
//!
//! * **Streaming** (`stream: true`, SSE `chat.completion.chunk`): content and reasoning deltas
//!   reach the sink at once ([`AgentEvent`]); calls are assembled from their fragments
//!   ([`ToolCallAccumulator`], the Rust twin of the UI's accumulator).
//! * **Where the model lives** is a [`ModelEndpoint`]: [`HttpEndpoint`] for a plain
//!   OpenAI-compatible server, the gateway's own implementation for a scheduler lease per round.
//! * **Two kinds of tools.** The ones the loop *owns* (Harness tools of the registry) run
//!   here. Tools the *client* owns (`client_tools`, opaque OpenAI tool objects) are offered to
//!   the model too but never run here: when a round calls one, the loop stops and hands the
//!   client's calls back ([`StopReason::ClientToolCalls`]). The gateway's server-side tools use
//!   this to serve a client that has tools of its own next to Harness's.
//! * **Cancellation** ends the stream read and a running tool call.
//! * **A hook before the next round** ([`RoundHook`]) may rewrite the conversation (the place
//!   for in-answer compaction, which the Playground does with its `beforeNextRound`).
//! * **Idle timeout** per stream chunk, **per-call timeout** and **result size limit** from
//!   [`ToolLimits`] (the latter is enforced by the registry).
//!
//! Approval: the loop never asks a human by itself. An [`Approver`] answers when the policy says
//! "ask"; the gateway has nobody to ask and passes [`super::agent::DenyAll`].

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

use bytes::Bytes;
use futures_util::future::BoxFuture;
use futures_util::{Stream, StreamExt};
use serde_json::{json, Map, Value};

use super::agent::{AgentCall, Approver, CallContext, StopReason};
use super::limits::ToolLimits;
use super::policy::ToolSettings;
use super::registry::CancelToken;
use super::service::ToolService;
use super::types::*;
use crate::gateway::record::Usage;

/// The body of one streaming model response.
pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>>;

/// A model response that is open for reading.
pub struct OpenedRound {
    pub stream: ByteStream,
    /// Kept alive until the round's stream is read to its end or dropped (the gateway keeps its
    /// scheduler lease here, so the slot is held exactly while the model generates).
    pub hold: Option<Box<dyn Any + Send>>,
}

/// A failure of the model side of the loop (not of a tool: tool failures are results the model
/// reads).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentError {
    /// `request` (no connection), `http_status`, `bad_stream` (not chat completions),
    /// `idle_timeout`, `upstream` (an error inside the stream) or a code of the endpoint.
    pub code: String,
    pub message: String,
}

impl AgentError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if crate::error_key::parse(&self.message).is_some() {
            f.write_str(&self.message)
        } else {
            write!(f, "agent: {}: {}", self.code, self.message)
        }
    }
}

/// Where the loop sends its model requests.
pub trait ModelEndpoint: Send + Sync {
    /// Opens one streaming `chat/completions` request with `body` (already `stream: true`).
    /// `round` is 1-based.
    fn open<'a>(
        &'a self,
        round: u32,
        body: Value,
    ) -> BoxFuture<'a, Result<OpenedRound, AgentError>>;
}

/// A plain OpenAI-compatible server (`http://127.0.0.1:8080/v1`).
pub struct HttpEndpoint {
    pub base_url: String,
    pub api_key: Option<String>,
    http: reqwest::Client,
}

impl HttpEndpoint {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key,
            http: reqwest::Client::new(),
        }
    }
}

impl ModelEndpoint for HttpEndpoint {
    fn open<'a>(
        &'a self,
        _round: u32,
        body: Value,
    ) -> BoxFuture<'a, Result<OpenedRound, AgentError>> {
        Box::pin(async move {
            let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
            let mut rb = self.http.post(&url).json(&body);
            if let Some(k) = &self.api_key {
                rb = rb.bearer_auth(k);
            }
            let reply = rb
                .send()
                .await
                .map_err(|e| AgentError::new("request", crate::http_error::transport(&e, &url)))?;
            let status = reply.status();
            if !status.is_success() {
                let text = reply.text().await.unwrap_or_default();
                return Err(AgentError::new("http_status", format!("{status}: {text}")));
            }
            Ok(OpenedRound {
                stream: reply
                    .bytes_stream()
                    .map(move |r| r.map_err(|e| crate::http_error::transport(&e, &url)))
                    .boxed(),
                hold: None,
            })
        })
    }
}

/// Which Harness tools the loop offers (and runs).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ToolSelection {
    /// Every registered tool.
    #[default]
    All,
    /// Only these (unknown names are ignored).
    Only(Vec<String>),
}

/// One streaming agent run.
pub struct StreamRequest {
    /// `model` of the chat-completions body.
    pub model: String,
    /// The conversation so far, OpenAI messages.
    pub messages: Vec<Value>,
    /// Other chat-completions fields passed through (`temperature`, `max_tokens`, `top_p`...).
    /// `tool_choice` counts for the **first round only**: asking for a tool on every round would
    /// never let the model answer. `stream` and `stream_options` are the loop's own.
    pub params: Map<String, Value>,
    /// OpenAI tool objects the client owns: offered to the model, never run by the loop.
    pub client_tools: Vec<Value>,
    pub tools: ToolSelection,
    pub origin: ToolOrigin,
    pub workdir: Option<PathBuf>,
    /// Per-call time limit; `None` = `limits.timeout()`.
    pub call_timeout: Option<Duration>,
    /// Policy to apply; `None` = the saved settings.
    pub settings: Option<ToolSettings>,
    pub limits: ToolLimits,
    /// The longest pause inside a model stream; `None` = no limit.
    pub stream_idle: Option<Duration>,
    pub cancel: CancelToken,
}

impl StreamRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Value>) -> Self {
        Self {
            model: model.into(),
            messages,
            params: Map::new(),
            client_tools: Vec::new(),
            tools: ToolSelection::All,
            origin: ToolOrigin::new("agent"),
            workdir: None,
            call_timeout: None,
            settings: None,
            limits: ToolLimits::default(),
            stream_idle: None,
            cancel: CancelToken::new(),
        }
    }
}

/// A tool call the model wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCall {
    pub id: String,
    pub name: String,
    /// The arguments text exactly as the model wrote it.
    pub arguments: String,
}

/// What the sink hears, in order.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// A model request starts (1-based).
    Round(u32),
    /// Answer text.
    Text(String),
    /// Reasoning text (`reasoning_content`).
    Reasoning(String),
    /// A call of a tool the loop owns is about to run.
    ToolStart {
        round: u32,
        id: String,
        tool: String,
        arguments: String,
    },
    /// ...and has run (also when it failed: the result says so).
    ToolDone(AgentCall),
    /// The round called tools of the client: the loop stops and these are for the client.
    ClientCalls(Vec<ModelCall>),
}

/// Lets a caller rewrite the conversation before each model request after the first (the place
/// for compaction between rounds).
pub trait RoundHook: Send {
    /// `round` is the number of the request about to be made (2, 3, ...).
    fn before_next_round<'a>(
        &'a mut self,
        round: u32,
        messages: &'a mut Vec<Value>,
    ) -> BoxFuture<'a, ()>;
}

/// What a streaming run ended with.
#[derive(Debug, Clone)]
pub struct StreamOutcome {
    /// The whole conversation, the assistant and `tool` messages of every round included.
    pub messages: Vec<Value>,
    /// The last assistant text (empty when the run stopped on the round limit).
    pub text: String,
    /// Model requests made.
    pub rounds: u32,
    /// The calls the loop ran.
    pub calls: Vec<AgentCall>,
    /// The calls of the final round that belong to the client ([`StopReason::ClientToolCalls`]).
    pub client_calls: Vec<ModelCall>,
    /// Calls of Harness tools that shared the final round with client calls: not run (their
    /// results could not be shown to the client), so the model must ask again.
    pub dropped_calls: u32,
    pub stop: StopReason,
    /// `finish_reason` of the last response (`stop`, `length`, `tool_calls`...).
    pub finish_reason: Option<String>,
    /// Token counts summed over every round (the last round's timings).
    pub usage: Usage,
}

// ---------------------------------------------------------------------------------------------
// Reading the stream
// ---------------------------------------------------------------------------------------------

/// Splits an SSE byte stream into the payloads of its `data:` lines.
#[derive(Debug, Default)]
pub struct SseData {
    buf: Vec<u8>,
}

impl SseData {
    /// Payloads completed by `chunk` (`[DONE]` included, as text).
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(nl) = self.buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=nl).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\r', '\n']);
            if let Some(data) = line.strip_prefix("data:") {
                out.push(data.strip_prefix(' ').unwrap_or(data).to_string());
            }
        }
        out
    }

    /// A last line without its newline.
    pub fn finish(&mut self) -> Vec<String> {
        if !self.buf.is_empty() && !self.buf.ends_with(b"\n") {
            self.buf.push(b'\n');
        }
        self.push(b"")
    }
}

#[derive(Debug, Default, Clone)]
struct PartialCall {
    id: String,
    name: String,
    args: String,
}

/// Accumulates streamed `delta.tool_calls` fragments. Each fragment carries an `index`; the
/// first one of a call usually has `id` and `function.name`, later ones append to
/// `function.arguments`. Fragments without an index continue the latest call unless they start a
/// new one (a new `id`). Port of `ToolCallAccumulator` in `src/lib/mcp-client.ts`.
#[derive(Debug, Default)]
pub struct ToolCallAccumulator {
    calls: Vec<PartialCall>,
    by_index: HashMap<i64, usize>,
}

impl ToolCallAccumulator {
    pub fn push(&mut self, deltas: &Value) {
        let Some(list) = deltas.as_array() else {
            return;
        };
        for d in list.iter().filter(|d| d.is_object()) {
            let id = d["id"].as_str().unwrap_or("");
            let fname = d["function"]["name"].as_str().unwrap_or("");
            let pos = if let Some(index) = d["index"].as_i64() {
                *self.by_index.entry(index).or_insert_with(|| {
                    self.calls.push(PartialCall::default());
                    self.calls.len() - 1
                })
            } else {
                let last = self.calls.len().checked_sub(1);
                let prev = last.map(|l| &self.calls[l]);
                let new_id = !id.is_empty() && Some(id) != prev.map(|p| p.id.as_str());
                let starts_new = match prev {
                    None => true,
                    Some(p) => {
                        new_id && (!p.id.is_empty() || (!fname.is_empty() && !p.name.is_empty()))
                    }
                };
                if starts_new {
                    self.calls.push(PartialCall::default());
                    self.calls.len() - 1
                } else {
                    last.unwrap_or(0)
                }
            };
            let c = &mut self.calls[pos];
            if !id.is_empty() && c.id.is_empty() {
                c.id = id.to_string();
            }
            if !fname.is_empty() {
                // Some servers repeat the full name on every chunk; others stream it in pieces.
                if c.name.is_empty() || fname.starts_with(&c.name) {
                    c.name = fname.to_string();
                } else {
                    c.name.push_str(fname);
                }
            }
            match &d["function"]["arguments"] {
                Value::String(s) => c.args.push_str(s),
                Value::Object(_) | Value::Array(_) => {
                    c.args = d["function"]["arguments"].to_string()
                }
                _ => {}
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.calls.is_empty()
    }

    /// The finished calls (ids generated where the server sent none); calls without a name are
    /// dropped.
    pub fn finish(self, round: u32) -> Vec<ModelCall> {
        self.calls
            .into_iter()
            .filter(|c| !c.name.is_empty())
            .enumerate()
            .map(|(n, c)| ModelCall {
                id: if c.id.is_empty() {
                    format!("call_{round}_{n}")
                } else {
                    c.id
                },
                name: c.name,
                arguments: c.args,
            })
            .collect()
    }
}

/// What one streamed response said.
#[derive(Debug, Default)]
struct Round {
    text: String,
    calls: Vec<ModelCall>,
    usage: Usage,
    cancelled: bool,
}

fn add_usage(total: &mut Usage, round: &Usage) {
    let sum = |a: Option<u64>, b: Option<u64>| match (a, b) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
    };
    total.prompt_tokens = sum(total.prompt_tokens, round.prompt_tokens);
    total.completion_tokens = sum(total.completion_tokens, round.completion_tokens);
    total.reasoning_tokens = sum(total.reasoning_tokens, round.reasoning_tokens);
    total.cached_tokens = sum(total.cached_tokens, round.cached_tokens);
    total.prompt_ms = sum_f(total.prompt_ms, round.prompt_ms);
    total.predicted_ms = sum_f(total.predicted_ms, round.predicted_ms);
    total.prompt_per_second = round.prompt_per_second.or(total.prompt_per_second);
    total.predicted_per_second = round.predicted_per_second.or(total.predicted_per_second);
    total.draft_n = sum(total.draft_n, round.draft_n);
    total.draft_n_accepted = sum(total.draft_n_accepted, round.draft_n_accepted);
    if round.finish_reason.is_some() {
        total.finish_reason = round.finish_reason.clone();
    }
}

fn sum_f(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
    }
}

/// Reads one response to its end, feeding the sink.
async fn read_round(
    opened: OpenedRound,
    round: u32,
    idle: Option<Duration>,
    cancel: &CancelToken,
    sink: &mut (dyn FnMut(AgentEvent) + Send),
    names: &[String],
) -> Result<Round, AgentError> {
    let OpenedRound {
        mut stream,
        hold: _hold,
    } = opened;
    let mut out = Round::default();
    let mut sse = SseData::default();
    let mut acc = ToolCallAccumulator::default();
    let mut content_buffer = super::text_call::TextCallStreamBuffer::default();
    let mut visible_text = String::new();
    let mut recovered_text = false;
    let mut saw_data = false;
    let mut done = false;
    loop {
        let next = tokio::select! {
            r = async {
                match idle {
                    Some(d) => tokio::time::timeout(d, stream.next()).await.map_err(|_| ()),
                    None => Ok(stream.next().await),
                }
            } => r,
            _ = cancel.cancelled() => {
                out.cancelled = true;
                break;
            }
        };
        let (payloads, ended) = match next {
            Err(()) => {
                return Err(AgentError::new(
                    "idle_timeout",
                    "the model stream stalled (no data within the idle limit)",
                ))
            }
            Ok(None) => (sse.finish(), true),
            Ok(Some(Err(e))) => return Err(AgentError::new("upstream", e)),
            Ok(Some(Ok(chunk))) => (sse.push(&chunk), false),
        };
        for data in &payloads {
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" {
                done = true;
                continue;
            }
            let v: Value = serde_json::from_str(data).map_err(|e| {
                AgentError::new(
                    "bad_stream",
                    format!("a stream event is not JSON ({e}): {data}"),
                )
            })?;
            if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
                let message = err["message"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| err.to_string());
                return Err(AgentError::new("upstream", message));
            }
            saw_data = true;
            out.usage.absorb(&v);
            let delta = &v["choices"][0]["delta"];
            if let Some(t) = delta["content"].as_str().filter(|t| !t.is_empty()) {
                out.text.push_str(t);
                // Offered-tool rounds may contain a textual call: do not leak its JSON to
                // append-only wire sinks before the shared parser can classify the full turn.
                let visible = if names.is_empty() {
                    t.to_string()
                } else {
                    content_buffer.push(t)
                };
                visible_text.push_str(&visible);
                if !visible.is_empty() {
                    sink(AgentEvent::Text(visible));
                }
            }
            let reasoning = delta["reasoning_content"]
                .as_str()
                .or_else(|| delta["reasoning"].as_str())
                .filter(|t| !t.is_empty());
            if let Some(t) = reasoning {
                sink(AgentEvent::Reasoning(t.to_string()));
            }
            if delta["tool_calls"].is_array() {
                acc.push(&delta["tool_calls"]);
            }
        }
        if ended || done {
            break;
        }
    }
    if !out.cancelled && !saw_data {
        return Err(AgentError::new(
            "bad_stream",
            "the model answered with an empty stream",
        ));
    }
    out.calls = acc.finish(round);
    if !names.is_empty() {
        if !out.cancelled
            && out.calls.is_empty()
            && !matches!(
                out.usage.finish_reason.as_deref(),
                Some("length" | "content_filter")
            )
        {
            let recovered = super::text_call::recover_text_calls(&out.text, names);
            if !recovered.tool_calls.is_empty() {
                recovered_text = true;
                out.text = recovered.content;
                out.calls = recovered
                    .tool_calls
                    .into_iter()
                    .map(|c| ModelCall {
                        id: c.id,
                        name: c.name,
                        arguments: c.arguments,
                    })
                    .collect();
                out.usage.finish_reason = Some("tool_calls".into());
            }
        }
        // Recovery may trim the already emitted introduction; never emit it twice.
        let tail = if !recovered_text {
            content_buffer.drain()
        } else {
            out.text
                .strip_prefix(visible_text.trim_start())
                .or_else(|| out.text.strip_prefix(visible_text.trim()))
                .unwrap_or("")
                .to_string()
        };
        if !tail.is_empty() {
            sink(AgentEvent::Text(tail));
        }
    }
    Ok(out)
}

fn assistant_message(text: &str, calls: &[ModelCall]) -> Value {
    let mut m = json!({"role": "assistant", "content": text});
    if !calls.is_empty() {
        m["tool_calls"] = Value::Array(
            calls
                .iter()
                .map(|c| {
                    json!({
                        "id": c.id,
                        "type": "function",
                        "function": {"name": c.name, "arguments": c.arguments},
                    })
                })
                .collect(),
        );
    }
    m
}

fn request_body(req: &StreamRequest, messages: &[Value], tools: &[Value], round: u32) -> Value {
    let mut body = Map::new();
    for (k, v) in &req.params {
        if k == "tool_choice" && round > 1 {
            continue;
        }
        body.insert(k.clone(), v.clone());
    }
    body.insert("model".into(), json!(req.model));
    body.insert("messages".into(), Value::Array(messages.to_vec()));
    body.insert("stream".into(), json!(true));
    body.insert("stream_options".into(), json!({"include_usage": true}));
    if tools.is_empty() {
        body.remove("tool_choice");
    } else {
        body.insert("tools".into(), Value::Array(tools.to_vec()));
    }
    Value::Object(body)
}

/// Runs the loop. `Err` is a failure of the model side (connection, HTTP status, a stream that
/// is not chat completions, an idle timeout); tool failures are results the model reads.
pub async fn run_agent_stream(
    service: &ToolService,
    endpoint: &dyn ModelEndpoint,
    req: StreamRequest,
    approver: &dyn Approver,
    sink: &mut (dyn FnMut(AgentEvent) + Send),
    mut hook: Option<&mut dyn RoundHook>,
) -> Result<StreamOutcome, AgentError> {
    let settings = req
        .settings
        .clone()
        .unwrap_or_else(|| crate::config::AppSettings::load().tools);
    // A client tool of the same name wins: the loop never shadows what the client asked for.
    let client_names: HashSet<String> = req
        .client_tools
        .iter()
        .filter_map(|t| t["function"]["name"].as_str())
        .map(str::to_string)
        .collect();
    let defs: Vec<ToolDef> = service
        .registry()
        .list()
        .into_iter()
        .filter(|d| match &req.tools {
            ToolSelection::All => true,
            ToolSelection::Only(names) => names.contains(&d.name),
        })
        .filter(|d| !client_names.contains(&d.name))
        .collect();
    let owned: HashSet<String> = defs.iter().map(|d| d.name.clone()).collect();
    let mut tool_json = req.client_tools.clone();
    tool_json.extend(defs.iter().map(ToolDef::to_openai));
    let offered_names: Vec<String> = tool_json
        .iter()
        .filter_map(|t| t["function"]["name"].as_str().map(str::to_string))
        .collect();
    let call_timeout = req.call_timeout.unwrap_or_else(|| req.limits.timeout());

    let mut messages = req.messages.clone();
    if !tool_json.is_empty() {
        crate::injection::with_notice(&mut messages);
    }
    let mut calls: Vec<AgentCall> = Vec::new();
    let mut usage = Usage::default();
    let mut rounds = 0u32;
    let finish = |messages: Vec<Value>,
                  rounds: u32,
                  calls: Vec<AgentCall>,
                  usage: Usage,
                  stop: StopReason,
                  text: String| StreamOutcome {
        messages,
        text,
        rounds,
        calls,
        client_calls: Vec::new(),
        dropped_calls: 0,
        stop,
        finish_reason: usage.finish_reason.clone(),
        usage,
    };
    loop {
        if req.cancel.is_cancelled() {
            return Ok(finish(
                messages,
                rounds,
                calls,
                usage,
                StopReason::Cancelled,
                String::new(),
            ));
        }
        if rounds > 0 {
            if let Some(h) = hook.as_deref_mut() {
                h.before_next_round(rounds + 1, &mut messages).await;
            }
        }
        rounds += 1;
        sink(AgentEvent::Round(rounds));
        let body = request_body(&req, &messages, &tool_json, rounds);
        let opened = tokio::select! {
            r = endpoint.open(rounds, body) => r?,
            _ = req.cancel.cancelled() => {
                return Ok(finish(messages, rounds, calls, usage, StopReason::Cancelled, String::new()));
            }
        };
        let round = read_round(
            opened,
            rounds,
            req.stream_idle,
            &req.cancel,
            sink,
            &offered_names,
        )
        .await?;
        add_usage(&mut usage, &round.usage);
        if round.cancelled {
            return Ok(finish(
                messages,
                rounds,
                calls,
                usage,
                StopReason::Cancelled,
                round.text,
            ));
        }
        let (mine, theirs): (Vec<ModelCall>, Vec<ModelCall>) = round
            .calls
            .into_iter()
            .partition(|c| owned.contains(&c.name));
        if !theirs.is_empty() {
            messages.push(assistant_message(&round.text, &theirs));
            sink(AgentEvent::ClientCalls(theirs.clone()));
            let mut out = finish(
                messages,
                rounds,
                calls,
                usage,
                StopReason::ClientToolCalls,
                round.text,
            );
            out.client_calls = theirs;
            out.dropped_calls = mine.len() as u32;
            return Ok(out);
        }
        messages.push(assistant_message(&round.text, &mine));
        if mine.is_empty() {
            return Ok(finish(
                messages,
                rounds,
                calls,
                usage,
                StopReason::Answered,
                round.text,
            ));
        }
        let corpus = crate::injection::tool_corpus(&messages);
        let ctx = CallContext {
            service,
            settings: &settings,
            origin: &req.origin,
            workdir: req.workdir.as_deref(),
            timeout: Some(call_timeout),
            cancel: &req.cancel,
            untrusted_corpus: &corpus,
            hooks: None,
        };
        for c in mine {
            if req.cancel.is_cancelled() {
                return Ok(finish(
                    messages,
                    rounds,
                    calls,
                    usage,
                    StopReason::Cancelled,
                    String::new(),
                ));
            }
            sink(AgentEvent::ToolStart {
                round: rounds,
                id: c.id.clone(),
                tool: c.name.clone(),
                arguments: c.arguments.clone(),
            });
            let call = ctx
                .run_call(rounds, c.id.clone(), c.name, c.arguments, approver)
                .await;
            messages.push(json!({"role": "tool", "tool_call_id": c.id, "content": call.sent}));
            sink(AgentEvent::ToolDone(call.clone()));
            calls.push(call);
        }
        if req.cancel.is_cancelled() {
            return Ok(finish(
                messages,
                rounds,
                calls,
                usage,
                StopReason::Cancelled,
                String::new(),
            ));
        }
        if rounds >= req.limits.max_rounds {
            return Ok(finish(
                messages,
                rounds,
                calls,
                usage,
                StopReason::RoundLimit,
                String::new(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real llama-server (b11342, Qwen3-4B-Instruct-2507) streams, captured with
    /// `stream_options.include_usage`.
    const TOOLCALL: &str =
        include_str!("../../tests/data/b11342-qwen3-4b-instruct-2507.chat-stream-toolcall.sse");
    const TEXT: &str =
        include_str!("../../tests/data/b11342-qwen3-4b-instruct-2507.chat-stream-text.sse");

    /// The stream as a model response, cut into chunks of `size` bytes (events end up split
    /// anywhere, like on a real connection).
    fn opened(sse: &'static str, size: usize) -> OpenedRound {
        let chunks: Vec<Result<Bytes, String>> = sse
            .as_bytes()
            .chunks(size)
            .map(|c| Ok(Bytes::copy_from_slice(c)))
            .collect();
        OpenedRound {
            stream: futures_util::stream::iter(chunks).boxed(),
            hold: None,
        }
    }

    async fn read(sse: &'static str, size: usize) -> (Round, Vec<AgentEvent>) {
        let mut events = Vec::new();
        let mut sink = |e: AgentEvent| events.push(e);
        let r = read_round(
            opened(sse, size),
            1,
            None,
            &CancelToken::new(),
            &mut sink,
            &[],
        )
        .await
        .unwrap();
        (r, events)
    }

    #[tokio::test]
    async fn a_real_tool_call_stream_is_assembled_whatever_the_chunking() {
        for size in [1, 7, 64, 1000, 100_000] {
            let (r, events) = read(TOOLCALL, size).await;
            assert_eq!(r.calls.len(), 1, "chunk size {size}");
            assert_eq!(r.calls[0].name, "calculator");
            assert!(!r.calls[0].id.is_empty());
            let args: Value = serde_json::from_str(&r.calls[0].arguments).unwrap();
            assert_eq!(args["expression"], "1234 + 4321", "chunk size {size}");
            assert_eq!(r.usage.finish_reason.as_deref(), Some("tool_calls"));
            assert_eq!(r.usage.prompt_tokens, Some(211));
            assert_eq!(r.usage.completion_tokens, Some(28));
            assert!(r.text.is_empty());
            assert!(events.iter().all(|e| !matches!(e, AgentEvent::Text(_))));
        }
    }

    #[tokio::test]
    async fn real_text_deltas_reach_the_sink_as_they_come() {
        let (r, events) = read(TEXT, 13).await;
        assert_eq!(r.text, "Merhaba!");
        assert!(r.calls.is_empty());
        let deltas: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(deltas.concat(), "Merhaba!");
        assert!(deltas.len() >= 3, "{deltas:?}");
        assert_eq!(r.usage.finish_reason.as_deref(), Some("stop"));
    }

    #[tokio::test]
    async fn offered_tools_do_not_delay_plain_text_or_duplicate_emitted_prefixes() {
        let mut events = Vec::new();
        let r = read_round(
            opened(TEXT, 13),
            1,
            None,
            &CancelToken::new(),
            &mut |e| events.push(e),
            &["web_search".into()],
        )
        .await
        .unwrap();
        let deltas: Vec<_> = events
            .iter()
            .filter_map(|e| {
                if let AgentEvent::Text(t) = e {
                    Some(t.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(deltas.concat(), r.text);
        assert!(deltas.len() >= 3);
        for (content, expected, count) in [
            (
                "Önce. <tools>{\"name\":\"web_search\",\"arguments\":{}}</tools> Sonra.",
                "Önce.  Sonra.",
                1,
            ),
            (
                "Önce. ```xml\n{\"name\":\"web_search\",\"arguments\":{}}\n``` açıklama",
                "Önce. ```xml\n{\"name\":\"web_search\",\"arguments\":{}}\n``` açıklama",
                0,
            ),
            ("Önce. <tools>broken", "Önce. <tools>broken", 0),
        ] {
            let sse: String = content
                .chars()
                .map(|c| {
                    format!(
                        "data: {}\n\n",
                        json!({"choices":[{"delta":{"content":c.to_string()}}]})
                    )
                })
                .collect::<String>()
                + "data: [DONE]\n\n";
            let opened = OpenedRound {
                stream: futures_util::stream::iter(vec![Ok(Bytes::from(sse))]).boxed(),
                hold: None,
            };
            let mut events = Vec::new();
            let r = read_round(
                opened,
                1,
                None,
                &CancelToken::new(),
                &mut |e| events.push(e),
                &["web_search".into()],
            )
            .await
            .unwrap();
            assert_eq!(r.calls.len(), count);
            assert_eq!(
                events
                    .iter()
                    .filter_map(|e| if let AgentEvent::Text(t) = e {
                        Some(t.as_str())
                    } else {
                        None
                    })
                    .collect::<String>(),
                expected
            );
        }
    }

    #[tokio::test]
    async fn recorded_coder_stream_recovers_without_leaking_json_to_the_sink() {
        let fixture =
            include_str!("../../tests/data/tooldiag/b11342-qwen2.5-coder-14b.chat-stream.resp.sse");
        for size in [1, 17, 100_000] {
            let mut events = Vec::new();
            let r = read_round(
                opened(fixture, size),
                1,
                None,
                &CancelToken::new(),
                &mut |e| events.push(e),
                &["get_weather".into()],
            )
            .await
            .unwrap();
            assert_eq!(r.calls.len(), 1, "{r:?}");
            assert_eq!(r.calls[0].name, "get_weather");
            assert!(r.text.is_empty());
            assert!(events.iter().all(|e| !matches!(e, AgentEvent::Text(_))));
            let m = assistant_message(&r.text, &r.calls);
            assert_eq!(m["tool_calls"][0]["id"], r.calls[0].id);
        }
    }

    struct TextCallEndpoint;

    impl ModelEndpoint for TextCallEndpoint {
        fn open<'a>(
            &'a self,
            round: u32,
            body: Value,
        ) -> BoxFuture<'a, Result<OpenedRound, AgentError>> {
            Box::pin(async move {
                let content = if round == 1 {
                    "Calculating. <tool_call>{\"name\":\"calculator\",\"arguments\":{\"expression\":\"2+3\"}}</tool_call>"
                } else {
                    let messages = body["messages"].as_array().unwrap();
                    let assistant = &messages[messages.len() - 2];
                    let tool = &messages[messages.len() - 1];
                    assert_eq!(assistant["content"], "Calculating.");
                    assert_eq!(assistant["tool_calls"][0]["function"]["name"], "calculator");
                    assert_eq!(tool["role"], "tool");
                    assert_eq!(tool["tool_call_id"], assistant["tool_calls"][0]["id"]);
                    assert!(tool["content"].as_str().unwrap().contains('5'));
                    "The answer is 5."
                };
                let event = format!(
                    "data: {}\n\ndata: [DONE]\n\n",
                    json!({"choices":[{"delta":{"content":content}, "finish_reason":"stop"}]})
                );
                Ok(OpenedRound {
                    stream: futures_util::stream::iter(vec![Ok(Bytes::from(event))]).boxed(),
                    hold: None,
                })
            })
        }
    }

    #[tokio::test]
    async fn recovered_call_runs_through_the_engine_and_next_round_history() {
        let service = ToolService::new(
            super::super::ToolRegistry::with_builtin(),
            super::super::LogSink::None,
        );
        let mut req = StreamRequest::new(
            "fixture",
            vec![json!({"role":"user", "content":"Calculate 2+3"})],
        );
        req.tools = ToolSelection::Only(vec!["calculator".into()]);
        req.settings = Some(ToolSettings::default());
        let mut events = Vec::new();
        let out = run_agent_stream(
            &service,
            &TextCallEndpoint,
            req,
            &super::super::agent::DenyAll,
            &mut |e| events.push(e),
            None,
        )
        .await
        .unwrap();
        assert_eq!(out.stop, StopReason::Answered);
        assert_eq!(out.rounds, 2);
        assert_eq!(out.calls.len(), 1);
        assert_eq!(out.text, "The answer is 5.");
        assert!(events
            .iter()
            .all(|e| !matches!(e, AgentEvent::Text(t) if t.contains("<tool_call>"))));
    }

    #[tokio::test]
    async fn recovered_client_calls_are_returned_without_executing_them() {
        let service = ToolService::new(
            super::super::ToolRegistry::with_builtin(),
            super::super::LogSink::None,
        );
        let mut req = StreamRequest::new("fixture", Vec::new());
        req.tools = ToolSelection::Only(Vec::new());
        req.client_tools = vec![json!({"type":"function", "function":{"name":"calculator"}})];
        req.settings = Some(ToolSettings::default());
        let out = run_agent_stream(
            &service,
            &TextCallEndpoint,
            req,
            &super::super::agent::DenyAll,
            &mut |_| {},
            None,
        )
        .await
        .unwrap();
        assert_eq!(out.stop, StopReason::ClientToolCalls);
        assert!(out.calls.is_empty());
        assert_eq!(out.client_calls.len(), 1);
        assert_eq!(out.text, "Calculating.");
    }

    #[tokio::test]
    async fn recovered_calls_still_obey_policy_and_round_limits() {
        let service = ToolService::new(
            super::super::ToolRegistry::with_builtin(),
            super::super::LogSink::None,
        );
        let mut req = StreamRequest::new("fixture", Vec::new());
        req.tools = ToolSelection::Only(vec!["calculator".into()]);
        req.settings = Some(ToolSettings {
            policy: super::super::ToolPolicy::Off,
            ..ToolSettings::default()
        });
        req.limits.max_rounds = 1;
        let out = run_agent_stream(
            &service,
            &TextCallEndpoint,
            req,
            &super::super::agent::DenyAll,
            &mut |_| {},
            None,
        )
        .await
        .unwrap();
        assert_eq!(out.stop, StopReason::RoundLimit);
        assert_eq!(out.calls.len(), 1);
        assert!(out.calls[0].result.as_ref().unwrap().is_error());
        assert!(crate::injection::is_wrapped(&out.calls[0].sent));
        assert!(out.calls[0].sent.contains("Error:"));
    }

    #[tokio::test]
    async fn truncated_text_call_does_not_execute_even_when_json_is_complete() {
        let text = "<tool_call>{\"name\":\"calculator\",\"arguments\":{\"expression\":\"2+3\"}}</tool_call>";
        for finish in ["length", "content_filter"] {
            let event = format!(
                "data: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":{"content":text},"finish_reason":finish}]})
            );
            let opened = OpenedRound {
                stream: futures_util::stream::iter(vec![Ok(Bytes::from(event))]).boxed(),
                hold: None,
            };
            let r = read_round(
                opened,
                1,
                None,
                &CancelToken::new(),
                &mut |_| {},
                &["calculator".into()],
            )
            .await
            .unwrap();
            assert!(r.calls.is_empty());
            assert_eq!(r.text, text);
        }
    }

    #[tokio::test]
    async fn a_stalled_or_empty_or_broken_stream_is_an_error() {
        // Nothing but the end: not chat completions.
        let empty = OpenedRound {
            stream: futures_util::stream::iter(Vec::<Result<Bytes, String>>::new()).boxed(),
            hold: None,
        };
        let mut sink = |_: AgentEvent| {};
        let e = read_round(empty, 1, None, &CancelToken::new(), &mut sink, &[])
            .await
            .unwrap_err();
        assert_eq!(e.code, "bad_stream");
        // A transport error in the middle.
        let broken = OpenedRound {
            stream: futures_util::stream::iter(vec![
                Ok(Bytes::from_static(
                    b"data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n",
                )),
                Err("connection reset".to_string()),
            ])
            .boxed(),
            hold: None,
        };
        let e = read_round(broken, 1, None, &CancelToken::new(), &mut sink, &[])
            .await
            .unwrap_err();
        assert_eq!(
            (e.code.as_str(), e.message.as_str()),
            ("upstream", "connection reset")
        );
        // An error object inside the stream.
        let err_event = OpenedRound {
            stream: futures_util::stream::iter(vec![Ok(Bytes::from_static(
                b"data: {\"error\":{\"message\":\"context length exceeded\"}}\n\n",
            ))])
            .boxed(),
            hold: None,
        };
        let e = read_round(err_event, 1, None, &CancelToken::new(), &mut sink, &[])
            .await
            .unwrap_err();
        assert_eq!(e.message, "context length exceeded");
        // No byte within the idle limit.
        let silent = OpenedRound {
            stream: futures_util::stream::pending().boxed(),
            hold: None,
        };
        let e = read_round(
            silent,
            1,
            Some(Duration::from_millis(80)),
            &CancelToken::new(),
            &mut sink,
            &[],
        )
        .await
        .unwrap_err();
        assert_eq!(e.code, "idle_timeout");
        // Cancelled while waiting.
        let cancel = CancelToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            c2.cancel();
        });
        let silent = OpenedRound {
            stream: futures_util::stream::pending().boxed(),
            hold: None,
        };
        let r = read_round(silent, 1, None, &cancel, &mut sink, &[])
            .await
            .unwrap();
        assert!(r.cancelled);
    }

    #[test]
    fn fragments_without_an_index_continue_the_latest_call() {
        let mut a = ToolCallAccumulator::default();
        a.push(&json!([{"id": "a1", "function": {"name": "read_", "arguments": "{\"p"}}]));
        a.push(&json!([{"function": {"name": "file", "arguments": "ath\":\"x\"}"}}]));
        a.push(&json!([{"id": "a2", "function": {"name": "grep_search", "arguments": "{}"}}]));
        let calls = a.finish(3);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].arguments, "{\"path\":\"x\"}");
        assert_eq!(calls[1].id, "a2");
        // A call without an id gets one; a nameless fragment never becomes a call.
        let mut a = ToolCallAccumulator::default();
        a.push(&json!([{"index": 0, "function": {"name": "f", "arguments": "{}"}}, {"index": 1}]));
        let calls = a.finish(2);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_2_0");
    }
}
