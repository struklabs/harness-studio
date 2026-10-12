//! The connection manager: the servers of `mcp_servers.json` (the file the UI edits) are
//! connected on demand and kept alive, and their tools are registered in the
//! [`ToolRegistry`](crate::tools::ToolRegistry) with source `mcp`, independent of any
//! llama-server launch.
//!
//! * **Lazy.** Nothing starts until a surface asks for the tool list or a call names an
//!   unknown tool ([`McpHub::sync`]); then every enabled server is connected in parallel.
//! * **Kept alive.** A connection stays until the server's definition changes, it is disabled
//!   or removed, or it dies; a call on a dead connection reconnects first. A server that failed
//!   to start is not retried for [`RETRY_AFTER`] unless asked (`sync(true)`).
//! * **Names** are `<server>_<tool>` (the convention llama-server's MCP client used), limited
//!   to what function names allow (`[A-Za-z0-9_-]`, 64 characters) and made unique.
//! * **Permission class** comes from the tool's MCP `annotations`
//!   ([`class_from_annotations`]); the user's per-tool override lives in
//!   `settings.tools.mcp_classes` and is applied by the tool service.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::annotations::class_from_annotations;
use super::{
    oauth, ConnectOptions, McpConnection, McpError, McpTarget, RemoteTool, ResourceContent,
    ResourceList,
};
use crate::mcp::McpServerDef;
use crate::tools::registry::{Tool, ToolCtx, ToolRegistry};
use crate::tools::types::{ToolDef, ToolError, ToolOutput, ToolSource};

/// How long a server that failed to start is left alone before the next list retries it.
pub const RETRY_AFTER: Duration = Duration::from_secs(60);
/// Longest function name the OpenAI format allows.
const MAX_NAME: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum McpServerState {
    /// In the list but switched off.
    Disabled,
    Connected,
    /// The last start failed (`error` says why).
    Failed,
    /// Not started yet (nothing asked for its tools).
    Idle,
    /// Starting now (a first `npx` run can take a while).
    Connecting,
}

/// Where a remote server stands with its OAuth authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum McpAuthState {
    /// The server answered 401: it needs the user to authorize it.
    Required,
    /// A stored sign-in exists for the server's URL.
    Authorized,
}

/// What `mcp-client status` shows for one configured server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct McpServerStatus {
    pub name: String,
    /// `stdio`, `http` (Streamable HTTP) or `sse` (the older HTTP+SSE transport).
    pub transport: String,
    pub state: McpServerState,
    pub error: Option<String>,
    /// Number of tools registered.
    pub tools: usize,
    /// Negotiated MCP version (`2026-07-28`, `2025-11-25`...).
    pub protocol: Option<String>,
    /// `serverInfo` as the server reports it (display only).
    pub server: Option<String>,
    /// OAuth state of an HTTP server (`None` for stdio servers and servers that never asked).
    #[serde(default)]
    pub auth: Option<McpAuthState>,
    /// The connected server offers resources.
    #[serde(default)]
    pub resources: bool,
}

/// Where the hub reads the server definitions from.
enum Source {
    /// No servers (a service built for tests or tools that only use built-ins).
    None,
    /// `mcp_servers.json` in the app's config directory (secrets in the keychain).
    App,
    /// A fixed list, secrets already resolved (tests, embedding).
    Fixed(Mutex<BTreeMap<String, McpServerDef>>),
}

struct Slot {
    fingerprint: String,
    conn: Option<Arc<McpConnection>>,
    /// Registry names this server's tools were registered under.
    registered: Vec<String>,
    error: Option<String>,
    failed_at: Option<Instant>,
    /// The last start failed with a 401: the user has to authorize the server.
    auth_required: bool,
}

struct Inner {
    registry: Arc<ToolRegistry>,
    source: Source,
    slots: Mutex<BTreeMap<String, Slot>>,
    /// Servers being started right now.
    starting: Mutex<HashSet<String>>,
    /// One start / stop / reconnect at a time.
    sync_lock: tokio::sync::Mutex<()>,
}

