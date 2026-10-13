//! Tool compatibility of the OpenAI Responses (Codex), Chat Completions and Anthropic
//! Messages APIs with llama-server. **No tool is dropped silently:**
//!
//! * `function` tools pass unchanged.
//! * Responses `namespace` tools (Codex groups each MCP server's tools in one) are flattened
//!   into plain functions named `<namespace>__<name>` (`mcp__github` + `search` →
//!   `mcp__github__search`, the name Codex itself uses when it flattens). Earlier calls in
//!   `input` and a `tool_choice` naming a namespaced function are flattened the same way,
//!   and calls in the response (JSON or SSE) are mapped back to `name` + `namespace`.
//! * Tools that cannot be expressed as functions without changing their meaning (hosted
//!   `web_search`, `file_search`, `image_generation`, `code_interpreter`, remote `mcp`,
//!   freeform `custom` grammar tools, Anthropic server tools...) reject the request with
//!   `400 unsupported_tool_type` naming the tool and the reason ([`Unsupported`]).
//! * Only with the user's explicit per-definition consent (`GatewaySettings::tool_ignores`,
//!   by type or name) is such a tool removed; the response then carries
//!   `X-Harness-Compat: ignored=<tool>` and the gateway logs it.

use serde_json::{Map, Value};
use std::collections::HashMap;

/// The inference APIs the gateway serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    Chat,
    Completions,
    Responses,
    Embeddings,
    Rerank,
    Messages,
    CountTokens,
}

impl Api {
    /// Path on llama-server (same as on the gateway).
    pub fn path(self) -> &'static str {
        match self {
            Api::Chat => "/v1/chat/completions",
            Api::Completions => "/v1/completions",
            Api::Responses => "/v1/responses",
            Api::Embeddings => "/v1/embeddings",
            Api::Rerank => "/v1/rerank",
            Api::Messages => "/v1/messages",
            Api::CountTokens => "/v1/messages/count_tokens",
        }
    }

    /// Errors in the Anthropic shape.
    pub fn anthropic(self) -> bool {
        matches!(self, Api::Messages | Api::CountTokens)
    }

    fn has_tools(self) -> bool {
        matches!(
            self,
            Api::Chat | Api::Responses | Api::Messages | Api::CountTokens
        )
    }
}

/// Flattened name → (namespace, original name).
pub type NameMap = HashMap<String, (String, String)>;

/// What [`prepare`] changed.
#[derive(Debug, Default)]
pub struct Prepared {
    /// Rewritten body (`None` = forward the original bytes).
    pub body: Option<Vec<u8>>,
    pub names: NameMap,
    /// Tools removed with the user's consent (`type` or `type:name`).
    pub ignored: Vec<String>,
}

impl Prepared {
    /// `X-Harness-Compat` value (empty = nothing changed).
    pub fn header(&self) -> String {
        let mut parts = Vec::new();
        if !self.names.is_empty() {
            parts.push(format!("namespace_flattened={}", self.names.len()));
        }
        if !self.ignored.is_empty() {
            parts.push(format!("ignored={}", self.ignored.join(",")));
        }
        parts.join("; ")
    }

    pub fn notes(&self) -> Vec<String> {
        self.header()
            .split("; ")
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }
}

/// A tool the local server cannot run.
#[derive(Debug, Clone, PartialEq)]
pub struct Unsupported {
    /// JSON path of the tool (`tools[2]`, `tools[0].tools[1]`, `tool_choice`, `mcp_servers`).
    pub param: String,
    pub tool_type: String,
    pub tool_name: Option<String>,
    pub reason: String,
}

impl Unsupported {
    pub fn message(&self) -> String {
        format!(
            "Tool {}{} ({}) is not supported by the local llama.cpp server: {}. Nothing was \
             dropped silently: remove the tool from the request, or allow Harness to ignore it \
             for this model (Settings > Gateway > Ignored tools; recorded in the X-Harness-Compat \
             response header).",
            self.tool_type,
            self.tool_name
                .as_deref()
                .map(|n| format!(" \"{}\"", n))
                .unwrap_or_default(),
            self.param,
            self.reason
        )
    }
}

