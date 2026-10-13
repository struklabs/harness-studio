//! Client request → the chat-completions conversation the core agent loop runs.
//!
//! Server-side tools need one loop, and the loop speaks chat completions to llama-server. A
//! Chat Completions request is taken as it is; a Responses (`input` items, `instructions`) or an
//! Anthropic Messages request (content blocks) is converted. Only what shapes the model's answer
//! is carried over (messages, tools, sampling parameters); client-only fields (`store`,
//! `metadata`, `user`...) are not forwarded.

use serde_json::{json, Map, Value};

use crate::compat::Api;

/// A client request, ready for the loop.
#[derive(Debug)]
pub(crate) struct Job {
    pub messages: Vec<Value>,
    /// Chat-completions fields passed to every round (see `StreamRequest::params`).
    pub params: Map<String, Value>,
    /// The client's own tools, in chat-completions form.
    pub client_tools: Vec<Value>,
    pub stream: bool,
    /// The client asked for usage in the stream (`stream_options.include_usage`).
    pub include_usage: bool,
}

pub(crate) fn job(api: Api, doc: &Map<String, Value>) -> Result<Job, String> {
    let stream = doc.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let include_usage = doc
        .get("stream_options")
        .and_then(|o| o.get("include_usage"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let (messages, client_tools, params) = match api {
        Api::Chat => chat(doc)?,
        Api::Responses => responses(doc)?,
        Api::Messages => messages(doc)?,
        _ => return Err("this endpoint has no tools".into()),
    };
    Ok(Job {
        messages,
        params,
        client_tools,
        stream,
        include_usage,
    })
}

type Converted = (Vec<Value>, Vec<Value>, Map<String, Value>);

fn copy_params(doc: &Map<String, Value>, keys: &[&str], params: &mut Map<String, Value>) {
    for k in keys {
        if let Some(v) = doc.get(*k).filter(|v| !v.is_null()) {
            params.insert((*k).to_string(), v.clone());
        }
    }
}

// ----- Chat Completions ------------------------------------------------------------------------

fn chat(doc: &Map<String, Value>) -> Result<Converted, String> {
    let messages = doc
        .get("messages")
        .and_then(Value::as_array)
        .ok_or("'messages' must be an array")?
        .clone();
    let tools = doc
        .get("tools")
        .and_then(Value::as_array)
        .map(|t| {
            t.iter()
                .filter(|t| {
                    t.get("type").and_then(Value::as_str).unwrap_or("function") == "function"
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let mut params = Map::new();
    for (k, v) in doc {
        if !matches!(
            k.as_str(),
            "model"
                | "messages"
                | "tools"
                | "stream"
                | "stream_options"
                | "n"
                | "web_search_options"
        ) && !v.is_null()
        {
            params.insert(k.clone(), v.clone());
        }
    }
    Ok((messages, tools, params))
}

// ----- Responses --------------------------------------------------------------------------------

fn text_of_parts(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(
                |p| match p.get("type").and_then(Value::as_str).unwrap_or("text") {
                    "input_text" | "output_text" | "text" | "refusal" => p
                        .get("text")
                        .or_else(|| p.get("refusal"))
                        .and_then(Value::as_str),
                    _ => None,
                },
            )
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn image_url(p: &Value) -> Option<String> {
    match p.get("image_url") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(o) => o.get("url").and_then(Value::as_str).map(str::to_string),
        None => None,
    }
}

/// Message content for the chat API: a string when it is all text, parts when it has images or audio.
fn chat_content(content: &Value) -> Value {
    let Value::Array(parts) = content else {
        return json!(text_of_parts(content));
    };
    let has_media = parts.iter().any(|p| {
        p.get("type")
            .and_then(Value::as_str)
            .is_some_and(|t| matches!(t, "input_image" | "input_audio"))
    });
    if !has_media {
        return json!(text_of_parts(content));
    }
    let out: Vec<Value> = parts
        .iter()
        .filter_map(
            |p| match p.get("type").and_then(Value::as_str).unwrap_or("text") {
                "input_audio" => Some(p.clone()),
                "input_image" => {
                    image_url(p).map(|u| json!({"type": "image_url", "image_url": {"url": u}}))
                }
                "input_text" | "output_text" | "text" => p
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|t| json!({"type": "text", "text": t})),
                _ => None,
            },
        )
        .collect();
    Value::Array(out)
}

fn push_call(messages: &mut Vec<Value>, call: Value) {
    if let Some(last) = messages.last_mut() {
        if last["role"] == "assistant" {
            if let Some(list) = last.get_mut("tool_calls").and_then(Value::as_array_mut) {
                list.push(call);
                return;
            }
            if last.get("tool_calls").is_none() {
                last["tool_calls"] = json!([call]);
                return;
            }
        }
    }
    messages.push(json!({"role": "assistant", "content": "", "tool_calls": [call]}));
}

fn responses(doc: &Map<String, Value>) -> Result<Converted, String> {
    let mut messages: Vec<Value> = Vec::new();
    if let Some(i) = doc
        .get("instructions")
        .and_then(Value::as_str)
        .filter(|i| !i.trim().is_empty())
    {
        messages.push(json!({"role": "system", "content": i}));
    }
    for item in harness_core::gateway::responses::normalize_input(doc.get("input")) {
        let ty =
            item.get("type")
                .and_then(Value::as_str)
                .unwrap_or(if item.get("role").is_some() {
                    "message"
                } else {
                    ""
                });
        match ty {
            "message" => {
                let role = match item.get("role").and_then(Value::as_str).unwrap_or("user") {
                    "developer" => "system",
                    r @ ("system" | "user" | "assistant") => r,
                    _ => "user",
                };
                let content = item.get("content").unwrap_or(&Value::Null);
                let content = if role == "user" {
                    chat_content(content)
                } else {
                    json!(text_of_parts(content))
                };
                messages.push(json!({"role": role, "content": content}));
            }
            "function_call" => {
                let id = item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or("call_0");
                push_call(
                    &mut messages,
                    json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": item.get("name").and_then(Value::as_str).unwrap_or(""),
                            "arguments": item.get("arguments").and_then(Value::as_str).unwrap_or("{}"),
                        }
                    }),
                );
            }
            "function_call_output" => {
                let out = match item.get("output") {
                    Some(Value::String(s)) => s.clone(),
                    Some(other) => text_of_parts(other),
                    None => String::new(),
                };
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": item.get("call_id").and_then(Value::as_str).unwrap_or(""),
                    "content": out,
                }));
            }
            // reasoning, web_search_call and everything else belongs to the client's own
            // bookkeeping; the model never saw it as text.
            _ => {}
        }
    }
    let mut client_tools = Vec::new();
    for t in doc
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if t.get("type").and_then(Value::as_str).unwrap_or("function") != "function" {
            continue;
        }
        let Some(name) = t.get("name").and_then(Value::as_str) else {
            continue;
        };
        client_tools.push(json!({
            "type": "function",
            "function": {
                "name": name,
                "description": t.get("description").and_then(Value::as_str).unwrap_or(""),
                "parameters": t.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object", "properties": {}})),
            }
        }));
    }
    let mut params = Map::new();
    copy_params(
        doc,
        &[
            "temperature",
            "top_p",
            "seed",
            "parallel_tool_calls",
            "top_k",
        ],
        &mut params,
    );
    if let Some(m) = doc.get("max_output_tokens").filter(|v| v.is_number()) {
        params.insert("max_tokens".into(), m.clone());
    }
    match doc.get("tool_choice") {
        Some(Value::String(s)) if matches!(s.as_str(), "auto" | "none" | "required") => {
            params.insert("tool_choice".into(), json!(s));
        }
        Some(Value::Object(o)) if o.get("type").and_then(Value::as_str) == Some("function") => {
            if let Some(n) = o.get("name").and_then(Value::as_str) {
                params.insert(
                    "tool_choice".into(),
                    json!({"type": "function", "function": {"name": n}}),
                );
            }
        }
        _ => {}
    }
    if let Some(fmt) = doc.get("text").and_then(|t| t.get("format")) {
        if fmt.get("type").and_then(Value::as_str) == Some("json_schema") {
            params.insert(
                "response_format".into(),
                json!({"type": "json_schema", "json_schema": {
                    "name": fmt.get("name").and_then(Value::as_str).unwrap_or("response"),
                    "schema": fmt.get("schema").cloned().unwrap_or(Value::Null),
                    "strict": fmt.get("strict").cloned().unwrap_or(Value::Null),
                }}),
            );
        } else if fmt.get("type").and_then(Value::as_str) == Some("json_object") {
            params.insert("response_format".into(), json!({"type": "json_object"}));
        }
    }
    if let Some(effort) = doc
        .get("reasoning")
        .and_then(|r| r.get("effort"))
        .and_then(Value::as_str)
    {
        params.insert("reasoning_effort".into(), json!(effort));
    }
    Ok((messages, client_tools, params))
}