/// Cheap handle to the connection manager.
#[derive(Clone)]
pub struct McpHub {
    inner: Arc<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl McpHub {
    fn new(registry: Arc<ToolRegistry>, source: Source) -> Self {
        Self {
            inner: Arc::new(Inner {
                registry,
                source,
                slots: Mutex::default(),
                starting: Mutex::default(),
                sync_lock: tokio::sync::Mutex::new(()),
            }),
        }
    }

    /// A hub with no servers: `sync` does nothing.
    pub fn none(registry: Arc<ToolRegistry>) -> Self {
        Self::new(registry, Source::None)
    }

    /// The hub of the app: reads `mcp_servers.json`.
    pub fn from_app_config(registry: Arc<ToolRegistry>) -> Self {
        Self::new(registry, Source::App)
    }

    /// A hub over a fixed server list (secrets already resolved).
    pub fn with_servers(
        registry: Arc<ToolRegistry>,
        servers: BTreeMap<String, McpServerDef>,
    ) -> Self {
        Self::new(registry, Source::Fixed(Mutex::new(servers)))
    }

    /// Replaces the list of a fixed hub (tests); the next `sync` applies it.
    pub fn set_servers(&self, servers: BTreeMap<String, McpServerDef>) {
        if let Source::Fixed(m) = &self.inner.source {
            *lock(m) = servers;
        }
    }

    /// The configured servers: `(masked, for fingerprints)`; secrets are only resolved when a
    /// server is about to be started.
    fn defs(&self) -> Result<BTreeMap<String, McpServerDef>, String> {
        match &self.inner.source {
            Source::None => Ok(BTreeMap::new()),
            Source::App => crate::mcp::load().map(|c| c.mcp_servers),
            Source::Fixed(m) => Ok(lock(m).clone()),
        }
    }

    fn resolved(&self) -> Result<BTreeMap<String, McpServerDef>, String> {
        match &self.inner.source {
            Source::None => Ok(BTreeMap::new()),
            Source::App => crate::mcp::load_resolved().map(|c| c.mcp_servers),
            Source::Fixed(m) => Ok(lock(m).clone()),
        }
    }

    /// Brings the connections in line with the configuration: removes servers that are gone,
    /// disabled or changed, starts the ones that are missing (in parallel). A server that
    /// failed recently is skipped unless `retry_failed`.
    pub async fn sync(&self, retry_failed: bool) {
        let _g = self.inner.sync_lock.lock().await;
        let defs = match self.defs() {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[Harness MCP] {e}");
                return;
            }
        };
        // Remove what no longer applies.
        let stale: Vec<String> = lock(&self.inner.slots)
            .iter()
            .filter(|(name, slot)| match defs.get(*name) {
                None => true,
                Some(d) => d.disabled || fingerprint(d) != slot.fingerprint,
            })
            .map(|(n, _)| n.clone())
            .collect();
        for name in stale {
            self.remove_server(&name);
        }
        // Dead connections are replaced.
        let dead: Vec<String> = lock(&self.inner.slots)
            .iter()
            .filter(|(_, s)| s.conn.as_ref().is_some_and(|c| c.is_broken()))
            .map(|(n, _)| n.clone())
            .collect();
        for name in dead {
            self.remove_server(&name);
        }
        let to_start: Vec<String> = defs
            .iter()
            .filter(|(name, d)| {
                if d.disabled {
                    return false;
                }
                match lock(&self.inner.slots).get(*name) {
                    None => true,
                    Some(s) if s.conn.is_some() => false,
                    Some(s) => {
                        retry_failed || s.failed_at.is_none_or(|t| t.elapsed() >= RETRY_AFTER)
                    }
                }
            })
            .map(|(n, _)| n.clone())
            .collect();
        if to_start.is_empty() {
            return;
        }
        let resolved = match self.resolved() {
            Ok(r) => r,
            Err(e) => {
                // The keychain could not be read: every server that was to start fails with it.
                for name in &to_start {
                    self.record_failure(name, defs.get(name), e.clone(), false);
                }
                return;
            }
        };
        lock(&self.inner.starting).extend(to_start.iter().cloned());
        futures_util::future::join_all(to_start.iter().map(|name| {
            let def = resolved.get(name).or(defs.get(name)).cloned();
            let masked = defs.get(name).cloned();
            async move {
                let (Some(def), Some(masked)) = (def, masked) else {
                    return;
                };
                self.start_server(name, &def, &masked).await;
                lock(&self.inner.starting).remove(name);
            }
        }))
        .await;
        // Servers that could not even be looked up must not stay "connecting".
        lock(&self.inner.starting).clear();
    }

