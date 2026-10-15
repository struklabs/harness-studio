//! Gateway (ARCHITECTURE §13.4): one optional, in-process endpoint for every
//! model definition, with a scheduler that loads models on demand.
//!
//! This module holds everything that does not need an HTTP server, so every front end can use
//! it without `axum`:
//!
//! * [`GatewaySettings`] (`settings.json` → `gateway`) and their validation.
//! * The **managed vs direct endpoint** concept ([`ClientEndpoint`]): while the gateway runs,
//!   Harness's own clients (Playground, applied integrations, `harness` commands) use the
//!   managed endpoint; the direct llama-server ports keep working for advanced use.
//! * [`scheduler::Scheduler`]: `model` → definition → llama-server, JIT load, idle TTL, max
//!   loaded models, memory-aware LRU eviction, pinned (warm) models, per-definition bounded
//!   queue, OOM recovery (evict + retry once).
//! * [`record::RequestRecord`]: per-request metadata published on the event bus (the hook
//!   the analytics store will consume; nothing is persisted here).
//!
//! The HTTP side (routes, streaming proxy, Responses/Codex compatibility) lives in the
//! `harness-gateway` crate; the GUI and `harness serve` run it in their own process. There is
//! no separate daemon.

pub mod admission;
pub mod limits;
pub mod record;
pub mod responses;
pub mod scheduler;
pub mod tool_diag;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 11435;
/// Ollama's default port: the gateway can also listen here (off by default).
pub const OLLAMA_DEFAULT_PORT: u16 = 11434;
/// File (inside the data directory) holding the gateway token of Harness's own clients.
pub const CLIENT_TOKEN_FILE: &str = "gateway-token";

/// Error keys (i18n) of the settings validation.
pub const ERR_INVALID_PORT: &str = "gateway.invalid_port";
pub const ERR_NETWORK_NOT_ALLOWED: &str = "gateway.network_not_allowed";
pub const ERR_NETWORK_WITHOUT_TOKEN: &str = "gateway.network_without_token";
pub const ERR_INVALID_ORIGIN: &str = "gateway.invalid_origin";
pub const ERR_PORT_CONFLICT: &str = "gateway.port_conflict";
pub const ERR_NOT_OWNER: &str = "gateway.not_owner";
pub const ERR_OLLAMA_PORT_CONFLICT: &str = "gateway.ollama_port_conflict";
/// `GatewayStatus::ollama_warning` keys: a real Ollama answers on the Ollama port, another
/// Harness gateway does, or something else holds it.
pub const WARN_OLLAMA_RUNNING: &str = "gateway.ollama_port_ollama";
pub const WARN_OLLAMA_HARNESS: &str = "gateway.ollama_port_harness";
pub const WARN_OLLAMA_BUSY: &str = "gateway.ollama_port_busy";
/// Ollama is installed (not running): its default port is left to it, or starting Ollama
/// later would fail.
pub const WARN_OLLAMA_INSTALLED: &str = "gateway.ollama_port_installed";

