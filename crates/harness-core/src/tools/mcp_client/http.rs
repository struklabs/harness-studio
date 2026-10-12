//! The Streamable HTTP transport: every JSON-RPC message is its own `POST` to the MCP endpoint;
//! the answer is one JSON object or an SSE stream scoped to that request (progress
//! notifications, then the response).
//!
//! Both protocol eras are carried: the modern revision (2026-07-28) adds `Mcp-Method` /
//! `Mcp-Name` / `Mcp-Param-*` headers and has no session; the legacy revisions use the
//! `Mcp-Session-Id` the server assigns on `initialize` and an optional `GET` stream for server
//! notifications.

use std::sync::Mutex;

use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE};
use serde_json::Value;
use tokio::sync::mpsc;

use super::legacy_sse::add_bearer;
use super::sse::SseParser;
use super::{McpError, MODERN_VERSION};

/// Which HTTP transport to speak.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HttpMode {
    /// Streamable HTTP first; the older HTTP+SSE transport when the server answers the first
    /// request with a 4xx (the backward-compatibility rule of the MCP spec).
    #[default]
    Auto,
    Streamable,
    Sse,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpSpec {
    pub mode: HttpMode,
    pub url: String,
    /// Extra headers of every request (`Authorization`...), secrets resolved.
    pub headers: Vec<(String, String)>,
    /// The configured server's name when its OAuth sign-in (if any) supplies the
    /// `Authorization` header; a header of the user's own wins.
    pub oauth: Option<String>,
}

pub struct HttpTransport {
    client: reqwest::Client,
    spec: HttpSpec,
    session: Mutex<Option<String>>,
    /// Value of the `MCP-Protocol-Version` header (set after negotiation / for modern).
    version: Mutex<Option<String>>,
    notifications: mpsc::UnboundedSender<Value>,
}

const SESSION_HEADER: &str = "mcp-session-id";