    fn record_failure(
        &self,
        name: &str,
        masked: Option<&McpServerDef>,
        error: String,
        auth_required: bool,
    ) {
        let mut slots = lock(&self.inner.slots);
        slots.insert(
            name.to_string(),
            Slot {
                fingerprint: masked.map(fingerprint).unwrap_or_default(),
                conn: None,
                registered: Vec::new(),
                error: Some(error),
                failed_at: Some(Instant::now()),
                auth_required,
            },
        );
    }

    async fn start_server(&self, name: &str, def: &McpServerDef, masked: &McpServerDef) {
        let target = match McpTarget::for_server(name, def) {
            Ok(t) => t,
            Err(e) => return self.record_failure(name, Some(masked), e.to_string(), false),
        };
        let weak = Arc::downgrade(&self.inner);
        let server = name.to_string();
        let opts = ConnectOptions {
            on_tools_changed: Arc::new(move || {
                if let Some(inner) = weak.upgrade() {
                    McpHub { inner }.register_tools(&server);
                }
            }),
            ..Default::default()
        };
        match McpConnection::connect(name, target, opts).await {
            Ok(conn) => {
                lock(&self.inner.slots).insert(
                    name.to_string(),
                    Slot {
                        fingerprint: fingerprint(masked),
                        conn: Some(conn),
                        registered: Vec::new(),
                        error: None,
                        failed_at: None,
                        auth_required: false,
                    },
                );
                self.register_tools(name);
            }
            Err(e) => {
                let auth = matches!(e, McpError::Http { status: 401, .. });
                self.record_failure(name, Some(masked), e.to_string(), auth)
            }
        }
    }

    /// Drops a server's connection (the server ends) and its tools.
    fn remove_server(&self, name: &str) {
        let slot = lock(&self.inner.slots).remove(name);
        if let Some(slot) = slot {
            for n in slot.registered {
                self.inner.registry.unregister(&n);
            }
        }
    }

    /// (Re)registers one server's tools from its connection's current list.
    fn register_tools(&self, name: &str) {
        let mut slots = lock(&self.inner.slots);
        let Some(slot) = slots.get_mut(name) else {
            return;
        };
        let Some(conn) = slot.conn.clone() else {
            return;
        };
        for n in slot.registered.drain(..) {
            self.inner.registry.unregister(&n);
        }
        let mut taken: HashSet<String> = self
            .inner
            .registry
            .list()
            .into_iter()
            .map(|d| d.name)
            .collect();
        for remote in conn.tools() {
            let registered = unique_name(name, &remote.name, &taken);
            taken.insert(registered.clone());
            let tool = McpTool::new(Arc::downgrade(&self.inner), name, &registered, &remote);
            self.inner.registry.register(Arc::new(tool));
            slot.registered.push(registered);
        }
    }