fn reason_for(tool_type: &str) -> String {
    let t = tool_type;
    let r = if t.starts_with("web_search") {
        "hosted web search runs on the provider's servers; llama-server has no search backend"
    } else if t.starts_with("web_fetch") {
        "hosted web fetch runs on the provider's servers"
    } else if t == "file_search" {
        "hosted file search needs the provider's vector stores"
    } else if t.starts_with("code_interpreter") || t.starts_with("code_execution") {
        "hosted code execution runs in the provider's sandbox"
    } else if t == "image_generation" {
        "hosted image generation has no llama-server equivalent"
    } else if t.starts_with("computer") {
        "computer use needs the provider's hosted tool"
    } else if t == "mcp" || t == "mcp_servers" {
        "remote MCP tools are called by the provider's servers; add the MCP server to the client instead"
    } else if t == "local_shell" || t == "shell" {
        "the built-in shell tool type has no function-tool equivalent; the client must send it as a function tool"
    } else if t == "custom" || t == "apply_patch" {
        "freeform (grammar) tools cannot be turned into JSON-schema functions without changing what the model may output"
    } else if t == "tool_search" {
        "hosted tool search runs on the provider's servers"
    } else if t.starts_with("bash_") || t.starts_with("text_editor_") || t.starts_with("memory_") {
        "Anthropic-defined tools carry a schema built into Claude; send them as custom tools with an input_schema"
    } else {
        "unknown tool type; only function tools can be passed to llama-server"
    };
    r.to_string()
}

/// Joins a namespace and a function name the way Codex flattens MCP tools.
pub fn join_name(namespace: &str, name: &str) -> String {
    if namespace.ends_with("__") {
        format!("{}{}", namespace, name)
    } else {
        format!("{}__{}", namespace, name)
    }
}

fn is_ignored(ignore: &[String], tool_type: &str, name: Option<&str>) -> bool {
    ignore.iter().any(|i| {
        i.eq_ignore_ascii_case(tool_type)
            || name.is_some_and(|n| i == n)
            || (tool_type.starts_with("web_search") && i.eq_ignore_ascii_case("web_search"))
    })
}

fn label(tool_type: &str, name: Option<&str>) -> String {
    match name {
        Some(n) if n != tool_type => format!("{}:{}", tool_type, n),
        _ => tool_type.to_string(),
    }
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(|x| x.as_str())
}

/// Checks (and where needed rewrites) the tools of a request body.
pub fn prepare(api: Api, body: &[u8], ignore: &[String]) -> Result<Prepared, Unsupported> {
    if !api.has_tools() {
        return Ok(Prepared::default());
    }
    let mentions = |needle: &[u8]| body.windows(needle.len()).any(|w| w == needle);
    if !mentions(b"\"tools\"")
        && !mentions(b"\"tool_choice\"")
        && !mentions(b"\"namespace\"")
        && !mentions(b"\"mcp_servers\"")
    {
        return Ok(Prepared::default());
    }
    let Ok(mut doc) = serde_json::from_slice::<Value>(body) else {
        // Invalid JSON is llama-server's to reject.
        return Ok(Prepared::default());
    };
    let Some(obj) = doc.as_object_mut() else {
        return Ok(Prepared::default());
    };
    let mut out = Prepared::default();
    let changed = match api {
        Api::Responses => responses(obj, ignore, &mut out)?,
        Api::Chat => chat(obj, ignore, &mut out)?,
        Api::Messages | Api::CountTokens => messages(obj, ignore, &mut out)?,
        _ => false,
    };
    if changed {
        out.body = Some(serde_json::to_vec(&doc).unwrap_or_default());
    }
    Ok(out)
}

