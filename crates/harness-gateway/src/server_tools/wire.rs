//! The loop's events → the client's wire format (Chat Completions, Responses, Messages), as one
//! JSON body or as an SSE stream.
//!
//! One implementation per format serves both: a non-streaming reply feeds the same events through
//! the encoder, drops the bytes and reads the finished object ([`Wire::json`]).
//!
//! What the client sees of the loop:
//!
//! * text and reasoning tokens as they come (rounds' texts follow each other),
//! * the calls of **its own** tools, at the end, like a model's answer that asks for them,
//! * of Harness's server-side tools only the hosted `web_search`, as a `web_search_call` item in
//!   Responses (Codex shows it). Other server tools run unseen; their traces are the tool log
//!   and the request record.

use bytes::Bytes;
use harness_core::gateway::record::Usage;
use harness_core::tools::agent::StopReason;
use harness_core::tools::agent_stream::{AgentError, AgentEvent, ModelCall, StreamOutcome};
use serde_json::{json, Value};

use crate::compat::{map_back, NameMap};

/// How the run ended, as the encoders need it.
#[derive(Debug, Clone)]
pub(crate) struct Finish {
    pub stop: StopReason,
    pub finish_reason: Option<String>,
    pub usage: Usage,
}

impl Finish {
    pub fn of(o: &StreamOutcome) -> Self {
        Self {
            stop: o.stop,
            finish_reason: o.finish_reason.clone(),
            usage: o.usage.clone(),
        }
    }

    fn truncated(&self) -> bool {
        self.stop == StopReason::Answered && self.finish_reason.as_deref() == Some("length")
    }

    fn input(&self) -> u64 {
        self.usage.prompt_tokens.unwrap_or(0)
    }

    fn output(&self) -> u64 {
        self.usage.completion_tokens.unwrap_or(0)
    }
}

/// Identity of the reply.
#[derive(Debug, Clone)]
pub(crate) struct Meta {
    /// Gateway request id (`req_…`); the object ids derive from it.
    pub id: String,
    /// Model alias.
    pub model: String,
    pub created: u64,
    pub include_usage: bool,
    /// Flattened namespace names → (namespace, name), for Responses.
    pub names: NameMap,
}

pub(crate) enum Wire {
    Chat(ChatWire),
    Responses(Box<RespWire>),
    Messages(MsgWire),
}

impl Wire {
    pub fn begin(&mut self) -> Vec<Bytes> {
        match self {
            Wire::Chat(w) => w.begin(),
            Wire::Responses(w) => w.begin(),
            Wire::Messages(w) => w.begin(),
        }
    }

    pub fn event(&mut self, ev: &AgentEvent) -> Vec<Bytes> {
        match self {
            Wire::Chat(w) => w.event(ev),
            Wire::Responses(w) => w.event(ev),
            Wire::Messages(w) => w.event(ev),
        }
    }

    /// The end of a stream (a Responses stream is ended by [`RespWire::end_with`] instead).
    pub fn end(&mut self, fin: &Finish) -> Vec<Bytes> {
        match self {
            Wire::Chat(w) => w.end(fin),
            Wire::Responses(w) => {
                let mut out = w.close_events();
                let r = w.response(fin);
                out.extend(w.end_with(r));
                out
            }
            Wire::Messages(w) => w.end(fin),
        }
    }

    pub fn fail(&mut self, e: &AgentError) -> Vec<Bytes> {
        match self {
            Wire::Chat(w) => w.fail(e),
            Wire::Responses(w) => w.fail(e),
            Wire::Messages(w) => w.fail(e),
        }
    }

    /// The finished non-streaming body, after every event was fed.
    pub fn json(&mut self, fin: &Finish) -> Value {
        match self {
            Wire::Chat(w) => w.json(fin),
            Wire::Responses(w) => w.response(fin),
            Wire::Messages(w) => w.json(fin),
        }
    }
}

fn sse_data(v: &Value) -> Bytes {
    Bytes::from(format!("data: {v}\n\n"))
}

