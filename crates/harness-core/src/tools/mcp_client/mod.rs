//! Harness's own MCP client: the tools of MCP servers become Harness
//! tools, run by the same engine, policy and log as the built-in ones, for every runtime and
//! every surface; nothing depends on a llama-server being launched with
//! `--mcp-servers-config` any more.
//!
//! # Protocol
//! Hand-written, dual-era, like the server side (`harness-mcp`): no SDK, because only a small
//! part of the protocol is needed (tools, list changes, cancellation), the Windows process-tree
//! handling must be ours, and the official Rust SDK changes quickly (see `docs/en/MCP.md`).
//!
//! * **2026-07-28** (modern, latest revision): no handshake, every request carries
//!   `_meta.io.modelcontextprotocol/{protocolVersion,clientInfo,clientCapabilities}`;
//!   `server/discover` probes the era; list changes arrive on a `subscriptions/listen` stream;
//!   Streamable HTTP adds `Mcp-Method` / `Mcp-Name` / `Mcp-Param-*` headers.
//! * **2025-11-25 ... 2024-11-05** (legacy, what most servers speak today): `initialize` /
//!   `notifications/initialized`; we offer `2025-11-25` and accept what the server answers with
//!   when we know it; Streamable HTTP keeps the `Mcp-Session-Id`.
//!
//! Era detection follows the spec's backward-compatibility rules: stdio probes with
//! `server/discover` (anything but a modern answer, or silence for 5 s, means legacy);
//! HTTP tries it and falls back to `initialize` unless the `400` carries a recognised modern
//! error. A server that is modern but shares no version with us is an error, not a fallback.
//!
//! # Transports
//! [`stdio`] (child process, own process group / Job Object, no console window), [`http`]
//! (Streamable HTTP, JSON or SSE replies) and [`legacy_sse`] (the older HTTP+SSE transport of
//! 2024-11-05: an event stream whose first event names the URL to `POST` to). For a URL server
//! `transport` in its definition picks one; `auto` (the default) tries Streamable HTTP and,
//! when the server answers the first request with a 4xx (not 401/403), opens the SSE stream
//! instead, as the MCP backward-compatibility rules say.
//!
//! # Authorization
//! A server behind OAuth answers `401`; [`oauth`] runs the MCP authorization flow (discovery,
//! dynamic client registration, PKCE through the system browser, `resource` parameter) and the
//! transports send the stored access token, renewing it when it expires or is refused. A token
//! in the server's own `headers` takes precedence.
//!
//! # Resources
//! `resources/list`, `resources/templates/list` and `resources/read` are available
//! ([`McpConnection::list_resources`] and friends) for the UI and the CLI; the model does not
//! see resources by itself, a resource reaches a chat when the user attaches it.
//!
//! # What the client does not offer
//! No `roots`, `sampling` or `elicitation` capability: servers that need them get a
//! `Method not found` and, in the modern revision, a call that needs input fails with a clear
//! message. Prompts are not used.

pub mod annotations;
mod http;
pub mod hub;
mod legacy_sse;
pub mod oauth;
mod sse;
mod stdio;
#[cfg(test)]
pub(crate) mod testserver;

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::registry::CancelToken;
use super::types::{ToolError, ToolErrorCode, ToolOutput};
pub use hub::{McpAuthState, McpHub, McpServerState, McpServerStatus};

/// The latest revision (stateless, per-request `_meta`).
pub const MODERN_VERSION: &str = "2026-07-28";
/// `initialize`-based revisions we understand, newest first.
pub const LEGACY_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_INFO: &str = "io.modelcontextprotocol/clientInfo";
const META_CAPS: &str = "io.modelcontextprotocol/clientCapabilities";

/// How long a stdio server may stay silent on the `server/discover` probe before we assume it
/// is a legacy server.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// Most pages of `tools/list` followed (a server that never ends its cursor chain).
const MAX_PAGES: usize = 100;

#[derive(Debug, Clone, PartialEq)]
pub enum McpError {
    /// A JSON-RPC error answer.
    Rpc {
        code: i64,
        message: String,
        data: Option<Value>,
    },
    /// An HTTP error status (with the JSON-RPC error of its body, when it had one).
    Http {
        status: u16,
        code: Option<i64>,
        message: String,
        data: Option<Value>,
    },
    Transport(String),
    Timeout,
    Cancelled,
    /// The server process exited / the connection is gone (with what it said on stderr).
    Closed(String),
    Protocol(String),
}

impl McpError {
    pub(crate) fn from_rpc_error(e: &Value) -> Self {
        McpError::Rpc {
            code: e.get("code").and_then(Value::as_i64).unwrap_or(-32603),
            message: e
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
                .to_string(),
            data: e.get("data").cloned(),
        }
    }

    pub(crate) fn from_http(status: u16, body: &str) -> Self {
        let rpc = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| v.get("error").cloned());
        let (code, message, data) = match &rpc {
            Some(e) => (
                e.get("code").and_then(Value::as_i64),
                e.get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                e.get("data").cloned(),
            ),
            None => (None, String::new(), None),
        };
        let message = if !message.is_empty() {
            message
        } else if status == 401 || status == 403 {
            format!(
                "the server requires authorization (HTTP {status}); authorize it in Connections \
                 > MCP servers or with `harness tools mcp-authorize`, or put a token in the \
                 server's headers"
            )
        } else {
            let t: String = body.trim().chars().take(200).collect();
            if t.is_empty() {
                format!("HTTP {status}")
            } else {
                format!("HTTP {status}: {t}")
            }
        };
        McpError::Http {
            status,
            code,
            message,
            data,
        }
    }

    /// The JSON-RPC code, when the error carried one.
    pub fn code(&self) -> Option<i64> {
        match self {
            McpError::Rpc { code, .. } => Some(*code),
            McpError::Http { code, .. } => *code,
            _ => None,
        }
    }

    /// The server is gone or unusable (the hub reconnects).
    pub fn is_connection_lost(&self) -> bool {
        matches!(self, McpError::Closed(_))
            || matches!(self, McpError::Http { status: 404, code, .. } if *code != Some(-32601))
    }
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpError::Rpc { code, message, .. } => write!(f, "{message} (MCP error {code})"),
            McpError::Http { message, .. } => f.write_str(message),
            McpError::Transport(m) | McpError::Protocol(m) => f.write_str(m),
            McpError::Timeout => f.write_str("the MCP server did not answer in time"),
            McpError::Cancelled => f.write_str("cancelled"),
            McpError::Closed(tail) if tail.trim().is_empty() => {
                f.write_str("the MCP server closed the connection")
            }
            McpError::Closed(tail) => write!(
                f,
                "the MCP server closed the connection; it said: {}",
                tail.trim()
            ),
        }
    }
}

impl std::error::Error for McpError {}

/// The protocol era a connection speaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Era {
    /// 2026-07-28 or later.
    Modern(String),
    /// `initialize`-based; the negotiated version.
    Legacy(String),
}