fn responses(
    obj: &mut Map<String, Value>,
    ignore: &[String],
    out: &mut Prepared,
) -> Result<bool, Unsupported> {
    let mut changed = false;
    if let Some(Value::Array(tools)) = obj.get_mut("tools") {
        let mut kept = Vec::with_capacity(tools.len());
        let mut top_names: Vec<String> = tools
            .iter()
            .filter(|t| str_of(t, "type").unwrap_or("function") == "function")
            .filter_map(|t| str_of(t, "name").map(str::to_string))
            .collect();
        for (i, tool) in std::mem::take(tools).into_iter().enumerate() {
            let ty = str_of(&tool, "type").unwrap_or("function").to_string();
            let name = str_of(&tool, "name").map(str::to_string);
            match ty.as_str() {
                "function" => {
                    let mut tool = tool;
                    if let Some(o) = tool.as_object_mut() {
                        changed |= o.remove("defer_loading").is_some();
                    }
                    kept.push(tool);
                }
                "namespace" => {
                    let ns = name.clone().unwrap_or_default();
                    if is_ignored(ignore, &ty, name.as_deref()) {
                        out.ignored.push(label(&ty, name.as_deref()));
                        changed = true;
                        continue;
                    }
                    let inner = tool
                        .get("tools")
                        .and_then(|t| t.as_array())
                        .cloned()
                        .unwrap_or_default();
                    for (j, f) in inner.into_iter().enumerate() {
                        let fty = str_of(&f, "type").unwrap_or("function").to_string();
                        let fname = str_of(&f, "name").map(str::to_string);
                        if fty != "function" {
                            if is_ignored(ignore, &fty, fname.as_deref()) {
                                out.ignored.push(label(&fty, fname.as_deref()));
                                continue;
                            }
                            return Err(Unsupported {
                                param: format!("tools[{}].tools[{}]", i, j),
                                reason: reason_for(&fty),
                                tool_type: fty,
                                tool_name: fname,
                            });
                        }
                        let Some(fname) = fname else { continue };
                        let flat = join_name(&ns, &fname);
                        if top_names.contains(&flat) {
                            return Err(Unsupported {
                                param: format!("tools[{}].tools[{}]", i, j),
                                tool_type: "namespace".into(),
                                tool_name: Some(flat.clone()),
                                reason: format!(
                                    "flattening namespace \"{}\" would give \"{}\", which another function already uses",
                                    ns, flat
                                ),
                            });
                        }
                        top_names.push(flat.clone());
                        let mut f = f;
                        if let Some(o) = f.as_object_mut() {
                            o.remove("defer_loading");
                            o.insert("name".into(), Value::String(flat.clone()));
                        }
                        out.names.insert(flat, (ns.clone(), fname));
                        kept.push(f);
                    }
                    changed = true;
                }
                other => {
                    if is_ignored(ignore, other, name.as_deref()) {
                        out.ignored.push(label(other, name.as_deref()));
                        changed = true;
                        continue;
                    }
                    return Err(Unsupported {
                        param: format!("tools[{}]", i),
                        reason: reason_for(other),
                        tool_type: other.to_string(),
                        tool_name: name,
                    });
                }
            }
        }
        *tools = kept;
    }
    // tool_choice: a namespaced function, or a hosted tool type.
    if let Some(choice) = obj.get_mut("tool_choice") {
        if let Some(c) = choice.as_object_mut() {
            let ty = c
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("function")
                .to_string();
            match ty.as_str() {
                "function" => {
                    if let Some(Value::String(ns)) = c.remove("namespace") {
                        let name = c.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        let flat = join_name(&ns, name);
                        c.insert("name".into(), Value::String(flat));
                        changed = true;
                    }
                }
                "allowed_tools" | "auto" | "none" | "required" => {}
                other => {
                    if is_ignored(ignore, other, None) {
                        *choice = Value::String("auto".into());
                        changed = true;
                    } else {
                        return Err(Unsupported {
                            param: "tool_choice".into(),
                            reason: reason_for(other),
                            tool_type: other.to_string(),
                            tool_name: None,
                        });
                    }
                }
            }
        }
    }
    // Earlier calls of namespaced functions in the conversation.
    if let Some(Value::Array(items)) = obj.get_mut("input") {
        for item in items.iter_mut() {
            let Some(o) = item.as_object_mut() else {
                continue;
            };
            if o.get("type").and_then(|t| t.as_str()) != Some("function_call") {
                continue;
            }
            if let Some(Value::String(ns)) = o.remove("namespace") {
                let name = o.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let flat = join_name(&ns, name);
                out.names
                    .entry(flat.clone())
                    .or_insert_with(|| (ns.clone(), name.to_string()));
                o.insert("name".into(), Value::String(flat));
                changed = true;
            }
        }
    }
    Ok(changed)
}