/// `settings.json` → `gateway`. Changed only through [`validate_settings`] (the general
/// settings save keeps the stored block, like `control_api`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct GatewaySettings {
    /// Off by default; the user turns it on (Settings > Gateway) or runs `harness serve
    /// --gateway`.
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    /// A non-loopback bind must be allowed explicitly and always requires a token.
    pub allow_network: bool,
    /// Require a token on loopback too (a network bind always does). Accepted: named tokens
    /// with the `inference` (or `admin`) scope, and the gateway's own client token.
    pub require_token: bool,
    /// Browser origins allowed by CORS (the Harness window is always allowed).
    pub cors_origins: Vec<String>,
    /// Models the gateway keeps loaded at once (0 = no limit; memory decides).
    pub max_loaded_models: u32,
    /// Requests that may wait per definition (for a load or a free slot); more → 503 +
    /// `Retry-After`.
    pub max_queue: u32,
    /// Seconds a request may wait for a slot and for the upstream response to start, and the
    /// longest pause inside a stream. 0 = no limit.
    pub request_timeout_secs: u32,
    /// Idle unload after this many seconds for definitions without their own TTL. `None`
    /// (default) = never.
    pub default_ttl_secs: Option<u64>,
    /// Evict least recently used idle models when the next one does not fit in free VRAM
    /// (estimate/measurement of `vram.rs`, free memory of `--list-devices`).
    pub memory_aware: bool,
    /// VRAM kept free on top of the estimate, MiB. 0 = automatic (`vram::memory_headroom`:
    /// max(1 GiB, 8% of the card)); a value above 0 is used as given. A stored 512 (the old
    /// default) loads as 0.
    #[serde(deserialize_with = "headroom_from_stored")]
    pub memory_headroom_mib: u64,
    /// Explicit per-definition consent to ignore tools the local server cannot run
    /// (definition id → tool types or names, e.g. `web_search`). Every ignored tool is
    /// reported in the `X-Harness-Compat` response header and in the log.
    pub tool_ignores: BTreeMap<String, Vec<String>>,
    /// Server-side tools, per definition id: the gateway runs the tool loop itself for
    /// these models instead of rejecting hosted tools. Off (empty) by default; see
    /// [`ServerTools`].
    pub server_tools: BTreeMap<String, ServerTools>,
    /// Recover calls left in Chat Completions text, per definition. On by default: only an
    /// explicit `false` (the opt-out) is stored; a definition without an entry recovers. A
    /// request that offers no tools is never touched.
    pub text_tool_calls: BTreeMap<String, bool>,
    /// Also listen on the Ollama port (`ollama_port`, default 11434) with the same routes, for
    /// clients that only know Ollama's address. Off by default. When something already
    /// answers there (a real Ollama), the gateway does not bind it and reports
    /// `GatewayStatus::ollama_warning` instead; Ollama's default port is also refused while
    /// Ollama is installed (not running), so a later Ollama start still works.
    pub ollama_port_enabled: bool,
    pub ollama_port: u16,
    /// Let `/api/pull` queue Hugging Face downloads (explicit `hf.co/<owner>/<repo>[:quant]`
    /// references only). Off by default.
    pub ollama_allow_pull: bool,
    /// Stateful `/v1/responses` (`store`, `previous_response_id`): on by default, stored in
    /// `state.db` with retention (see [`responses`]).
    pub responses: responses::ResponsesSettings,
    /// `/v1/systemone` and `/v1/decisions` with a chat model: the gateway asks the model one
    /// constrained question per decision (GBNF grammar) and reads the probabilities from its
    /// logprobs. On by default; off = only native decision models answer, as in Ollama and LM
    /// Studio.
    pub decision_emulation: bool,
}

/// What the gateway fulfils itself for one model definition (`GatewaySettings::server_tools`).
///
/// A request for such a model is completed locally with the core streaming agent loop
/// (`harness_core::tools::agent_stream`): the model gets the tools, the gateway runs the calls
/// through the tool engine and the client only sees the final answer. Only tools the user's
/// tool policy lets run **without asking** may run (the gateway has nobody to ask); anything
/// else is a clear error, never a silent skip.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ServerTools {
    /// Fulfil the hosted `web_search` tool a client asks for (Responses `web_search`, Chat
    /// `web_search_options`, Anthropic `web_search_*`) with Harness's `web_search` tool and the
    /// search provider the user configured (Settings > Tools).
    pub web_search: bool,
    /// Harness tools (registry names; MCP tools are `<server>_<tool>`) offered to the model on
    /// every Chat / Responses / Messages request and run by the gateway. A tool the client
    /// sends with the same name stays the client's.
    pub tools: Vec<String>,
}

impl ServerTools {
    /// Anything to do for this definition.
    pub fn is_active(&self) -> bool {
        self.web_search || !self.tools.is_empty()
    }

    fn normalized(mut self) -> Self {
        let mut seen = std::collections::HashSet::new();
        self.tools = self
            .tools
            .into_iter()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty() && seen.insert(t.clone()))
            .collect();
        self
    }
}

impl Default for GatewaySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
            allow_network: false,
            require_token: false,
            cors_origins: Vec::new(),
            max_loaded_models: 0,
            max_queue: 32,
            request_timeout_secs: 600,
            default_ttl_secs: None,
            memory_aware: true,
            memory_headroom_mib: 0,
            tool_ignores: BTreeMap::new(),
            server_tools: BTreeMap::new(),
            text_tool_calls: BTreeMap::new(),
            ollama_port_enabled: false,
            ollama_port: OLLAMA_DEFAULT_PORT,
            ollama_allow_pull: false,
            responses: responses::ResponsesSettings::default(),
            decision_emulation: true,
        }
    }
}

/// The headroom stored before the automatic value existed (MiB; the old default).
const LEGACY_HEADROOM_MIB: u64 = 512;

