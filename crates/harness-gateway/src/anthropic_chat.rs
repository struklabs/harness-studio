//! Chat Completions to Anthropic Messages and back, for `cloud:` models of an Anthropic
//! provider: clients speak `/v1/chat/completions`, the provider's native `/v1/messages` is
//! called. Messages requests to such models are not translated (they pass through).
//!
//! Only what shapes the answer is carried over: messages (text, images, tool calls and tool
//! results), tools and `tool_choice`, sampling parameters and stop sequences.

use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// Anthropic requires `max_tokens`; used when the client sent no limit.
pub const DEFAULT_MAX_TOKENS: u64 = 4096;

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn image_block(url: &str) -> Value {
    if let Some(rest) = url.strip_prefix("data:") {
        if let Some((meta, data)) = rest.split_once(',') {
            let media = meta.split(';').next().unwrap_or("image/png");
            return json!({"type": "image", "source": {
                "type": "base64", "media_type": media, "data": data}});
        }
    }
    json!({"type": "image", "source": {"type": "url", "url": url}})
}

fn blocks(content: &Value) -> Result<Vec<Value>, String> {
    match content {
        Value::String(s) if !s.is_empty() => Ok(vec![json!({"type": "text", "text": s})]),
        Value::Array(parts) => {
            let mut out = Vec::new();
            for part in parts {
                match part["type"].as_str() {
                    Some("text") => {
                        if let Some(t) = part["text"].as_str().filter(|t| !t.is_empty()) {
                            out.push(json!({"type": "text", "text": t}));
                        }
                    }
                    Some("image_url") => {
                        let url = part["image_url"]["url"]
                            .as_str()
                            .or_else(|| part["image_url"].as_str())
                            .ok_or("An image part needs a url.")?;
                        out.push(image_block(url));
                    }
                    Some(other) => {
                        return Err(format!(
                        "Content parts of type `{other}` are not supported for Anthropic models."
                    ))
                    }
                    None => return Err("A content part needs a type.".into()),
                }
            }
            Ok(out)
        }
        _ => Ok(Vec::new()),
    }
}

/// Adds a message; consecutive messages of one role become one (Anthropic wants alternation).
fn push(messages: &mut Vec<Value>, role: &str, mut content: Vec<Value>) {
    if content.is_empty() {
        return;
    }
    if let Some(last) = messages.last_mut().filter(|m| m["role"] == role) {
        if let Some(existing) = last["content"].as_array_mut() {
            existing.append(&mut content);
            return;
        }
    }
    messages.push(json!({"role": role, "content": content}));
}

fn stop_sequences(stop: &Value) -> Option<Value> {
    match stop {
        Value::String(s) if !s.is_empty() => Some(json!([s])),
        Value::Array(list) if !list.is_empty() => Some(Value::Array(list.clone())),
        _ => None,
    }
}

fn tool_choice(choice: &Value) -> Option<Value> {
    match choice {
        Value::String(s) => match s.as_str() {
            "auto" => Some(json!({"type": "auto"})),
            "required" => Some(json!({"type": "any"})),
            "none" => Some(json!({"type": "none"})),
            _ => None,
        },
        Value::Object(_) => choice["function"]["name"]
            .as_str()
            .map(|name| json!({"type": "tool", "name": name})),
        _ => None,
    }
}

