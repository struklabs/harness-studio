//! What a request says about tools: the declared tools and their schemas, `tool_choice`, the
//! tool calls already in the conversation and the tool results the client just sent back.
//!
//! One reader for the three wire formats the gateway serves: Chat Completions (`tools[].function`,
//! `tool_calls`, `role: tool`), Anthropic Messages (`tools[].input_schema`, `tool_use` /
//! `tool_result` blocks) and Responses (`tools[]` with a flat `name`, `function_call` /
//! `function_call_output` items).

use serde_json::Value;

/// A tool the request offers.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDecl {
    pub name: String,
    /// JSON Schema of the arguments (`parameters` / `input_schema`).
    pub schema: Option<Value>,
}

/// `tool_choice` reduced to what the diagnosis needs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ToolChoice {
    /// Absent or `auto`: the model decides.
    #[default]
    Auto,
    /// `none`: no call expected.
    Off,
    /// `required` (OpenAI) / `any` (Anthropic): some call is expected.
    Required,
    /// A named function: that call is expected.
    Named(String),
}

/// A tool call the assistant made earlier in the conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryCall {
    pub id: Option<String>,
    pub name: String,
    /// The arguments with object keys sorted (equal calls compare equal).
    pub args: String,
}

/// A tool result the client sent back after the last assistant turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResultFacts {
    /// `tool_call_id` / `tool_use_id` / `call_id`.
    pub call_id: Option<String>,
    /// Why it reads as a failure of the tool, `None` for a normal result.
    pub error: Option<ResultError>,
    /// Start of the result text (for the inspector).
    pub excerpt: String,
}

/// How a tool result was recognised as an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultError {
    /// The client marked it (`is_error: true`).
    Flagged,
    /// The result is a JSON object with an `error` member (or `success` / `ok` false).
    Json,
    /// The text starts like an error message (`Error:`, `No match found`...).
    Text,
}

impl ResultError {
    pub fn as_str(self) -> &'static str {
        match self {
            ResultError::Flagged => "is_error",
            ResultError::Json => "json_error",
            ResultError::Text => "text",
        }
    }
}

/// The tool-related content of one request.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolRequest {
    pub tools: Vec<ToolDecl>,
    pub choice: ToolChoice,
    /// Every earlier assistant tool call, oldest first.
    pub history: Vec<HistoryCall>,
    /// The tool results after the last assistant turn (the answer to the previous round).
    pub results: Vec<ToolResultFacts>,
    /// The conversation contains tool results at all (also older ones).
    pub has_tool_messages: bool,
}

/// Prefixes (lowercase, after trimming) that make a tool result text read as a failure. They
/// are anchored at the start: a file whose *content* mentions an error is a normal result.
const ERROR_PREFIXES: &[&str] = &[
    "error",
    "err:",
    "failed",
    "failure",
    "fatal",
    "exception",
    "traceback",
    "no match found",
    "no matches found for the old",
    "no such file",
    "enoent",
    "permission denied",
    "command not found",
    "unknown tool",
    "tool not found",
    "tool execution failed",
    "<tool_use_error>",
];

/// Classifies a tool result text (see [`ResultError`]).
pub fn result_error(text: &str, flagged: bool) -> Option<ResultError> {
    if flagged {
        return Some(ResultError::Flagged);
    }
    let t = text.trim_start();
    if t.starts_with('{') {
        if let Ok(Value::Object(o)) = serde_json::from_str::<Value>(t) {
            let failed = o.get("error").is_some_and(|e| !e.is_null() && e != false)
                || o.get("success") == Some(&Value::Bool(false))
                || o.get("ok") == Some(&Value::Bool(false));
            return failed.then_some(ResultError::Json);
        }
    }
    let head: String = t.chars().take(40).collect::<String>().to_lowercase();
    ERROR_PREFIXES
        .iter()
        .any(|p| head.starts_with(p))
        .then_some(ResultError::Text)
}

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| {
                p.get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| p.as_str().map(str::to_string))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn excerpt(text: &str) -> String {
    text.chars().take(200).collect()
}