    /// The live connection of a server, reconnecting first when it died.
    async fn connection(&self, name: &str) -> Result<Arc<McpConnection>, String> {
        let live = lock(&self.inner.slots)
            .get(name)
            .and_then(|s| s.conn.clone())
            .filter(|c| !c.is_broken());
        if let Some(c) = live {
            return Ok(c);
        }
        {
            // Drop the dead one (kills a stdio process that is half alive) and start again.
            let _g = self.inner.sync_lock.lock().await;
            let still_live = lock(&self.inner.slots)
                .get(name)
                .and_then(|s| s.conn.clone())
                .filter(|c| !c.is_broken());
            if let Some(c) = still_live {
                return Ok(c);
            }
            self.remove_server(name);
            let defs = self.defs()?;
            let masked = defs
                .get(name)
                .cloned()
                .filter(|d| !d.disabled)
                .ok_or_else(|| {
                    format!("the MCP server \"{name}\" is not configured or is disabled")
                })?;
            let def = self
                .resolved()?
                .get(name)
                .cloned()
                .unwrap_or_else(|| masked.clone());
            self.start_server(name, &def, &masked).await;
        }
        lock(&self.inner.slots)
            .get(name)
            .and_then(|s| s.conn.clone())
            .ok_or_else(|| {
                lock(&self.inner.slots)
                    .get(name)
                    .and_then(|s| s.error.clone())
                    .unwrap_or_else(|| "the MCP server could not be started".into())
            })
    }

    /// Drops the connection of a server that was found dead mid-call and starts it again.
    async fn reconnect(
        &self,
        name: &str,
        failed: &Arc<McpConnection>,
    ) -> Result<Arc<McpConnection>, String> {
        {
            let _g = self.inner.sync_lock.lock().await;
            // Another call may have replaced it already; keep that one.
            let same = lock(&self.inner.slots)
                .get(name)
                .and_then(|s| s.conn.as_ref())
                .is_some_and(|c| Arc::ptr_eq(c, failed));
            if same {
                self.remove_server(name);
            }
        }
        self.connection(name).await
    }

    /// Status of every configured server.
    pub fn status(&self) -> Vec<McpServerStatus> {
        let defs = self.defs().unwrap_or_default();
        let slots = lock(&self.inner.slots);
        defs.iter()
            .map(|(name, d)| {
                let slot = slots.get(name);
                let transport = match slot.and_then(|s| s.conn.as_ref()) {
                    Some(c) => c.transport_name(),
                    None if d.url.is_some() && d.transport.as_deref() == Some("sse") => "sse",
                    None if d.url.is_some() => "http",
                    None => "stdio",
                };
                let auth = match &d.url {
                    Some(_) if slot.is_some_and(|s| s.auth_required) => {
                        Some(McpAuthState::Required)
                    }
                    Some(url) if oauth::is_authorized(name, url) => Some(McpAuthState::Authorized),
                    _ => None,
                };
                let connecting = lock(&self.inner.starting).contains(name);
                let (state, error) = if d.disabled {
                    (McpServerState::Disabled, None)
                } else if connecting {
                    (McpServerState::Connecting, None)
                } else {
                    match slot {
                        Some(s) if s.conn.is_some() => (McpServerState::Connected, None),
                        Some(s) => (McpServerState::Failed, s.error.clone()),
                        None => (McpServerState::Idle, None),
                    }
                };
                let conn = slot.and_then(|s| s.conn.as_ref());
                McpServerStatus {
                    name: name.clone(),
                    transport: transport.into(),
                    auth,
                    resources: slot
                        .and_then(|s| s.conn.as_ref())
                        .is_some_and(|c| c.has_resources()),
                    state,
                    error,
                    tools: slot.map_or(0, |s| s.registered.len()),
                    protocol: conn.map(|c| c.era().version().to_string()),
                    server: conn.and_then(|c| c.server_info()).map(|i| {
                        let n = i.get("name").and_then(Value::as_str).unwrap_or("?");
                        match i.get("version").and_then(Value::as_str) {
                            Some(v) => format!("{n} {v}"),
                            None => n.to_string(),
                        }
                    }),
                }
            })
            .collect()
    }