impl Era {
    pub fn version(&self) -> &str {
        match self {
            Era::Modern(v) | Era::Legacy(v) => v,
        }
    }
}

/// How to reach one server.
#[derive(Debug, Clone, PartialEq)]
pub enum McpTarget {
    Stdio(stdio::StdioSpec),
    Http(http::HttpSpec),
}

impl McpTarget {
    /// From a configured server (secrets already resolved): `command` = stdio, `url` = HTTP.
    pub fn from_def(def: &crate::mcp::McpServerDef) -> Result<Self, McpError> {
        match (&def.command, &def.url) {
            (Some(command), None) if !command.trim().is_empty() => {
                // Windows cannot start `npx` / `uvx` style `.cmd` shims directly.
                let def = crate::mcp::windows_wrap(def);
                Ok(McpTarget::Stdio(stdio::StdioSpec {
                    command: def.command.clone().unwrap_or_else(|| command.clone()),
                    args: def.args,
                    env: def.env.into_iter().collect(),
                    cwd: None,
                }))
            }
            (None, Some(url)) => Ok(McpTarget::Http(http::HttpSpec {
                mode: match def.transport.as_deref() {
                    Some("http") => http::HttpMode::Streamable,
                    Some("sse") => http::HttpMode::Sse,
                    _ => http::HttpMode::Auto,
                },
                url: url.clone(),
                headers: def
                    .headers
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
                oauth: None,
            })),
            _ => Err(McpError::Protocol(
                "a server needs either a command or a URL".into(),
            )),
        }
    }

    /// [`Self::from_def`] for the configured server `name`: an HTTP server also sends the
    /// access token of its OAuth sign-in (see [`oauth`]), if it has one.
    pub fn for_server(name: &str, def: &crate::mcp::McpServerDef) -> Result<Self, McpError> {
        let mut target = Self::from_def(def)?;
        if let McpTarget::Http(spec) = &mut target {
            spec.oauth = Some(name.to_string());
        }
        Ok(target)
    }
}

/// A resource the server offers (`resources/list`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RemoteResource {
    pub uri: String,
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub mime_type: Option<String>,
    pub size: Option<u64>,
}

impl RemoteResource {
    fn parse(v: &Value) -> Option<Self> {
        let uri = v.get("uri")?.as_str()?.to_string();
        let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        Some(Self {
            name: text("name").unwrap_or_else(|| uri.clone()),
            title: text("title"),
            description: text("description"),
            mime_type: text("mimeType"),
            size: v.get("size").and_then(Value::as_u64),
            uri,
        })
    }
}

/// A parameterised resource (`resources/templates/list`, an RFC 6570 URI template).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RemoteResourceTemplate {
    pub uri_template: String,
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

impl RemoteResourceTemplate {
    fn parse(v: &Value) -> Option<Self> {
        let uri_template = v.get("uriTemplate")?.as_str()?.to_string();
        let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        Some(Self {
            name: text("name").unwrap_or_else(|| uri_template.clone()),
            title: text("title"),
            description: text("description"),
            mime_type: text("mimeType"),
            uri_template,
        })
    }
}

/// One piece of a resource read (`resources/read` returns a list of them).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ResourceContent {
    pub uri: String,
    pub mime_type: Option<String>,
    /// Text contents.
    pub text: Option<String>,
    /// Binary contents, base64.
    pub blob: Option<String>,
}

impl ResourceContent {
    fn parse(v: &Value) -> Option<Self> {
        let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        let (t, b) = (text("text"), text("blob"));
        if t.is_none() && b.is_none() {
            return None;
        }
        Some(Self {
            uri: text("uri").unwrap_or_default(),
            mime_type: text("mimeType"),
            text: t,
            blob: b,
        })
    }
}

/// The resources and templates of one server.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ResourceList {
    pub resources: Vec<RemoteResource>,
    pub templates: Vec<RemoteResourceTemplate>,
}

/// The file a resource becomes when the user attaches it to a chat as a document: its name
/// (with an extension the document reader knows, from the name or the MIME type) and bytes.
/// Text pieces are joined; a resource that is only binary is kept as it is when it is a PDF and
/// refused otherwise.
pub fn resource_as_file(
    uri: &str,
    name: Option<&str>,
    contents: &[ResourceContent],
) -> Result<(String, Vec<u8>), String> {
    use base64::Engine;
    let texts: Vec<&str> = contents.iter().filter_map(|c| c.text.as_deref()).collect();
    let mime = contents
        .iter()
        .find_map(|c| c.mime_type.as_deref())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let (bytes, ext) = if !texts.is_empty() {
        let ext = match mime.as_str() {
            "text/markdown" => "md",
            "application/json" => "json",
            "text/html" => "html",
            "text/csv" => "csv",
            "text/x-python" => "py",
            "application/xml" | "text/xml" => "xml",
            _ => "txt",
        };
        (texts.join("\n\n").into_bytes(), ext)
    } else if let Some(blob) = contents.iter().find_map(|c| c.blob.as_deref()) {
        if mime != "application/pdf" {
            return Err(format!(
                "the resource {uri} is binary data ({}) that Harness cannot read as a document",
                if mime.is_empty() {
                    "unknown type"
                } else {
                    &mime
                }
            ));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(blob.trim())
            .map_err(|e| format!("the resource {uri} has invalid base64 data: {e}"))?;
        (bytes, "pdf")
    } else {
        return Err(format!("the resource {uri} is empty"));
    };
    let from_uri = uri
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("");
    let stem = crate::paths::sanitize_file_name(name.unwrap_or(""))
        .or_else(|| crate::paths::sanitize_file_name(from_uri))
        .unwrap_or_else(|| "resource".to_string());
    let known = std::path::Path::new(&stem)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            crate::rag::extract::supported_extensions()
                .iter()
                .any(|x| x.eq_ignore_ascii_case(e))
        });
    let file = if known { stem } else { format!("{stem}.{ext}") };
    Ok((file, bytes))
}

/// The `resources` of a `resources/list` result.
pub fn parse_resources(result: &Value) -> Vec<RemoteResource> {
    result
        .get("resources")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(RemoteResource::parse).collect())
        .unwrap_or_default()
}

/// The `resourceTemplates` of a `resources/templates/list` result.
pub fn parse_resource_templates(result: &Value) -> Vec<RemoteResourceTemplate> {
    result
        .get("resourceTemplates")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(RemoteResourceTemplate::parse).collect())
        .unwrap_or_default()
}

/// The `contents` of a `resources/read` result.
pub fn parse_resource_contents(result: &Value) -> Vec<ResourceContent> {
    result
        .get("contents")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(ResourceContent::parse).collect())
        .unwrap_or_default()
}