fn sse_event(kind: &str, v: &Value) -> Bytes {
    Bytes::from(format!("event: {kind}\ndata: {v}\n\n"))
}

fn query_of(arguments: &str) -> String {
    serde_json::from_str::<Value>(arguments)
        .ok()
        .and_then(|v| v["query"].as_str().map(str::to_string))
        .unwrap_or_default()
}

fn server_error(e: &AgentError) -> Value {
    json!({"message": e.message, "type": "server_error", "code": e.code})
}

// ---------------------------------------------------------------------------------------------
// Chat Completions
// ---------------------------------------------------------------------------------------------

pub(crate) struct ChatWire {
    meta: Meta,
    content: String,
    reasoning: String,
    calls: Vec<ModelCall>,
    /// A server tool ran since the last text: the next text starts a new paragraph.
    gap: bool,
}

impl ChatWire {
    pub fn new(meta: Meta) -> Self {
        Self {
            meta,
            content: String::new(),
            reasoning: String::new(),
            calls: Vec::new(),
            gap: false,
        }
    }

    fn chunk(&self, delta: Value, finish: Option<&str>) -> Bytes {
        sse_data(&json!({
            "id": format!("chatcmpl-{}", self.meta.id),
            "object": "chat.completion.chunk",
            "created": self.meta.created,
            "model": self.meta.model,
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
        }))
    }

    fn begin(&mut self) -> Vec<Bytes> {
        vec![self.chunk(json!({"role": "assistant", "content": ""}), None)]
    }

