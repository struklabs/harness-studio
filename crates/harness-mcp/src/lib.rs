//! Harness MCP server: Model Context Protocol tools that let coding agents
//! (Claude Code, Codex, OpenCode, Cursor...) manage local llama.cpp models through Harness.
//!
//! * **One implementation, two transports.** [`McpServer`] handles JSON-RPC messages; stdio
//!   (`harness mcp`, [`stdio::serve`]) and streamable HTTP (`POST /api/harness/v1/mcp` in
//!   `harness-api`) only move them.
//! * **No logic of its own.** Every tool calls the Harness Control API through a [`Backend`]:
//!   the CLI's HTTP client for stdio, an in-process call into the API router for HTTP. So the
//!   tools use exactly the core services, validation, network rules and token scopes of the
//!   API ("Tümü Control API ile aynı çekirdek servislerini çağırır").
//! * **Protocol.** Hand-written, dual-era: MCP 2026-07-28 (stateless, per-request `_meta`,
//!   `server/discover`) and the `initialize`-based revisions 2025-11-25, 2025-06-18,
//!   2025-03-26 and 2024-11-05 that most clients speak today. Only tools are offered (no
//!   resources, prompts, sampling or subscriptions), which keeps the protocol surface small;
//!   see `docs/en/MCP.md` for why the official `rmcp` SDK is not used.
//! * **Scopes.** Each tool needs a Control API scope ([`tools::ToolDef::scope`]). `tools/list`
//!   shows only the tools the caller's scopes allow and `tools/call` refuses the rest, before
//!   the API enforces the same scopes again.

pub mod harness_tools;
pub mod protocol;
pub mod resolve;
pub mod stdio;
pub mod tools;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use harness_core::control_api::Scope;
use serde_json::{json, Map, Value};

pub use protocol::{Era, Reply, LEGACY_VERSIONS, MODERN_VERSIONS};

/// Name reported as `serverInfo.name`.
pub const SERVER_NAME: &str = "harness";
/// Version reported as `serverInfo.version`.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// HTTP method of a Control API request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Delete,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Delete => "DELETE",
        }
    }
}

/// A failed Control API request: the API's error body (`code`, `params`) or a transport error.
#[derive(Debug, Clone)]
pub struct BackendError {
    /// HTTP status (`None`: the API was not reached).
    pub status: Option<u16>,
    /// Machine code (`api.not_found`, `servers.out_of_memory`, `cli.no_instance`...).
    pub code: String,
    /// Human-readable text (may be the code itself).
    pub message: String,
    pub params: Box<Map<String, Value>>,
}

impl BackendError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: None,
            code: code.into(),
            message: message.into(),
            params: Box::default(),
        }
    }

    /// From an API error body `{"error": {"code", "message_key", "params"}}`.
    pub fn from_api(status: u16, body: &Value) -> Self {
        let err = &body["error"];
        let code = err["code"]
            .as_str()
            .or_else(|| err["message_key"].as_str())
            .unwrap_or("api.error")
            .to_string();
        // `core.error` carries the core's own (localized) text in `params.message`.
        let message = err["message"]
            .as_str()
            .or_else(|| err["params"]["message"].as_str())
            .map(str::to_string)
            .unwrap_or_else(|| code.clone());
        Self {
            status: Some(status),
            code,
            message,
            params: Box::new(err["params"].as_object().cloned().unwrap_or_default()),
        }
    }

    pub fn with(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.params.insert(key.to_string(), value.into());
        self
    }

    pub fn to_json(&self) -> Value {
        json!({
            "code": self.code,
            "message": self.message,
            "http_status": self.status,
            "params": self.params,
        })
    }
}

pub type BackendResult<T> = Result<T, BackendError>;
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The Control API the tools talk to. `path` is relative to `/api/harness/v1` and already
/// percent-encoded (see [`resolve::seg`]).
pub trait Backend: Send + Sync {
    fn request<'a>(
        &'a self,
        method: Method,
        path: &'a str,
        body: Option<Value>,
    ) -> BoxFuture<'a, BackendResult<Value>>;

    /// Rewrites a llama-server endpoint for the client (a remote instance reports
    /// `127.0.0.1` URLs that only work on its own machine). Default: unchanged.
    fn client_endpoint(&self, endpoint: &str) -> String {
        endpoint.to_string()
    }
}

