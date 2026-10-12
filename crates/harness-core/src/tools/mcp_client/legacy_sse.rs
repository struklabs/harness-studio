//! The older HTTP+SSE transport (MCP revision 2024-11-05): the client opens a `GET` event
//! stream on the server's SSE URL; the first event is `endpoint` and names the URL every
//! JSON-RPC message is `POST`ed to; the server's answers and notifications come back as
//! `message` events on the stream. Streamable HTTP replaced it, but many servers still speak
//! only this one.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Url;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::http::HttpSpec;
use super::sse::SseParser;
use super::{oauth, McpError};

/// How long the server may take to send its `endpoint` event.
const ENDPOINT_TIMEOUT: Duration = Duration::from_secs(20);

type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, McpError>>>>>;

/// The POST URL from an `endpoint` event's data: a path or a URL, resolved against the SSE URL.
/// It must stay on the server's origin: a stream that points the client at another host would
/// make it send its messages (and its token) there.
pub fn parse_endpoint(sse_url: &str, data: &str) -> Result<String, McpError> {
    let base = Url::parse(sse_url)
        .map_err(|e| McpError::Transport(format!("invalid URL \"{sse_url}\": {e}")))?;
    let data = data.trim();
    if data.is_empty() {
        return Err(McpError::Protocol("the endpoint event is empty".into()));
    }
    let url = base
        .join(data)
        .map_err(|e| McpError::Protocol(format!("invalid endpoint \"{data}\": {e}")))?;
    if url.origin() != base.origin() {
        return Err(McpError::Protocol(format!(
            "the server named an endpoint on another origin ({}); refused",
            url.origin().ascii_serialization()
        )));
    }
    Ok(url.into())
}

pub struct SseTransport {
    client: reqwest::Client,
    spec: HttpSpec,
    endpoint: String,
    pending: Pending,
    closed: Arc<AtomicBool>,
    reader: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for SseTransport {
    fn drop(&mut self) {
        if let Some(r) = self.reader.lock().unwrap_or_else(|e| e.into_inner()).take() {
            r.abort();
        }
    }
}

fn user_headers(spec: &HttpSpec) -> HeaderMap {
    let mut h = HeaderMap::new();
    for (k, v) in &spec.headers {
        if let (Ok(k), Ok(v)) = (HeaderName::try_from(k.as_str()), HeaderValue::from_str(v)) {
            h.insert(k, v);
        }
    }
    h
}

/// Adds the OAuth access token of the server, unless the user's own headers carry an
/// `Authorization`. Returns the token that was used.
pub(super) async fn add_bearer(
    spec: &HttpSpec,
    headers: &mut HeaderMap,
    rejected: Option<&str>,
) -> Option<String> {
    if headers.contains_key(AUTHORIZATION) {
        return None;
    }
    let name = spec.oauth.as_deref()?;
    let token = oauth::bearer(name, &spec.url, rejected).await?;
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}")).ok()?,
    );
    Some(token)
}

impl SseTransport {
    /// Opens the stream and waits for the `endpoint` event. Server notifications go to
    /// `notifications`.
    pub async fn connect(
        spec: HttpSpec,
        notifications: mpsc::UnboundedSender<Value>,
    ) -> Result<Self, McpError> {
        Url::parse(&spec.url)
            .map_err(|e| McpError::Transport(format!("invalid URL \"{}\": {e}", spec.url)))?;
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| McpError::Transport(crate::http_error::transport(&e, &spec.url)))?;
        let mut rejected: Option<String> = None;
        let resp = loop {
            let mut headers = user_headers(&spec);
            let sent = add_bearer(&spec, &mut headers, rejected.as_deref()).await;
            headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
            let resp = client
                .get(&spec.url)
                .headers(headers)
                .send()
                .await
                .map_err(|e| McpError::Transport(crate::http_error::transport(&e, &spec.url)))?;
            if resp.status().as_u16() == 401 && rejected.is_none() && sent.is_some() {
                rejected = sent;
                continue;
            }
            break resp;
        };
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(McpError::from_http(status.as_u16(), &text));
        }

        let pending: Pending = Arc::default();
        let closed = Arc::new(AtomicBool::new(false));
        let (endpoint_tx, endpoint_rx) = oneshot::channel::<Result<String, McpError>>();
        let reader = tokio::spawn(read_stream(
            resp,
            spec.clone(),
            client.clone(),
            pending.clone(),
            closed.clone(),
            notifications,
            endpoint_tx,
        ));
        let transport_reader = Mutex::new(Some(reader));
        let endpoint = match tokio::time::timeout(ENDPOINT_TIMEOUT, endpoint_rx).await {
            Ok(Ok(Ok(e))) => e,
            Ok(Ok(Err(e))) => return Err(abort(transport_reader, e)),
            Ok(Err(_)) => {
                return Err(abort(
                    transport_reader,
                    McpError::Protocol("the event stream ended before the endpoint event".into()),
                ))
            }
            Err(_) => {
                return Err(abort(
                    transport_reader,
                    McpError::Protocol(
                        "the server sent no endpoint event on its event stream".into(),
                    ),
                ))
            }
        };
        Ok(Self {
            client,
            spec,
            endpoint,
            pending,
            closed,
            reader: transport_reader,
        })
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    async fn post(&self, msg: &Value) -> Result<(), McpError> {
        let mut rejected: Option<String> = None;
        loop {
            let mut headers = user_headers(&self.spec);
            let sent = add_bearer(&self.spec, &mut headers, rejected.as_deref()).await;
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            let resp = self
                .client
                .post(&self.endpoint)
                .headers(headers)
                .body(msg.to_string())
                .send()
                .await
                .map_err(|e| {
                    McpError::Transport(crate::http_error::transport(&e, &self.endpoint))
                })?;
            let status = resp.status();
            if status.as_u16() == 401 && rejected.is_none() && sent.is_some() {
                rejected = sent;
                continue;
            }
            if status.is_success() {
                return Ok(());
            }
            let text = resp.text().await.unwrap_or_default();
            return Err(McpError::from_http(status.as_u16(), &text));
        }
    }

    /// Sends a request and waits for its answer on the event stream.
    pub async fn request(&self, id: i64, msg: &Value) -> Result<Value, McpError> {
        if self.is_closed() {
            return Err(McpError::Closed(String::new()));
        }
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, tx);
        // Gone when this future is dropped (timeout, cancel) or finished.
        let _forget = Forget {
            pending: self.pending.clone(),
            id,
        };
        self.post(msg).await?;
        match rx.await {
            Ok(r) => r,
            Err(_) => Err(McpError::Closed(String::new())),
        }
    }

    pub async fn notify(&self, msg: &Value) -> Result<(), McpError> {
        self.post(msg).await
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        if let Some(r) = self.reader.lock().unwrap_or_else(|e| e.into_inner()).take() {
            r.abort();
        }
    }
}