/// One tool as the server describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteTool {
    pub name: String,
    pub title: Option<String>,
    pub description: String,
    pub input_schema: Value,
    pub annotations: Option<Value>,
    /// `x-mcp-header` parameters (Streamable HTTP, modern revision).
    pub header_params: Vec<annotations::HeaderParam>,
}

impl RemoteTool {
    fn parse(v: &Value) -> Option<Self> {
        let name = v.get("name")?.as_str()?.to_string();
        let schema = match v.get("inputSchema") {
            Some(s) if s.is_object() => s.clone(),
            _ => json!({"type": "object", "properties": {}}),
        };
        Some(Self {
            name,
            title: v.get("title").and_then(Value::as_str).map(str::to_string),
            description: v
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            input_schema: schema,
            annotations: v.get("annotations").filter(|a| a.is_object()).cloned(),
            header_params: Vec::new(),
        })
    }
}

enum Transport {
    Stdio(Box<stdio::StdioTransport>),
    Http(http::HttpTransport),
    /// The older HTTP+SSE transport.
    Sse(Box<legacy_sse::SseTransport>),
}

/// Connection options.
#[derive(Clone)]
pub struct ConnectOptions {
    /// The whole connect (spawn, handshake, first tool list).
    pub timeout: Duration,
    /// Called when the server's tool list changed and was fetched again.
    pub on_tools_changed: Arc<dyn Fn() + Send + Sync>,
    /// Do not probe with `server/discover`: speak the `initialize` protocol straight away
    /// (for servers that misbehave on the probe, and for tests of the legacy path).
    pub legacy_only: bool,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(60),
            on_tools_changed: Arc::new(|| {}),
            legacy_only: false,
        }
    }
}

/// State shared with the background tasks (they must not keep the [`McpConnection`] handle
/// alive: dropping the handle is what stops the server).
struct Shared {
    name: String,
    transport: Transport,
    era: RwLock<Era>,
    next_id: AtomicI64,
    tools: Mutex<Vec<RemoteTool>>,
    capabilities: Mutex<Value>,
    server_info: Mutex<Option<Value>>,
    instructions: Mutex<Option<String>>,
    broken: AtomicBool,
    on_tools_changed: Arc<dyn Fn() + Send + Sync>,
}

/// A live connection to one MCP server. Dropping it ends the server (stdio: the whole process
/// tree).
pub struct McpConnection {
    s: Arc<Shared>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl Drop for McpConnection {
    fn drop(&mut self) {
        for t in self
            .tasks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
        {
            t.abort();
        }
    }
}

fn client_info() -> Value {
    json!({"name": "harness-studio", "title": "Harness Studio", "version": env!("CARGO_PKG_VERSION")})
}

/// Tells the server a request is no longer wanted when its future ends before the response.
struct InFlight {
    s: Arc<Shared>,
    id: i64,
    era: Era,
    armed: bool,
    reason: &'static str,
}

impl Drop for InFlight {
    fn drop(&mut self) {
        // Modern Streamable HTTP: the dropped response stream already cancelled it.
        if !self.armed || (self.s.is_http() && matches!(self.era, Era::Modern(_))) {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let (s, id, reason) = (self.s.clone(), self.id, self.reason);
        rt.spawn(async move {
            let msg = json!({
                "jsonrpc": "2.0",
                "method": "notifications/cancelled",
                "params": {"requestId": id, "reason": reason},
            });
            let _ = tokio::time::timeout(Duration::from_secs(2), s.notify(&msg)).await;
        });
    }
}

impl Shared {
    fn era(&self) -> Era {
        self.era.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn modern_meta() -> Value {
        json!({
            META_VERSION: MODERN_VERSION,
            META_INFO: client_info(),
            META_CAPS: {},
        })
    }

    /// `params` with the era's request metadata added.
    fn with_meta(era: &Era, mut params: Value) -> Value {
        if matches!(era, Era::Modern(_)) {
            if !params.is_object() {
                params = json!({});
            }
            params["_meta"] = Self::modern_meta();
        }
        params
    }

    fn is_http(&self) -> bool {
        matches!(self.transport, Transport::Http(_))
    }

    async fn send(
        &self,
        id: i64,
        msg: &Value,
        extra: &[(String, String)],
    ) -> Result<Value, McpError> {
        match &self.transport {
            Transport::Stdio(t) => t.request(id, msg).await,
            Transport::Http(t) => t.request(id, msg, extra).await,
            Transport::Sse(t) => t.request(id, msg).await,
        }
    }

    async fn notify(&self, msg: &Value) -> Result<(), McpError> {
        match &self.transport {
            Transport::Stdio(t) => t.notify(msg),
            Transport::Http(t) => t.notify(msg).await,
            Transport::Sse(t) => t.notify(msg).await,
        }
    }

    /// One request with a time limit and cancellation. However the request ends early (a
    /// timeout, a cancel, or the caller dropping the future), the server is told
    /// (`notifications/cancelled`; over modern HTTP, dropping the response stream is the signal).
    async fn rpc_in(
        self: &Arc<Self>,
        era: &Era,
        method: &str,
        params: Value,
        timeout: Option<Duration>,
        cancel: Option<&CancelToken>,
        extra: &[(String, String)],
    ) -> Result<Value, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": Self::with_meta(era, params),
        });
        let mut guard = InFlight {
            s: self.clone(),
            id,
            era: era.clone(),
            // The spec forbids cancelling `initialize`; a long-lived listen is ended by closing.
            armed: method != "initialize",
            reason: "cancelled",
        };
        let sleep = async {
            match timeout {
                Some(t) => tokio::time::sleep(t).await,
                None => std::future::pending().await,
            }
        };
        let cancelled = async {
            match cancel {
                Some(c) => c.cancelled().await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            r = self.send(id, &msg, extra) => {
                guard.armed = false;
                self.note_result(r)
            }
            _ = sleep => {
                guard.reason = "timeout";
                Err(McpError::Timeout)
            }
            _ = cancelled => Err(McpError::Cancelled),
        }
    }

    fn note_result(&self, r: Result<Value, McpError>) -> Result<Value, McpError> {
        if let Err(e) = &r {
            if e.is_connection_lost() {
                self.broken.store(true, Ordering::SeqCst);
            }
        }
        r
    }

    async fn rpc(
        self: &Arc<Self>,
        method: &str,
        params: Value,
        timeout: Option<Duration>,
        cancel: Option<&CancelToken>,
        extra: &[(String, String)],
    ) -> Result<Value, McpError> {
        let era = self.era();
        self.rpc_in(&era, method, params, timeout, cancel, extra)
            .await
    }