/// A Chat Completions request as an Anthropic Messages request.
pub fn request(body: &[u8]) -> Result<Vec<u8>, String> {
    let doc: Value = serde_json::from_slice(body).map_err(|e| e.to_string())?;
    let doc = doc
        .as_object()
        .ok_or("The request body must be an object.")?;
    let mut out = Map::new();
    out.insert(
        "model".into(),
        doc.get("model").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "max_tokens".into(),
        doc.get("max_completion_tokens")
            .or_else(|| doc.get("max_tokens"))
            .filter(|v| v.is_u64())
            .cloned()
            .unwrap_or(json!(DEFAULT_MAX_TOKENS)),
    );
    let mut system: Vec<String> = Vec::new();
    let mut messages: Vec<Value> = Vec::new();
    for message in doc
        .get("messages")
        .and_then(Value::as_array)
        .ok_or("`messages` must be an array.")?
    {
        match message["role"].as_str() {
            Some("system" | "developer") => {
                let text = text_of(&message["content"]);
                if !text.is_empty() {
                    system.push(text);
                }
            }
            Some("user") => push(&mut messages, "user", blocks(&message["content"])?),
            Some("assistant") => {
                let mut content = blocks(&message["content"])?;
                for call in message["tool_calls"].as_array().into_iter().flatten() {
                    let args = call["function"]["arguments"].as_str().unwrap_or("{}");
                    let input: Value = serde_json::from_str(args).unwrap_or_else(|_| json!({}));
                    content.push(json!({
                        "type": "tool_use",
                        "id": call["id"],
                        "name": call["function"]["name"],
                        "input": if input.is_object() { input } else { json!({}) },
                    }));
                }
                push(&mut messages, "assistant", content);
            }
            Some("tool") => push(
                &mut messages,
                "user",
                vec![json!({
                    "type": "tool_result",
                    "tool_use_id": message["tool_call_id"],
                    "content": text_of(&message["content"]),
                })],
            ),
            _ => return Err("Every message needs a role (system, user, assistant, tool).".into()),
        }
    }
    if !system.is_empty() {
        out.insert("system".into(), system.join("\n\n").into());
    }
    out.insert("messages".into(), messages.into());
    let none = doc.get("tool_choice").and_then(Value::as_str) == Some("none");
    if let Some(tools) = doc.get("tools").and_then(Value::as_array).filter(|_| !none) {
        let mapped: Vec<Value> = tools
            .iter()
            .filter(|t| t["type"] == "function")
            .map(|t| {
                let f = &t["function"];
                let mut tool = json!({
                    "name": f["name"],
                    "input_schema": if f["parameters"].is_object() {
                        f["parameters"].clone()
                    } else {
                        json!({"type": "object"})
                    },
                });
                if let Some(d) = f["description"].as_str() {
                    tool["description"] = d.into();
                }
                tool
            })
            .collect();
        if !mapped.is_empty() {
            out.insert("tools".into(), mapped.into());
            if let Some(choice) = doc.get("tool_choice").and_then(tool_choice) {
                out.insert("tool_choice".into(), choice);
            }
        }
    }
    for key in ["temperature", "top_p", "stream"] {
        if let Some(v) = doc.get(key).filter(|v| !v.is_null()) {
            out.insert(key.into(), v.clone());
        }
    }
    if let Some(stop) = doc.get("stop").and_then(stop_sequences) {
        out.insert("stop_sequences".into(), stop);
    }
    serde_json::to_vec(&Value::Object(out)).map_err(|e| e.to_string())
}

fn finish_reason(stop: &str) -> &'static str {
    match stop {
        "max_tokens" => "length",
        "tool_use" => "tool_calls",
        "refusal" => "content_filter",
        _ => "stop",
    }
}

fn usage(u: &Value, output: Option<u64>) -> Value {
    let input = [
        "input_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
    ]
    .iter()
    .filter_map(|k| u[*k].as_u64())
    .sum::<u64>();
    let output = output.or_else(|| u["output_tokens"].as_u64()).unwrap_or(0);
    json!({"prompt_tokens": input, "completion_tokens": output, "total_tokens": input + output})
}

fn created() -> u64 {
    crate::proxy::unix_ms() / 1000
}

/// An Anthropic message as a chat completion.
pub fn response(body: &[u8]) -> Option<Vec<u8>> {
    let v: Value = serde_json::from_slice(body).ok()?;
    if v["type"] != "message" {
        return None;
    }
    let mut text = String::new();
    let mut calls = Vec::new();
    for block in v["content"].as_array()? {
        match block["type"].as_str() {
            Some("text") => text.push_str(block["text"].as_str().unwrap_or("")),
            Some("tool_use") => calls.push(json!({
                "id": block["id"], "type": "function",
                "function": {"name": block["name"], "arguments": block["input"].to_string()},
            })),
            _ => {}
        }
    }
    let mut message = json!({"role": "assistant", "content": if text.is_empty() && !calls.is_empty() {
        Value::Null
    } else {
        text.into()
    }});
    if !calls.is_empty() {
        message["tool_calls"] = calls.into();
    }
    serde_json::to_vec(&json!({
        "id": v["id"], "object": "chat.completion", "created": created(), "model": v["model"],
        "choices": [{"index": 0, "message": message,
            "finish_reason": finish_reason(v["stop_reason"].as_str().unwrap_or("end_turn"))}],
        "usage": usage(&v["usage"], None),
    }))
    .ok()
}