/// `value` as text with object keys sorted, so equal calls compare equal whatever the order
/// the model wrote the keys in.
pub fn canonical(v: &Value) -> String {
    fn sorted(v: &Value) -> Value {
        match v {
            Value::Object(o) => {
                let mut keys: Vec<&String> = o.keys().collect();
                keys.sort();
                Value::Object(
                    keys.into_iter()
                        .map(|k| (k.clone(), sorted(&o[k])))
                        .collect(),
                )
            }
            Value::Array(a) => Value::Array(a.iter().map(sorted).collect()),
            other => other.clone(),
        }
    }
    sorted(v).to_string()
}

/// Canonical form of an arguments string (`{}` for empty; the trimmed text when it is not JSON).
pub fn canonical_args(args: &str) -> String {
    let t = args.trim();
    if t.is_empty() {
        return "{}".into();
    }
    serde_json::from_str::<Value>(t)
        .map(|v| canonical(&v))
        .unwrap_or_else(|_| t.to_string())
}

fn declared(tools: &[Value]) -> Vec<ToolDecl> {
    tools
        .iter()
        .filter_map(|t| {
            // Chat: {type: function, function: {name, parameters}}. Responses: {type: function,
            // name, parameters}. Anthropic: {name, input_schema} (server tools carry a dated type
            // and no schema; they are not function tools).
            let f = t.get("function").unwrap_or(t);
            let name = f.get("name").and_then(Value::as_str)?;
            let schema = f
                .get("parameters")
                .or_else(|| f.get("input_schema"))
                .or_else(|| t.get("input_schema"))
                .cloned();
            Some(ToolDecl {
                name: name.to_string(),
                schema,
            })
        })
        .collect()
}

fn choice_of(v: Option<&Value>) -> ToolChoice {
    match v {
        Some(Value::String(s)) => match s.as_str() {
            "required" | "any" => ToolChoice::Required,
            "none" => ToolChoice::Off,
            _ => ToolChoice::Auto,
        },
        Some(Value::Object(o)) => {
            let name = o
                .get("function")
                .and_then(|f| f.get("name"))
                .or_else(|| o.get("name"))
                .and_then(Value::as_str);
            match (o.get("type").and_then(Value::as_str), name) {
                (_, Some(n)) => ToolChoice::Named(n.to_string()),
                (Some("any" | "required"), None) => ToolChoice::Required,
                (Some("none"), None) => ToolChoice::Off,
                _ => ToolChoice::Auto,
            }
        }
        _ => ToolChoice::Auto,
    }
}

impl ToolRequest {
    /// Reads a request body. `None` when it is not a JSON object.
    pub fn parse(body: &[u8]) -> Option<Self> {
        let v: Value = serde_json::from_slice(body).ok()?;
        Some(Self::from_value(&v))
    }

    pub fn from_value(v: &Value) -> Self {
        let mut out = ToolRequest {
            tools: v
                .get("tools")
                .and_then(Value::as_array)
                .map(|t| declared(t))
                .unwrap_or_default(),
            choice: choice_of(v.get("tool_choice")),
            ..Default::default()
        };
        let items = v.get("messages").or_else(|| v.get("input"));
        if let Some(Value::Array(items)) = items {
            for item in items {
                out.read_item(item);
            }
        }
        out
    }

    /// Declared tool names, in request order.
    pub fn tool_names(&self) -> Vec<&str> {
        self.tools.iter().map(|t| t.name.as_str()).collect()
    }

    pub fn tool(&self, name: &str) -> Option<&ToolDecl> {
        self.tools.iter().find(|t| t.name == name)
    }

    /// The name of the earlier call a result answers.
    pub fn call_name(&self, id: &str) -> Option<&str> {
        self.history
            .iter()
            .find(|c| c.id.as_deref() == Some(id))
            .map(|c| c.name.as_str())
    }