fn headroom_from_stored<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let mib = u64::deserialize(d)?;
    Ok(if mib == LEGACY_HEADROOM_MIB { 0 } else { mib })
}

impl GatewaySettings {
    pub fn is_loopback(&self) -> bool {
        crate::network::is_loopback_host(&self.host)
    }

    /// Whether requests must carry a token (always on a network bind).
    pub fn token_required(&self) -> bool {
        self.require_token || !self.is_loopback()
    }

    /// Tools the user allowed to ignore for a definition.
    pub fn ignored_tools(&self, definition_id: &str) -> &[String] {
        self.tool_ignores
            .get(definition_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Whether calls a model left in its Chat Completions text become real tool calls. On
    /// unless the definition opted out (`text_tool_calls[id] == false`).
    pub fn recover_text_tools(&self, definition_id: &str) -> bool {
        self.text_tool_calls
            .get(definition_id)
            .copied()
            .unwrap_or(true)
    }

    /// The server-side tools of a definition (`None` = off).
    pub fn server_tools_of(&self, definition_id: &str) -> Option<&ServerTools> {
        self.server_tools
            .get(definition_id)
            .filter(|s| s.is_active())
    }
}

/// Named tokens able to call the gateway (`inference` or `admin`).
pub fn inference_token_count(tokens: &crate::control_api::TokenStore) -> usize {
    use crate::control_api::Scope;
    tokens
        .list()
        .map(|l| {
            l.iter()
                .filter(|t| crate::control_api::allows(&t.scopes, Scope::Inference))
                .count()
        })
        .unwrap_or(0)
}

/// Canonical settings or the i18n error key. `inference_tokens` = named tokens with the
/// `inference` or `admin` scope (a network bind needs one). `control_api_port` must differ.
pub fn validate_settings(
    incoming: &GatewaySettings,
    inference_tokens: usize,
    control_api_port: Option<u16>,
) -> Result<GatewaySettings, String> {
    let mut s = incoming.clone();
    s.host = crate::network::normalize_bind_host(&s.host)?;
    if s.port < 1024 {
        return Err(ERR_INVALID_PORT.to_string());
    }
    if control_api_port == Some(s.port) {
        return Err(ERR_PORT_CONFLICT.to_string());
    }
    if !crate::network::is_loopback_host(&s.host) {
        if !s.allow_network {
            return Err(ERR_NETWORK_NOT_ALLOWED.to_string());
        }
        if inference_tokens == 0 {
            return Err(ERR_NETWORK_WITHOUT_TOKEN.to_string());
        }
    }
    if s.ollama_port_enabled
        && (s.ollama_port < 1024
            || s.ollama_port == s.port
            || control_api_port == Some(s.ollama_port))
    {
        return Err(ERR_OLLAMA_PORT_CONFLICT.to_string());
    }
    if s.ollama_port == 0 {
        s.ollama_port = OLLAMA_DEFAULT_PORT;
    }
    let mut origins = Vec::new();
    for raw in s.cors_origins.iter().filter(|o| !o.trim().is_empty()) {
        let o =
            crate::network::normalize_origin(raw).map_err(|_| ERR_INVALID_ORIGIN.to_string())?;
        if o == "*" || o == "localhost" {
            return Err(ERR_INVALID_ORIGIN.to_string());
        }
        if !origins.contains(&o) {
            origins.push(o);
        }
    }
    s.cors_origins = origins;
    s.max_queue = s.max_queue.clamp(1, 10_000);
    s.request_timeout_secs = s.request_timeout_secs.min(86_400);
    s.default_ttl_secs = s.default_ttl_secs.filter(|&t| t > 0);
    s.memory_headroom_mib = s.memory_headroom_mib.min(64 * 1024);
    s.responses = s.responses.validated();
    for list in s.tool_ignores.values_mut() {
        let mut clean: Vec<String> = list
            .iter()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        clean.sort();
        clean.dedup();
        *list = clean;
    }
    s.tool_ignores
        .retain(|k, v| !k.trim().is_empty() && !v.is_empty());
    s.text_tool_calls = std::mem::take(&mut s.text_tool_calls)
        .into_iter()
        .map(|(k, v)| (k.trim().to_string(), v))
        // Recovery is on by default: only the opt-outs (`false`) carry information.
        .filter(|(k, v)| !k.is_empty() && !*v)
        .collect();
    s.server_tools = std::mem::take(&mut s.server_tools)
        .into_iter()
        .map(|(k, v)| (k.trim().to_string(), v.normalized()))
        .filter(|(k, v)| !k.is_empty() && v.is_active())
        .collect();
    Ok(s)
}

// --- managed vs direct endpoint ---

/// Which kind of endpoint a client talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum EndpointKind {
    /// The gateway: one address for every model; JIT load, TTL, queue, per-request metadata
    /// (client, tokens, queue wait) and Codex/Responses compatibility.
    Managed,
    /// A llama-server port: no extra hop, one model, only the server's own `/metrics` totals.
    Direct,
}

/// An OpenAI/Anthropic-compatible address for clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ClientEndpoint {
    pub kind: EndpointKind,
    /// `http://host:port` (no path).
    pub host_url: String,
    /// `{host_url}/v1`.
    pub base_url: String,
    /// Requests need a token (managed: gateway setting; direct: the server has an API key).
    pub requires_token: bool,
}