/// An Anthropic error body as an OpenAI one (`{"error": {"message", "type", "code"}}`).
pub fn error(body: &[u8]) -> Vec<u8> {
    let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let e = &v["error"];
    serde_json::to_vec(&json!({"error": {
        "message": e["message"].as_str().unwrap_or("The provider returned an error."),
        "type": e["type"].as_str().unwrap_or("api_error"),
        "code": e["type"].as_str(),
    }}))
    .unwrap_or_default()
}

enum Block {
    Text,
    Tool(u64),
    Ignored,
}

/// Anthropic stream events as chat completion chunks, fed with the upstream bytes.
#[derive(Default)]
pub struct Stream {
    buffer: Vec<u8>,
    id: String,
    model: String,
    input: Value,
    blocks: HashMap<u64, Block>,
    tools: u64,
    done: bool,
}

impl Stream {
    pub fn new() -> Self {
        Self::default()
    }

    fn chunk(&self, delta: Value, finish: Option<&str>, usage: Option<Value>) -> Vec<u8> {
        let mut v = json!({
            "id": self.id, "object": "chat.completion.chunk", "created": created(),
            "model": self.model,
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
        });
        if let Some(u) = usage {
            v["usage"] = u;
        }
        format!("data: {v}\n\n").into_bytes()
    }