fn chat(
    obj: &mut Map<String, Value>,
    ignore: &[String],
    out: &mut Prepared,
) -> Result<bool, Unsupported> {
    let mut changed = false;
    if let Some(Value::Array(tools)) = obj.get_mut("tools") {
        let mut kept = Vec::with_capacity(tools.len());
        for (i, tool) in std::mem::take(tools).into_iter().enumerate() {
            let ty = str_of(&tool, "type").unwrap_or("function").to_string();
            if ty == "function" {
                kept.push(tool);
                continue;
            }
            let name = str_of(&tool, "name")
                .or_else(|| tool.get(&ty).and_then(|t| str_of(t, "name")))
                .map(str::to_string);
            if is_ignored(ignore, &ty, name.as_deref()) {
                out.ignored.push(label(&ty, name.as_deref()));
                changed = true;
                continue;
            }
            return Err(Unsupported {
                param: format!("tools[{}]", i),
                reason: reason_for(&ty),
                tool_type: ty,
                tool_name: name,
            });
        }
        *tools = kept;
    }
    Ok(changed)
}

fn messages(
    obj: &mut Map<String, Value>,
    ignore: &[String],
    out: &mut Prepared,
) -> Result<bool, Unsupported> {
    let mut changed = false;
    if let Some(Value::Array(tools)) = obj.get_mut("tools") {
        let mut kept = Vec::with_capacity(tools.len());
        for (i, tool) in std::mem::take(tools).into_iter().enumerate() {
            let ty = str_of(&tool, "type").map(str::to_string);
            let name = str_of(&tool, "name").map(str::to_string);
            match ty.as_deref() {
                None | Some("custom") => kept.push(tool),
                Some(other) => {
                    if is_ignored(ignore, other, name.as_deref()) {
                        out.ignored.push(label(other, name.as_deref()));
                        changed = true;
                        continue;
                    }
                    return Err(Unsupported {
                        param: format!("tools[{}]", i),
                        reason: reason_for(other),
                        tool_type: other.to_string(),
                        tool_name: name,
                    });
                }
            }
        }
        *tools = kept;
    }
    let has_mcp = obj
        .get("mcp_servers")
        .and_then(|m| m.as_array())
        .is_some_and(|a| !a.is_empty());
    if has_mcp {
        if is_ignored(ignore, "mcp_servers", None) || is_ignored(ignore, "mcp", None) {
            obj.remove("mcp_servers");
            out.ignored.push("mcp_servers".into());
            changed = true;
        } else {
            return Err(Unsupported {
                param: "mcp_servers".into(),
                reason: reason_for("mcp_servers"),
                tool_type: "mcp_servers".into(),
                tool_name: None,
            });
        }
    }
    Ok(changed)
}