impl ClientEndpoint {
    fn new(kind: EndpointKind, host: &str, port: u16, requires_token: bool) -> Self {
        let host_url = crate::engine::host_url(host, port);
        Self {
            kind,
            // The gateway and llama-server share the OpenAI layout; a direct endpoint of
            // another family asks its engine.
            base_url: crate::engine::llama_cpp().openai_base(&host_url),
            host_url,
            requires_token,
        }
    }

    /// The direct endpoint of a running server.
    pub fn direct(active: &crate::process::ActiveServerInfo) -> Self {
        Self::new(
            EndpointKind::Direct,
            &active.connect_host,
            active.port,
            active.api_key_set,
        )
    }
}

/// What the gateway listener is doing right now (process-wide; set by the HTTP side).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct GatewayStatus {
    pub running: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    /// The managed endpoint while running.
    pub endpoint: Option<ClientEndpoint>,
    pub require_token: bool,
    /// Why the last start failed (i18n key or text).
    pub error: Option<String>,
    /// Unix seconds of the start.
    pub started_at: Option<u64>,
    /// The Ollama port the gateway also listens on (`ollama_port_enabled`), when bound.
    #[serde(default)]
    pub ollama_port: Option<u16>,
    /// Why the Ollama port is not served (i18n key `WARN_OLLAMA_*`, `: <port>` appended).
    #[serde(default)]
    pub ollama_warning: Option<String>,
    /// The configured port was held by another program: the gateway runs on `port` instead
    /// (remembered in `config/ports.json`; Harness's client configs follow it).
    #[serde(default)]
    pub port_fallback: Option<crate::ports::PortFallback>,
    /// Client configs Harness moved to this port at the start (display names).
    #[serde(default)]
    pub clients_moved: Vec<String>,
    /// Client configs still on another address after that (a key in the file, a server's own
    /// port): their card shows the warning with "Düzelt".
    #[serde(default)]
    pub clients_pending: Vec<String>,
}

struct Current {
    status: GatewayStatus,
    scheduler: Option<Arc<scheduler::Scheduler>>,
}

fn current() -> &'static RwLock<Current> {
    static CURRENT: std::sync::OnceLock<RwLock<Current>> = std::sync::OnceLock::new();
    CURRENT.get_or_init(|| {
        RwLock::new(Current {
            status: GatewayStatus::default(),
            scheduler: None,
        })
    })
}

/// Records a running listener (called by the HTTP side after bind).
pub fn set_running(
    host: &str,
    port: u16,
    require_token: bool,
    scheduler: Arc<scheduler::Scheduler>,
) {
    let mut c = current().write().unwrap_or_else(|e| e.into_inner());
    // The Ollama listener is reported separately (`set_ollama_listener`) and survives a
    // limits-only settings change.
    let (ollama_port, ollama_warning) = (c.status.ollama_port, c.status.ollama_warning.clone());
    let port_fallback = c.status.port_fallback.clone();
    c.status = GatewayStatus {
        running: true,
        host: Some(host.to_string()),
        port: Some(port),
        endpoint: Some(ClientEndpoint::new(
            EndpointKind::Managed,
            host,
            port,
            require_token,
        )),
        require_token,
        error: None,
        started_at: Some(crate::capabilities::now_secs()),
        ollama_port,
        ollama_warning,
        port_fallback,
        clients_moved: Vec::new(),
        clients_pending: Vec::new(),
    };
    c.scheduler = Some(scheduler);
}