impl HttpTransport {
    pub fn new(
        spec: HttpSpec,
        notifications: mpsc::UnboundedSender<Value>,
    ) -> Result<Self, McpError> {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| McpError::Transport(crate::http_error::transport(&e, &spec.url)))?;
        reqwest::Url::parse(&spec.url)
            .map_err(|e| McpError::Transport(format!("invalid URL \"{}\": {e}", spec.url)))?;
        Ok(Self {
            client,
            spec,
            session: Mutex::new(None),
            version: Mutex::new(None),
            notifications,
        })
    }

    pub fn set_version(&self, version: Option<String>) {
        *self.version.lock().unwrap_or_else(|e| e.into_inner()) = version;
    }

    fn version(&self) -> Option<String> {
        self.version
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn base_headers(&self, msg: Option<&Value>, extra: &[(String, String)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        let mut put = |k: &str, v: &str| {
            if let (Ok(k), Ok(v)) = (HeaderName::try_from(k), HeaderValue::from_str(v)) {
                h.insert(k, v);
            }
        };
        for (k, v) in &self.spec.headers {
            put(k, v);
        }
        if let Some(s) = self
            .session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_deref()
        {
            put("Mcp-Session-Id", s);
        }
        let version = self.version();
        if let Some(v) = &version {
            put("MCP-Protocol-Version", v);
        }
        if version.as_deref() == Some(MODERN_VERSION) {
            if let Some(msg) = msg {
                if let Some(m) = msg.get("method").and_then(Value::as_str) {
                    put("Mcp-Method", m);
                    if matches!(m, "tools/call" | "resources/read" | "prompts/get") {
                        let p = msg.get("params");
                        if let Some(n) = p
                            .and_then(|p| p.get("name").or_else(|| p.get("uri")))
                            .and_then(Value::as_str)
                        {
                            put("Mcp-Name", &super::annotations::encode_header_value(n));
                        }
                    }
                }
                for (k, v) in extra {
                    put(&format!("Mcp-Param-{k}"), v);
                }
            }
        }
        h
    }

    async fn post(
        &self,
        msg: &Value,
        extra: &[(String, String)],
    ) -> Result<reqwest::Response, McpError> {
        let mut rejected: Option<String> = None;
        let resp = loop {
            let mut headers = self.base_headers(Some(msg), extra);
            let sent = add_bearer(&self.spec, &mut headers, rejected.as_deref()).await;
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            headers.insert(
                ACCEPT,
                HeaderValue::from_static("application/json, text/event-stream"),
            );
            let resp = self
                .client
                .post(&self.spec.url)
                .headers(headers)
                .body(msg.to_string())
                .send()
                .await
                .map_err(|e| {
                    McpError::Transport(crate::http_error::transport(&e, &self.spec.url))
                })?;
            // A token the server refused is renewed once; a second 401 is the answer.
            if resp.status().as_u16() == 401 && rejected.is_none() && sent.is_some() {
                rejected = sent;
                continue;
            }
            break resp;
        };
        if let Some(s) = resp
            .headers()
            .get(SESSION_HEADER)
            .and_then(|v| v.to_str().ok())
        {
            *self.session.lock().unwrap_or_else(|e| e.into_inner()) = Some(s.to_string());
        }
        Ok(resp)
    }

    /// POSTs a request and reads its response (JSON, or an SSE stream up to the response).
    /// Dropping the future closes the connection: for the modern revision that is the
    /// cancellation signal.
    pub async fn request(
        &self,
        id: i64,
        msg: &Value,
        extra: &[(String, String)],
    ) -> Result<Value, McpError> {
        let resp = self.post(msg, extra).await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(McpError::from_http(status.as_u16(), &text));
        }
        let is_sse = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"));
        if !is_sse {
            let text = resp.text().await.map_err(|e| {
                McpError::Transport(crate::http_error::transport(&e, &self.spec.url))
            })?;
            let v: Value = serde_json::from_str(&text).map_err(|_| {
                McpError::Protocol(format!("not a JSON-RPC reply: {}", clip(&text)))
            })?;
            let msgs = match v {
                Value::Array(a) => a,
                other => vec![other],
            };
            for m in msgs {
                if let Some(r) = self.take_response(&m, id) {
                    return r;
                }
            }
            return Err(McpError::Protocol("reply without a response".into()));
        }
        let mut parser = SseParser::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                McpError::Transport(crate::http_error::transport(&e, &self.spec.url))
            })?;
            for ev in parser.push(&chunk) {
                let Ok(m) = serde_json::from_str::<Value>(&ev.data) else {
                    continue;
                };
                if let Some(r) = self.take_response(&m, id) {
                    return r;
                }
            }
        }
        Err(McpError::Protocol(
            "the response stream ended without a response".into(),
        ))
    }

    /// `Some(result)` when `m` is the response to `id`; notifications are forwarded.
    fn take_response(&self, m: &Value, id: i64) -> Option<Result<Value, McpError>> {
        if m.get("method").is_some() {
            if m.get("id").is_none() {
                let _ = self.notifications.send(m.clone());
            }
            return None;
        }
        if m.get("id").and_then(Value::as_i64) != Some(id) {
            return None;
        }
        Some(match (m.get("result"), m.get("error")) {
            (_, Some(e)) => Err(McpError::from_rpc_error(e)),
            (Some(r), None) => Ok(r.clone()),
            _ => Err(McpError::Protocol("response without result".into())),
        })
    }

    pub async fn notify(&self, msg: &Value) -> Result<(), McpError> {
        let resp = self.post(msg, &[]).await?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        let text = resp.text().await.unwrap_or_default();
        Err(McpError::from_http(status.as_u16(), &text))
    }

    /// Legacy revisions: the standalone `GET` stream for server notifications. Returns when the
    /// stream ends; `Ok(false)` when the server offers none (405 and the like).
    pub async fn listen_get(&self) -> Result<bool, McpError> {
        let mut headers = self.base_headers(None, &[]);
        add_bearer(&self.spec, &mut headers, None).await;
        headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
        let resp = self
            .client
            .get(&self.spec.url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| McpError::Transport(crate::http_error::transport(&e, &self.spec.url)))?;
        if !resp.status().is_success() {
            return Ok(false);
        }
        let mut parser = SseParser::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                McpError::Transport(crate::http_error::transport(&e, &self.spec.url))
            })?;
            for ev in parser.push(&chunk) {
                if let Ok(m) = serde_json::from_str::<Value>(&ev.data) {
                    if m.get("method").is_some() && m.get("id").is_none() {
                        let _ = self.notifications.send(m);
                    }
                }
            }
        }
        Ok(true)
    }

    /// Ends a legacy session (`DELETE`); a server that does not allow it is not an error.
    pub async fn close(&self) {
        let session = self
            .session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if session.is_none() {
            return;
        }
        let headers = {
            let mut h = self.base_headers(None, &[]);
            add_bearer(&self.spec, &mut h, None).await;
            if let Some(s) = &session {
                if let Ok(v) = HeaderValue::from_str(s) {
                    h.insert(HeaderName::from_static(SESSION_HEADER), v);
                }
            }
            h
        };
        let _ = self
            .client
            .delete(&self.spec.url)
            .headers(headers)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await;
    }
}

fn clip(s: &str) -> String {
    let t: String = s.chars().take(200).collect();
    t
}