/// Maps flattened function names back to `name` + `namespace` inside any JSON value
/// (a Responses object, an output item, a stream event). Returns whether it changed.
pub fn map_back(v: &mut Value, names: &NameMap) -> bool {
    let mut changed = false;
    match v {
        Value::Object(o) => {
            let is_call = matches!(
                o.get("type").and_then(|t| t.as_str()),
                Some("function_call") | Some("response.function_call_arguments.done")
            ) || o.contains_key("call_id");
            if is_call {
                if let Some(Value::String(name)) = o.get("name") {
                    if let Some((ns, orig)) = names.get(name) {
                        let (ns, orig) = (ns.clone(), orig.clone());
                        o.insert("name".into(), Value::String(orig));
                        o.insert("namespace".into(), Value::String(ns));
                        changed = true;
                    }
                }
            }
            for (_, child) in o.iter_mut() {
                changed |= map_back(child, names);
            }
        }
        Value::Array(a) => {
            for child in a {
                changed |= map_back(child, names);
            }
        }
        _ => {}
    }
    changed
}

/// Rewrites a JSON response body (non-streaming).
pub fn rewrite_json(body: &[u8], names: &NameMap) -> Option<Vec<u8>> {
    let mut v: Value = serde_json::from_slice(body).ok()?;
    map_back(&mut v, names).then(|| serde_json::to_vec(&v).unwrap_or_default())
}

/// Rewrites SSE events of a Responses stream: complete events whose data mention a
/// flattened name are parsed and mapped back; every other byte passes unchanged.
#[derive(Debug)]
pub struct SseRewriter {
    names: NameMap,
    pending: Vec<u8>,
}

impl SseRewriter {
    pub fn new(names: NameMap) -> Self {
        Self {
            names,
            pending: Vec::new(),
        }
    }

    /// Bytes ready to send for this chunk (complete events only).
    pub fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(chunk);
        let mut out = Vec::with_capacity(self.pending.len());
        while let Some(end) = find_event_end(&self.pending) {
            let event: Vec<u8> = self.pending.drain(..end).collect();
            out.extend(self.rewrite_event(&event));
        }
        out
    }

    /// Whatever is left at the end of the stream.
    pub fn finish(&mut self) -> Vec<u8> {
        let rest = std::mem::take(&mut self.pending);
        self.rewrite_event(&rest)
    }

    fn rewrite_event(&self, event: &[u8]) -> Vec<u8> {
        let mentions = self
            .names
            .keys()
            .any(|n| event.windows(n.len()).any(|w| w == n.as_bytes()));
        if !mentions {
            return event.to_vec();
        }
        let text = String::from_utf8_lossy(event);
        let mut out = String::with_capacity(text.len());
        for line in text.split_inclusive('\n') {
            let (body, nl) = match line.strip_suffix("\r\n") {
                Some(b) => (b, "\r\n"),
                None => match line.strip_suffix('\n') {
                    Some(b) => (b, "\n"),
                    None => (line, ""),
                },
            };
            if let Some(data) = body.strip_prefix("data:") {
                if let Ok(mut v) = serde_json::from_str::<Value>(data.trim()) {
                    if map_back(&mut v, &self.names) {
                        out.push_str("data: ");
                        out.push_str(&serde_json::to_string(&v).unwrap_or_default());
                        out.push_str(nl);
                        continue;
                    }
                }
            }
            out.push_str(line);
        }
        out.into_bytes()
    }
}