/// Where the request came from (tool results mention it where it matters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Stdio,
    Http,
}

/// Handles MCP messages for one caller (its scopes) over one backend.
#[derive(Clone)]
pub struct McpServer {
    pub(crate) backend: Arc<dyn Backend>,
    pub(crate) scopes: Vec<Scope>,
    pub(crate) transport: Transport,
    /// `clientInfo.name` of the last `initialize` (stdio keeps one connection; over HTTP every
    /// request builds a new server, so there it stays empty).
    client_name: Arc<std::sync::Mutex<Option<String>>>,
    /// Label for the tool log when the client did not name itself.
    caller_label: Option<String>,
}

impl McpServer {
    pub fn new(backend: Arc<dyn Backend>, scopes: Vec<Scope>, transport: Transport) -> Self {
        Self {
            backend,
            scopes,
            transport,
            client_name: Arc::default(),
            caller_label: None,
        }
    }

    /// The tool-log label when the client names no one (HTTP: the token).
    pub fn with_caller_label(mut self, label: impl Into<String>) -> Self {
        self.caller_label = Some(label.into());
        self
    }

    pub(crate) fn set_client_name(&self, name: &str) {
        let name: String = name.trim().chars().take(48).collect();
        if !name.is_empty() {
            if let Ok(mut g) = self.client_name.lock() {
                *g = Some(name);
            }
        }
    }

    /// The `client` of a Harness tool call in the tool log: `mcp:<client name>`, else the
    /// caller's label, else `mcp`.
    pub(crate) fn client_label(&self, meta_name: Option<&str>) -> String {
        let named = meta_name
            .map(str::to_string)
            .or_else(|| self.client_name.lock().ok().and_then(|g| g.clone()));
        match named {
            Some(n) => format!("mcp:{n}"),
            None => self.caller_label.clone().unwrap_or_else(|| "mcp".into()),
        }
    }

    /// Whether the caller's scopes allow `scope`.
    pub fn allows(&self, scope: Scope) -> bool {
        harness_core::control_api::allows(&self.scopes, scope)
    }

    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }
}

/// Parses a comma-separated scope list (`read,servers:write`).
pub fn parse_scopes(raw: &str) -> Result<Vec<Scope>, String> {
    let mut out = Vec::new();
    for part in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let scope = Scope::ALL
            .into_iter()
            .find(|s| s.as_str() == part)
            .ok_or_else(|| format!("unknown scope \"{}\" ({})", part, Scope::names()))?;
        if !out.contains(&scope) {
            out.push(scope);
        }
    }
    if out.is_empty() {
        return Err("no scope given".into());
    }
    Ok(out)
}

/// Scopes of `harness mcp` over stdio when `--scope` is not given: read and server control
/// (load/unload/start/stop/benchmark) and Harness's own tools (offered only when the user turned
/// `settings.tools.mcp_expose` on; the tool policy decides each call). Downloads and deletions (`models:write`) are opt-in.
pub const DEFAULT_STDIO_SCOPES: [Scope; 3] = [Scope::Read, Scope::ServersWrite, Scope::Tools];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_lists_parse() {
        assert_eq!(
            parse_scopes("read, servers:write,read").unwrap(),
            vec![Scope::Read, Scope::ServersWrite]
        );
        assert!(parse_scopes("read,root").is_err());
        assert!(parse_scopes(" , ").is_err());
    }

    #[test]
    fn api_errors_keep_code_and_params() {
        let e = BackendError::from_api(
            409,
            &json!({"error": {"code": "models.in_use", "message_key": "models.in_use",
                "params": {"server": "a"}}}),
        );
        assert_eq!(e.code, "models.in_use");
        assert_eq!(e.status, Some(409));
        assert_eq!(e.params["server"], "a");
    }
}