    fn event(&mut self, ev: &AgentEvent) -> Vec<Bytes> {
        match ev {
            AgentEvent::Text(t) => {
                let mut t = t.clone();
                if self.gap && !self.content.is_empty() {
                    t.insert_str(0, "\n\n");
                }
                self.gap = false;
                self.content.push_str(&t);
                vec![self.chunk(json!({"content": t}), None)]
            }
            AgentEvent::Reasoning(t) => {
                self.reasoning.push_str(t);
                vec![self.chunk(json!({"reasoning_content": t}), None)]
            }
            AgentEvent::ToolDone(_) => {
                self.gap = true;
                Vec::new()
            }
            AgentEvent::ClientCalls(calls) => {
                self.calls = calls.clone();
                let list: Vec<Value> = calls
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        json!({"index": i, "id": c.id, "type": "function",
                               "function": {"name": c.name, "arguments": c.arguments}})
                    })
                    .collect();
                vec![self.chunk(json!({"tool_calls": list}), None)]
            }
            _ => Vec::new(),
        }
    }

    fn finish_reason(fin: &Finish) -> &'static str {
        match fin.stop {
            StopReason::ClientToolCalls => "tool_calls",
            _ if fin.truncated() => "length",
            _ => "stop",
        }
    }

    fn usage(fin: &Finish) -> Value {
        let mut u = json!({
            "prompt_tokens": fin.input(),
            "completion_tokens": fin.output(),
            "total_tokens": fin.input() + fin.output(),
        });
        if let Some(c) = fin.usage.cached_tokens {
            u["prompt_tokens_details"] = json!({"cached_tokens": c});
        }
        u
    }

    fn end(&mut self, fin: &Finish) -> Vec<Bytes> {
        let mut out = vec![self.chunk(json!({}), Some(Self::finish_reason(fin)))];
        if self.meta.include_usage {
            out.push(sse_data(&json!({
                "id": format!("chatcmpl-{}", self.meta.id),
                "object": "chat.completion.chunk",
                "created": self.meta.created,
                "model": self.meta.model,
                "choices": [],
                "usage": Self::usage(fin),
            })));
        }
        out.push(Bytes::from_static(b"data: [DONE]\n\n"));
        out
    }

    fn fail(&mut self, e: &AgentError) -> Vec<Bytes> {
        vec![
            sse_data(&json!({"error": server_error(e)})),
            Bytes::from_static(b"data: [DONE]\n\n"),
        ]
    }

    fn json(&mut self, fin: &Finish) -> Value {
        let mut message = json!({"role": "assistant"});
        message["content"] = if self.content.is_empty() && !self.calls.is_empty() {
            Value::Null
        } else {
            json!(self.content)
        };
        if !self.reasoning.is_empty() {
            message["reasoning_content"] = json!(self.reasoning);
        }
        if !self.calls.is_empty() {
            message["tool_calls"] = Value::Array(
                self.calls
                    .iter()
                    .map(|c| {
                        json!({"id": c.id, "type": "function",
                               "function": {"name": c.name, "arguments": c.arguments}})
                    })
                    .collect(),
            );
        }
        json!({
            "id": format!("chatcmpl-{}", self.meta.id),
            "object": "chat.completion",
            "created": self.meta.created,
            "model": self.meta.model,
            "choices": [{"index": 0, "message": message, "finish_reason": Self::finish_reason(fin)}],
            "usage": Self::usage(fin),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------------------------

enum Open {
    Message {
        index: usize,
        id: String,
        text: String,
    },
    Reasoning {
        index: usize,
        id: String,
        text: String,
    },
    Search {
        index: usize,
        item: Value,
    },
}

pub(crate) struct RespWire {
    meta: Meta,
    seq: u64,
    items: Vec<Value>,
    open: Option<Open>,
    /// The stateful part of the request (`previous_response_id`, `store`): decorates every
    /// response object the stream carries and stores the final one.
    pub pending: Option<crate::responses::Pending>,
    /// The sampling and tool fields echoed in the response object.
    echo: Value,
}

impl RespWire {
    pub fn new(meta: Meta, pending: Option<crate::responses::Pending>, echo: Value) -> Self {
        Self {
            meta,
            seq: 0,
            items: Vec::new(),
            open: None,
            pending,
            echo,
        }
    }

    fn response_id(&self) -> String {
        format!("resp_{}", self.meta.id.trim_start_matches("req_"))
    }

    fn object(&self, status: &str, output: Vec<Value>, usage: Value) -> Value {
        let mut r = json!({
            "id": self.response_id(),
            "object": "response",
            "created_at": self.meta.created,
            "status": status,
            "background": false,
            "error": null,
            "incomplete_details": null,
            "model": self.meta.model,
            "output": output,
            "parallel_tool_calls": true,
            "tool_choice": "auto",
            "tools": [],
            "truncation": "disabled",
            "usage": usage,
            "metadata": {},
        });
        if let (Some(o), Some(e)) = (r.as_object_mut(), self.echo.as_object()) {
            for (k, v) in e {
                o.insert(k.clone(), v.clone());
            }
        }
        if let Some(p) = &self.pending {
            p.decorate(&mut r);
        }
        r
    }

    fn usage(fin: &Finish) -> Value {
        json!({
            "input_tokens": fin.input(),
            "input_tokens_details": {"cached_tokens": fin.usage.cached_tokens.unwrap_or(0)},
            "output_tokens": fin.output(),
            "output_tokens_details": {"reasoning_tokens": 0},
            "total_tokens": fin.input() + fin.output(),
        })
    }

    /// One SSE event (names flattened for the model are mapped back to the client's).
    fn ev(&mut self, kind: &str, mut body: Value) -> Bytes {
        body["type"] = json!(kind);
        body["sequence_number"] = json!(self.seq);
        self.seq += 1;
        if !self.meta.names.is_empty() {
            map_back(&mut body, &self.meta.names);
        }
        sse_event(kind, &body)
    }

    fn begin(&mut self) -> Vec<Bytes> {
        let r = self.object("in_progress", Vec::new(), Value::Null);
        vec![
            self.ev("response.created", json!({"response": r.clone()})),
            self.ev("response.in_progress", json!({"response": r})),
        ]
    }

    fn next_id(&self, prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            self.meta.id.trim_start_matches("req_"),
            self.items.len()
        )
    }

    /// Closes the open item (message, reasoning or search).
    fn close(&mut self) -> Vec<Bytes> {
        let Some(open) = self.open.take() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        match open {
            Open::Message { index, id, text } => {
                out.push(self.ev(
                    "response.output_text.done",
                    json!({"item_id": id, "output_index": index, "content_index": 0,
                           "text": text, "logprobs": []}),
                ));
                let part =
                    json!({"type": "output_text", "text": text, "annotations": [], "logprobs": []});
                out.push(self.ev(
                    "response.content_part.done",
                    json!({"item_id": id, "output_index": index, "content_index": 0, "part": part}),
                ));
                let item = json!({"id": id, "type": "message", "status": "completed",
                                  "role": "assistant", "content": [part]});
                out.push(self.ev(
                    "response.output_item.done",
                    json!({"output_index": index, "item": item.clone()}),
                ));
                self.items.push(item);
            }
            Open::Reasoning { index, id, text } => {
                out.push(self.ev(
                    "response.reasoning_summary_text.done",
                    json!({"item_id": id, "output_index": index, "summary_index": 0, "text": text}),
                ));
                let part = json!({"type": "summary_text", "text": text});
                out.push(self.ev(
                    "response.reasoning_summary_part.done",
                    json!({"item_id": id, "output_index": index, "summary_index": 0, "part": part.clone()}),
                ));
                let item = json!({"id": id, "type": "reasoning", "summary": [part]});
                out.push(self.ev(
                    "response.output_item.done",
                    json!({"output_index": index, "item": item.clone()}),
                ));
                self.items.push(item);
            }
            Open::Search { index, mut item } => {
                out.push(self.ev(
                    "response.web_search_call.completed",
                    json!({"item_id": item["id"], "output_index": index}),
                ));
                item["status"] = json!("completed");
                out.push(self.ev(
                    "response.output_item.done",
                    json!({"output_index": index, "item": item.clone()}),
                ));
                self.items.push(item);
            }
        }
        out
    }

    fn event(&mut self, ev: &AgentEvent) -> Vec<Bytes> {
        match ev {
            AgentEvent::Text(t) => {
                let mut out = Vec::new();
                if !matches!(self.open, Some(Open::Message { .. })) {
                    out.extend(self.close());
                    let (index, id) = (self.items.len(), self.next_id("msg"));
                    out.push(self.ev(
                        "response.output_item.added",
                        json!({"output_index": index, "item": {
                            "id": id, "type": "message", "status": "in_progress",
                            "role": "assistant", "content": []}}),
                    ));
                    out.push(self.ev(
                        "response.content_part.added",
                        json!({"item_id": id, "output_index": index, "content_index": 0,
                               "part": {"type": "output_text", "text": "", "annotations": [], "logprobs": []}}),
                    ));
                    self.open = Some(Open::Message {
                        index,
                        id,
                        text: String::new(),
                    });
                }
                let (index, id) = match self.open.as_mut() {
                    Some(Open::Message { index, id, text }) => {
                        text.push_str(t);
                        (*index, id.clone())
                    }
                    _ => return out,
                };
                out.push(self.ev(
                    "response.output_text.delta",
                    json!({"item_id": id, "output_index": index, "content_index": 0,
                           "delta": t, "logprobs": []}),
                ));
                out
            }
            AgentEvent::Reasoning(t) => {
                let mut out = Vec::new();
                if !matches!(self.open, Some(Open::Reasoning { .. })) {
                    out.extend(self.close());
                    let (index, id) = (self.items.len(), self.next_id("rs"));
                    out.push(self.ev(
                        "response.output_item.added",
                        json!({"output_index": index, "item": {"id": id, "type": "reasoning", "summary": []}}),
                    ));
                    out.push(self.ev(
                        "response.reasoning_summary_part.added",
                        json!({"item_id": id, "output_index": index, "summary_index": 0,
                               "part": {"type": "summary_text", "text": ""}}),
                    ));
                    self.open = Some(Open::Reasoning {
                        index,
                        id,
                        text: String::new(),
                    });
                }
                let (index, id) = match self.open.as_mut() {
                    Some(Open::Reasoning { index, id, text }) => {
                        text.push_str(t);
                        (*index, id.clone())
                    }
                    _ => return out,
                };
                out.push(self.ev(
                    "response.reasoning_summary_text.delta",
                    json!({"item_id": id, "output_index": index, "summary_index": 0, "delta": t}),
                ));
                out
            }
            AgentEvent::ToolStart {
                tool, arguments, ..
            } => {
                let mut out = self.close();
                if tool == "web_search" {
                    let (index, id) = (self.items.len(), self.next_id("ws"));
                    let item = json!({"id": id, "type": "web_search_call", "status": "in_progress",
                                      "action": {"type": "search", "query": query_of(arguments)}});
                    out.push(self.ev(
                        "response.output_item.added",
                        json!({"output_index": index, "item": item.clone()}),
                    ));
                    out.push(self.ev(
                        "response.web_search_call.in_progress",
                        json!({"item_id": id, "output_index": index}),
                    ));
                    out.push(self.ev(
                        "response.web_search_call.searching",
                        json!({"item_id": id, "output_index": index}),
                    ));
                    self.open = Some(Open::Search { index, item });
                }
                out
            }
            AgentEvent::ToolDone(_) => self.close(),
            AgentEvent::ClientCalls(calls) => {
                let mut out = self.close();
                for c in calls {
                    let (index, id) = (self.items.len(), self.next_id("fc"));
                    let mut item = json!({"id": id, "type": "function_call", "status": "in_progress",
                                          "call_id": c.id, "name": c.name, "arguments": ""});
                    out.push(self.ev(
                        "response.output_item.added",
                        json!({"output_index": index, "item": item.clone()}),
                    ));
                    out.push(self.ev(
                        "response.function_call_arguments.delta",
                        json!({"item_id": id, "output_index": index, "delta": c.arguments}),
                    ));
                    out.push(self.ev(
                        "response.function_call_arguments.done",
                        json!({"item_id": id, "output_index": index, "name": c.name,
                               "arguments": c.arguments}),
                    ));
                    item["status"] = json!("completed");
                    item["arguments"] = json!(c.arguments);
                    out.push(self.ev(
                        "response.output_item.done",
                        json!({"output_index": index, "item": item.clone()}),
                    ));
                    self.items.push(item);
                }
                out
            }
            _ => Vec::new(),
        }
    }

    /// The final response object (closes what is still open: call [`Self::end_with`] with it for
    /// a stream).
    pub fn response(&mut self, fin: &Finish) -> Value {
        let _ = self.close();
        let mut items = self.items.clone();
        if !self.meta.names.is_empty() {
            for i in items.iter_mut() {
                map_back(i, &self.meta.names);
            }
        }
        let mut r = self.object("completed", items, Self::usage(fin));
        if fin.truncated() {
            r["status"] = json!("incomplete");
            r["incomplete_details"] = json!({"reason": "max_output_tokens"});
        }
        r
    }

    /// The stream's last event, carrying `response` (decorated and stored by the caller).
    pub fn end_with(&mut self, response: Value) -> Vec<Bytes> {
        let kind = if response["status"] == "incomplete" {
            "response.incomplete"
        } else {
            "response.completed"
        };
        vec![self.ev(kind, json!({"response": response}))]
    }

    /// The events that close the item still open (call before [`Self::response`], which would
    /// close it without telling the stream).
    pub fn close_events(&mut self) -> Vec<Bytes> {
        self.close()
    }

    fn fail(&mut self, e: &AgentError) -> Vec<Bytes> {
        let mut out = self.close();
        let mut r = self.object("failed", self.items.clone(), Value::Null);
        r["error"] = json!({"code": e.code, "message": e.message});
        out.push(self.ev("response.failed", json!({"response": r})));
        out
    }
}

// ---------------------------------------------------------------------------------------------
// Anthropic Messages
// ---------------------------------------------------------------------------------------------

pub(crate) struct MsgWire {
    meta: Meta,
    /// Finished content blocks (the non-streaming body).
    blocks: Vec<Value>,
    /// The text block being streamed: (index, text).
    open: Option<(usize, String)>,
    searches: u64,
}

impl MsgWire {
    pub fn new(meta: Meta) -> Self {
        Self {
            meta,
            blocks: Vec::new(),
            open: None,
            searches: 0,
        }
    }

    fn message_id(&self) -> String {
        format!("msg_{}", self.meta.id.trim_start_matches("req_"))
    }

    fn begin(&mut self) -> Vec<Bytes> {
        vec![sse_event(
            "message_start",
            &json!({"type": "message_start", "message": {
                "id": self.message_id(), "type": "message", "role": "assistant",
                "model": self.meta.model, "content": [],
                "stop_reason": null, "stop_sequence": null,
                "usage": {"input_tokens": 0, "output_tokens": 0}}}),
        )]
    }

    fn close(&mut self) -> Vec<Bytes> {
        let Some((index, text)) = self.open.take() else {
            return Vec::new();
        };
        self.blocks.push(json!({"type": "text", "text": text}));
        vec![sse_event(
            "content_block_stop",
            &json!({"type": "content_block_stop", "index": index}),
        )]
    }

    fn event(&mut self, ev: &AgentEvent) -> Vec<Bytes> {
        match ev {
            AgentEvent::Text(t) => {
                let mut out = Vec::new();
                if self.open.is_none() {
                    let index = self.blocks.len();
                    out.push(sse_event(
                        "content_block_start",
                        &json!({"type": "content_block_start", "index": index,
                                "content_block": {"type": "text", "text": ""}}),
                    ));
                    self.open = Some((index, String::new()));
                }
                if let Some((index, text)) = self.open.as_mut() {
                    text.push_str(t);
                    out.push(sse_event(
                        "content_block_delta",
                        &json!({"type": "content_block_delta", "index": *index,
                                "delta": {"type": "text_delta", "text": t}}),
                    ));
                }
                out
            }
            AgentEvent::ToolStart { .. } => self.close(),
            AgentEvent::ToolDone(call) => {
                if call.tool == "web_search" {
                    self.searches += 1;
                }
                self.close()
            }
            AgentEvent::ClientCalls(calls) => {
                let mut out = self.close();
                for c in calls {
                    let index = self.blocks.len();
                    let input: Value =
                        serde_json::from_str(&c.arguments).unwrap_or_else(|_| json!({}));
                    out.push(sse_event(
                        "content_block_start",
                        &json!({"type": "content_block_start", "index": index,
                                "content_block": {"type": "tool_use", "id": c.id, "name": c.name, "input": {}}}),
                    ));
                    out.push(sse_event(
                        "content_block_delta",
                        &json!({"type": "content_block_delta", "index": index,
                                "delta": {"type": "input_json_delta", "partial_json": c.arguments}}),
                    ));
                    out.push(sse_event(
                        "content_block_stop",
                        &json!({"type": "content_block_stop", "index": index}),
                    ));
                    self.blocks.push(
                        json!({"type": "tool_use", "id": c.id, "name": c.name, "input": input}),
                    );
                }
                out
            }
            _ => Vec::new(),
        }
    }

    fn stop_reason(fin: &Finish) -> &'static str {
        match fin.stop {
            StopReason::ClientToolCalls => "tool_use",
            _ if fin.truncated() => "max_tokens",
            _ => "end_turn",
        }
    }

    fn usage(&self, fin: &Finish) -> Value {
        let mut u = json!({"input_tokens": fin.input(), "output_tokens": fin.output()});
        if self.searches > 0 {
            u["server_tool_use"] = json!({"web_search_requests": self.searches});
        }
        u
    }

    fn end(&mut self, fin: &Finish) -> Vec<Bytes> {
        let mut out = self.close();
        out.push(sse_event(
            "message_delta",
            &json!({"type": "message_delta",
                    "delta": {"stop_reason": Self::stop_reason(fin), "stop_sequence": null},
                    "usage": self.usage(fin)}),
        ));
        out.push(sse_event("message_stop", &json!({"type": "message_stop"})));
        out
    }

    fn fail(&mut self, e: &AgentError) -> Vec<Bytes> {
        let mut out = self.close();
        out.push(sse_event(
            "error",
            &json!({"type": "error", "error": {"type": "api_error", "message": e.message}}),
        ));
        out
    }

    fn json(&mut self, fin: &Finish) -> Value {
        let _ = self.close();
        json!({
            "id": self.message_id(),
            "type": "message",
            "role": "assistant",
            "model": self.meta.model,
            "content": self.blocks,
            "stop_reason": Self::stop_reason(fin),
            "stop_sequence": null,
            "usage": self.usage(fin),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::tools::agent::AgentCall;

    fn meta() -> Meta {
        Meta {
            id: "req_abc".into(),
            model: "qwen".into(),
            created: 1,
            include_usage: true,
            names: NameMap::new(),
        }
    }

    fn fin(stop: StopReason, _calls: Vec<ModelCall>) -> Finish {
        Finish {
            stop,
            finish_reason: Some("stop".into()),
            usage: Usage {
                prompt_tokens: Some(10),
                completion_tokens: Some(5),
                ..Default::default()
            },
        }
    }

    fn done(tool: &str, args: &str) -> AgentCall {
        AgentCall {
            round: 1,
            id: "c1".into(),
            tool: tool.into(),
            arguments: args.into(),
            result: None,
            sent: "ok".into(),
        }
    }

    fn feed(w: &mut Wire, events: &[AgentEvent]) -> String {
        let mut out = String::new();
        for b in w.begin() {
            out.push_str(&String::from_utf8_lossy(&b));
        }
        for e in events {
            for b in w.event(e) {
                out.push_str(&String::from_utf8_lossy(&b));
            }
        }
        out
    }

    fn start(args: &str) -> AgentEvent {
        AgentEvent::ToolStart {
            round: 1,
            id: "c1".into(),
            tool: "web_search".into(),
            arguments: args.into(),
        }
    }

    #[test]
    fn chat_json_joins_rounds_and_reports_calls() {
        let mut w = Wire::Chat(ChatWire::new(meta()));
        let calls = vec![ModelCall {
            id: "t1".into(),
            name: "mine".into(),
            arguments: "{}".into(),
        }];
        feed(
            &mut w,
            &[
                AgentEvent::Text("Searching.".into()),
                start("{\"query\":\"x\"}"),
                AgentEvent::ToolDone(done("web_search", "{}")),
                AgentEvent::Text("Found it.".into()),
                AgentEvent::ClientCalls(calls.clone()),
            ],
        );
        let v = w.json(&fin(StopReason::ClientToolCalls, calls));
        assert_eq!(
            v["choices"][0]["message"]["content"],
            "Searching.\n\nFound it."
        );
        assert_eq!(v["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(
            v["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
            "mine"
        );
        assert_eq!(v["usage"]["total_tokens"], 15);
    }

    #[test]
    fn chat_stream_ends_with_usage_and_done() {
        let mut w = Wire::Chat(ChatWire::new(meta()));
        let mut text = feed(&mut w, &[AgentEvent::Text("hi".into())]);
        for b in w.end(&fin(StopReason::Answered, vec![])) {
            text.push_str(&String::from_utf8_lossy(&b));
        }
        assert!(text.contains("\"role\":\"assistant\""), "{text}");
        assert!(text.contains("\"content\":\"hi\""));
        assert!(text.contains("\"finish_reason\":\"stop\""));
        assert!(text.contains("\"usage\":{\"prompt_tokens\":10"));
        assert!(text.ends_with("data: [DONE]\n\n"));
    }

    #[test]
    fn responses_show_the_web_search_and_hide_other_tools() {
        let mut w = Wire::Responses(Box::new(RespWire::new(meta(), None, json!({}))));
        let text = feed(
            &mut w,
            &[
                start("{\"query\":\"harness studio\"}"),
                AgentEvent::ToolDone(done("web_search", "{}")),
                AgentEvent::ToolStart {
                    round: 2,
                    id: "c2".into(),
                    tool: "calculator".into(),
                    arguments: "{}".into(),
                },
                AgentEvent::ToolDone(done("calculator", "{}")),
                AgentEvent::Text("Answer.".into()),
            ],
        );
        assert!(text.contains("event: response.created"), "{text}");
        assert!(text.contains("response.web_search_call.searching"));
        assert!(text.contains("\"query\":\"harness studio\""));
        assert!(!text.contains("calculator"), "{text}");
        let v = w.json(&fin(StopReason::Answered, vec![]));
        let types: Vec<&str> = v["output"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["type"].as_str().unwrap())
            .collect();
        assert_eq!(types, ["web_search_call", "message"]);
        assert_eq!(v["output"][1]["content"][0]["text"], "Answer.");
        assert_eq!(v["status"], "completed");
        assert_eq!(v["usage"]["input_tokens"], 10);
        // Sequence numbers count up without gaps.
        let seqs: Vec<u64> = text
            .lines()
            .filter_map(|l| l.strip_prefix("data: "))
            .map(|d| {
                serde_json::from_str::<Value>(d).unwrap()["sequence_number"]
                    .as_u64()
                    .unwrap()
            })
            .collect();
        assert!(seqs.windows(2).all(|p| p[1] == p[0] + 1), "{seqs:?}");
    }

    #[test]
    fn responses_flattened_names_map_back() {
        let mut names = NameMap::new();
        names.insert("mcp__gh__issue".into(), ("mcp__gh".into(), "issue".into()));
        let mut m = meta();
        m.names = names;
        let mut w = Wire::Responses(Box::new(RespWire::new(m, None, json!({}))));
        let calls = vec![ModelCall {
            id: "t1".into(),
            name: "mcp__gh__issue".into(),
            arguments: "{}".into(),
        }];
        let text = feed(&mut w, &[AgentEvent::ClientCalls(calls.clone())]);
        assert!(text.contains("\"namespace\":\"mcp__gh\""), "{text}");
        let v = w.json(&fin(StopReason::ClientToolCalls, calls));
        assert_eq!(v["output"][0]["name"], "issue");
        assert_eq!(v["output"][0]["namespace"], "mcp__gh");
        assert_eq!(v["output"][0]["call_id"], "t1");
    }

    #[test]
    fn messages_blocks_and_stop_reasons() {
        let mut w = Wire::Messages(MsgWire::new(meta()));
        let calls = vec![ModelCall {
            id: "toolu_1".into(),
            name: "mine".into(),
            arguments: "{\"a\":1}".into(),
        }];
        let text = feed(
            &mut w,
            &[
                AgentEvent::Text("Let me look.".into()),
                start("{}"),
                AgentEvent::ToolDone(done("web_search", "{}")),
                AgentEvent::Text("Done.".into()),
                AgentEvent::ClientCalls(calls.clone()),
            ],
        );
        assert!(text.starts_with("event: message_start"), "{text}");
        assert!(text.contains("\"type\":\"input_json_delta\""));
        let v = w.json(&fin(StopReason::ClientToolCalls, calls));
        assert_eq!(v["stop_reason"], "tool_use");
        let types: Vec<&str> = v["content"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["type"].as_str().unwrap())
            .collect();
        assert_eq!(types, ["text", "text", "tool_use"]);
        assert_eq!(v["content"][2]["input"]["a"], 1);
        assert_eq!(v["usage"]["server_tool_use"]["web_search_requests"], 1);
    }
}
