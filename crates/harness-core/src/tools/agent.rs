//! The seed of the core agent loop: a model that can call tools, driven without a UI.
//!
//! This is deliberately small. It talks OpenAI chat completions **without streaming** and
//! decides nothing itself: the policy, working directory, limits and log all come from
//! [`ToolService`], and the only thing the caller supplies is an [`Approver`] (who answers when
//! the policy says "ask"). It exists so the engine can be exercised end to end with a real model
//! (`tests/tools_e2e.rs`). The streaming variant, which the gateway's server-side tools run,
//! is [`super::agent_stream`]; both share [`CallContext::run_call`] (arguments, policy, approval,
//! run, log). The Playground does **not** use either: its loop streams tokens into the UI and
//! stays in TypeScript (see `docs/en/ARCHITECTURE.md`, "Tool loop").
//!
//! The loop shape is the Playground's: request with the tool definitions -> `tool_calls` ->
//! for each call (policy, approval, run, log) -> `tool` messages -> request again, until the
//! model answers without calls, the round limit ([`ToolLimits::max_rounds`]) is reached or the
//! run is cancelled.

use std::path::PathBuf;
use std::time::Duration;

use futures_util::future::BoxFuture;
use serde_json::{json, Value};

use super::limits::ToolLimits;
use super::policy::{Decision, ToolSettings};
use super::registry::CancelToken;
use super::service::{RunOptions, ToolService};
use super::types::*;

/// Answers "may this call run?" when the policy says [`Decision::Ask`].
pub trait Approver: Send + Sync {
    fn approve<'a>(&'a self, def: &'a ToolDef, args: &'a Value) -> BoxFuture<'a, bool>;
}

/// Approves every call it is asked about.
pub struct ApproveAll;

impl Approver for ApproveAll {
    fn approve<'a>(&'a self, _: &'a ToolDef, _: &'a Value) -> BoxFuture<'a, bool> {
        Box::pin(async { true })
    }
}

/// Refuses every call it is asked about (nobody is there to ask).
pub struct DenyAll;

impl Approver for DenyAll {
    fn approve<'a>(&'a self, _: &'a ToolDef, _: &'a Value) -> BoxFuture<'a, bool> {
        Box::pin(async { false })
    }
}

/// One agent run.
#[derive(Debug, Clone)]
pub struct AgentRequest {
    /// OpenAI-compatible base URL including the version prefix, e.g. `http://127.0.0.1:8080/v1`.
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    /// The conversation so far (system / user / ...), OpenAI messages.
    pub messages: Vec<Value>,
    /// Tools offered to the model; empty = every registered tool.
    pub tools: Vec<String>,
    pub origin: ToolOrigin,
    pub workdir: Option<PathBuf>,
    /// Per-call time limit; `None` = the default.
    pub call_timeout: Option<Duration>,
    /// Policy to apply; `None` = the saved settings.
    pub settings: Option<ToolSettings>,
    pub limits: ToolLimits,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub cancel: CancelToken,
}

impl AgentRequest {
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        messages: Vec<Value>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: None,
            model: model.into(),
            messages,
            tools: Vec::new(),
            origin: ToolOrigin::new("agent"),
            workdir: None,
            call_timeout: None,
            settings: None,
            limits: ToolLimits::default(),
            temperature: Some(0.0),
            max_tokens: None,
            cancel: CancelToken::new(),
        }
    }
}

/// One call the model made and what became of it.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentCall {
    /// 1-based round the call was made in.
    pub round: u32,
    pub id: String,
    pub tool: String,
    /// The arguments text exactly as the model wrote it.
    pub arguments: String,
    /// `None` when the call never reached the engine (arguments that are not a JSON object).
    pub result: Option<ToolResult>,
    /// The text sent back to the model as the `tool` message.
    pub sent: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The model answered without asking for a tool.
    Answered,
    /// The round limit was reached while the model still asked for tools.
    RoundLimit,
    Cancelled,
    /// The model called tools the client owns (streaming loop with `client_tools`): they are
    /// for the client to run.
    ClientToolCalls,
}

#[derive(Debug, Clone)]
pub struct AgentOutcome {
    /// The whole conversation, including the assistant and `tool` messages of every round.
    pub messages: Vec<Value>,
    /// The last assistant text (empty when the run stopped on the round limit).
    pub text: String,
    /// Model requests made.
    pub rounds: u32,
    pub calls: Vec<AgentCall>,
    pub stop: StopReason,
}