/// Records the Ollama-port listener of the running gateway (`port` when bound, else why not).
pub fn set_ollama_listener(port: Option<u16>, warning: Option<String>) {
    let mut c = current().write().unwrap_or_else(|e| e.into_inner());
    c.status.ollama_port = port;
    c.status.ollama_warning = warning;
}

/// Records why the running gateway is not on its configured port (`None` = it is).
pub fn set_port_fallback(fallback: Option<crate::ports::PortFallback>) {
    let mut c = current().write().unwrap_or_else(|e| e.into_inner());
    c.status.port_fallback = fallback;
}

/// Records which client configs followed the gateway to its port (and which did not).
pub fn set_client_follow(moved: Vec<String>, pending: Vec<String>) {
    let mut c = current().write().unwrap_or_else(|e| e.into_inner());
    c.status.clients_moved = moved;
    c.status.clients_pending = pending;
}

/// Records a stopped listener (`error` = why a start failed).
pub fn set_stopped(error: Option<String>) {
    let mut c = current().write().unwrap_or_else(|e| e.into_inner());
    c.status = GatewayStatus {
        error,
        ..Default::default()
    };
    c.scheduler = None;
}

pub fn status() -> GatewayStatus {
    current()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .status
        .clone()
}

/// The running gateway's scheduler (Control API `/gateway`, GUI).
pub fn scheduler() -> Option<Arc<scheduler::Scheduler>> {
    current()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .scheduler
        .clone()
}

/// The managed endpoint while the gateway runs.
pub fn managed_endpoint() -> Option<ClientEndpoint> {
    status().endpoint.filter(|_| status().running)
}

/// The endpoint Harness's own clients should use: managed while the gateway runs, else the
/// direct endpoint of `direct` (when given and running).
pub fn preferred_endpoint(
    direct: Option<&crate::process::ActiveServerInfo>,
) -> Option<ClientEndpoint> {
    managed_endpoint().or_else(|| direct.map(ClientEndpoint::direct))
}

// --- client token (Harness's own clients) ---

/// A fresh gateway client token (`hsg_` + 256 bits).
pub fn new_client_token() -> Result<String, String> {
    let token = crate::control_api::new_session_token()?;
    Ok(format!(
        "hsg_{}",
        token.strip_prefix("hss_").unwrap_or(&token)
    ))
}

pub fn client_token_path(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join(CLIENT_TOKEN_FILE)
}

/// Writes the client token readable only by the current user (like the API session token).
pub fn write_client_token(
    data_dir: &std::path::Path,
    token: &str,
) -> Result<std::path::PathBuf, String> {
    let path = client_token_path(data_dir);
    crate::instance::write_private_file(&path, token.as_bytes())?;
    Ok(path)
}

