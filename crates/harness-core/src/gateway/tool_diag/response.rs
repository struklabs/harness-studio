//! What a response says about tools, read while it streams through the gateway.
//!
//! [`ToolObserver`] understands the three wire formats the gateway serves, as a JSON body or as
//! SSE: Chat Completions (`message.tool_calls` / `delta.tool_calls`), Anthropic Messages
//! (`tool_use` blocks, `input_json_delta`) and Responses (`function_call` items,
//! `function_call_arguments.delta`). It keeps the answer text (bounded), the calls with their
//! argument text, an upstream error body and whether the stream itself was malformed. It runs
//! only for requests that involve tools.

use serde_json::Value;

/// Most answer text kept for the text-call check (the call markup is near the start).
const TEXT_LIMIT: usize = 32 * 1024;
/// Most argument text kept per call.
const ARGS_LIMIT: usize = 64 * 1024;
/// Largest non-streaming body parsed (same bound as `UsageObserver`).
const JSON_LIMIT: usize = 4 * 1024 * 1024;

/// One tool call of the response.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResponseCall {
    pub id: Option<String>,
    pub name: String,
    /// The arguments as written by the model (a JSON text).
    pub args: String,
    /// The arguments came as a JSON object (Anthropic `input`), not as text.
    pub args_object: bool,
    /// Key of the stream parts that belong together (`choices[].tool_calls[].index`,
    /// content block index, `output_index` / `item_id`).
    key: String,
}

/// A JSON error body of the upstream (llama-server `exceed_context_size_error`...).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpstreamError {
    pub kind: Option<String>,
    pub message: Option<String>,
    pub n_prompt_tokens: Option<u64>,
    pub n_ctx: Option<u64>,
}

/// The tool-related content of one response.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolResponse {
    /// The answer text (not the reasoning), bounded.
    pub text: String,
    pub calls: Vec<ResponseCall>,
    /// The stream broke the protocol (see [`ToolObserver`]); the reason.
    pub malformed: Option<&'static str>,
    pub error: Option<UpstreamError>,
}

/// Collects a [`ToolResponse`] from response bytes.
#[derive(Debug, Default)]
pub struct ToolObserver {
    sse: bool,
    line: Vec<u8>,
    json: Vec<u8>,
    overflow: bool,
    out: ToolResponse,
    /// Anthropic: the response came as content blocks (an empty `input` means `{}`).
    anthropic: bool,
}

