//! Wire adaptation only: recognition is the core/Playground parser's responsibility.
use harness_core::tools::text_call::{
    recover_message, recover_text_calls, RecoveredCall, TextCallStreamBuffer,
};
use serde_json::{json, Value};

pub fn offered(body: &[u8]) -> Vec<String> {
    if serde_json::from_slice::<Value>(body)
        .ok()
        .is_some_and(|v| v["n"].as_u64().is_some_and(|n| n != 1))
    {
        return Vec::new();
    }
    harness_core::gateway::tool_diag::ToolRequest::parse(body)
        .map(|r| r.tools.into_iter().map(|t| t.name).collect())
        .unwrap_or_default()
}

pub fn recover_json(bytes: &[u8], names: &[String]) -> (Vec<u8>, Vec<RecoveredCall>) {
    let Ok(mut value) = serde_json::from_slice::<Value>(bytes) else {
        return (bytes.to_vec(), vec![]);
    };
    // Never manufacture calls from failed, truncated, or native responses.
    if value.get("error").is_some()
        || value["choices"][0]["finish_reason"] != "stop"
        || value["choices"].as_array().is_none_or(|c| c.len() != 1)
    {
        return (bytes.to_vec(), vec![]);
    }
    let message = &mut value["choices"][0]["message"];
    if message["tool_calls"]
        .as_array()
        .is_some_and(|c| !c.is_empty())
    {
        return (bytes.to_vec(), vec![]);
    }
    recover_message(message, names);
    let calls = calls_from_message(message);
    if calls.is_empty() {
        return (bytes.to_vec(), calls);
    }
    if message["content"].as_str().is_some_and(str::is_empty) {
        message["content"] = Value::Null;
    }
    value["choices"][0]["finish_reason"] = json!("tool_calls");
    (serde_json::to_vec(&value).unwrap(), calls)
}

fn calls_from_message(message: &Value) -> Vec<RecoveredCall> {
    message["tool_calls"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| RecoveredCall {
            id: c["id"].as_str().unwrap_or_default().into(),
            name: c["function"]["name"].as_str().unwrap_or_default().into(),
            arguments: c["function"]["arguments"]
                .as_str()
                .unwrap_or_default()
                .into(),
        })
        .collect()
}

/// SSE framing is independent of TCP/UTF-8 chunk boundaries. Only content after a possible
/// marker is held; full text is retained for the parser's conservative weak-format rules.
pub struct RecoveryStream {
    names: Vec<String>,
    wire: Vec<u8>,
    filter: TextCallStreamBuffer,
    text: String,
    visible: String,
    trailing_space: String,
    ended: bool,
    envelope: Value,
    pub calls: Vec<RecoveredCall>,
}