    /// The resources and resource templates of a connected server.
    pub async fn resources(&self, server: &str) -> Result<ResourceList, String> {
        let conn = self.connection(server).await?;
        if !conn.has_resources() {
            return Err(format!("the MCP server \"{server}\" offers no resources"));
        }
        conn.list_resources().await.map_err(|e| e.to_string())
    }

    /// Reads one resource of a connected server.
    pub async fn read_resource(
        &self,
        server: &str,
        uri: &str,
    ) -> Result<Vec<ResourceContent>, String> {
        let conn = self.connection(server).await?;
        if !conn.has_resources() {
            return Err(format!("the MCP server \"{server}\" offers no resources"));
        }
        conn.read_resource(uri)
            .await
            .map_err(|e: McpError| e.to_string())
    }

    /// Signs a remote server in through the browser (see [`oauth::authorize`]); `open` shows the
    /// authorization URL. The server is connected again with the new token.
    pub async fn authorize(
        &self,
        server: &str,
        open: impl FnOnce(&str) -> Result<(), String>,
    ) -> Result<(), String> {
        let url = self
            .defs()?
            .get(server)
            .and_then(|d| d.url.clone())
            .ok_or_else(|| format!("\"{server}\" is not a configured HTTP MCP server"))?;
        oauth::authorize(server, &url, open).await?;
        self.remove_server(server);
        self.sync(true).await;
        Ok(())
    }

    /// Forgets the OAuth sign-in of a server and ends its connection.
    pub fn logout(&self, server: &str) {
        oauth::logout(server);
        self.remove_server(server);
    }

    /// The MCP server a registered tool name belongs to.
    pub fn server_of(&self, tool: &str) -> Option<String> {
        lock(&self.inner.slots)
            .iter()
            .find(|(_, s)| s.registered.iter().any(|n| n == tool))
            .map(|(n, _)| n.clone())
    }

    /// Process ids of the stdio servers that run (tests, diagnostics).
    pub fn pids(&self) -> Vec<(String, u32)> {
        lock(&self.inner.slots)
            .iter()
            .filter_map(|(n, s)| Some((n.clone(), s.conn.as_ref()?.pid()?)))
            .collect()
    }

    /// Forgets every connection (the next `sync` starts them again): after the server list
    /// was saved, so changed secrets and commands apply.
    pub fn reset(&self) {
        let names: Vec<String> = lock(&self.inner.slots).keys().cloned().collect();
        for n in names {
            self.remove_server(&n);
        }
    }

    /// Ends every server: stdin closes, a server that does not exit within `grace` is killed.
    pub async fn shutdown(&self, grace: Duration) {
        let _g = self.inner.sync_lock.lock().await;
        let slots: Vec<Slot> = {
            let mut s = lock(&self.inner.slots);
            std::mem::take(&mut *s).into_values().collect()
        };
        for slot in &slots {
            for n in &slot.registered {
                self.inner.registry.unregister(n);
            }
        }
        futures_util::future::join_all(
            slots
                .iter()
                .filter_map(|s| s.conn.clone())
                .map(|c| async move { c.close(grace).await }),
        )
        .await;
    }
}

/// What identifies a server's definition: a change restarts it.
fn fingerprint(d: &McpServerDef) -> String {
    serde_json::to_string(d).unwrap_or_default()
}

/// A function name: only `[A-Za-z0-9_-]`, at most 64 characters.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// `<server>_<tool>`, sanitised, cut to 64 characters and made unique among `taken`.
pub fn unique_name(server: &str, tool: &str, taken: &HashSet<String>) -> String {
    let base: String = format!("{}_{}", sanitize(server), sanitize(tool))
        .chars()
        .take(MAX_NAME)
        .collect();
    if !taken.contains(&base) {
        return base;
    }
    for i in 2.. {
        let suffix = format!("_{i}");
        let head: String = base.chars().take(MAX_NAME - suffix.len()).collect();
        let cand = format!("{head}{suffix}");
        if !taken.contains(&cand) {
            return cand;
        }
    }
    unreachable!()
}

