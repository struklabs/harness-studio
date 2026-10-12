//! Conservative recovery of complete tool calls left in assistant content.
//! Shared by both core loops and the Playground's Tauri adapter. No execution or policy here.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
pub struct RecoveredCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Serialize)]
pub struct Recovery {
    pub content: String,
    pub tool_calls: Vec<RecoveredCall>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Markers {
    strong: Vec<String>,
    fence: String,
    initial_json: Vec<String>,
    weak_prefixes: Vec<String>,
    intro_max_chars: usize,
}
static MARKERS: LazyLock<Markers> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../../../shared/text-tool-call-markers.json"
    ))
    .unwrap()
});

/// Incremental visibility filter; all markers come from the shared Rust/TS manifest.
#[derive(Default)]
pub struct TextCallStreamBuffer {
    pending: String,
    buffering: bool,
    started: bool,
    visible_intro: String,
}
impl TextCallStreamBuffer {
    pub fn push(&mut self, chunk: &str) -> String {
        self.pending.push_str(chunk);
        if self.buffering {
            return String::new();
        }
        let markers = MARKERS.strong.iter().chain(std::iter::once(&MARKERS.fence));
        let mut boundary = self.pending.len();
        for marker in markers.clone() {
            if let Some(index) = self.pending.find(marker) {
                boundary = boundary.min(index);
            }
        }
        for marker in &MARKERS.initial_json {
            for (index, _) in self.pending.match_indices(marker.as_str()) {
                let intro = format!("{}{}", self.visible_intro, &self.pending[..index]);
                if short_intro(&intro) && possible_json(&self.pending[index..]) {
                    boundary = boundary.min(index);
                }
            }
        }
        if !self.started {
            let initial = self.pending.trim_start();
            let prefixed = MARKERS
                .weak_prefixes
                .iter()
                .find_map(|p| initial.strip_prefix(p.as_str()));
            let candidate = prefixed.unwrap_or(initial).trim_start();
            if MARKERS
                .initial_json
                .iter()
                .any(|m| candidate.starts_with(m) && possible_json(candidate))
                || markers.clone().any(|m| candidate.starts_with(m))
            {
                boundary = 0;
            } else if candidate.is_empty() || markers.clone().any(|m| m.starts_with(candidate)) {
                // Hold leading whitespace and a partial opening until call intent is known.
                return String::new();
            }
        }
        if boundary < self.pending.len() {
            self.buffering = self.pending[boundary..].contains('{')
                || markers
                    .clone()
                    .any(|m| self.pending[boundary..].contains(m.as_str()));
        } else {
            for marker in markers {
                for length in 1..marker.len() {
                    if self.pending.ends_with(&marker[..length]) {
                        boundary = boundary.min(self.pending.len() - length);
                    }
                }
            }
        }
        let tail = self.pending.split_off(boundary);
        let visible = std::mem::replace(&mut self.pending, tail);
        if !visible.trim().is_empty() {
            self.started = true;
        }
        self.visible_intro.push_str(&visible);
        visible
    }
    pub fn drain(&mut self) -> String {
        std::mem::take(&mut self.pending)
    }
}

// Arrays of calls must start with an object (or another array), never a scalar.
fn possible_json(text: &str) -> bool {
    if let Some(tail) = text.strip_prefix('[') {
        let tail = tail.trim_start();
        tail.is_empty() || tail.starts_with(['{', '['])
    } else {
        text.starts_with('{')
    }
}

// A single manifest prefix does not count against the introduction limit.
fn short_intro(text: &str) -> bool {
    let initial = text.trim_start();
    let intro = MARKERS
        .weak_prefixes
        .iter()
        .find_map(|p| initial.strip_prefix(p.as_str()))
        .unwrap_or(initial)
        .trim();
    !intro.starts_with('>')
        && !intro.contains(['\r', '\n'])
        && intro.chars().count() <= MARKERS.intro_max_chars
}

/// Closing tag of the Hermes/Qwen call wrapper.
const CLOSE_TOOL_CALL: &str = "</tool_call>";

static FENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)```(?:[A-Za-z0-9_+-]+)?[ \t]*\r?\n(.*?)```").unwrap());

static WRAPPER: LazyLock<Regex> = LazyLock::new(|| {
    // Group 1 is the name of `<function=name>`; every other group is a body. qwen2.5-coder also
    // writes the plural and Anthropic-style tags, not only the `<tool_call>` of its template.
    Regex::new(concat!(
        r"(?s)<function=([^>\s]+)>\s*(.*?)\s*</function>",
        r"|<tool_call>\s*(.*?)\s*</tool_call>|<tool_calls>\s*(.*?)\s*(?:</tool_calls>|end_tool_calls\s*$|$)",
        r"|<tools>\s*(.*?)\s*</tools>|<function_calls>\s*(.*?)\s*</function_calls>",
        r"|<function>\s*(.*?)\s*</function>",
    ))
    .unwrap()
});

/// Returns the unrecognized part of a JSON value, preserving unknown/invalid calls as text.
fn extract(value: Value, names: &[String], calls: &mut Vec<RecoveredCall>) -> Option<Value> {
    if let Value::Array(values) = value {
        let rest: Vec<Value> = values
            .into_iter()
            .filter_map(|v| extract(v, names, calls))
            .collect();
        return (!rest.is_empty()).then_some(Value::Array(rest));
    }
    let f = value.get("function").unwrap_or(&value);
    let Some(name) = f
        .get("name")
        .or_else(|| f.get("function_name"))
        .or_else(|| f.get("tool_name"))
        .or_else(|| f.get("tool"))
        .and_then(Value::as_str)
        .filter(|n| names.iter().any(|x| x == n))
    else {
        return Some(value);
    };
    let args = f
        .get("arguments")
        .or_else(|| f.get("parameters"))
        .or_else(|| f.get("args"))
        .or_else(|| f.get("input"));
    let args = match args {
        Some(Value::String(s)) => serde_json::from_str::<Value>(s).ok(),
        Some(v) => Some(v.clone()),
        None => None,
    };
    // A tool definition echoed back (description + JSON-schema parameters) is not a call.
    let is_schema = f.get("description").is_some()
        || args
            .as_ref()
            .is_some_and(|a| a.get("properties").is_some() && a.get("type").is_some());
    let Some(args) = args.filter(|a| a.is_object() && !is_schema) else {
        return Some(value);
    };
    calls.push(RecoveredCall {
        id: format!("call_{}", uuid::Uuid::new_v4().simple()),
        name: name.to_string(),
        arguments: args.to_string(),
    });
    None
}

/// Complete JSON values at the start of `text`, with a byte offset (strings may contain braces).
fn prefix(text: &str) -> Vec<(usize, usize, Value)> {
    let mut out = Vec::new();
    let mut offset = 0;
    loop {
        let tail = &text[offset..];
        let start = offset + tail.len() - tail.trim_start().len();
        if !matches!(text.as_bytes().get(start), Some(b'{' | b'[')) {
            break;
        }
        let mut stream = serde_json::Deserializer::from_str(&text[start..]).into_iter::<Value>();
        let Some(Ok(value)) = stream.next() else {
            break;
        };
        offset = start + stream.byte_offset();
        out.push((start, offset, value));
    }
    out
}