struct Forget {
    pending: Pending,
    id: i64,
}

impl Drop for Forget {
    fn drop(&mut self) {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

fn abort(reader: Mutex<Option<JoinHandle<()>>>, e: McpError) -> McpError {
    if let Some(r) = reader.lock().unwrap_or_else(|e| e.into_inner()).take() {
        r.abort();
    }
    e
}

/// Reads the stream until it ends: the endpoint event, then answers and notifications.
async fn read_stream(
    resp: reqwest::Response,
    spec: HttpSpec,
    client: reqwest::Client,
    pending: Pending,
    closed: Arc<AtomicBool>,
    notifications: mpsc::UnboundedSender<Value>,
    endpoint_tx: oneshot::Sender<Result<String, McpError>>,
) {
    let mut endpoint_tx = Some(endpoint_tx);
    let mut endpoint = String::new();
    let mut parser = SseParser::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else { break };
        for ev in parser.push(&chunk) {
            if ev.event.as_deref() == Some("endpoint") {
                if let Some(tx) = endpoint_tx.take() {
                    let parsed = parse_endpoint(&spec.url, &ev.data);
                    if let Ok(e) = &parsed {
                        endpoint = e.clone();
                    }
                    let _ = tx.send(parsed);
                }
                continue;
            }
            if !matches!(ev.event.as_deref(), None | Some("message")) {
                continue;
            }
            let Ok(m) = serde_json::from_str::<Value>(&ev.data) else {
                continue;
            };
            dispatch(&m, &pending, &notifications, &client, &spec, &endpoint);
        }
    }
    closed.store(true, Ordering::SeqCst);
    // Whoever still waits learns the connection is gone.
    let waiting: Vec<_> = pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .drain()
        .collect();
    for (_, tx) in waiting {
        let _ = tx.send(Err(McpError::Closed("the event stream ended".to_string())));
    }
}

fn dispatch(
    m: &Value,
    pending: &Pending,
    notifications: &mpsc::UnboundedSender<Value>,
    client: &reqwest::Client,
    spec: &HttpSpec,
    endpoint: &str,
) {
    if let Some(method) = m.get("method").and_then(Value::as_str) {
        match m.get("id") {
            None => {
                let _ = notifications.send(m.clone());
            }
            // A request from the server: `ping` is answered, the rest is not supported.
            Some(id) => {
                let reply = if method == "ping" {
                    json!({"jsonrpc": "2.0", "id": id, "result": {}})
                } else {
                    json!({"jsonrpc": "2.0", "id": id,
                           "error": {"code": -32601, "message": "Method not found"}})
                };
                let (client, spec, endpoint) = (client.clone(), spec.clone(), endpoint.to_string());
                tokio::spawn(async move {
                    let mut headers = user_headers(&spec);
                    add_bearer(&spec, &mut headers, None).await;
                    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                    let _ = client
                        .post(endpoint)
                        .headers(headers)
                        .body(reply.to_string())
                        .send()
                        .await;
                });
            }
        }
        return;
    }
    let Some(id) = m.get("id").and_then(Value::as_i64) else {
        return;
    };
    let waiting = pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    if let Some(tx) = waiting {
        let _ = tx.send(match (m.get("result"), m.get("error")) {
            (_, Some(e)) => Err(McpError::from_rpc_error(e)),
            (Some(r), None) => Ok(r.clone()),
            _ => Err(McpError::Protocol("response without result".into())),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_event_is_resolved_and_kept_on_the_origin() {
        assert_eq!(
            parse_endpoint("http://127.0.0.1:8000/sse", "/messages?sessionId=abc").unwrap(),
            "http://127.0.0.1:8000/messages?sessionId=abc"
        );
        assert_eq!(
            parse_endpoint("https://x.example/api/sse", "messages?s=1").unwrap(),
            "https://x.example/api/messages?s=1"
        );
        assert_eq!(
            parse_endpoint("https://x.example/sse", " https://x.example/post?id=2\n").unwrap(),
            "https://x.example/post?id=2"
        );
        // Another host or another port would receive the messages: refused.
        assert!(parse_endpoint("https://x.example/sse", "https://evil.example/post").is_err());
        assert!(parse_endpoint("http://127.0.0.1:8000/sse", "http://127.0.0.1:9/post").is_err());
        assert!(parse_endpoint("https://x.example/sse", "  ").is_err());
    }
}