    /// `tools/list`, following `nextCursor`.
    async fn list_tools(self: &Arc<Self>, timeout: Duration) -> Result<Vec<RemoteTool>, McpError> {
        let mut out: Vec<RemoteTool> = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let params = match &cursor {
                Some(c) => json!({"cursor": c}),
                None => json!({}),
            };
            let r = self
                .rpc("tools/list", params, Some(timeout), None, &[])
                .await?;
            if let Some(list) = r.get("tools").and_then(Value::as_array) {
                for t in list {
                    if let Some(tool) = RemoteTool::parse(t) {
                        out.push(tool);
                    }
                }
            }
            match r.get("nextCursor").and_then(Value::as_str) {
                Some(c) if !c.is_empty() => cursor = Some(c.to_string()),
                _ => return Ok(self.vet_tools(out)),
            }
        }
        Err(McpError::Protocol(format!(
            "tools/list did not end after {MAX_PAGES} pages"
        )))
    }

    /// Modern Streamable HTTP: a tool whose `x-mcp-header` annotations are invalid is dropped.
    fn vet_tools(&self, tools: Vec<RemoteTool>) -> Vec<RemoteTool> {
        let modern_http = self.is_http() && matches!(self.era(), Era::Modern(_));
        if !modern_http {
            return tools;
        }
        tools
            .into_iter()
            .filter_map(|mut t| match annotations::header_params(&t.input_schema) {
                Ok(p) => {
                    t.header_params = p;
                    Some(t)
                }
                Err(why) => {
                    eprintln!(
                        "[Harness MCP] {}: tool \"{}\" dropped: {why}",
                        self.name, t.name
                    );
                    None
                }
            })
            .collect()
    }

    async fn refresh_tools(self: &Arc<Self>) -> Result<(), McpError> {
        let tools = self.list_tools(Duration::from_secs(30)).await?;
        *self.tools.lock().unwrap_or_else(|e| e.into_inner()) = tools;
        Ok(())
    }
}