fn failed(what: &str, e: impl std::fmt::Display) -> String {
    format!("agent: {what}: {e}")
}

/// Runs the loop. `Err` is a failure of the model request (connection, HTTP status, a reply
/// that is not chat completions); tool failures are results the model reads, not errors.
pub async fn run_agent(
    service: &ToolService,
    req: AgentRequest,
    approver: &dyn Approver,
) -> Result<AgentOutcome, String> {
    let defs: Vec<ToolDef> = service
        .registry()
        .list()
        .into_iter()
        .filter(|d| req.tools.is_empty() || req.tools.contains(&d.name))
        .collect();
    let tool_json: Vec<Value> = defs.iter().map(ToolDef::to_openai).collect();
    let settings = req
        .settings
        .clone()
        .unwrap_or_else(|| crate::config::AppSettings::load().tools);
    let http = reqwest::Client::new();
    let url = format!("{}/chat/completions", req.base_url.trim_end_matches('/'));

    let mut messages = req.messages.clone();
    let mut calls = Vec::new();
    let mut rounds = 0u32;
    loop {
        if req.cancel.is_cancelled() {
            return Ok(outcome(messages, rounds, calls, StopReason::Cancelled));
        }
        rounds += 1;
        let mut body = json!({"model": req.model, "messages": messages, "stream": false});
        if !tool_json.is_empty() {
            body["tools"] = Value::Array(tool_json.clone());
        }
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(m) = req.max_tokens {
            body["max_tokens"] = json!(m);
        }
        let mut rb = http.post(&url).json(&body);
        if let Some(k) = &req.api_key {
            rb = rb.bearer_auth(k);
        }
        let reply = tokio::select! {
            r = rb.send() => r.map_err(|e| crate::http_error::transport(&e, &url))?,
            _ = req.cancel.cancelled() => {
                return Ok(outcome(messages, rounds, calls, StopReason::Cancelled));
            }
        };
        let status = reply.status();
        let text = reply
            .text()
            .await
            .map_err(|e| crate::http_error::transport(&e, &url))?;
        if !status.is_success() {
            return Err(failed("HTTP", format!("{status}: {text}")));
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| failed("reply is not JSON", e))?;
        let mut message = v["choices"][0]["message"].clone();
        if !message.is_object() {
            return Err(failed("reply has no message", &text));
        }
        if !req.cancel.is_cancelled()
            && !matches!(
                v["choices"][0]["finish_reason"].as_str(),
                Some("length" | "content_filter")
            )
        {
            super::text_call::recover_message(
                &mut message,
                &defs.iter().map(|d| d.name.clone()).collect::<Vec<_>>(),
            );
        }
        let content = message["content"].as_str().unwrap_or("").to_string();
        let tool_calls = message["tool_calls"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut assistant = json!({"role": "assistant", "content": content});
        if !tool_calls.is_empty() {
            assistant["tool_calls"] = Value::Array(tool_calls.clone());
        }
        messages.push(assistant);
        if tool_calls.is_empty() {
            let mut out = outcome(messages, rounds, calls, StopReason::Answered);
            out.text = content;
            return Ok(out);
        }

        let ctx = CallContext {
            service,
            settings: &settings,
            origin: &req.origin,
            workdir: req.workdir.as_deref(),
            timeout: req.call_timeout,
            cancel: &req.cancel,
        };
        for (n, tc) in tool_calls.iter().enumerate() {
            let name = tc["function"]["name"].as_str().unwrap_or("").to_string();
            let arguments = match &tc["function"]["arguments"] {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            let id = tc["id"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| format!("call_{rounds}_{n}"));
            let call = ctx
                .run_call(rounds, id.clone(), name, arguments, approver)
                .await;
            messages.push(json!({"role": "tool", "tool_call_id": id, "content": call.sent}));
            calls.push(call);
        }
        if req.cancel.is_cancelled() {
            return Ok(outcome(messages, rounds, calls, StopReason::Cancelled));
        }
        if rounds >= req.limits.max_rounds {
            return Ok(outcome(messages, rounds, calls, StopReason::RoundLimit));
        }
    }
}

/// What one tool call of an agent loop needs besides the call itself.
pub(crate) struct CallContext<'a> {
    pub service: &'a ToolService,
    pub settings: &'a ToolSettings,
    pub origin: &'a ToolOrigin,
    pub workdir: Option<&'a std::path::Path>,
    pub timeout: Option<Duration>,
    pub cancel: &'a CancelToken,
}

/// The arguments text a model wrote, as the JSON object a tool takes (`{}` when empty).
pub(crate) fn parse_arguments(arguments: &str) -> Result<Value, String> {
    if arguments.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str::<Value>(arguments)
        .map_err(|e| e.to_string())
        .and_then(|v| {
            if v.is_object() {
                Ok(v)
            } else {
                Err("arguments must be a JSON object".into())
            }
        })
}

impl CallContext<'_> {
    /// One call of the loop: arguments, policy, approval, run, log. Tool failures are results
    /// the model reads, never errors.
    pub(crate) async fn run_call(
        &self,
        round: u32,
        id: String,
        name: String,
        arguments: String,
        approver: &dyn Approver,
    ) -> AgentCall {
        let (result, sent) = match parse_arguments(&arguments) {
            Err(e) => (None, format!("Error: invalid JSON arguments ({e}).")),
            Ok(args) => {
                let approved = match self.service.decision(&name, self.settings) {
                    Some(Decision::Ask) => match self.service.registry().get(&name) {
                        Some(tool) => approver.approve(tool.def(), &args).await,
                        None => false,
                    },
                    _ => false,
                };
                let inv = ToolInvocation::new(name.clone(), args, self.origin.clone())
                    .with_id(format!("{}-{id}", uuid::Uuid::new_v4().simple()));
                let r = self
                    .service
                    .run(
                        inv,
                        RunOptions {
                            workdir: self.workdir.map(PathBuf::from),
                            approved,
                            cancel: self.cancel.clone(),
                            timeout: self.timeout,
                            settings: Some(self.settings.clone()),
                        },
                    )
                    .await;
                let sent = if r.is_error() {
                    format!("Error: {}", r.content)
                } else {
                    r.content.clone()
                };
                (Some(r), sent)
            }
        };
        AgentCall {
            round,
            id,
            tool: name,
            arguments,
            result,
            sent,
        }
    }
}

