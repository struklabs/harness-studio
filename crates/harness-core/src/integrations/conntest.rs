//! "Bağlantıyı test et": one small real request, with a tool, in the API the
//! integration speaks: Anthropic Messages (`/v1/messages`) for Claude Code / Claude Desktop,
//! OpenAI Responses (`/v1/responses`) for Codex, Chat Completions for everything else.
//!
//! The result says whether the endpoint answered in the right shape and whether the model
//! called the offered tool (small models may answer in text instead; that is reported, not
//! treated as a failure).

use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

use super::launch::WireApi;
use crate::error_key::err;

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionTest {
    pub ok: bool,
    pub api: WireApi,
    pub url: String,
    pub status: Option<u16>,
    pub latency_ms: u64,
    /// The model called the `get_weather` tool.
    pub tool_called: bool,
    /// The model id the answer reports.
    pub model: Option<String>,
    /// A short excerpt of the answer text, or the error message.
    pub detail: String,
}

const PROMPT: &str = "What is the weather in Paris right now? Use the get_weather tool.";

fn weather_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "city": { "type": "string", "description": "City name" } },
        "required": ["city"]
    })
}

/// The request body and path for `api`.
pub fn request_for(api: WireApi, model: &str) -> (&'static str, Value) {
    match api {
        WireApi::Anthropic => (
            "/messages",
            json!({
                "model": model,
                "max_tokens": 256,
                "messages": [{"role": "user", "content": PROMPT}],
                "tools": [{
                    "name": "get_weather",
                    "description": "Current weather for a city",
                    "input_schema": weather_schema()
                }]
            }),
        ),
        WireApi::Responses => (
            "/responses",
            json!({
                "model": model,
                "max_output_tokens": 256,
                "input": PROMPT,
                "tools": [{
                    "type": "function",
                    "name": "get_weather",
                    "description": "Current weather for a city",
                    "parameters": weather_schema()
                }]
            }),
        ),
        WireApi::Chat => (
            "/chat/completions",
            json!({
                "model": model,
                "max_tokens": 256,
                "messages": [{"role": "user", "content": PROMPT}],
                "tools": [{
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "description": "Current weather for a city",
                        "parameters": weather_schema()
                    }
                }]
            }),
        ),
    }
}

/// Did the answer call the tool, and what text came back?
pub fn read_answer(api: WireApi, body: &Value) -> Result<(bool, String), String> {
    match api {
        WireApi::Anthropic => {
            let content = body["content"]
                .as_array()
                .ok_or_else(|| err("conntest.unexpected_answer").with("field", "content"))?;
            let tool = content.iter().any(|c| c["type"] == "tool_use");
            let text = content
                .iter()
                .filter_map(|c| c["text"].as_str())
                .collect::<Vec<_>>()
                .join(" ");
            Ok((tool, text))
        }
        WireApi::Responses => {
            let output = body["output"]
                .as_array()
                .ok_or_else(|| err("conntest.unexpected_answer").with("field", "output"))?;
            let tool = output.iter().any(|o| o["type"] == "function_call");
            let text = output
                .iter()
                .flat_map(|o| o["content"].as_array().cloned().unwrap_or_default())
                .filter_map(|c| c["text"].as_str().map(str::to_string))
                .collect::<Vec<_>>()
                .join(" ");
            Ok((tool, text))
        }
        WireApi::Chat => {
            let msg = &body["choices"][0]["message"];
            if msg.is_null() {
                return Err(err("conntest.unexpected_answer")
                    .with("field", "choices[0].message")
                    .into());
            }
            let tool = msg["tool_calls"].as_array().is_some_and(|a| !a.is_empty());
            Ok((
                tool,
                msg["content"].as_str().unwrap_or_default().to_string(),
            ))
        }
    }
}

fn excerpt(s: &str) -> String {
    let t = s.trim();
    if t.chars().count() > 160 {
        format!("{}…", t.chars().take(160).collect::<String>())
    } else {
        t.to_string()
    }
}

/// Sends the test request to `base_url` (`http://host:port/v1`).
pub async fn test_connection(
    api: WireApi,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    client_name: &str,
) -> ConnectionTest {
    let (path, body) = request_for(api, model);
    let url = format!("{}{}", base_url.trim_end_matches('/'), path);
    let started = Instant::now();
    let mut result = ConnectionTest {
        ok: false,
        api,
        url: url.clone(),
        status: None,
        latency_ms: 0,
        tool_called: false,
        model: None,
        detail: String::new(),
    };
    let http = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        // A JIT load through the gateway can take a while.
        .timeout(Duration::from_secs(300))
        .no_proxy()
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            result.detail = crate::http_error::transport(&e, &url);
            return result;
        }
    };
    let mut req = http
        .post(&url)
        .header("x-harness-client", client_name)
        .json(&body);
    if api == WireApi::Anthropic {
        req = req.header("anthropic-version", "2023-06-01");
    }
    if let Some(k) = api_key.filter(|k| !k.trim().is_empty()) {
        req = req.bearer_auth(k);
    }
    let resp = req.send().await;
    result.latency_ms = started.elapsed().as_millis() as u64;
    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            result.detail = crate::http_error::transport(&e, &url);
            return result;
        }
    };
    let status = resp.status().as_u16();
    result.status = Some(status);
    let text = match resp.text().await {
        Ok(text) => text,
        Err(e) => {
            result.detail = crate::http_error::transport(&e, &url);
            return result;
        }
    };
    let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        result.detail = parsed["error"]["message"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| excerpt(&text));
        return result;
    }
    result.model = parsed["model"].as_str().map(str::to_string);
    match read_answer(api, &parsed) {
        Ok((tool, text)) => {
            result.ok = true;
            result.tool_called = tool;
            result.detail = excerpt(&text);
        }
        Err(e) => result.detail = e,
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shapes captured from llama-server b11236 through the gateway (e2e).
    #[test]
    fn answers_are_read_per_api() {
        let a = json!({"model": "m", "content": [{"type": "tool_use", "name": "get_weather", "input": {"city": "Paris"}}]});
        assert!(read_answer(WireApi::Anthropic, &a).unwrap().0);
        let r = json!({"output": [{"type": "message", "content": [{"type": "output_text", "text": "Sunny"}]}]});
        assert_eq!(
            read_answer(WireApi::Responses, &r).unwrap(),
            (false, "Sunny".to_string())
        );
        let c = json!({"choices": [{"message": {"content": null, "tool_calls": [{"id": "1"}]}}]});
        assert!(read_answer(WireApi::Chat, &c).unwrap().0);
        assert!(read_answer(WireApi::Chat, &json!({})).is_err());
        let (path, body) = request_for(WireApi::Responses, "x");
        assert_eq!(path, "/responses");
        assert_eq!(body["tools"][0]["type"], "function");
    }
}