impl RecoveryStream {
    pub fn new(names: Vec<String>) -> Self {
        Self {
            names,
            wire: vec![],
            filter: TextCallStreamBuffer::default(),
            text: String::new(),
            visible: String::new(),
            trailing_space: String::new(),
            ended: false,
            envelope: Value::Null,
            calls: vec![],
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.wire.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some((end, separator)) = boundary(&self.wire) {
            let frame: Vec<u8> = self.wire.drain(..end + separator).collect();
            let data = std::str::from_utf8(&frame).ok().map(|s| {
                s.lines()
                    .filter_map(|l| l.strip_prefix("data:").map(str::trim_start))
                    .collect::<Vec<_>>()
                    .join("\n")
            });
            if self.ended {
                out.extend(frame);
                continue;
            }
            if data.as_deref() == Some("[DONE]") {
                // Missing finish event: flush verbatim, never recover an incomplete stream.
                out.extend(self.flush(false));
                self.ended = true;
                out.extend(frame);
                continue;
            }
            let has_data = data.as_ref().is_some_and(|s| !s.is_empty());
            let Some(mut v) = data.and_then(|s| serde_json::from_str::<Value>(&s).ok()) else {
                if has_data {
                    out.extend(self.flush(false));
                    self.ended = true;
                }
                out.extend(frame);
                continue;
            };
            if v.get("error").is_some() {
                out.extend(self.flush(false));
                self.ended = true;
                out.extend(frame);
                continue;
            }
            let Some(choices) = v["choices"].as_array() else {
                out.extend(frame);
                continue;
            };
            if choices.is_empty() {
                out.extend(frame);
                continue;
            } // usage
            if choices.len() != 1 || v["choices"][0]["index"].as_u64().unwrap_or(0) != 0 {
                out.extend(self.flush(false));
                self.ended = true;
                out.extend(frame);
                continue;
            }
            self.envelope = v.clone();
            if v["choices"][0]["delta"]["tool_calls"]
                .as_array()
                .is_some_and(|c| !c.is_empty())
            {
                out.extend(self.flush(false));
                self.ended = true;
                out.extend(frame);
                continue;
            }
            let mut changed = false;
            if let Some(content) = v["choices"][0]["delta"]["content"].as_str() {
                if self.text.len() + content.len() > 1024 * 1024 {
                    out.extend(self.flush(false));
                    self.ended = true;
                    out.extend(frame);
                    continue;
                }
                self.text.push_str(content);
                let filtered = self.filter.push(content);
                let combined = format!("{}{}", self.trailing_space, filtered);
                let visible = combined.trim_end().to_string();
                self.trailing_space = combined[visible.len()..].to_string();
                changed = visible != content;
                self.visible.push_str(&visible);
                v["choices"][0]["delta"]["content"] = json!(visible);
            }
            if let Some(reason) = v["choices"][0]["finish_reason"]
                .as_str()
                .map(str::to_string)
            {
                // A provider may put the final text and finish reason in the same event.
                if v["choices"][0]["delta"]["content"].is_string() {
                    let mut content_event = v.clone();
                    content_event["choices"][0]["finish_reason"] = Value::Null;
                    content_event.as_object_mut().unwrap().remove("usage");
                    out.extend(encode(&content_event));
                    v["choices"][0]["delta"]
                        .as_object_mut()
                        .unwrap()
                        .remove("content");
                    changed = true;
                }
                out.extend(self.flush(reason == "stop"));
                if !self.calls.is_empty() {
                    v["choices"][0]["finish_reason"] = json!("tool_calls");
                    changed = true;
                }
                self.ended = true;
            }
            out.extend(if changed { encode(&v) } else { frame });
        }
        out
    }

    fn chunk(&self, delta: Value) -> Vec<u8> {
        let mut v = self.envelope.clone();
        v["choices"] = json!([{"index":0, "delta":delta, "finish_reason":null}]);
        v.as_object_mut().unwrap().remove("usage");
        encode(&v)
    }

    fn flush(&mut self, recover: bool) -> Vec<u8> {
        let pending = self.filter.drain();
        if pending.is_empty() && self.trailing_space.is_empty() {
            return vec![];
        }
        let mut out = Vec::new();
        let recovery = recover.then(|| recover_text_calls(&self.text, &self.names));
        let rest = if let Some(r) = recovery.filter(|r| !r.tool_calls.is_empty()) {
            self.calls = r.tool_calls;
            // Boundary whitespace is deferred so recovered content keeps the core trim behavior.
            r.content
                .strip_prefix(self.visible.trim_start())
                .or_else(|| r.content.strip_prefix(self.visible.trim()))
                .unwrap_or(&r.content)
                .to_string()
        } else {
            format!("{}{}", self.trailing_space, pending)
        };
        self.trailing_space.clear();
        if !rest.is_empty() {
            out.extend(self.chunk(json!({"content":rest})));
        }
        for (index, c) in self.calls.iter().enumerate() {
            out.extend(self.chunk(
                json!({"tool_calls":[{"index":index,"id":c.id,"type":"function",
                "function":{"name":c.name,"arguments":c.arguments}}]}),
            ));
        }
        out
    }

    pub fn finish(&mut self) -> Vec<u8> {
        let mut out = if self.envelope.is_object() {
            self.flush(false)
        } else {
            vec![]
        };
        out.append(&mut self.wire);
        out
    }
}

fn boundary(bytes: &[u8]) -> Option<(usize, usize)> {
    bytes
        .windows(2)
        .position(|w| w == b"\n\n")
        .map(|i| (i, 2))
        .into_iter()
        .chain(
            bytes
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|i| (i, 4)),
        )
        .min_by_key(|(i, _)| *i)
}
fn encode(value: &Value) -> Vec<u8> {
    format!("data: {value}\n\n").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    const CALL: &str = r#"<tools>{"name":"read","arguments":{"path":"README.md"}}</tools>"#;
    fn names() -> Vec<String> {
        vec!["read".into()]
    }
    fn response(text: &str) -> Value {
        json!({"id":"chatcmpl-test","choices":[{"index":0,"message":{"role":"assistant","content":text},"finish_reason":"stop"}],"usage":{"completion_tokens":12}})
    }
    fn event(delta: Value, finish: Value) -> Vec<u8> {
        encode(
            &json!({"id":"chatcmpl-test","object":"chat.completion.chunk","model":"test","created":1,
            "choices":[{"index":0,"delta":delta,"finish_reason":finish}]}),
        )
    }
    fn events(out: &[u8]) -> Vec<Value> {
        String::from_utf8_lossy(out)
            .lines()
            .filter_map(|l| {
                l.strip_prefix("data:")
                    .and_then(|s| serde_json::from_str(s.trim()).ok())
            })
            .collect()
    }
    fn content(out: &[u8]) -> String {
        events(out)
            .iter()
            .filter_map(|v| v["choices"][0]["delta"]["content"].as_str())
            .collect()
    }
    fn stream(text: &str, finish: &str) -> (RecoveryStream, Vec<u8>) {
        let mut r = RecoveryStream::new(names());
        let mut input = event(json!({"role":"assistant"}), Value::Null);
        for c in text.chars() {
            input.extend(event(json!({"content":c.to_string()}), Value::Null));
        }
        input.extend(event(json!({}), json!(finish)));
        input.extend(encode(
            &json!({"choices":[],"usage":{"completion_tokens":12}}),
        ));
        input.extend(b"data: [DONE]\n\n");
        let mut out = Vec::new();
        for byte in input {
            out.extend(r.push(&[byte]));
        }
        out.extend(r.finish());
        (r, out)
    }

    #[test]
    fn stream_chunks_keep_logprobs_when_text_is_plain() {
        let mut r = RecoveryStream::new(names());
        let mut input = Vec::new();
        for (i, token) in ["Hel", "lo"].iter().enumerate() {
            input.extend(encode(&json!({"id":"c","choices":[{"index":0,
                "delta":{"content":token},"finish_reason":null,
                "logprobs":{"content":[{"token":token,"logprob":-0.25 * (i as f64 + 1.0),
                    "top_logprobs":[{"token":token,"logprob":-0.25}]}]}}]})));
        }
        input.extend(event(json!({}), json!("stop")));
        input.extend(b"data: [DONE]\n\n");
        let mut out = r.push(&input);
        out.extend(r.finish());
        let chunks: Vec<Value> = events(&out)
            .into_iter()
            .filter(|v| v["choices"][0]["logprobs"].is_object())
            .collect();
        assert_eq!(chunks.len(), 2);
        assert_eq!(
            chunks[0]["choices"][0]["logprobs"]["content"][0]["token"],
            "Hel"
        );
        assert_eq!(
            chunks[1]["choices"][0]["logprobs"]["content"][0]["top_logprobs"][0]["logprob"],
            -0.25
        );
    }

    #[test]
    fn review_newline_intro_has_exact_json_sse_parity() {
        let text = format!("Arıyorum.\n{CALL}");
        let (r, out) = stream(&text, "stop");
        let (bytes, calls) = recover_json(&serde_json::to_vec(&response(&text)).unwrap(), &names());
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(r.calls.len(), calls.len());
        assert_eq!(content(&out), "Arıyorum.");
        assert_eq!(value["choices"][0]["message"]["content"], content(&out));
    }

    #[test]
    fn review_quote_sse_releases_non_call_chunks() {
        let mut r = RecoveryStream::new(names());
        for (chunk, expected) in [
            (">", ""),
            (" Not: ", "> Not:"),
            ("[1]", " [1]"),
            (" Bu normal alıntı.", " Bu normal alıntı."),
        ] {
            assert_eq!(
                content(&r.push(&event(json!({"content":chunk}), Value::Null))),
                expected
            );
        }
        assert_eq!(content(&r.push(&event(json!({}), json!("stop")))), "");
    }

    #[test]
    fn prefixed_json_and_prose_examples_have_json_sse_parity() {
        // The introduction limit comes from the shared Rust/TS marker manifest.
        let markers: Value =
            serde_json::from_str(include_str!("../../../shared/text-tool-call-markers.json"))
                .unwrap();
        let intro_max = markers["introMaxChars"].as_u64().unwrap() as usize;
        let call = r#"{"name":"read","arguments":{"path":"README.md"}}"#;
        for (text, count) in [
            (format!(">{call}"), 1),
            (format!(" \n> {call} \n"), 1),
            (format!("> ```json\n{call}\n```"), 1),
            (format!("Örnek: {call}"), 1),
            (format!("[örnek] {call}"), 1),
            (format!("Örnek:\n```json\n{call}\n```"), 1),
            (format!(">>{call}"), 0),
            (format!("> {}\n{call}", "ş".repeat(intro_max)), 1),
            (format!("> {}\n{call}", "ş".repeat(intro_max + 1)), 0),
            (format!("> Bir\niki\n{call}"), 0),
            (format!("Arıyorum.\n{call}"), 1),
            (format!("Arıyorum.\n```json\n{call}\n```"), 1),
            (format!("> Örnek: {call}"), 1),
            (
                format!("İşte örnek:\n```json\n{call}\n```\nBu şekilde çağırabilirsiniz."),
                0,
            ),
            (format!("{}\n{call}", "ş".repeat(intro_max)), 1),
            (format!("{}\n{call}", "ş".repeat(intro_max + 1)), 0),
            (format!("Bir\niki\n{call}"), 0),
            (format!("{call} Done."), 0),
            (format!("{call}>"), 0),
            (format!("> Örnek: {call} açıklama"), 0),
        ] {
            let (bytes, calls) =
                recover_json(&serde_json::to_vec(&response(&text)).unwrap(), &names());
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            let (streamed, out) = stream(&text, "stop");
            assert_eq!(calls.len(), count, "{text}");
            assert_eq!(streamed.calls.len(), count, "{text}");
            if count == 0 {
                assert_eq!(content(&out), text);
                assert_eq!(value["choices"][0]["message"]["content"], text);
            } else {
                let expected = recover_text_calls(&text, &names()).content;
                assert_eq!(content(&out), expected);
                let actual = &value["choices"][0]["message"]["content"];
                if expected.is_empty() {
                    assert!(actual.is_null());
                } else {
                    assert_eq!(actual, &expected);
                }
                assert_eq!(streamed.calls[0].arguments, calls[0].arguments);
            }
        }
    }

    #[test]
    fn json_recovers_multiple_and_preserves_explanation_and_usage() {
        let (out, calls) = recover_json(
            &serde_json::to_vec(&response(&format!("Önce {CALL}{CALL} sonra"))).unwrap(),
            &names(),
        );
        let out: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(calls.len(), 2);
        assert_ne!(calls[0].id, calls[1].id);
        assert_eq!(out["choices"][0]["message"]["content"], "Önce  sonra");
        assert_eq!(out["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(out["usage"]["completion_tokens"], 12);
        let (out, _) = recover_json(&serde_json::to_vec(&response(CALL)).unwrap(), &names());
        assert!(
            serde_json::from_slice::<Value>(&out).unwrap()["choices"][0]["message"]["content"]
                .is_null()
        );
    }

    #[test]
    fn json_native_truncated_unknown_schema_and_examples_are_unchanged() {
        let mut native = response(CALL);
        native["choices"][0]["message"]["tool_calls"] = json!([{"id":"native"}]);
        let mut truncated = response(CALL);
        truncated["choices"][0]["finish_reason"] = json!("length");
        for v in [
            native,
            truncated,
            response(r#"<tools>{"name":"missing","arguments":{}}</tools>"#),
            response(
                r#"<tools>{"name":"read","parameters":{"type":"object","properties":{}}}</tools>"#,
            ),
            response(
                "Example:\n```json\n{\"name\":\"read\",\"arguments\":{}}\n```\nUse this format.",
            ),
        ] {
            let bytes = serde_json::to_vec(&v).unwrap();
            let (out, calls) = recover_json(&bytes, &names());
            assert!(calls.is_empty());
            assert_eq!(out, bytes);
        }
        let bytes = serde_json::to_vec(&response(CALL)).unwrap();
        assert_eq!(recover_json(&bytes, &[]).0, bytes);
    }

    #[test]
    fn sse_split_markers_utf8_multiple_calls_and_terminal_order() {
        let (r, out) = stream(&format!("Önce {CALL}{CALL} sonra"), "stop");
        assert_eq!(r.calls.len(), 2);
        assert_eq!(content(&out), "Önce  sonra");
        let events = events(&out);
        let calls: Vec<_> = events
            .iter()
            .filter(|v| v["choices"][0]["delta"]["tool_calls"].is_array())
            .collect();
        assert_eq!(calls.len(), 2);
        for (i, v) in calls.iter().enumerate() {
            let c = &v["choices"][0]["delta"]["tool_calls"][0];
            assert_eq!(c["index"], i);
            assert_eq!(c["type"], "function");
            assert_eq!(c["function"]["name"], "read");
            assert!(c["id"].as_str().unwrap().starts_with("call_"));
            assert_eq!(
                serde_json::from_str::<Value>(c["function"]["arguments"].as_str().unwrap())
                    .unwrap()["path"],
                "README.md"
            );
        }
        assert_eq!(
            events[events.len() - 2]["choices"][0]["finish_reason"],
            "tool_calls"
        );
        assert_eq!(events.last().unwrap()["usage"]["completion_tokens"], 12);
        assert!(out.ends_with(b"data: [DONE]\n\n"));
    }

    #[test]
    fn sse_weak_formats_only_calls_and_examples_or_length_flush_exact_text() {
        for text in [
            "```xml\n{\"name\":\"read\",\"arguments\":{}}\n```",
            r#"{"name":"read","arguments":{}}"#,
        ] {
            assert_eq!(stream(text, "stop").0.calls.len(), 1);
        }
        for (text, reason) in [
            (CALL, "length"),
            ("plain text \n ", "stop"),
            (" \n> Not: [1] normal quotation. \n ", "stop"),
            (
                "Example:\n```json\n{\"name\":\"read\",\"arguments\":{}}\n```\nUse this.",
                "stop",
            ),
            (
                "<tools>{\"name\":\"missing\",\"arguments\":{}}</tools>",
                "stop",
            ),
            ("<tools>{", "stop"),
        ] {
            let (r, out) = stream(text, reason);
            assert!(r.calls.is_empty());
            assert_eq!(content(&out), text);
        }
    }

    #[test]
    fn sse_native_flushes_pending_and_ordinary_text_is_immediate() {
        let mut r = RecoveryStream::new(names());
        let plain = event(json!({"content":"normal prose"}), Value::Null);
        assert_eq!(r.push(&plain), plain);
        let mut out = r.push(&event(json!({"content":CALL}), Value::Null));
        let native = event(
            json!({"tool_calls":[{"index":0,"id":"native","type":"function","function":{"name":"read","arguments":"{}"}}]}),
            Value::Null,
        );
        out.extend(r.push(&native));
        out.extend(r.push(&event(json!({}), json!("tool_calls"))));
        assert_eq!(content(&out), CALL);
        assert!(r.calls.is_empty());
        assert_eq!(
            events(&out)
                .iter()
                .filter(|v| v["choices"][0]["delta"]["tool_calls"].is_array())
                .count(),
            1
        );
    }

    #[test]
    fn sse_same_event_content_finish_and_missing_finish() {
        let mut r = RecoveryStream::new(names());
        let out = r.push(&event(
            json!({"content":format!("Önce {CALL}")}),
            json!("stop"),
        ));
        assert_eq!(r.calls.len(), 1);
        assert_eq!(content(&out), "Önce");
        let mut r = RecoveryStream::new(names());
        let mut out = r.push(&event(json!({"content":CALL}), Value::Null));
        out.extend(r.push(b"data: [DONE]\n\n"));
        assert_eq!(content(&out), CALL);
        assert!(r.calls.is_empty());
    }
}