fn caps_have_list_changed(caps: &Value) -> bool {
    caps.get("tools")
        .and_then(|t| t.get("listChanged"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

impl McpConnection {
    /// Connects, negotiates the protocol, lists the tools and starts the background tasks.
    pub async fn connect(
        name: &str,
        target: McpTarget,
        opts: ConnectOptions,
    ) -> Result<Arc<Self>, McpError> {
        match tokio::time::timeout(
            opts.timeout,
            Self::connect_inner(name, target, opts.clone()),
        )
        .await
        {
            Ok(r) => r,
            Err(_) => Err(McpError::Transport(format!(
                "the MCP server did not become ready within {} seconds",
                opts.timeout.as_secs()
            ))),
        }
    }

    async fn connect_inner(
        name: &str,
        target: McpTarget,
        opts: ConnectOptions,
    ) -> Result<Arc<Self>, McpError> {
        let (shared, mut note_rx) = match &target {
            McpTarget::Stdio(spec) => {
                let (note_tx, note_rx) = mpsc::unbounded_channel::<Value>();
                let transport =
                    Transport::Stdio(Box::new(stdio::StdioTransport::spawn(spec, note_tx)?));
                (establish(name, transport, &opts, false).await?, note_rx)
            }
            McpTarget::Http(spec) => match spec.mode {
                http::HttpMode::Streamable => establish_http(name, spec, &opts, false).await?,
                http::HttpMode::Sse => establish_http(name, spec, &opts, true).await?,
                http::HttpMode::Auto => match establish_http(name, spec, &opts, false).await {
                    Ok(done) => done,
                    // The MCP backward-compatibility rule: a 4xx for the first POST means an
                    // older server, which speaks HTTP+SSE on this URL.
                    Err(e) if should_try_sse(&e) => {
                        match establish_http(name, spec, &opts, true).await {
                            Ok(done) => done,
                            Err(sse_err) => {
                                return Err(McpError::Transport(format!(
                                    "Streamable HTTP failed ({e}); HTTP+SSE failed too ({sse_err})"
                                )))
                            }
                        }
                    }
                    Err(e) => return Err(e),
                },
            },
        };

        let caps = shared
            .capabilities
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if caps.get("tools").is_some() {
            shared.refresh_tools().await?;
        }

        let conn = Arc::new(McpConnection {
            s: shared.clone(),
            tasks: Mutex::new(Vec::new()),
        });
        let mut tasks = Vec::new();

        // Notifications: a list change fetches the tools again.
        {
            let sh = shared.clone();
            tasks.push(tokio::spawn(async move {
                while let Some(n) = note_rx.recv().await {
                    if n.get("method").and_then(Value::as_str)
                        != Some("notifications/tools/list_changed")
                    {
                        continue;
                    }
                    // A burst of notifications is one refresh.
                    while note_rx.try_recv().is_ok() {}
                    if sh.refresh_tools().await.is_ok() {
                        (sh.on_tools_changed)();
                    }
                }
            }));
        }

        if caps_have_list_changed(&caps) {
            let sh = shared.clone();
            match sh.era() {
                Era::Modern(_) => tasks.push(tokio::spawn(async move {
                    // A long-lived request: the answer is the stream; it ends with the
                    // connection.
                    let _ = sh
                        .rpc(
                            "subscriptions/listen",
                            json!({"notifications": {"toolsListChanged": true}}),
                            None,
                            None,
                            &[],
                        )
                        .await;
                })),
                Era::Legacy(_) => {
                    if let Transport::Http(_) = &sh.transport {
                        tasks.push(tokio::spawn(async move {
                            // The standalone GET stream; a few reconnects, then give up
                            // (the list is still re-read on a failed call).
                            for _ in 0..5 {
                                let Transport::Http(t) = &sh.transport else {
                                    return;
                                };
                                match t.listen_get().await {
                                    Ok(false) => return,
                                    _ => tokio::time::sleep(Duration::from_secs(2)).await,
                                }
                            }
                        }));
                    }
                    // stdio: notifications already arrive on the pipe.
                }
            }
        }
        *conn.tasks.lock().unwrap_or_else(|e| e.into_inner()) = tasks;
        Ok(conn)
    }

    pub fn name(&self) -> &str {
        &self.s.name
    }

    pub fn era(&self) -> Era {
        self.s.era()
    }

    pub fn tools(&self) -> Vec<RemoteTool> {
        self.s
            .tools
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn server_info(&self) -> Option<Value> {
        self.s
            .server_info
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn instructions(&self) -> Option<String> {
        self.s
            .instructions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Process id of a stdio server.
    pub fn pid(&self) -> Option<u32> {
        match &self.s.transport {
            Transport::Stdio(t) => t.pid(),
            Transport::Http(_) | Transport::Sse(_) => None,
        }
    }

    /// The connection can no longer be used (process exited, session gone).
    pub fn is_broken(&self) -> bool {
        self.s.broken.load(Ordering::SeqCst)
            || match &self.s.transport {
                Transport::Stdio(t) => t.is_closed(),
                Transport::Sse(t) => t.is_closed(),
                Transport::Http(_) => false,
            }
    }

    /// Fetches the tool list again.
    pub async fn refresh_tools(&self) -> Result<Vec<RemoteTool>, McpError> {
        self.s.refresh_tools().await?;
        Ok(self.tools())
    }

    /// Calls a tool. A tool that ran and failed is `Err(ToolError)` with the server's text;
    /// protocol problems, timeouts and cancellation are errors with their own codes.
    pub async fn call_tool(
        &self,
        tool: &str,
        args: Value,
        timeout: Duration,
        cancel: &CancelToken,
    ) -> Result<ToolOutput, ToolError> {
        self.call_tool_once(tool, args, timeout, cancel)
            .await
            .map_err(|(e, _)| e)
    }

    /// Like [`call_tool`](Self::call_tool); the flag on an error says the connection was lost
    /// before any answer for this request arrived (the caller may reconnect and try again).
    pub async fn call_tool_once(
        &self,
        tool: &str,
        args: Value,
        timeout: Duration,
        cancel: &CancelToken,
    ) -> Result<ToolOutput, (ToolError, bool)> {
        let extra = self
            .s
            .tools
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|t| t.name == tool)
            .map(|t| annotations::header_values(&t.header_params, &args))
            .unwrap_or_default();
        let r = self
            .s
            .rpc(
                "tools/call",
                json!({"name": tool, "arguments": args}),
                Some(timeout),
                Some(cancel),
                &extra,
            )
            .await;
        match r {
            Ok(result) => tool_result(&result).map_err(|e| (e, false)),
            Err(McpError::Timeout) => Err((
                ToolError::new(
                    ToolErrorCode::Timeout,
                    format!(
                        "the tool did not finish within {} seconds and was stopped",
                        timeout.as_secs().max(1)
                    ),
                ),
                false,
            )),
            Err(McpError::Cancelled) => Err((
                ToolError::new(ToolErrorCode::Cancelled, "the call was cancelled"),
                false,
            )),
            Err(e @ McpError::Rpc { .. }) if e.code() == Some(-32602) => {
                Err((ToolError::invalid(e.to_string()), false))
            }
            Err(McpError::Transport(message)) if crate::error_key::parse(&message).is_some() => {
                Err((ToolError::failed(message), false))
            }
            Err(e) => {
                let lost = e.is_connection_lost();
                Err((ToolError::failed(format!("MCP server error: {e}")), lost))
            }
        }
    }

    /// Ends the server: stdin closes, a server that does not exit within `grace` is killed.
    pub async fn close(&self, grace: Duration) {
        for t in self
            .tasks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
        {
            t.abort();
        }
        match &self.s.transport {
            Transport::Stdio(t) => t.close(grace).await,
            Transport::Http(t) => t.close().await,
            Transport::Sse(t) => t.close(),
        }
    }

    /// `stdio`, `http` (Streamable HTTP) or `sse` (the older HTTP+SSE transport): what the
    /// connection actually uses, which for an `auto` server is what detection found.
    pub fn transport_name(&self) -> &'static str {
        match &self.s.transport {
            Transport::Stdio(_) => "stdio",
            Transport::Http(_) => "http",
            Transport::Sse(_) => "sse",
        }
    }

    /// The server declared the `resources` capability.
    pub fn has_resources(&self) -> bool {
        self.s
            .capabilities
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get("resources")
            .is_some()
    }

    /// `resources/list` (all pages) and `resources/templates/list`. A server that has no
    /// templates, or does not know the method, simply has none.
    pub async fn list_resources(&self) -> Result<ResourceList, McpError> {
        const TIMEOUT: Duration = Duration::from_secs(30);
        let mut resources = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let params = match &cursor {
                Some(c) => json!({"cursor": c}),
                None => json!({}),
            };
            let r = self
                .s
                .rpc("resources/list", params, Some(TIMEOUT), None, &[])
                .await?;
            resources.extend(parse_resources(&r));
            match r.get("nextCursor").and_then(Value::as_str) {
                Some(c) if !c.is_empty() => cursor = Some(c.to_string()),
                _ => break,
            }
        }
        let mut templates = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let params = match &cursor {
                Some(c) => json!({"cursor": c}),
                None => json!({}),
            };
            match self
                .s
                .rpc("resources/templates/list", params, Some(TIMEOUT), None, &[])
                .await
            {
                Ok(r) => {
                    templates.extend(parse_resource_templates(&r));
                    match r.get("nextCursor").and_then(Value::as_str) {
                        Some(c) if !c.is_empty() => cursor = Some(c.to_string()),
                        _ => break,
                    }
                }
                Err(e) if matches!(e.code(), Some(-32601 | -32602)) => break,
                Err(e) => return Err(e),
            }
        }
        Ok(ResourceList {
            resources,
            templates,
        })
    }

    /// `resources/read`: the pieces of one resource.
    pub async fn read_resource(&self, uri: &str) -> Result<Vec<ResourceContent>, McpError> {
        let r = self
            .s
            .rpc(
                "resources/read",
                json!({"uri": uri}),
                Some(Duration::from_secs(60)),
                None,
                &[],
            )
            .await?;
        Ok(parse_resource_contents(&r))
    }
}

/// 4xx answers that mean "this URL is not a Streamable HTTP endpoint" (so the HTTP+SSE
/// transport is tried); not the ones that mean "you may not": authorization, a throttle.
fn should_try_sse(e: &McpError) -> bool {
    matches!(e, McpError::Http { status, .. }
        if (400..500).contains(status) && !matches!(status, 401 | 403 | 407 | 429))
}

/// Builds the connection state around a transport and negotiates the protocol.
async fn establish(
    name: &str,
    transport: Transport,
    opts: &ConnectOptions,
    legacy_only: bool,
) -> Result<Arc<Shared>, McpError> {
    let shared = Arc::new(Shared {
        name: name.to_string(),
        transport,
        era: RwLock::new(Era::Legacy(LEGACY_VERSIONS[0].to_string())),
        next_id: AtomicI64::new(1),
        tools: Mutex::new(Vec::new()),
        capabilities: Mutex::new(json!({})),
        server_info: Mutex::new(None),
        instructions: Mutex::new(None),
        broken: AtomicBool::new(false),
        on_tools_changed: opts.on_tools_changed.clone(),
    });
    handshake(&shared, legacy_only || opts.legacy_only).await?;
    Ok(shared)
}

/// One attempt with an HTTP transport: Streamable HTTP, or (`sse`) the older HTTP+SSE one.
async fn establish_http(
    name: &str,
    spec: &http::HttpSpec,
    opts: &ConnectOptions,
    sse: bool,
) -> Result<(Arc<Shared>, mpsc::UnboundedReceiver<Value>), McpError> {
    let (note_tx, note_rx) = mpsc::unbounded_channel::<Value>();
    let transport = if sse {
        Transport::Sse(Box::new(
            legacy_sse::SseTransport::connect(spec.clone(), note_tx).await?,
        ))
    } else {
        Transport::Http(http::HttpTransport::new(spec.clone(), note_tx)?)
    };
    // The HTTP+SSE transport only exists in the `initialize` revisions.
    Ok((establish(name, transport, opts, sse).await?, note_rx))
}

/// Era detection and, for legacy servers, the `initialize` handshake.
async fn handshake(s: &Arc<Shared>, legacy_only: bool) -> Result<(), McpError> {
    let modern = Era::Modern(MODERN_VERSION.to_string());
    if !legacy_only {
        if let Some(done) = probe_modern(s, &modern).await? {
            return Ok(done);
        }
    }
    legacy_handshake(s).await
}

/// `Ok(Some(()))`: modern server, handshake done. `Ok(None)`: a legacy server.
async fn probe_modern(s: &Arc<Shared>, modern: &Era) -> Result<Option<()>, McpError> {
    if let Transport::Http(t) = &s.transport {
        t.set_version(Some(MODERN_VERSION.to_string()));
    }
    let probe_timeout = if s.is_http() {
        Duration::from_secs(30)
    } else {
        PROBE_TIMEOUT
    };
    let probe = s
        .rpc_in(
            modern,
            "server/discover",
            json!({}),
            Some(probe_timeout),
            None,
            &[],
        )
        .await;
    match probe {
        Ok(r) => {
            let supported: Vec<&str> = r
                .get("supportedVersions")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            if supported.contains(&MODERN_VERSION) {
                *s.era.write().unwrap_or_else(|e| e.into_inner()) = modern.clone();
                *s.capabilities.lock().unwrap_or_else(|e| e.into_inner()) =
                    r.get("capabilities").cloned().unwrap_or_else(|| json!({}));
                *s.server_info.lock().unwrap_or_else(|e| e.into_inner()) = r
                    .get("_meta")
                    .and_then(|m| m.get("io.modelcontextprotocol/serverInfo"))
                    .cloned();
                *s.instructions.lock().unwrap_or_else(|e| e.into_inner()) = r
                    .get("instructions")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                return Ok(Some(()));
            }
            return Err(McpError::Protocol(format!(
                "the server supports protocol versions {} and none of them is understood",
                supported.join(", ")
            )));
        }
        // A modern server that does not share our version, or objects to the request: not a
        // legacy server, so no fallback.
        Err(e) if matches!(e.code(), Some(-32022 | -32020)) => {
            let supported = match &e {
                McpError::Rpc { data, .. } | McpError::Http { data, .. } => data
                    .as_ref()
                    .and_then(|d| d.get("supported"))
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default(),
                _ => String::new(),
            };
            return Err(McpError::Protocol(format!(
                "the server speaks only MCP versions this client does not ({supported}); {e}"
            )));
        }
        // Network trouble and refused access are not an era question.
        Err(e @ (McpError::Transport(_) | McpError::Closed(_))) => return Err(e),
        Err(McpError::Http {
            status: 401 | 403, ..
        }) => {
            return Err(McpError::from_http(401, ""));
        }
        // Anything else (an error answer, a 4xx without a modern error, silence): legacy.
        Err(_) => {}
    }
    Ok(None)
}

/// The `initialize` handshake of the legacy revisions.
async fn legacy_handshake(s: &Arc<Shared>) -> Result<(), McpError> {
    if let Transport::Http(t) = &s.transport {
        t.set_version(None);
    }
    let legacy = Era::Legacy(LEGACY_VERSIONS[0].to_string());
    let r = s
        .rpc_in(
            &legacy,
            "initialize",
            json!({
                "protocolVersion": LEGACY_VERSIONS[0],
                "capabilities": {},
                "clientInfo": client_info(),
            }),
            None,
            None,
            &[],
        )
        .await?;
    let version = r
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if !LEGACY_VERSIONS.contains(&version.as_str()) {
        return Err(McpError::Protocol(format!(
            "the server answered with protocol version \"{version}\", which this client does not support"
        )));
    }
    *s.era.write().unwrap_or_else(|e| e.into_inner()) = Era::Legacy(version.clone());
    *s.capabilities.lock().unwrap_or_else(|e| e.into_inner()) =
        r.get("capabilities").cloned().unwrap_or_else(|| json!({}));
    *s.server_info.lock().unwrap_or_else(|e| e.into_inner()) = r.get("serverInfo").cloned();
    *s.instructions.lock().unwrap_or_else(|e| e.into_inner()) = r
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::to_string);
    if let Transport::Http(t) = &s.transport {
        t.set_version(Some(version));
    }
    s.notify(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        .await?;
    Ok(())
}

/// A `tools/call` result as the text a model reads.
fn tool_result(result: &Value) -> Result<ToolOutput, ToolError> {
    match result.get("resultType").and_then(Value::as_str) {
        None | Some("complete") => {}
        Some("input_required") => {
            return Err(ToolError::failed(
                "the MCP server asked for more input (elicitation or sampling), which Harness does not support yet",
            ))
        }
        Some(other) => {
            return Err(ToolError::failed(format!(
                "the MCP server answered with an unknown result type \"{other}\""
            )))
        }
    }
    let mut parts: Vec<String> = Vec::new();
    if let Some(blocks) = result.get("content").and_then(Value::as_array) {
        for b in blocks {
            match b.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(t) = b.get("text").and_then(Value::as_str) {
                        parts.push(t.to_string());
                    }
                }
                Some(kind @ ("image" | "audio")) => {
                    let mime = b.get("mimeType").and_then(Value::as_str).unwrap_or("?");
                    let len = b.get("data").and_then(Value::as_str).map_or(0, str::len);
                    parts.push(format!(
                        "[{kind} result, {mime}, {len} base64 characters not shown]"
                    ));
                }
                Some("resource_link") => {
                    let uri = b.get("uri").and_then(Value::as_str).unwrap_or("");
                    let name = b.get("name").and_then(Value::as_str).unwrap_or("");
                    parts.push(format!("[resource link: {uri} {name}]").replace("  ", " "));
                }
                Some("resource") => {
                    let r = b.get("resource");
                    match r.and_then(|r| r.get("text")).and_then(Value::as_str) {
                        Some(t) => parts.push(t.to_string()),
                        None => {
                            let uri = r
                                .and_then(|r| r.get("uri"))
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            parts.push(format!("[binary resource: {uri}]"));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    if parts.is_empty() {
        if let Some(sc) = result.get("structuredContent") {
            parts.push(sc.to_string());
        }
    }
    let text = parts.join("\n");
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return Err(ToolError::failed(if text.is_empty() {
            "the tool reported an error".to_string()
        } else {
            text
        }));
    }
    Ok(ToolOutput::text(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_become_text_and_errors() {
        let ok = tool_result(
            &json!({"content": [{"type":"text","text":"a"},{"type":"text","text":"b"}]}),
        )
        .unwrap();
        assert_eq!(ok.content, "a\nb");
        let img = tool_result(
            &json!({"content": [{"type":"image","mimeType":"image/png","data":"AAAA"}]}),
        )
        .unwrap();
        assert!(img.content.contains("image/png") && !img.content.contains("AAAA"));
        let structured =
            tool_result(&json!({"content": [], "structuredContent": {"x": 1}})).unwrap();
        assert_eq!(structured.content, "{\"x\":1}");
        let err =
            tool_result(&json!({"isError": true, "content": [{"type":"text","text":"boom"}]}))
                .unwrap_err();
        assert_eq!(
            (err.code, err.message.as_str()),
            (ToolErrorCode::Failed, "boom")
        );
        let more = tool_result(&json!({"resultType": "input_required"})).unwrap_err();
        assert!(more.message.contains("more input"));
    }

    #[test]
    fn http_errors_keep_the_rpc_code() {
        let e = McpError::from_http(
            400,
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32022,"message":"Unsupported protocol version","data":{"supported":["2025-11-25"]}}}"#,
        );
        assert_eq!(e.code(), Some(-32022));
        let e = McpError::from_http(401, "");
        assert!(e.to_string().contains("authoriz"));
        assert_eq!(McpError::from_http(502, "bad gateway").code(), None);
    }

    #[test]
    fn a_server_definition_picks_its_transport() {
        let http = crate::mcp::McpServerDef {
            url: Some("https://x.example/mcp".into()),
            ..Default::default()
        };
        assert!(matches!(McpTarget::from_def(&http), Ok(McpTarget::Http(_))));
        assert!(McpTarget::from_def(&crate::mcp::McpServerDef::default()).is_err());
        let mode = |t: Option<&str>| {
            let def = crate::mcp::McpServerDef {
                url: Some("https://x.example/mcp".into()),
                transport: t.map(str::to_string),
                ..Default::default()
            };
            match McpTarget::for_server("srv", &def).unwrap() {
                McpTarget::Http(spec) => (spec.mode, spec.oauth),
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(mode(None), (http::HttpMode::Auto, Some("srv".into())));
        assert_eq!(mode(Some("auto")).0, http::HttpMode::Auto);
        assert_eq!(mode(Some("http")).0, http::HttpMode::Streamable);
        assert_eq!(mode(Some("sse")).0, http::HttpMode::Sse);
    }

    #[test]
    fn resources_are_parsed_from_the_three_results() {
        let list = parse_resources(&json!({"resources": [
            {"uri": "file:///a.md", "name": "a.md", "title": "A", "mimeType": "text/markdown", "size": 12},
            {"uri": "mem://b"},
            {"name": "no uri, dropped"},
        ]}));
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].mime_type.as_deref(), Some("text/markdown"));
        assert_eq!(
            (list[0].title.as_deref(), list[0].size),
            (Some("A"), Some(12))
        );
        // The name falls back to the URI.
        assert_eq!(list[1].name, "mem://b");
        let templates = parse_resource_templates(&json!({"resourceTemplates": [
            {"uriTemplate": "file:///{path}", "name": "files", "description": "d"},
            {"name": "no template, dropped"},
        ]}));
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].uri_template, "file:///{path}");
        let contents = parse_resource_contents(&json!({"contents": [
            {"uri": "file:///a.md", "mimeType": "text/markdown", "text": "# hi"},
            {"uri": "file:///b.bin", "blob": "AAEC"},
            {"uri": "file:///empty"},
        ]}));
        assert_eq!(contents.len(), 2);
        assert_eq!(contents[0].text.as_deref(), Some("# hi"));
        assert_eq!(contents[1].blob.as_deref(), Some("AAEC"));
        assert!(parse_resources(&json!({})).is_empty());
    }

    #[test]
    fn a_resource_becomes_a_document_file() {
        let text = |mime: Option<&str>, t: &str| ResourceContent {
            uri: "u".into(),
            mime_type: mime.map(str::to_string),
            text: Some(t.into()),
            blob: None,
        };
        let (name, bytes) = resource_as_file(
            "file:///docs/guide",
            Some("User Guide"),
            &[
                text(Some("text/markdown; charset=utf-8"), "# a"),
                text(None, "b"),
            ],
        )
        .unwrap();
        assert_eq!(
            (name.as_str(), bytes.as_slice()),
            ("User Guide.md", &b"# a\n\nb"[..])
        );
        // A name that already has a readable extension is kept; the URI names it otherwise.
        let (name, _) =
            resource_as_file("mem://x/notes.txt", Some("notes.txt"), &[text(None, "t")]).unwrap();
        assert_eq!(name, "notes.txt");
        let (name, _) = resource_as_file(
            "mem://x/report",
            None,
            &[text(Some("application/json"), "{}")],
        )
        .unwrap();
        assert_eq!(name, "report.json");
        let (name, _) = resource_as_file("///", None, &[text(None, "t")]).unwrap();
        assert_eq!(name, "resource.txt");
        // A name cannot escape the folder.
        let (name, _) =
            resource_as_file("mem://x", Some("../../etc/passwd"), &[text(None, "t")]).unwrap();
        assert_eq!(name, "passwd.txt");

        let pdf = ResourceContent {
            uri: "u".into(),
            mime_type: Some("application/pdf".into()),
            text: None,
            blob: Some("JVBERg==".into()),
        };
        let (name, bytes) =
            resource_as_file("mem://doc", Some("doc"), std::slice::from_ref(&pdf)).unwrap();
        assert_eq!((name.as_str(), bytes.as_slice()), ("doc.pdf", &b"%PDF"[..]));
        let image = ResourceContent {
            mime_type: Some("image/png".into()),
            ..pdf
        };
        assert!(resource_as_file("mem://img", None, &[image])
            .unwrap_err()
            .contains("binary"));
        assert!(resource_as_file("mem://none", None, &[])
            .unwrap_err()
            .contains("empty"));
    }

    #[test]
    fn only_the_not_an_endpoint_4xx_answers_fall_back_to_sse() {
        let http = |status| McpError::Http {
            status,
            code: None,
            message: String::new(),
            data: None,
        };
        for status in [400, 404, 405, 406, 415] {
            assert!(should_try_sse(&http(status)), "{status}");
        }
        for status in [401, 403, 429, 500, 502] {
            assert!(!should_try_sse(&http(status)), "{status}");
        }
        assert!(!should_try_sse(&McpError::Timeout));
    }

    use super::testserver::{serve, Req, Resp, TestServer};
    use std::sync::atomic::AtomicUsize;

    /// A server of the 2024-11-05 era: `POST /sse` is refused (405), `GET /sse` is an event
    /// stream whose first event is `endpoint`, and what is POSTed to `/messages` is answered on
    /// that stream. `endpoint` is what the `endpoint` event says.
    struct LegacyFixture {
        server: TestServer,
        stream: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
        tool_version: Arc<AtomicUsize>,
    }

    async fn legacy_server(endpoint: &'static str) -> LegacyFixture {
        let stream: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>> = Arc::default();
        let tool_version = Arc::new(AtomicUsize::new(1));
        let (st, tv) = (stream.clone(), tool_version.clone());
        let server = serve(move |_, req: Req| match (req.method.as_str(), req.path()) {
            ("GET", "/sse") => {
                let (tx, rx) = mpsc::unbounded_channel();
                tx.send(format!("event: endpoint\ndata: {endpoint}\n\n"))
                    .unwrap();
                *st.lock().unwrap() = Some(tx);
                Resp::Stream(rx)
            }
            ("POST", "/messages") => {
                let v: Value = serde_json::from_str(&req.body).unwrap();
                let Some(id) = v.get("id").cloned() else {
                    return Resp::status(202);
                };
                let result = match v["method"].as_str().unwrap() {
                    "initialize" => json!({
                        "protocolVersion": "2024-11-05",
                        "capabilities": {"tools": {"listChanged": true}, "resources": {}},
                        "serverInfo": {"name": "legacy-fixture", "version": "0.1"},
                    }),
                    "tools/list" => json!({"tools": [{
                        "name": format!("tool{}", tv.load(Ordering::SeqCst)),
                        "inputSchema": {"type": "object"},
                    }]}),
                    "tools/call" => json!({"content": [{
                        "type": "text",
                        "text": format!("called {}", v["params"]["name"].as_str().unwrap()),
                    }]}),
                    "resources/list" => json!({"resources": [
                        {"uri": "mem://notes", "name": "notes", "mimeType": "text/plain"},
                    ]}),
                    "resources/templates/list" => json!({"resourceTemplates": [
                        {"uriTemplate": "mem://notes/{id}", "name": "note"},
                    ]}),
                    "resources/read" => json!({"contents": [{
                        "uri": v["params"]["uri"], "mimeType": "text/plain", "text": "note text",
                    }]}),
                    _ => json!({}),
                };
                if let Some(tx) = st.lock().unwrap().as_ref() {
                    let msg = json!({"jsonrpc": "2.0", "id": id, "result": result});
                    tx.send(format!("event: message\ndata: {msg}\n\n")).unwrap();
                }
                Resp::status(202)
            }
            ("POST", "/sse") => Resp::status(405),
            _ => Resp::status(404),
        })
        .await;
        LegacyFixture {
            server,
            stream,
            tool_version,
        }
    }

    fn sse_def(base: &str, transport: Option<&str>) -> crate::mcp::McpServerDef {
        crate::mcp::McpServerDef {
            url: Some(format!("{base}/sse")),
            transport: transport.map(str::to_string),
            ..Default::default()
        }
    }

    async fn connect_def(
        name: &str,
        def: &crate::mcp::McpServerDef,
        opts: ConnectOptions,
    ) -> Result<Arc<McpConnection>, McpError> {
        McpConnection::connect(name, McpTarget::for_server(name, def)?, opts).await
    }

    #[tokio::test]
    async fn an_auto_server_falls_back_to_http_sse_and_serves_tools_and_resources() {
        let f = legacy_server("/messages?sessionId=s1").await;
        let conn = connect_def(
            "legacy-auto",
            &sse_def(&f.server.base, None),
            Default::default(),
        )
        .await
        .unwrap();
        // Streamable HTTP was tried first (the POST answered 405), then the stream was opened.
        assert!(f.server.count("POST", "/sse") >= 1);
        assert_eq!(f.server.count("GET", "/sse"), 1);
        assert_eq!(conn.transport_name(), "sse");
        assert_eq!(conn.era(), Era::Legacy("2024-11-05".into()));
        assert_eq!(conn.tools()[0].name, "tool1");
        assert_eq!(conn.server_info().unwrap()["name"], "legacy-fixture");

        let out = conn
            .call_tool(
                "tool1",
                json!({}),
                Duration::from_secs(5),
                &CancelToken::default(),
            )
            .await
            .unwrap();
        assert_eq!(out.content, "called tool1");

        assert!(conn.has_resources());
        let list = conn.list_resources().await.unwrap();
        assert_eq!(list.resources[0].uri, "mem://notes");
        assert_eq!(list.templates[0].uri_template, "mem://notes/{id}");
        let read = conn.read_resource("mem://notes").await.unwrap();
        assert_eq!(read[0].text.as_deref(), Some("note text"));
        // The endpoint event's relative URL was used for every message.
        assert!(f.server.count("POST", "/messages") >= 5);
    }

    #[tokio::test]
    async fn a_list_change_on_the_sse_stream_refreshes_the_tools() {
        let f = legacy_server("/messages?sessionId=s2").await;
        let changed = Arc::new(AtomicUsize::new(0));
        let c = changed.clone();
        let conn = connect_def(
            "legacy-changed",
            &sse_def(&f.server.base, Some("sse")),
            ConnectOptions {
                on_tools_changed: Arc::new(move || {
                    c.fetch_add(1, Ordering::SeqCst);
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        // An explicit `sse` server never tries Streamable HTTP.
        assert_eq!(f.server.count("POST", "/sse"), 0);
        f.tool_version.store(2, Ordering::SeqCst);
        f.stream
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .send("event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/tools/list_changed\"}\n\n".into())
            .unwrap();
        for _ in 0..200 {
            if changed.load(Ordering::SeqCst) > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert_eq!(changed.load(Ordering::SeqCst), 1);
        assert_eq!(conn.tools()[0].name, "tool2");

        // The stream ends: the connection is broken (the hub would reconnect).
        f.stream.lock().unwrap().take();
        for _ in 0..200 {
            if conn.is_broken() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(conn.is_broken());
    }

    #[tokio::test]
    async fn an_endpoint_on_another_origin_and_a_forced_streamable_server_are_refused() {
        let f = legacy_server("http://127.0.0.1:1/steal").await;
        let err = connect_def(
            "legacy-evil",
            &sse_def(&f.server.base, Some("sse")),
            Default::default(),
        )
        .await
        .err()
        .unwrap();
        assert!(err.to_string().contains("another origin"), "{err}");

        // `http` is Streamable HTTP only: the 405 is the answer, no fallback.
        let f = legacy_server("/messages?sessionId=s3").await;
        let err = connect_def(
            "legacy-forced",
            &sse_def(&f.server.base, Some("http")),
            Default::default(),
        )
        .await
        .err()
        .unwrap();
        assert!(matches!(err, McpError::Http { status: 405, .. }), "{err}");
        assert_eq!(f.server.count("GET", "/sse"), 0);

        // 401 is not a reason to try another transport.
        let denied = serve(|_, _| Resp::status(401)).await;
        let err = connect_def(
            "legacy-401",
            &sse_def(&denied.base, None),
            Default::default(),
        )
        .await
        .err()
        .unwrap();
        assert!(matches!(err, McpError::Http { status: 401, .. }), "{err}");
        assert_eq!(denied.count("GET", "/sse"), 0);
    }
}