    fn handle(&mut self, v: &Value) -> Vec<u8> {
        match v["type"].as_str() {
            Some("message_start") => {
                self.id = v["message"]["id"]
                    .as_str()
                    .unwrap_or("chatcmpl")
                    .to_string();
                self.model = v["message"]["model"].as_str().unwrap_or("").to_string();
                self.input = v["message"]["usage"].clone();
                self.chunk(json!({"role": "assistant", "content": ""}), None, None)
            }
            Some("content_block_start") => {
                let index = v["index"].as_u64().unwrap_or(0);
                let block = &v["content_block"];
                match block["type"].as_str() {
                    Some("text") => {
                        self.blocks.insert(index, Block::Text);
                        Vec::new()
                    }
                    Some("tool_use") => {
                        let n = self.tools;
                        self.tools += 1;
                        self.blocks.insert(index, Block::Tool(n));
                        self.chunk(
                            json!({"tool_calls": [{"index": n, "id": block["id"],
                                "type": "function",
                                "function": {"name": block["name"], "arguments": ""}}]}),
                            None,
                            None,
                        )
                    }
                    _ => {
                        self.blocks.insert(index, Block::Ignored);
                        Vec::new()
                    }
                }
            }
            Some("content_block_delta") => {
                let index = v["index"].as_u64().unwrap_or(0);
                let delta = &v["delta"];
                match (self.blocks.get(&index), delta["type"].as_str()) {
                    (Some(Block::Text), Some("text_delta")) => {
                        self.chunk(json!({"content": delta["text"]}), None, None)
                    }
                    (Some(Block::Tool(n)), Some("input_json_delta")) => self.chunk(
                        json!({"tool_calls": [{"index": n,
                            "function": {"arguments": delta["partial_json"]}}]}),
                        None,
                        None,
                    ),
                    (_, Some("thinking_delta")) => {
                        self.chunk(json!({"reasoning_content": delta["thinking"]}), None, None)
                    }
                    _ => Vec::new(),
                }
            }
            Some("message_delta") => {
                let stop = v["delta"]["stop_reason"].as_str().unwrap_or("end_turn");
                let output = v["usage"]["output_tokens"].as_u64();
                self.chunk(
                    json!({}),
                    Some(finish_reason(stop)),
                    Some(usage(&self.input, output)),
                )
            }
            Some("message_stop") => {
                self.done = true;
                b"data: [DONE]\n\n".to_vec()
            }
            Some("error") => {
                self.done = true;
                let e = &v["error"];
                let mut out = format!(
                    "data: {}\n\n",
                    json!({"error": {
                        "message": e["message"].as_str().unwrap_or("The provider returned an error."),
                        "type": e["type"].as_str().unwrap_or("api_error"),
                    }})
                )
                .into_bytes();
                out.extend_from_slice(b"data: [DONE]\n\n");
                out
            }
            _ => Vec::new(),
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.buffer.extend(bytes.iter().filter(|b| **b != b'\r'));
        let mut out = Vec::new();
        while let Some(end) = self.buffer.windows(2).position(|w| w == b"\n\n") {
            let event: Vec<u8> = self.buffer.drain(..end + 2).collect();
            let text = String::from_utf8_lossy(&event);
            let data: String = text
                .lines()
                .filter_map(|l| l.strip_prefix("data:"))
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n");
            if let Ok(v) = serde_json::from_str::<Value>(&data) {
                out.extend(self.handle(&v));
            }
        }
        out
    }

    /// Nothing is added at the end: a stream that stopped without `message_stop` stays
    /// visibly cut off.
    pub fn finish(&mut self) -> Vec<u8> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(body: Value) -> Value {
        serde_json::from_slice(&request(&serde_json::to_vec(&body).unwrap()).unwrap()).unwrap()
    }

    #[test]
    fn requests_map_system_tools_and_results() {
        let out = run(json!({
            "model": "claude-x", "stream": true, "stop": "END", "max_completion_tokens": 50,
            "messages": [
                {"role": "system", "content": "Be brief."},
                {"role": "user", "content": [{"type": "text", "text": "weather?"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}]},
                {"role": "assistant", "content": null, "tool_calls": [{"id": "t1", "type": "function",
                    "function": {"name": "get_weather", "arguments": "{\"city\":\"Paris\"}"}}]},
                {"role": "tool", "tool_call_id": "t1", "content": "sunny"},
                {"role": "user", "content": "thanks"}
            ],
            "tools": [{"type": "function", "function": {"name": "get_weather",
                "description": "d", "parameters": {"type": "object"}}}],
            "tool_choice": "required",
        }));
        assert_eq!(out["system"], "Be brief.");
        assert_eq!(out["max_tokens"], 50);
        assert_eq!(out["stream"], true);
        assert_eq!(out["stop_sequences"], json!(["END"]));
        assert_eq!(out["tool_choice"], json!({"type": "any"}));
        assert_eq!(out["tools"][0]["input_schema"], json!({"type": "object"}));
        let m = out["messages"].as_array().unwrap();
        assert_eq!(m.len(), 3);
        assert_eq!(m[0]["content"][1]["source"]["media_type"], "image/png");
        assert_eq!(m[1]["content"][0]["type"], "tool_use");
        assert_eq!(m[1]["content"][0]["input"]["city"], "Paris");
        // The tool result and the next user text share one user message.
        assert_eq!(m[2]["content"][0]["type"], "tool_result");
        assert_eq!(m[2]["content"][1]["text"], "thanks");
    }

    #[test]
    fn a_default_limit_is_added() {
        let out = run(json!({"model": "m", "messages": [{"role": "user", "content": "x"}]}));
        assert_eq!(out["max_tokens"], DEFAULT_MAX_TOKENS);
    }

    #[test]
    fn responses_and_errors_map_back() {
        let out: Value = serde_json::from_slice(
            &response(
                json!({"id": "msg_1", "type": "message", "model": "claude-x",
                    "content": [{"type": "text", "text": "hi"},
                        {"type": "tool_use", "id": "t1", "name": "f", "input": {"a": 1}}],
                    "stop_reason": "tool_use",
                    "usage": {"input_tokens": 3, "output_tokens": 2}})
                .to_string()
                .as_bytes(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(out["choices"][0]["message"]["content"], "hi");
        assert_eq!(out["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(
            out["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
            "{\"a\":1}"
        );
        assert_eq!(out["usage"]["total_tokens"], 5);
        let e: Value = serde_json::from_slice(&error(
            br#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#,
        ))
        .unwrap();
        assert_eq!(e["error"]["message"], "slow down");
        assert_eq!(e["error"]["type"], "rate_limit_error");
    }

    #[test]
    fn streams_map_text_tools_and_usage_across_chunk_boundaries() {
        let sse = "event: message_start\r\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-x\",\"usage\":{\"input_tokens\":7}}}\r\n\r\n\
event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n\
event: ping\ndata: {\"type\":\"ping\"}\n\n\
event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"f\",\"input\":{}}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"a\\\":1}\"}}\n\n\
event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":4}}\n\n\
event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
        let mut stream = Stream::new();
        let mut out = Vec::new();
        for chunk in sse.as_bytes().chunks(13) {
            out.extend(stream.push(chunk));
        }
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\"role\":\"assistant\""));
        assert!(text.contains("\"content\":\"hi\""));
        assert!(text.contains("\"name\":\"f\"") && text.contains("\"index\":0"));
        assert!(text.contains("\"arguments\":\"{\\\"a\\\":1}\""));
        assert!(text.contains("\"finish_reason\":\"tool_calls\""));
        assert!(text.contains("\"prompt_tokens\":7") && text.contains("\"completion_tokens\":4"));
        assert!(text.ends_with("data: [DONE]\n\n"));
    }
}