// ----- Anthropic Messages -----------------------------------------------------------------------

fn block_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str).unwrap_or("text") == "text")
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn anthropic_image(b: &Value) -> Option<Value> {
    let src = b.get("source")?;
    let url = match src.get("type").and_then(Value::as_str)? {
        "base64" => format!(
            "data:{};base64,{}",
            src.get("media_type")
                .and_then(Value::as_str)
                .unwrap_or("image/png"),
            src.get("data").and_then(Value::as_str)?
        ),
        "url" => src.get("url").and_then(Value::as_str)?.to_string(),
        _ => return None,
    };
    Some(json!({"type": "image_url", "image_url": {"url": url}}))
}

fn messages(doc: &Map<String, Value>) -> Result<Converted, String> {
    let mut out: Vec<Value> = Vec::new();
    let system = block_text(doc.get("system").unwrap_or(&Value::Null));
    if !system.trim().is_empty() {
        out.push(json!({"role": "system", "content": system}));
    }
    let list = doc
        .get("messages")
        .and_then(Value::as_array)
        .ok_or("'messages' must be an array")?;
    for m in list {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("user");
        let content = m.get("content").unwrap_or(&Value::Null);
        let Value::Array(blocks) = content else {
            out.push(json!({"role": role, "content": block_text(content)}));
            continue;
        };
        if role == "assistant" {
            let mut calls = Vec::new();
            for b in blocks {
                if b.get("type").and_then(Value::as_str) == Some("tool_use") {
                    calls.push(json!({
                        "id": b.get("id").and_then(Value::as_str).unwrap_or("call_0"),
                        "type": "function",
                        "function": {
                            "name": b.get("name").and_then(Value::as_str).unwrap_or(""),
                            "arguments": b.get("input").cloned().unwrap_or_else(|| json!({})).to_string(),
                        }
                    }));
                }
            }
            let mut msg = json!({"role": "assistant", "content": block_text(content)});
            if !calls.is_empty() {
                msg["tool_calls"] = Value::Array(calls);
            }
            out.push(msg);
            continue;
        }
        // User turn: tool results first (they answer the previous assistant turn), then the
        // user's own content.
        let mut rest: Vec<Value> = Vec::new();
        for b in blocks {
            match b.get("type").and_then(Value::as_str).unwrap_or("text") {
                "tool_result" => {
                    let mut text = block_text(b.get("content").unwrap_or(&Value::Null));
                    if b.get("is_error").and_then(Value::as_bool) == Some(true) {
                        text = format!("Error: {text}");
                    }
                    out.push(json!({
                        "role": "tool",
                        "tool_call_id": b.get("tool_use_id").and_then(Value::as_str).unwrap_or(""),
                        "content": text,
                    }));
                }
                "text" => {
                    if let Some(t) = b.get("text").and_then(Value::as_str) {
                        rest.push(json!({"type": "text", "text": t}));
                    }
                }
                "image" => {
                    if let Some(i) = anthropic_image(b) {
                        rest.push(i);
                    }
                }
                _ => {}
            }
        }
        if rest.iter().any(|p| p["type"] == "image_url") {
            out.push(json!({"role": "user", "content": rest}));
        } else if !rest.is_empty() {
            let text: String = rest
                .iter()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("");
            out.push(json!({"role": "user", "content": text}));
        }
    }
    let mut tools = Vec::new();
    for t in doc
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if !matches!(t.get("type").and_then(Value::as_str), None | Some("custom")) {
            continue;
        }
        let Some(name) = t.get("name").and_then(Value::as_str) else {
            continue;
        };
        tools.push(json!({
            "type": "function",
            "function": {
                "name": name,
                "description": t.get("description").and_then(Value::as_str).unwrap_or(""),
                "parameters": t.get("input_schema").cloned().unwrap_or_else(|| json!({"type": "object", "properties": {}})),
            }
        }));
    }
    let mut params = Map::new();
    copy_params(
        doc,
        &["max_tokens", "temperature", "top_p", "top_k"],
        &mut params,
    );
    if let Some(s) = doc.get("stop_sequences").filter(|s| s.is_array()) {
        params.insert("stop".into(), s.clone());
    }
    if let Some(Value::Object(o)) = doc.get("tool_choice") {
        match o.get("type").and_then(Value::as_str) {
            Some("auto") => {
                params.insert("tool_choice".into(), json!("auto"));
            }
            Some("any") => {
                params.insert("tool_choice".into(), json!("required"));
            }
            Some("none") => {
                params.insert("tool_choice".into(), json!("none"));
            }
            Some("tool") => {
                if let Some(n) = o.get("name").and_then(Value::as_str) {
                    params.insert(
                        "tool_choice".into(),
                        json!({"type": "function", "function": {"name": n}}),
                    );
                }
            }
            _ => {}
        }
    }
    Ok((out, tools, params))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn input_audio_parts_survive_conversion() {
        let part = json!({"type":"input_audio", "input_audio":{"data":"Zm9v", "format":"wav"}});
        let chat = job(
            Api::Chat,
            &doc(json!({"messages":[{"role":"user", "content":[part.clone()]}]})),
        )
        .unwrap();
        assert_eq!(chat.messages[0]["content"][0], part);
        let response = job(
            Api::Responses,
            &doc(json!({"input":[{"role":"user", "content":[part.clone()]}]})),
        )
        .unwrap();
        assert_eq!(response.messages[0]["content"][0], part);
    }

    #[test]
    fn responses_items_become_chat_messages() {
        let j = job(
            Api::Responses,
            &doc(json!({
                "model": "m", "stream": true, "instructions": "Be brief.",
                "max_output_tokens": 300, "temperature": 0.2,
                "input": [
                    {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "dev note"}]},
                    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "weather in Ankara?"}]},
                    {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Checking."}]},
                    {"type": "function_call", "call_id": "c1", "name": "get_weather", "arguments": "{\"city\":\"Ankara\"}"},
                    {"type": "function_call_output", "call_id": "c1", "output": "12 C"},
                    {"type": "reasoning", "summary": []},
                    {"type": "web_search_call", "id": "ws_1", "status": "completed"}
                ],
                "tools": [{"type": "function", "name": "get_weather", "description": "d",
                           "parameters": {"type": "object"}}],
                "tool_choice": {"type": "function", "name": "get_weather"}
            })),
        )
        .unwrap();
        assert!(j.stream);
        let roles: Vec<&str> = j
            .messages
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, ["system", "system", "user", "assistant", "tool"]);
        assert_eq!(j.messages[0]["content"], "Be brief.");
        // The call joins the assistant message it follows.
        assert_eq!(j.messages[3]["content"], "Checking.");
        assert_eq!(j.messages[3]["tool_calls"][0]["id"], "c1");
        assert_eq!(
            j.messages[3]["tool_calls"][0]["function"]["name"],
            "get_weather"
        );
        assert_eq!(j.messages[4]["tool_call_id"], "c1");
        assert_eq!(j.client_tools[0]["function"]["name"], "get_weather");
        assert_eq!(j.params["max_tokens"], 300);
        assert_eq!(j.params["tool_choice"]["function"]["name"], "get_weather");
        assert!(j.params.get("model").is_none() && j.params.get("input").is_none());
    }

    #[test]
    fn responses_string_input_and_images() {
        let j = job(Api::Responses, &doc(json!({"model": "m", "input": "hi"}))).unwrap();
        assert_eq!(j.messages, [json!({"role": "user", "content": "hi"})]);
        let j = job(
            Api::Responses,
            &doc(json!({"model": "m", "input": [{"role": "user", "content": [
                {"type": "input_text", "text": "what is this"},
                {"type": "input_image", "image_url": "data:image/png;base64,AAAA"}]}]})),
        )
        .unwrap();
        assert_eq!(
            j.messages[0]["content"][1]["image_url"]["url"],
            "data:image/png;base64,AAAA"
        );
    }

    #[test]
    fn anthropic_blocks_become_chat_messages() {
        let j = job(
            Api::Messages,
            &doc(json!({
                "model": "m", "max_tokens": 100, "system": [{"type": "text", "text": "sys"}],
                "stop_sequences": ["END"],
                "messages": [
                    {"role": "user", "content": "weather?"},
                    {"role": "assistant", "content": [
                        {"type": "text", "text": "ok"},
                        {"type": "tool_use", "id": "tu1", "name": "get_weather", "input": {"city": "Ankara"}}]},
                    {"role": "user", "content": [
                        {"type": "tool_result", "tool_use_id": "tu1", "content": [{"type": "text", "text": "12 C"}]},
                        {"type": "text", "text": "thanks"}]}
                ],
                "tools": [{"name": "get_weather", "description": "d", "input_schema": {"type": "object"}}],
                "tool_choice": {"type": "any"}
            })),
        )
        .unwrap();
        let roles: Vec<&str> = j
            .messages
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, ["system", "user", "assistant", "tool", "user"]);
        assert_eq!(
            j.messages[2]["tool_calls"][0]["function"]["arguments"],
            "{\"city\":\"Ankara\"}"
        );
        assert_eq!(j.messages[3]["content"], "12 C");
        assert_eq!(j.messages[4]["content"], "thanks");
        assert_eq!(j.params["tool_choice"], "required");
        assert_eq!(j.params["stop"], json!(["END"]));
        assert_eq!(
            j.client_tools[0]["function"]["parameters"]["type"],
            "object"
        );
    }

    #[test]
    fn chat_passes_through() {
        let j = job(
            Api::Chat,
            &doc(
                json!({"model": "m", "stream": true, "stream_options": {"include_usage": true},
                "messages": [{"role": "user", "content": "x"}], "temperature": 0, "top_p": 0.9,
                "tools": [{"type": "function", "function": {"name": "f", "parameters": {}}}],
                "web_search_options": {}}),
            ),
        )
        .unwrap();
        assert!(j.stream && j.include_usage);
        assert_eq!(j.messages.len(), 1);
        assert_eq!(j.client_tools.len(), 1);
        assert_eq!(j.params["top_p"], 0.9);
        assert!(j.params.get("web_search_options").is_none());
        assert!(j.params.get("stream_options").is_none());
    }
}