pub fn remove_client_token(data_dir: &std::path::Path) {
    let _ = std::fs::remove_file(client_token_path(data_dir));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_old_default_headroom_means_automatic() {
        let old: GatewaySettings = serde_json::from_str(r#"{"memory_headroom_mib": 512}"#).unwrap();
        assert_eq!(old.memory_headroom_mib, 0);
        let own: GatewaySettings =
            serde_json::from_str(r#"{"memory_headroom_mib": 2048}"#).unwrap();
        assert_eq!(own.memory_headroom_mib, 2048);
        assert_eq!(GatewaySettings::default().memory_headroom_mib, 0);
        let missing: GatewaySettings = serde_json::from_str("{}").unwrap();
        assert_eq!(missing.memory_headroom_mib, 0);
    }

    #[test]
    fn settings_validation() {
        let ok = validate_settings(&GatewaySettings::default(), 0, Some(11500)).unwrap();
        assert_eq!(ok.host, "127.0.0.1");
        assert_eq!(ok.port, DEFAULT_PORT);
        assert!(
            !ok.ollama_port_enabled && !ok.ollama_allow_pull,
            "Ollama extras are off"
        );
        assert_eq!(ok.ollama_port, OLLAMA_DEFAULT_PORT);
        for bad in [DEFAULT_PORT, 11500, 80] {
            let s = GatewaySettings {
                ollama_port_enabled: true,
                ollama_port: bad,
                ..Default::default()
            };
            assert_eq!(
                validate_settings(&s, 0, Some(11500)).unwrap_err(),
                ERR_OLLAMA_PORT_CONFLICT
            );
        }
        // Stored settings from before the Ollama fields load with the defaults.
        let old: GatewaySettings = serde_json::from_str(r#"{"enabled": true}"#).unwrap();
        assert_eq!(old.ollama_port, OLLAMA_DEFAULT_PORT);
        assert!(!ok.enabled);
        assert!(!ok.token_required(), "loopback: no token by default");

        let same_port = GatewaySettings {
            port: 11500,
            ..Default::default()
        };
        assert_eq!(
            validate_settings(&same_port, 0, Some(11500)).unwrap_err(),
            ERR_PORT_CONFLICT
        );
        let low = GatewaySettings {
            port: 80,
            ..Default::default()
        };
        assert_eq!(
            validate_settings(&low, 0, None).unwrap_err(),
            ERR_INVALID_PORT
        );

        let lan = GatewaySettings {
            host: "0.0.0.0".into(),
            ..Default::default()
        };
        assert_eq!(
            validate_settings(&lan, 2, None).unwrap_err(),
            ERR_NETWORK_NOT_ALLOWED
        );
        let lan = GatewaySettings {
            allow_network: true,
            ..lan
        };
        assert_eq!(
            validate_settings(&lan, 0, None).unwrap_err(),
            ERR_NETWORK_WITHOUT_TOKEN,
            "no keyless LAN"
        );
        let lan = validate_settings(&lan, 1, None).unwrap();
        assert!(lan.token_required(), "network bind always needs a token");

        let origins = GatewaySettings {
            cors_origins: vec!["*".into()],
            ..Default::default()
        };
        assert_eq!(
            validate_settings(&origins, 0, None).unwrap_err(),
            ERR_INVALID_ORIGIN
        );

        let mut ignores = GatewaySettings::default();
        ignores.tool_ignores.insert(
            "md-1".into(),
            vec![" web_search ".into(), "web_search".into()],
        );
        ignores
            .tool_ignores
            .insert("md-2".into(), vec!["  ".into()]);
        let v = validate_settings(&ignores, 0, None).unwrap();
        assert_eq!(v.ignored_tools("md-1"), ["web_search".to_string()]);
        assert!(v.ignored_tools("md-2").is_empty());

        // Server-side tools: names are cleaned, inactive entries dropped, default is off.
        assert!(GatewaySettings::default().server_tools_of("md-1").is_none());
        let mut st = GatewaySettings::default();
        st.server_tools.insert(
            " md-1 ".into(),
            ServerTools {
                web_search: false,
                tools: vec![" calculator ".into(), "calculator".into(), "".into()],
            },
        );
        st.server_tools
            .insert("md-2".into(), ServerTools::default());
        let v = validate_settings(&st, 0, None).unwrap();
        assert_eq!(v.server_tools.len(), 1);
        assert_eq!(
            v.server_tools_of("md-1").unwrap().tools,
            ["calculator".to_string()]
        );
        assert!(v.server_tools_of("md-2").is_none());
        // Old settings files (without the field) load with everything off.
        let old: GatewaySettings = serde_json::from_str("{\"enabled\": true}").unwrap();
        assert!(old.server_tools.is_empty());
        assert!(old.text_tool_calls.is_empty());
        // On by default (in the comparison benchmark: Qwen2.5-Coder-14B wrote every call as text).
        assert!(old.recover_text_tools("md-1"));
        st.text_tool_calls = [
            (" md-1 ".into(), false),
            ("md-2".into(), true),
            (" ".into(), false),
        ]
        .into();
        let v = validate_settings(&st, 0, None).unwrap();
        assert!(!v.recover_text_tools("md-1"));
        assert!(v.recover_text_tools("md-2"));
        assert!(v.recover_text_tools("md-3"));
        assert_eq!(v.text_tool_calls.len(), 1);
        let saved: GatewaySettings =
            serde_json::from_str(&serde_json::to_string(&v).unwrap()).unwrap();
        assert!(!saved.recover_text_tools("md-1"));
    }

    #[test]
    fn endpoints() {
        let e = ClientEndpoint::new(EndpointKind::Managed, "0.0.0.0", 11435, true);
        assert_eq!(e.base_url, "http://127.0.0.1:11435/v1");
        let e = ClientEndpoint::new(EndpointKind::Direct, "::1", 8080, false);
        assert_eq!(e.host_url, "http://[::1]:8080");
        let t = new_client_token().unwrap();
        assert!(t.starts_with("hsg_") && t.len() > 40, "{t}");
    }
}