/// End (exclusive) of the first complete SSE event (`\n\n` or `\r\n\r\n`).
fn find_event_end(buf: &[u8]) -> Option<usize> {
    let lf = buf.windows(2).position(|w| w == b"\n\n").map(|p| p + 2);
    let crlf = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4);
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(api: Api, body: Value, ignore: &[&str]) -> Result<(Value, Prepared), Unsupported> {
        let ignore: Vec<String> = ignore.iter().map(|s| s.to_string()).collect();
        let bytes = serde_json::to_vec(&body).unwrap();
        let p = prepare(api, &bytes, &ignore)?;
        let v = p
            .body
            .as_ref()
            .map(|b| serde_json::from_slice(b).unwrap())
            .unwrap_or(body);
        Ok((v, p))
    }

    /// A Codex-style request: a plain function, one MCP namespace (shape of the Responses
    /// API's namespace tool type) and an earlier namespaced call in `input`.
    fn codex_request() -> Value {
        json!({
            "model": "qwen",
            "stream": true,
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]},
                {"type": "function_call", "name": "list_issues", "namespace": "mcp__github",
                 "call_id": "call_1", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "[]"}
            ],
            "tools": [
                {"type": "function", "name": "shell", "parameters": {"type": "object"}},
                {"type": "namespace", "name": "mcp__github", "description": "GitHub MCP server",
                 "tools": [
                    {"type": "function", "name": "list_issues", "defer_loading": true,
                     "parameters": {"type": "object", "properties": {"repo": {"type": "string"}}}},
                    {"type": "function", "name": "get_issue", "parameters": {"type": "object"}}
                 ]}
            ],
            "tool_choice": "auto"
        })
    }

    #[test]
    fn input_audio_passthrough() {
        let part = json!({"type":"input_audio", "input_audio":{"data":"A".repeat(5 * 1024 * 1024), "format":"flac"}});
        let body = json!({"model":"audio", "messages":[{"role":"user", "content":[{"type":"text", "text":"Listen"}, part]}],
            "tools":[{"type":"function", "function":{"name":"lookup", "parameters":{"type":"object"}}}]});
        let (out, _) = run(Api::Chat, body.clone(), &[]).unwrap();
        assert_eq!(out["messages"], body["messages"]);
        let bytes = serde_json::to_vec(&body).unwrap();
        assert!(bytes.len() < crate::BODY_LIMIT);
    }

    #[test]
    fn namespace_is_flattened_and_mapped_back() {
        let (v, p) = run(Api::Responses, codex_request(), &[]).unwrap();
        let names: Vec<&str> = v["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "shell",
                "mcp__github__list_issues",
                "mcp__github__get_issue"
            ]
        );
        assert!(v["tools"][1].get("defer_loading").is_none());
        assert!(v["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["type"] == "function"));
        assert_eq!(v["input"][1]["name"], "mcp__github__list_issues");
        assert!(v["input"][1].get("namespace").is_none());
        assert_eq!(p.header(), "namespace_flattened=2");

        // Response (non-streaming) maps back.
        let resp = json!({"object": "response", "output": [
            {"type": "function_call", "name": "mcp__github__get_issue", "call_id": "c2", "arguments": "{\"n\":1}"},
            {"type": "function_call", "name": "shell", "call_id": "c3", "arguments": "{}"}
        ]});
        let back: Value = serde_json::from_slice(
            &rewrite_json(&serde_json::to_vec(&resp).unwrap(), &p.names).unwrap(),
        )
        .unwrap();
        assert_eq!(back["output"][0]["name"], "get_issue");
        assert_eq!(back["output"][0]["namespace"], "mcp__github");
        assert_eq!(back["output"][1]["name"], "shell");
        assert!(back["output"][1].get("namespace").is_none());
    }

    #[test]
    fn sse_rewrite_keeps_other_bytes() {
        let mut names = NameMap::new();
        names.insert(
            "mcp__github__get_issue".into(),
            ("mcp__github".into(), "get_issue".into()),
        );
        let mut r = SseRewriter::new(names);
        let delta = b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"x\"}\n\n";
        let mut out = r.push(delta);
        out.extend(r.push(b"event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"name\":\"mcp__gith"));
        out.extend(r.push(b"ub__get_issue\",\"call_id\":\"c\",\"arguments\":\"{}\"}}\n\n"));
        out.extend(r.finish());
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with(std::str::from_utf8(delta).unwrap()));
        assert!(text.contains("\"name\":\"get_issue\""), "{text}");
        assert!(text.contains("\"namespace\":\"mcp__github\""), "{text}");
        assert!(text.ends_with("\n\n"));
    }

    #[test]
    fn hosted_tools_are_rejected_with_reason() {
        let mut req = codex_request();
        req["tools"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type": "web_search", "search_context_size": "medium"}));
        let err = run(Api::Responses, req.clone(), &[]).unwrap_err();
        assert_eq!(err.tool_type, "web_search");
        assert_eq!(err.param, "tools[2]");
        assert!(err.message().contains("web search"), "{}", err.message());

        // Explicit consent: removed and reported.
        let (v, p) = run(Api::Responses, req, &["web_search"]).unwrap();
        assert_eq!(v["tools"].as_array().unwrap().len(), 3);
        assert_eq!(p.ignored, ["web_search"]);
        assert_eq!(p.header(), "namespace_flattened=2; ignored=web_search");

        for ty in [
            "image_generation",
            "custom",
            "local_shell",
            "mcp",
            "file_search",
            "brand_new",
        ] {
            let req = json!({"model": "m", "input": "x", "tools": [{"type": ty, "name": "t"}]});
            let err = run(Api::Responses, req, &[]).unwrap_err();
            assert_eq!(err.tool_type, ty);
        }
        // A hosted tool inside a namespace.
        let req = json!({"model": "m", "input": "x", "tools": [
            {"type": "namespace", "name": "ns", "tools": [{"type": "web_search"}]}]});
        assert_eq!(
            run(Api::Responses, req, &[]).unwrap_err().param,
            "tools[0].tools[0]"
        );
        // tool_choice forcing a hosted tool.
        let req =
            json!({"model": "m", "input": "x", "tool_choice": {"type": "web_search_preview"}});
        assert_eq!(
            run(Api::Responses, req, &[]).unwrap_err().param,
            "tool_choice"
        );
    }

    #[test]
    fn plain_requests_are_untouched() {
        let req = json!({"model": "m", "input": "x", "tools": [{"type": "function", "name": "f", "parameters": {}}]});
        let bytes = serde_json::to_vec(&req).unwrap();
        let p = prepare(Api::Responses, &bytes, &[]).unwrap();
        assert!(p.body.is_none(), "forwarded as sent");
        let p = prepare(Api::Chat, b"{\"model\":\"m\",\"messages\":[]}", &[]).unwrap();
        assert!(p.body.is_none());
        let p = prepare(
            Api::Embeddings,
            b"{\"tools\":[{\"type\":\"web_search\"}]}",
            &[],
        )
        .unwrap();
        assert!(p.body.is_none(), "not a tool API");
    }

    #[test]
    fn chat_and_anthropic() {
        let req = json!({"model": "m", "messages": [], "tools": [
            {"type": "function", "function": {"name": "f", "parameters": {}}},
            {"type": "custom", "custom": {"name": "apply_patch"}}]});
        let err = run(Api::Chat, req, &[]).unwrap_err();
        assert_eq!(
            (err.tool_type.as_str(), err.tool_name.as_deref()),
            ("custom", Some("apply_patch"))
        );

        let req = json!({"model": "m", "max_tokens": 10, "messages": [], "tools": [
            {"name": "get_weather", "input_schema": {"type": "object"}},
            {"type": "custom", "name": "c", "input_schema": {"type": "object"}},
            {"type": "web_search_20250305", "name": "web_search", "max_uses": 3}]});
        let err = run(Api::Messages, req.clone(), &[]).unwrap_err();
        assert_eq!(err.tool_type, "web_search_20250305");
        let (v, p) = run(Api::Messages, req, &["web_search"]).unwrap();
        assert_eq!(v["tools"].as_array().unwrap().len(), 2);
        assert_eq!(p.ignored, ["web_search_20250305:web_search"]);

        let req = json!({"model": "m", "messages": [], "mcp_servers": [{"type": "url", "url": "https://x"}]});
        assert_eq!(
            run(Api::CountTokens, req, &[]).unwrap_err().param,
            "mcp_servers"
        );
    }
}