/// Recover only offered names with object arguments. Callers must skip this when native
/// tool_calls exist, or a response failed, was cancelled or ended with a token limit.
pub fn recover_text_calls(text: &str, names: &[String]) -> Recovery {
    let mut result = Recovery {
        content: text.to_string(),
        tool_calls: Vec::new(),
    };
    if names.is_empty() {
        return result;
    }
    let mut edits: Vec<(usize, usize, String, Vec<RecoveredCall>)> = Vec::new();
    // XML fences carry explicit call intent. Remove the fence only when all
    // inner bytes were recovered; incomplete JSON remains visible text.
    for captures in FENCE.captures_iter(text) {
        let full = captures.get(0).unwrap();
        let body = captures.get(1).unwrap().as_str();
        if body.contains("<tool_calls>") {
            let recovered = recover_text_calls(body, names);
            if !recovered.tool_calls.is_empty() && recovered.content.is_empty() {
                edits.push((
                    full.start(),
                    full.end(),
                    String::new(),
                    recovered.tool_calls,
                ));
            }
        }
    }
    for (captures, weak) in WRAPPER
        .captures_iter(text)
        .map(|c| (c, false))
        .chain(FENCE.captures_iter(text).map(|c| (c, true)))
    {
        let full = captures.get(0).unwrap();
        if edits
            .iter()
            .any(|(a, b, _, _)| full.start() < *b && full.end() > *a)
        {
            continue;
        }
        let body = if weak {
            captures.get(1)
        } else {
            (2..captures.len()).find_map(|i| captures.get(i))
        }
        .unwrap();
        let values = prefix(body.as_str());
        // A wrapper must contain only complete JSON values. Never execute a truncated block.
        if values.is_empty() || !body.as_str()[values.last().unwrap().1..].trim().is_empty() {
            continue;
        }
        let mut calls = Vec::new();
        let mut rest = Vec::new();
        for (_, _, value) in values {
            let value = if let Some(name) = (!weak).then(|| captures.get(1)).flatten() {
                json!({"name": name.as_str(), "arguments": value})
            } else {
                value
            };
            if let Some(value) = extract(value, names, &mut calls) {
                rest.push(value.to_string())
            }
        }
        if !calls.is_empty() {
            let replacement = if rest.is_empty() {
                String::new()
            } else {
                // Keep the original wrapper around any unknown calls.
                format!(
                    "{}{}{}",
                    &text[full.start()..body.start()],
                    rest.join("\n"),
                    &text[body.end()..full.end()]
                )
            };
            edits.push((full.start(), full.end(), replacement, calls));
        }
    }
    // Tagged JSON has explicit call intent and can occur anywhere in prose.
    for marker in &MARKERS.strong[3..] {
        for (offset, _) in text.match_indices(marker.as_str()) {
            let base = offset + marker.len();
            for (start, end, value) in prefix(&text[base..]) {
                if edits
                    .iter()
                    .any(|(a, b, _, _)| base + start < *b && base + end > *a)
                {
                    continue;
                }
                let mut calls = Vec::new();
                let rest = extract(value, names, &mut calls);
                if !calls.is_empty() {
                    edits.push((
                        if start == text[base..].len() - text[base..].trim_start().len() {
                            offset
                        } else {
                            base + start
                        },
                        base + end,
                        rest.map(|v| v.to_string()).unwrap_or_default(),
                        calls,
                    ));
                }
            }
        }
    }
    let initial = text.trim_start();
    let weak_prefix = MARKERS
        .weak_prefixes
        .iter()
        .find(|p| initial.starts_with(p.as_str()));
    let base = text.len() - initial.len() + weak_prefix.map_or(0, |p| p.len());
    let mut offsets = vec![base];
    for (index, _) in text[base..].match_indices(['{', '[']) {
        let intro = &text[..base + index];
        // Never reinterpret JSON inside an unfinished wrapper or fence as a bare call.
        if short_intro(intro)
            && !MARKERS
                .strong
                .iter()
                .chain(std::iter::once(&MARKERS.fence))
                .any(|marker| intro.contains(marker.as_str()))
        {
            offsets.push(base + index);
        }
    }
    offsets.extend(edits.iter().map(|e| e.1));
    for offset in offsets {
        for (start, end, value) in prefix(&text[offset..]) {
            let start = offset + start;
            let mut end = offset + end;
            // A call whose opening `<tool_call>` was lost but whose closing tag ends the response
            // (seen with a q4_0 KV cache on CUDA): the tag belongs to the call.
            let tail = text[end..].trim_start();
            if let Some(after) = tail.strip_prefix(CLOSE_TOOL_CALL) {
                if after.trim().is_empty() {
                    end = text.len() - after.len();
                }
            }
            if edits.iter().any(|(a, b, _, _)| start < *b && end > *a) {
                continue;
            }
            let mut calls = Vec::new();
            let rest = extract(value, names, &mut calls);
            if !calls.is_empty() {
                edits.push((
                    start,
                    end,
                    rest.map(|v| v.to_string()).unwrap_or_default(),
                    calls,
                ));
            }
        }
    }
    // Weak formats allow a short leading introduction, never trailing prose.
    let first = edits.iter().map(|e| e.0).min().unwrap_or(text.len());
    let mut remainder = text.to_string();
    let mut ordered: Vec<_> = edits.iter().collect();
    ordered.sort_by_key(|e| e.0);
    for (start, end, replacement, _) in ordered.into_iter().rev() {
        remainder.replace_range(*start..*end, replacement);
    }
    let intro = text[..first].trim();
    let weak_allowed = remainder.trim() == intro && short_intro(intro);
    if !weak_allowed {
        edits.retain(|(start, _, _, _)| {
            MARKERS.strong.iter().any(|m| text[*start..].starts_with(m))
        });
    }
    edits.sort_by_key(|(start, _, _, _)| *start);
    for (start, end, replacement, _) in edits.iter().rev() {
        result.content.replace_range(*start..*end, replacement)
    }
    result.tool_calls = edits
        .into_iter()
        .flat_map(|(_, _, _, calls)| calls)
        .collect();
    if !result.tool_calls.is_empty() {
        result.content = result.content.trim().to_string();
        if weak_allowed && MARKERS.weak_prefixes.contains(&result.content) {
            result.content.clear();
        }
    }
    result
}