    fn call(&mut self, id: Option<&Value>, name: &str, args: Option<&Value>) {
        let args = match args {
            Some(Value::String(s)) => canonical_args(s),
            Some(other) => canonical(other),
            None => "{}".into(),
        };
        self.history.push(HistoryCall {
            id: id.and_then(Value::as_str).map(str::to_string),
            name: name.to_string(),
            args,
        });
        // A new assistant turn: the results before it were answered already.
        self.results.clear();
    }

    fn result(&mut self, id: Option<&Value>, text: String, flagged: bool) {
        self.has_tool_messages = true;
        self.results.push(ToolResultFacts {
            call_id: id.and_then(Value::as_str).map(str::to_string),
            error: result_error(&text, flagged),
            excerpt: excerpt(&text),
        });
    }

    fn read_item(&mut self, m: &Value) {
        // Responses items carry `type`, chat/Anthropic messages carry `role`.
        match m.get("type").and_then(Value::as_str) {
            Some("function_call") => {
                let name = m.get("name").and_then(Value::as_str).unwrap_or("");
                self.call(
                    m.get("call_id").or_else(|| m.get("id")),
                    name,
                    m.get("arguments"),
                );
                return;
            }
            Some("function_call_output") => {
                let text = m.get("output").map(text_of).unwrap_or_default();
                self.result(m.get("call_id"), text, false);
                return;
            }
            _ => {}
        }
        match m.get("role").and_then(Value::as_str) {
            Some("assistant") => {
                let mut any = false;
                if let Some(Value::Array(calls)) = m.get("tool_calls") {
                    for c in calls {
                        let f = c.get("function").unwrap_or(c);
                        let name = f.get("name").and_then(Value::as_str).unwrap_or("");
                        self.call(c.get("id"), name, f.get("arguments"));
                        any = true;
                    }
                }
                if let Some(Value::Array(blocks)) = m.get("content") {
                    for b in blocks {
                        if b.get("type").and_then(Value::as_str) == Some("tool_use") {
                            let name = b.get("name").and_then(Value::as_str).unwrap_or("");
                            self.call(b.get("id"), name, b.get("input"));
                            any = true;
                        }
                    }
                }
                if !any {
                    self.results.clear();
                }
            }
            Some("tool") => {
                let text = m.get("content").map(text_of).unwrap_or_default();
                self.result(m.get("tool_call_id"), text, false);
            }
            Some("user") => {
                let mut found = false;
                if let Some(Value::Array(blocks)) = m.get("content") {
                    for b in blocks {
                        if b.get("type").and_then(Value::as_str) == Some("tool_result") {
                            let text = b.get("content").map(text_of).unwrap_or_default();
                            let flagged = b.get("is_error").and_then(Value::as_bool) == Some(true);
                            self.result(b.get("tool_use_id"), text, flagged);
                            found = true;
                        }
                    }
                }
                if !found {
                    // A plain user turn: what came before was dealt with.
                    self.results.clear();
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn error_texts_are_anchored_at_the_start() {
        assert_eq!(
            result_error("Error: no such file", false),
            Some(ResultError::Text)
        );
        assert_eq!(
            result_error("  No match found for the old string", false),
            Some(ResultError::Text)
        );
        assert_eq!(
            result_error("ENOENT: no such file", false),
            Some(ResultError::Text)
        );
        assert_eq!(
            result_error("{\"error\": \"city not found\"}", false),
            Some(ResultError::Json)
        );
        assert_eq!(
            result_error("{\"success\": false}", false),
            Some(ResultError::Json)
        );
        assert_eq!(result_error("anything", true), Some(ResultError::Flagged));
        // A normal result that merely mentions an error is not one.
        assert_eq!(
            result_error("fn main() { /* Error: handled */ }", false),
            None
        );
        assert_eq!(
            result_error("{\"temp_c\": 21, \"sky\": \"clear\"}", false),
            None
        );
        assert_eq!(result_error("{\"error\": null, \"ok\": true}", false), None);
    }

    #[test]
    fn canonical_args_ignore_key_order_and_spacing() {
        assert_eq!(
            canonical_args("{\"b\": 1, \"a\": [2, 1]}"),
            canonical_args("{\"a\":[2,1],\"b\":1}")
        );
        assert_eq!(canonical_args(""), "{}");
        assert_eq!(canonical_args("{city"), "{city");
    }

    #[test]
    fn reads_the_three_wire_formats() {
        let chat = ToolRequest::from_value(&json!({
            "tools": [{"type": "function", "function": {"name": "a", "parameters": {"type": "object"}}}],
            "tool_choice": {"type": "function", "function": {"name": "a"}},
            "messages": [
                {"role": "user", "content": "x"},
                {"role": "assistant", "tool_calls": [{"id": "c1", "type": "function",
                    "function": {"name": "a", "arguments": "{\"q\": 1}"}}]},
                {"role": "tool", "tool_call_id": "c1", "content": "Error: boom"}]
        }));
        assert_eq!(chat.tool_names(), ["a"]);
        assert_eq!(chat.choice, ToolChoice::Named("a".into()));
        assert_eq!(chat.history.len(), 1);
        assert_eq!(chat.call_name("c1"), Some("a"));
        assert_eq!(chat.results.len(), 1);
        assert_eq!(chat.results[0].error, Some(ResultError::Text));

        let anthropic = ToolRequest::from_value(&json!({
            "tools": [{"name": "bash", "input_schema": {"type": "object"}},
                      {"type": "web_search_20250305", "name": "web_search"}],
            "tool_choice": {"type": "any"},
            "messages": [
                {"role": "user", "content": "x"},
                {"role": "assistant", "content": [{"type": "tool_use", "id": "t1", "name": "bash", "input": {"cmd": "ls"}}]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1",
                    "is_error": true, "content": [{"type": "text", "text": "denied"}]}]}]
        }));
        assert_eq!(anthropic.choice, ToolChoice::Required);
        assert_eq!(anthropic.history[0].args, "{\"cmd\":\"ls\"}");
        assert_eq!(anthropic.results[0].error, Some(ResultError::Flagged));
        assert_eq!(anthropic.results[0].excerpt, "denied");

        let responses = ToolRequest::from_value(&json!({
            "tools": [{"type": "function", "name": "a", "parameters": {"type": "object"}}],
            "input": [
                {"type": "message", "role": "user", "content": "x"},
                {"type": "function_call", "call_id": "k1", "name": "a", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "k1", "output": "fine"}]
        }));
        assert_eq!(responses.tool_names(), ["a"]);
        assert_eq!(responses.results.len(), 1);
        assert_eq!(responses.results[0].error, None);
        // `input` as a plain string has no messages.
        assert!(ToolRequest::from_value(&json!({"input": "hi"}))
            .history
            .is_empty());
    }

    #[test]
    fn only_the_trailing_results_count() {
        let r = ToolRequest::from_value(&json!({"messages": [
            {"role": "assistant", "tool_calls": [{"id": "c1", "function": {"name": "a", "arguments": "{}"}}]},
            {"role": "tool", "tool_call_id": "c1", "content": "Error: old"},
            {"role": "assistant", "tool_calls": [{"id": "c2", "function": {"name": "a", "arguments": "{}"}}]},
            {"role": "tool", "tool_call_id": "c2", "content": "fine"}]}));
        assert!(r.has_tool_messages);
        assert_eq!(r.results.len(), 1);
        assert_eq!(r.results[0].call_id.as_deref(), Some("c2"));
        // A new user turn after the results: nothing to attribute any more.
        let r = ToolRequest::from_value(&json!({"messages": [
            {"role": "assistant", "tool_calls": [{"id": "c1", "function": {"name": "a", "arguments": "{}"}}]},
            {"role": "tool", "tool_call_id": "c1", "content": "Error: old"},
            {"role": "assistant", "content": "done"},
            {"role": "user", "content": "thanks"}]}));
        assert!(r.has_tool_messages);
        assert!(r.results.is_empty());
    }
}