/// An MCP tool in the registry.
struct McpTool {
    def: ToolDef,
    hub: Weak<Inner>,
    server: String,
    remote: String,
}

impl McpTool {
    fn new(hub: Weak<Inner>, server: &str, registered: &str, remote: &RemoteTool) -> Self {
        let mut schema = remote.input_schema.clone();
        if let Some(o) = schema.as_object_mut() {
            o.remove("$schema");
            o.entry("type").or_insert_with(|| json!("object"));
            o.entry("properties").or_insert_with(|| json!({}));
        }
        let description = if !remote.description.trim().is_empty() {
            remote.description.clone()
        } else {
            remote.title.clone().unwrap_or_default()
        };
        Self {
            def: ToolDef {
                name: registered.to_string(),
                description,
                input_schema: schema,
                class: class_from_annotations(remote.annotations.as_ref()),
                source: ToolSource::Mcp,
                uses_workspace: false,
            },
            hub,
            server: server.to_string(),
            remote: remote.name.clone(),
        }
    }
}

impl Tool for McpTool {
    fn def(&self) -> &ToolDef {
        &self.def
    }

    fn call<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolCtx,
    ) -> BoxFuture<'a, Result<ToolOutput, ToolError>> {
        Box::pin(async move {
            let inner = self
                .hub
                .upgrade()
                .ok_or_else(|| ToolError::failed("the MCP connection manager is shut down"))?;
            let hub = McpHub { inner };
            let conn = hub.connection(&self.server).await.map_err(|e| {
                if crate::error_key::parse(&e).is_some() {
                    ToolError::failed(e)
                } else {
                    ToolError::failed(format!("MCP server \"{}\": {e}", self.server))
                }
            })?;
            match conn
                .call_tool_once(&self.remote, args.clone(), ctx.timeout, &ctx.cancel)
                .await
            {
                Err((_, true)) if !ctx.cancel.is_cancelled() => {
                    // The connection died before any answer: drop it, start the server again
                    // and try once more (nothing was answered, so nothing is repeated).
                    let conn = hub.reconnect(&self.server, &conn).await.map_err(|e| {
                        ToolError::failed(format!("MCP server \"{}\": {e}", self.server))
                    })?;
                    conn.call_tool(&self.remote, args, ctx.timeout, &ctx.cancel)
                        .await
                }
                Ok(o) => Ok(o),
                Err((e, _)) => Err(e),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_function_safe_bounded_and_unique() {
        let none = HashSet::new();
        assert_eq!(unique_name("fs", "read.file", &none), "fs_read_file");
        assert_eq!(unique_name("a b", "ü", &none), "a_b__");
        let long = "x".repeat(100);
        assert_eq!(unique_name("s", &long, &none).len(), 64);
        let taken: HashSet<String> = ["s_t".to_string(), "s_t_2".to_string()].into();
        assert_eq!(unique_name("s", "t", &taken), "s_t_3");
        let taken: HashSet<String> = [unique_name("s", &long, &none)].into();
        let second = unique_name("s", &long, &taken);
        assert!(second.len() <= 64 && second.ends_with("_2") && !taken.contains(&second));
    }

    #[test]
    fn a_registered_tool_keeps_the_server_schema_and_the_annotation_class() {
        let remote = RemoteTool {
            name: "echo".into(),
            title: Some("Echo".into()),
            description: String::new(),
            input_schema: json!({"$schema": "x", "type": "object", "properties": {"m": {"type": "string"}}}),
            annotations: Some(json!({"readOnlyHint": true})),
            header_params: vec![],
        };
        let t = McpTool::new(Weak::new(), "srv", "srv_echo", &remote);
        assert_eq!(t.def.description, "Echo");
        assert!(t.def.input_schema.get("$schema").is_none());
        assert_eq!(t.def.class, crate::tools::PermissionClass::Restricted);
        assert_eq!(t.def.source, ToolSource::Mcp);
    }
}