/// Normalize a non-streamed assistant message without touching native calls.
pub fn recover_message(message: &mut Value, names: &[String]) {
    if message["tool_calls"]
        .as_array()
        .is_some_and(|c| !c.is_empty())
    {
        return;
    }
    let recovery = recover_text_calls(message["content"].as_str().unwrap_or(""), names);
    if recovery.tool_calls.is_empty() {
        return;
    }
    message["content"] = json!(recovery.content);
    message["tool_calls"] = Value::Array(recovery.tool_calls.into_iter().map(|c| json!({
        "id": c.id, "type": "function", "function": {"name": c.name, "arguments": c.arguments}
    })).collect());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creator_aliases_with_introduction() {
        let intro = "İşte text_stats aracını kullanarak verdiğiniz metnin sözcük ve karakter sayısını hesaplamak üzere hazırlanmış, araç çağrısını içeren JSON nesnesi:";
        assert!(intro.chars().count() > 120);
        for name_key in ["name", "function_name", "tool_name", "tool"] {
            for args_key in ["arguments", "parameters", "args", "input"] {
                let call = json!({name_key: "text_stats", args_key: {"text": "Merhaba dünya"}})
                    .to_string();
                for body in [call.clone(), format!("```json\n{call}\n```")] {
                    let recovered =
                        recover_text_calls(&format!("{intro}\n{body}"), &["text_stats".into()]);
                    assert_eq!(recovered.tool_calls.len(), 1, "{body}");
                    assert_eq!(recovered.content, intro);
                }
                for args in [json!([]), json!({"type": "object", "properties": {}})] {
                    let call = json!({name_key: "text_stats", args_key: args}).to_string();
                    assert!(recover_text_calls(&call, &["text_stats".into()])
                        .tool_calls
                        .is_empty());
                }
                assert!(recover_text_calls(&call, &["other".into()])
                    .tool_calls
                    .is_empty());
            }
        }
    }

    #[test]
    fn plural_missing_close_and_xml_fence() {
        let call = r#"{"name":"web_search","arguments":{"query":"x"}}"#;
        for tail in ["end_tool_calls", "", "</tool_calls>"] {
            let text = format!("<tool_calls>{call}{tail}");
            for wrapped in [text.clone(), format!("```xml\n{text}\n```")] {
                let r = recover_text_calls(&wrapped, &["web_search".into()]);
                assert_eq!(r.tool_calls.len(), 1, "{wrapped}");
                assert!(r.content.is_empty(), "{}", r.content);
            }
        }
        assert!(
            recover_text_calls("<tool_calls>{\"name\":", &["web_search".into()])
                .tool_calls
                .is_empty()
        );
    }

    #[test]
    fn review_plain_quote_is_immediate() {
        let mut b = TextCallStreamBuffer::default();
        assert_eq!(b.push(">"), "");
        assert_eq!(b.push(" Not: "), "> Not: ");
        assert_eq!(b.push("[1]"), "[1]");
        assert_eq!(b.push(" Bu normal alıntı."), " Bu normal alıntı.");
        assert_eq!(b.drain(), "");
        for prefix in ["", "> "] {
            let mut b = TextCallStreamBuffer::default();
            assert_eq!(b.push(&format!("{prefix}[")), "");
            assert_eq!(b.push("1"), format!("{prefix}[1"));
            assert_eq!(b.push("] normal"), "] normal");
        }
    }
    #[test]
    fn review_bracket_intro_recovers() {
        let r = recover_text_calls(
            r#"[örnek] {"name":"web_search","arguments":{}}"#,
            &["web_search".into()],
        );
        assert_eq!(r.tool_calls.len(), 1);
        assert_eq!(r.content, "[örnek]");
    }
    #[test]
    fn supported_formats_and_string_arguments() {
        for text in [
            r#"<tool_call>{"name":"web_search","arguments":{"query":"a } [ b"}}</tool_call>"#,
            "```json\n{\"name\":\"web_search\",\"parameters\":{\"query\":\"x\"}}\n```",
            "```\n{\"name\":\"web_search\",\"arguments\":{}}\n```",
            // qwen2.5-coder-14b, 06.10: a JSON call fenced as XML, and a bare pretty-printed call.
            "```xml\n{\n  \"name\": \"web_search\",\n  \"arguments\": {\"query\": \"x\"}\n}\n```",
            "{\n  \"name\": \"web_search\",\n  \"arguments\": {\"query\": \"x\"}\n}",
            r#"{"function":{"name":"web_search","arguments":"{\"query\":\"x\"}"}}"#,
            r#"<function=web_search>{"query":"x"}</function>"#,
            r#"<tool_calls>{"name":"web_search","arguments":{"query":"x"}}</tool_calls>"#,
            r#"<function_calls>{"name":"web_search","arguments":{"query":"x"}}</function_calls>"#,
            r#"<function>{"name":"web_search","arguments":{"query":"x"}}</function>"#,
            r#"[TOOL_CALLS][{"name":"web_search","arguments":{}}]"#,
        ] {
            let r = recover_text_calls(text, &["web_search".into()]);
            assert_eq!(r.tool_calls.len(), 1, "{text}");
            assert!(r.content.is_empty(), "{}", r.content);
            assert!(serde_json::from_str::<Value>(&r.tool_calls[0].arguments)
                .unwrap()
                .is_object());
        }
    }

    #[test]
    fn multiple_calls_explanations_and_unknown_calls() {
        let r = recover_text_calls("Arıyorum.\n<tool_call>{\"name\":\"web_search\",\"arguments\":{}}</tool_call>\n<tool_call>{\"name\":\"web_search\",\"arguments\":{}}</tool_call>", &["web_search".into()]);
        assert_eq!(r.content, "Arıyorum.");
        assert_eq!(r.tool_calls.len(), 2);
        assert_ne!(r.tool_calls[0].id, r.tool_calls[1].id);
        let r = recover_text_calls(
            r#"[{"name":"unknown","arguments":{}},{"name":"web_search","arguments":{}}]"#,
            &["web_search".into()],
        );
        assert!(r.tool_calls.is_empty());
        assert!(r.content.contains("unknown"));
        let r = recover_text_calls(
            r#"{"name":"web_search","arguments":{}} {"name":"web_search","parameters":{}} Done."#,
            &["web_search".into()],
        );
        assert!(r.tool_calls.is_empty());
        assert!(r.content.ends_with("Done."));
        let r = recover_text_calls(
            r#"{"name":"web_search","arguments":{"query":"first"}} <tool_call>{"name":"web_search","arguments":{"query":"second"}}</tool_call>"#,
            &["web_search".into()],
        );
        assert_eq!(r.tool_calls.len(), 2);
        assert_eq!(
            serde_json::from_str::<Value>(&r.tool_calls[0].arguments).unwrap()["query"],
            "first"
        );
        assert_eq!(
            serde_json::from_str::<Value>(&r.tool_calls[1].arguments).unwrap()["query"],
            "second"
        );
    }

    #[test]
    fn call_missing_its_opening_tag_is_recovered_only_when_the_closing_tag_ends_it() {
        // Live (Qwen3-4B, q4_0/q4_0 KV on CUDA): the opening `<tool_call>` was lost.
        let names = ["calculator".to_string()];
        for text in [
            r#"{"name": "calculator", "arguments": {"expression": "221 / 13"}}</tool_call>"#,
            "Hesaplıyorum.\n{\"name\":\"calculator\",\"arguments\":{\"expression\":\"1\"}}\n</tool_call>\n",
        ] {
            let r = recover_text_calls(text, &names);
            assert_eq!(r.tool_calls.len(), 1, "{text}");
            assert_eq!(r.tool_calls[0].name, "calculator");
            assert!(!r.content.contains("</tool_call>"), "{}", r.content);
        }
        for text in [
            r#"{"name":"calculator","arguments":{"expression":"1"}}</tool_call> Sonuç 1."#,
            r#"{"name":"calculator","arguments":{"expression":"1"}}</tool_call></tool_call>"#,
            r#"{"name":"other","arguments":{}}</tool_call>"#,
        ] {
            let r = recover_text_calls(text, &names);
            assert!(r.tool_calls.is_empty(), "{text}");
            assert_eq!(r.content, text);
        }
    }

    #[test]
    fn ordinary_unknown_invalid_and_incomplete_json_stays_text() {
        for text in [
            r#"{"name":"other","arguments":{}}"#,
            r#"{"name":"web_search"}"#,
            r#"{"name":"web_search","arguments":[]}"#,
            r#"{"name":"web_search","arguments":"oops"}"#,
            r#"Example: {"name":"web_search","arguments":{}} trailing"#,
            r#"<tool_call>{"name":"web_search","arguments":{}}"#,
            r#"<tools>{"name":"web_search","arguments":{}}"#,
            r#"```{"name":"web_search","arguments":{}}"#,
            r#"<tool_call>{"name":"web_search","arguments":{}} broken</tool_call>"#,
            "```json\n{\"name\":\"web_search\",\"arguments\":{}",
            r#"{"name":"web_search","arguments":{"q":""#,
        ] {
            let r = recover_text_calls(text, &["web_search".into()]);
            assert!(r.tool_calls.is_empty(), "{text}");
            assert_eq!(r.content, text);
        }
        assert!(
            recover_text_calls(r#"{"name":"web_search","arguments":{}}"#, &[])
                .tool_calls
                .is_empty()
        );
    }

    #[test]
    fn weak_calls_require_a_call_only_response_or_short_leading_intro() {
        let call = r#"{"name":"web_search","arguments":{"query":"x"}}"#;
        let fence = format!("```xml\n{call}\n```");
        for text in [
            format!("İşte örnek:\n{fence}\nBu şekilde çağırabilirsiniz."),
            format!("{call} Done."),
            format!("{}\n{fence}", "ş".repeat(MARKERS.intro_max_chars + 1)),
            format!("Bir\niki\n{fence}"),
            format!(">> {call}"),
            format!("> {}\n{fence}", "ş".repeat(MARKERS.intro_max_chars + 1)),
            format!("> Bir\niki\n{fence}"),
            format!("> Örnek: {call} açıklama"),
            format!("{call}>"),
            format!("{fence}\n{call} trailing"),
        ] {
            let r = recover_text_calls(&text, &["web_search".into()]);
            assert!(r.tool_calls.is_empty(), "{text}");
            assert_eq!(r.content, text);
        }
        for text in [
            format!("> {fence}"),
            format!("Örnek: {call}"),
            format!("Örnek:\n{fence}"),
            format!("Arıyorum.\n{fence}"),
            format!("> Örnek: {call}"),
            format!("{}\n{fence}", "ş".repeat(120)),
            format!("> {}\n{fence}", "ş".repeat(120)),
            format!("{}\n{call}", "😀".repeat(120)),
            format!("{fence}\n{call}"),
            format!("Önce [TOOL_CALLS]{call} sonra"),
            format!("Önce <|python_tag|>{call} sonra"),
            format!("Önce <tools>{call}</tools> sonra"),
        ] {
            assert!(
                !recover_text_calls(&text, &["web_search".into()])
                    .tool_calls
                    .is_empty(),
                "{text}"
            );
        }
    }

    #[test]
    fn echoed_tool_definition_is_not_a_call() {
        let def = r#"{"type":"function","function":{"name":"web_search","description":"d","parameters":{"type":"object","properties":{"query":{"type":"string"}}}}}"#;
        let text = format!(
            "<tools>
{def}
</tools>"
        );
        let r = recover_text_calls(&text, &["web_search".into()]);
        assert!(r.tool_calls.is_empty());
        assert_eq!(r.content, text);
    }

    #[test]
    fn stream_visibility_and_lossless_fallback() {
        let mut b = TextCallStreamBuffer::default();
        assert_eq!(b.push("Merhaba "), "Merhaba ");
        assert_eq!(b.push("dünya"), "dünya");
        assert_eq!(b.push(" <too"), " ");
        assert_eq!(b.push("ls>broken"), "");
        assert_eq!(b.drain(), "<tools>broken");
        for marker in MARKERS.strong.iter().chain(std::iter::once(&MARKERS.fence)) {
            let mut b = TextCallStreamBuffer::default();
            let mut shown = String::new();
            for c in format!("Önce {marker}payload").chars() {
                shown.push_str(&b.push(&c.to_string()));
            }
            assert_eq!(shown, "Önce ");
            assert_eq!(b.drain(), format!("{marker}payload"));
        }
    }

    #[test]
    fn short_intro_streams_immediately_then_holds_calls() {
        for intro in ["Arıyorum.".to_string(), "ş".repeat(120), "😀".repeat(120)] {
            for opening in ["{", "```json\n"] {
                let mut buffer = TextCallStreamBuffer::default();
                assert_eq!(buffer.push(&intro), intro);
                assert_eq!(buffer.push(&format!("\n{opening}")), "\n");
                assert_eq!(buffer.push("payload"), "");
                assert_eq!(buffer.drain(), format!("{opening}payload"));
            }
        }
    }

    #[test]
    fn short_intro_stream_and_non_stream_match_at_every_character_boundary() {
        let call = r#"{"name":"web_search","arguments":{}}"#;
        let names = ["web_search".into()];
        for intro in ["Arıyorum.".to_string(), "ş".repeat(120), "😀".repeat(120)] {
            for body in [call.to_string(), format!("```json\n{call}\n```")] {
                let text = format!("{intro}\n{body}");
                for split in text
                    .char_indices()
                    .map(|(i, _)| i)
                    .chain(std::iter::once(text.len()))
                {
                    let mut buffer = TextCallStreamBuffer::default();
                    let mut visible = buffer.push(&text[..split]);
                    visible.push_str(&buffer.push(&text[split..]));
                    assert!(!visible.contains('{') && !visible.contains('`'));
                    visible.push_str(&buffer.drain());
                    assert_eq!(visible, text);
                    let recovered = recover_text_calls(&visible, &names);
                    assert_eq!(recovered.tool_calls.len(), 1);
                    assert_eq!(recovered.content, intro);
                }
            }
        }
    }

    #[test]
    fn prefixed_stream_matches_non_stream_and_fallback_is_lossless() {
        let chunks = [
            ">",
            "{\"name\"",
            ":\"web_search\",",
            "\"arguments\":{",
            "\"query\":\"x\"",
            "}}",
        ];
        let text = chunks.concat();
        let names = ["web_search".into()];
        let mut whitespace = TextCallStreamBuffer::default();
        assert_eq!(whitespace.push(" \n"), "");
        assert_eq!(whitespace.push("Merhaba"), " \nMerhaba");
        assert_eq!(whitespace.drain(), "");
        for split in 0..=text.len() {
            let mut buffer = TextCallStreamBuffer::default();
            assert_eq!(buffer.push(&text[..split]), "");
            assert_eq!(buffer.push(&text[split..]), "");
            let recovered = recover_text_calls(&buffer.drain(), &names);
            assert_eq!(recovered.tool_calls.len(), 1, "split {split}");
            assert_eq!(recovered.content, "");
        }
        for parts in [
            chunks.to_vec(),
            vec![text.as_str()],
            vec![" \n", ">", " \n", &text[1..]],
            vec![
                ">{\"name\"",
                ":\"web_search\",\"arguments\":{\"query\":\"x\"}}",
            ],
        ] {
            let expected = parts.concat();
            let mut buffer = TextCallStreamBuffer::default();
            for part in parts {
                assert_eq!(buffer.push(part), "");
            }
            let raw = buffer.drain();
            assert_eq!(raw, expected);
            let streamed = recover_text_calls(&raw, &names);
            let mut message = json!({"content": raw});
            recover_message(&mut message, &names);
            assert_eq!(streamed.content, "");
            assert_eq!(streamed.tool_calls.len(), 1);
            assert_eq!(message["content"], streamed.content);
            assert_eq!(
                message["tool_calls"][0]["function"]["arguments"],
                streamed.tool_calls[0].arguments
            );
        }
        for text in [
            ">",
            ">broken",
            ">{\"name\":",
            ">>{\"name\":\"web_search\",\"arguments\":{}}",
            "Örnek: {\"name\":\"web_search\",\"arguments\":{}} Done.",
            "Bir\niki\n```json\n{\"name\":\"web_search\",\"arguments\":{}}\n```",
        ] {
            let mut buffer = TextCallStreamBuffer::default();
            let mut visible = String::new();
            for c in text.chars() {
                visible.push_str(&buffer.push(&c.to_string()));
            }
            visible.push_str(&buffer.drain());
            assert_eq!(visible, text);
            let r = recover_text_calls(&visible, &names);
            assert!(r.tool_calls.is_empty());
            assert_eq!(r.content, text);
        }
        let r = recover_text_calls(&format!(" \n> \n{} \n", &text[1..]), &names);
        assert_eq!(r.tool_calls.len(), 1);
        assert_eq!(r.content, "");
    }

    /// Manual: HARNESS_LIVE_VARIANTS=path/to/variants.json cargo test -p harness-core live_variants -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_variants_report() {
        let path = std::env::var("HARNESS_LIVE_VARIANTS").expect("HARNESS_LIVE_VARIANTS");
        let rows: Vec<Value> =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for row in rows {
            let text = row["content"].as_str().unwrap_or_default();
            let r = recover_text_calls(text, &["web_search".into()]);
            println!(
                "#{} stream={} recovered={} head={:?}",
                row["i"],
                row["stream"],
                r.tool_calls.len(),
                text.chars().take(40).collect::<String>()
            );
        }
    }

    #[test]
    fn native_calls_take_precedence() {
        let mut m = json!({"content": "<tool_call>{\"name\":\"web_search\",\"arguments\":{}}</tool_call>", "tool_calls":[{"id":"native"}]});
        let original = m.clone();
        recover_message(&mut m, &["web_search".into()]);
        assert_eq!(m, original);
    }

    #[test]
    fn recorded_qwen_coder_response_is_recovered() {
        let v: Value = serde_json::from_str(include_str!(
            "../../tests/data/tooldiag/b11342-qwen2.5-coder-14b.chat.resp.json"
        ))
        .unwrap();
        let mut m = v["choices"][0]["message"].clone();
        recover_message(&mut m, &["get_weather".into()]);
        assert_eq!(m["content"], "");
        assert_eq!(m["tool_calls"][0]["function"]["name"], "get_weather");
        let args: Value = serde_json::from_str(
            m["tool_calls"][0]["function"]["arguments"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(args["city"], "Ankara");
    }
}