impl ToolObserver {
    pub fn new(sse: bool) -> Self {
        Self {
            sse,
            ..Default::default()
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) {
        if !self.sse {
            if !self.overflow {
                if self.json.len() + chunk.len() > JSON_LIMIT {
                    self.overflow = true;
                    self.json = Vec::new();
                } else {
                    self.json.extend_from_slice(chunk);
                }
            }
            return;
        }
        let mut rest = chunk;
        while let Some(pos) = rest.iter().position(|&b| b == b'\n') {
            self.line.extend_from_slice(&rest[..pos]);
            let line = std::mem::take(&mut self.line);
            self.line_done(&line);
            rest = &rest[pos + 1..];
        }
        if self.line.len() + rest.len() <= JSON_LIMIT {
            self.line.extend_from_slice(rest);
        } else {
            self.line.clear();
        }
    }

    fn line_done(&mut self, line: &[u8]) {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(data) = line.strip_prefix(b"data:") else {
            return;
        };
        let data = data.trim_ascii();
        if data.is_empty() || data == b"[DONE]" {
            return;
        }
        match serde_json::from_slice::<Value>(data) {
            Ok(v) => self.absorb(&v),
            Err(_) => {
                // A chunk of a tool call that is not JSON: the stream is broken. Plain text
                // chunks that fail to parse are not our concern.
                if data.windows(10).any(|w| w == b"tool_calls") {
                    self.out.malformed.get_or_insert("bad_chunk");
                }
            }
        }
    }

    /// Call once when the body ended; returns what was read.
    pub fn finish(mut self) -> ToolResponse {
        if self.sse {
            let line = std::mem::take(&mut self.line);
            self.line_done(&line);
        } else if !self.overflow && !self.json.is_empty() {
            let body = std::mem::take(&mut self.json);
            if let Ok(v) = serde_json::from_slice::<Value>(&body) {
                self.absorb(&v);
            }
        }
        let anthropic = self.anthropic;
        let mut out = self.out;
        for c in &mut out.calls {
            if c.args.trim().is_empty() && (anthropic || c.args_object) {
                c.args = "{}".into();
            }
        }
        if out.calls.iter().any(|c| c.name.is_empty()) {
            out.malformed.get_or_insert("call_without_name");
        }
        out
    }

    fn push_text(&mut self, s: &str) {
        let room = TEXT_LIMIT.saturating_sub(self.out.text.len());
        if room == 0 {
            return;
        }
        if s.len() <= room {
            self.out.text.push_str(s);
        } else {
            let mut end = room;
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            self.out.text.push_str(&s[..end]);
        }
    }

    /// The call stored under `key`, created when new.
    fn call(&mut self, key: &str) -> &mut ResponseCall {
        if let Some(i) = self.out.calls.iter().position(|c| c.key == key) {
            return &mut self.out.calls[i];
        }
        self.out.calls.push(ResponseCall {
            key: key.to_string(),
            ..Default::default()
        });
        self.out.calls.last_mut().expect("just pushed")
    }

    fn push_args(&mut self, key: &str, more: &str) {
        let c = self.call(key);
        let room = ARGS_LIMIT.saturating_sub(c.args.len());
        if more.len() <= room {
            c.args.push_str(more);
        }
    }

    fn absorb(&mut self, v: &Value) {
        if let Some(e) = v.get("error").filter(|e| e.is_object()) {
            self.out.error = Some(UpstreamError {
                kind: e.get("type").and_then(Value::as_str).map(str::to_string),
                message: e.get("message").and_then(Value::as_str).map(str::to_string),
                n_prompt_tokens: e.get("n_prompt_tokens").and_then(Value::as_u64),
                n_ctx: e.get("n_ctx").and_then(Value::as_u64),
            });
            return;
        }
        // Responses stream events wrap the response object; its `output` repeats what the item
        // events said, so only a non-streaming Responses body is read from it.
        let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
        if v.get("object").and_then(Value::as_str) == Some("response") && !self.sse {
            self.absorb_responses_output(v);
            return;
        }
        if kind.starts_with("response.") {
            self.absorb_responses_event(kind, v);
            return;
        }
        if kind == "message" && v.get("content").is_some_and(Value::is_array) && !self.sse {
            self.absorb_anthropic_blocks(v);
            return;
        }
        if kind.starts_with("content_block_") {
            self.absorb_anthropic_event(kind, v);
            return;
        }
        if let Some(Value::Array(choices)) = v.get("choices") {
            for (ci, choice) in choices.iter().enumerate() {
                let m = choice.get("message").or_else(|| choice.get("delta"));
                if let Some(m) = m {
                    self.absorb_chat_message(ci, m);
                }
            }
        }
    }

    fn absorb_chat_message(&mut self, ci: usize, m: &Value) {
        if let Some(Value::String(s)) = m.get("content") {
            self.push_text(s);
        } else if let Some(Value::Array(parts)) = m.get("content") {
            for p in parts {
                if let Some(s) = p.get("text").and_then(Value::as_str) {
                    self.push_text(s);
                }
            }
        }
        let Some(Value::Array(calls)) = m.get("tool_calls") else {
            return;
        };
        for (pos, c) in calls.iter().enumerate() {
            let index = c.get("index").and_then(Value::as_u64).unwrap_or(pos as u64);
            let key = format!("c{ci}.{index}");
            let Some(f) = c.get("function").filter(|f| f.is_object()) else {
                self.out.malformed.get_or_insert("call_without_function");
                continue;
            };
            let id = c.get("id").and_then(Value::as_str).map(str::to_string);
            let name = f.get("name").and_then(Value::as_str).map(str::to_string);
            let args = f.get("arguments");
            let call = self.call(&key);
            if id.is_some() {
                call.id = id;
            }
            if let Some(n) = name {
                call.name.push_str(&n);
            }
            match args {
                Some(Value::String(a)) => self.push_args(&key, a),
                Some(obj @ Value::Object(_)) => {
                    let call = self.call(&key);
                    call.args = obj.to_string();
                    call.args_object = true;
                }
                _ => {}
            }
        }
    }

    fn absorb_anthropic_blocks(&mut self, v: &Value) {
        self.anthropic = true;
        let Some(Value::Array(blocks)) = v.get("content") else {
            return;
        };
        for (i, b) in blocks.iter().enumerate() {
            match b.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(s) = b.get("text").and_then(Value::as_str) {
                        self.push_text(s);
                    }
                }
                Some("tool_use") => {
                    let key = format!("b{i}");
                    let call = self.call(&key);
                    call.id = b.get("id").and_then(Value::as_str).map(str::to_string);
                    call.name = b
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    call.args = b.get("input").map(Value::to_string).unwrap_or_default();
                    call.args_object = true;
                }
                _ => {}
            }
        }
    }

    fn absorb_anthropic_event(&mut self, kind: &str, v: &Value) {
        self.anthropic = true;
        let index = v.get("index").and_then(Value::as_u64).unwrap_or(0);
        let key = format!("b{index}");
        match kind {
            "content_block_start" => {
                let Some(b) = v.get("content_block") else {
                    return;
                };
                match b.get("type").and_then(Value::as_str) {
                    Some("tool_use") => {
                        let call = self.call(&key);
                        call.id = b.get("id").and_then(Value::as_str).map(str::to_string);
                        call.name = b
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        // A start event may carry the complete input.
                        if let Some(input) = b.get("input").filter(|i| !is_empty_object(i)) {
                            call.args = input.to_string();
                        }
                    }
                    Some("text") => {
                        if let Some(s) = b.get("text").and_then(Value::as_str) {
                            self.push_text(s);
                        }
                    }
                    _ => {}
                }
            }
            "content_block_delta" => {
                let Some(d) = v.get("delta") else { return };
                match d.get("type").and_then(Value::as_str) {
                    Some("input_json_delta") => {
                        if let Some(p) = d.get("partial_json").and_then(Value::as_str) {
                            self.push_args(&key, p);
                        }
                    }
                    Some("text_delta") => {
                        if let Some(s) = d.get("text").and_then(Value::as_str) {
                            self.push_text(s);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn absorb_responses_output(&mut self, v: &Value) {
        let Some(Value::Array(items)) = v.get("output") else {
            return;
        };
        for (i, item) in items.iter().enumerate() {
            match item.get("type").and_then(Value::as_str) {
                Some("function_call") => {
                    let key = format!("o{i}");
                    let call = self.call(&key);
                    call.id = item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    call.name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    call.args = item
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                }
                Some("message") => {
                    if let Some(Value::Array(parts)) = item.get("content") {
                        for p in parts {
                            if let Some(s) = p.get("text").and_then(Value::as_str) {
                                self.push_text(s);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn absorb_responses_event(&mut self, kind: &str, v: &Value) {
        let item_key = |v: &Value| -> String {
            v.get("item_id")
                .and_then(Value::as_str)
                .map(|s| format!("i{s}"))
                .or_else(|| {
                    v.get("output_index")
                        .and_then(Value::as_u64)
                        .map(|n| format!("o{n}"))
                })
                .unwrap_or_else(|| "o0".into())
        };
        match kind {
            "response.output_item.added" | "response.output_item.done" => {
                let Some(item) = v.get("item") else { return };
                if item.get("type").and_then(Value::as_str) != Some("function_call") {
                    return;
                }
                // The item id keys the call: argument deltas name the item, not the index.
                let key = item
                    .get("id")
                    .and_then(Value::as_str)
                    .map(|s| format!("i{s}"))
                    .unwrap_or_else(|| item_key(v));
                let call = self.call(&key);
                if let Some(id) = item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                {
                    call.id = Some(id.to_string());
                }
                if let Some(n) = item.get("name").and_then(Value::as_str) {
                    call.name = n.to_string();
                }
                // The finished item carries the complete arguments: authoritative.
                if kind.ends_with(".done") {
                    if let Some(a) = item.get("arguments").and_then(Value::as_str) {
                        self.call(&key).args = a.to_string();
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                if let Some(d) = v.get("delta").and_then(Value::as_str) {
                    let key = item_key(v);
                    self.push_args(&key, d);
                }
            }
            "response.output_text.delta" => {
                if let Some(d) = v.get("delta").and_then(Value::as_str) {
                    self.push_text(d);
                }
            }
            _ => {}
        }
    }
}

fn is_empty_object(v: &Value) -> bool {
    v.as_object().is_some_and(|o| o.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(sse: bool, chunks: &[&[u8]]) -> ToolResponse {
        let mut o = ToolObserver::new(sse);
        for c in chunks {
            o.feed(c);
        }
        o.finish()
    }

    #[test]
    fn chat_stream_chunks_split_anywhere() {
        let body = b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"type\":\"function\",\"function\":{\"name\":\"f\",\"arguments\":\"{\"}}]}}]}\n\ndata: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"a\\\":1}\"}}]}}]}\n\ndata: [DONE]\n\n";
        // Cut inside a line: the observer joins the pieces.
        let (a, b) = body.split_at(75);
        let r = read(true, &[a, b]);
        assert_eq!(r.calls.len(), 1);
        assert_eq!(r.calls[0].name, "f");
        assert_eq!(r.calls[0].id.as_deref(), Some("c1"));
        assert_eq!(r.calls[0].args, "{\"a\":1}");
        assert_eq!(r.malformed, None);
    }

    #[test]
    fn stream_without_function_or_name_is_malformed() {
        let r = read(
            true,
            &[b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\"}]}}]}\n\n"],
        );
        assert_eq!(r.malformed, Some("call_without_function"));
        let r = read(
            true,
            &[b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{}\"}}]}}]}\n\n"],
        );
        assert_eq!(r.malformed, Some("call_without_name"));
        let r = read(
            true,
            &[b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"ind\n\n"],
        );
        assert_eq!(r.malformed, Some("bad_chunk"));
        // A garbled plain-text chunk is not a tool-call problem.
        let r = read(true, &[b"data: {not json\n\n"]);
        assert_eq!(r.malformed, None);
    }

    #[test]
    fn upstream_error_body() {
        let r = read(
            false,
            &[b"{\"error\":{\"code\":400,\"message\":\"too long\",\"type\":\"exceed_context_size_error\",\"n_prompt_tokens\":1380,\"n_ctx\":512}}"],
        );
        let e = r.error.unwrap();
        assert_eq!(e.kind.as_deref(), Some("exceed_context_size_error"));
        assert_eq!((e.n_prompt_tokens, e.n_ctx), (Some(1380), Some(512)));
    }
}