fn outcome(
    messages: Vec<Value>,
    rounds: u32,
    calls: Vec<AgentCall>,
    stop: StopReason,
) -> AgentOutcome {
    AgentOutcome {
        messages,
        text: String::new(),
        rounds,
        calls,
        stop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn text_call_is_executed_and_sent_as_native_history_on_the_next_request() {
        // A local fixture endpoint on an ephemeral port; no app/model configuration involved.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for round in 1..=2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let body = loop {
                    let mut chunk = [0; 4096];
                    let n = socket.read(&mut chunk).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&chunk[..n]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                        let length: usize = headers
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .unwrap()
                            .trim()
                            .parse()
                            .unwrap();
                        if bytes.len() >= end + 4 + length {
                            break serde_json::from_slice::<Value>(
                                &bytes[end + 4..end + 4 + length],
                            )
                            .unwrap();
                        }
                    }
                };
                let content = if round == 1 {
                    "<tool_call>{\"name\":\"calculator\",\"parameters\":{\"expression\":\"2+3\"}}</tool_call>"
                } else {
                    let m = body["messages"].as_array().unwrap();
                    assert_eq!(m[1]["role"], "assistant");
                    assert_eq!(m[1]["content"], "");
                    assert_eq!(m[1]["tool_calls"][0]["function"]["name"], "calculator");
                    assert_eq!(m[2]["role"], "tool");
                    assert_eq!(m[2]["tool_call_id"], m[1]["tool_calls"][0]["id"]);
                    assert!(m[2]["content"].as_str().unwrap().contains('5'));
                    "5"
                };
                let reply = json!({"choices":[{"message":{"role":"assistant", "content":content}, "finish_reason":"stop"}]}).to_string();
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let service = ToolService::new(
            super::super::ToolRegistry::with_builtin(),
            super::super::LogSink::None,
        );
        let mut req = AgentRequest::new(
            format!("http://{address}/v1"),
            "fixture",
            vec![json!({"role":"user", "content":"2+3"})],
        );
        req.tools = vec!["calculator".into()];
        req.settings = Some(ToolSettings::default());
        let out = tokio::time::timeout(Duration::from_secs(10), run_agent(&service, req, &DenyAll))
            .await
            .unwrap()
            .unwrap();
        server.await.unwrap();
        assert_eq!(out.stop, StopReason::Answered);
        assert_eq!(out.rounds, 2);
        assert_eq!(out.calls.len(), 1);
        assert_eq!(out.text, "5");
    }
}
