//! Client integrations (Connections > Tools and clients).
//!
//! Three kinds (formats and sources: `docs/en/INTEGRATIONS.md`):
//!
//! * **config**: Harness writes the tool's own config file — OpenCode, Open Design, Continue,
//!   Claude Code (`settings.json` `env`), Codex (`config.toml` provider table + a
//!   `harness.config.toml` profile), VS Code Copilot BYOK (`chatLanguageModels.json`), Zed
//!   (`language_models.openai_compatible`), Kilo Code (`kilo.json` provider), Goose (a custom
//!   provider file) and LibreChat (`librechat.yaml` custom endpoint, path chosen by the user).
//!   Every write goes through [`keyed`]: key-level edits that keep comments, a ledger of what
//!   Harness set, multi-generation timestamped backups, drift warnings and a rollback that
//!   touches only Harness's keys.
//! * **guide**: step-by-step instructions and copyable fields (Claude Desktop, Cursor, Cline,
//!   Roo Code, JetBrains AI, Jan, Open WebUI, Obsidian Copilot); rendered by the UI.
//! * **launch**: [`launch`] starts Claude Code, Codex, OpenCode, Aider or Goose with
//!   process-only settings (`harness launch`, the GUI's "Başlat").
//!
//! Every integration can be checked with a real request ([`conntest`]).

use crate::error_key::err;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub mod agent_models;
pub mod cli_install;
pub mod conntest;
pub mod drift;
pub mod harness_mcp;
pub mod keyed;
pub mod launch;
pub mod opencode_desktop;
pub mod terminal;

pub use keyed::{ApplyOutcome, RevertOutcome};
use keyed::{Edit, FilePlan, Format, Seg};
use launch::{LaunchTool, WireApi};

/// Display name of the entries Harness writes (marker in lists and provider tables).
pub const ENTRY_NAME: &str = "Harness Studio";

type Markers = Vec<(PathBuf, Format, Vec<Vec<Seg>>)>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientStatus {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub configured: bool,
    pub config_path: Option<String>,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationKind {
    /// Harness writes the tool's config file.
    Config,
    /// Instructions and copyable fields only.
    Guide,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntegrationInfo {
    pub id: String,
    pub name: String,
    pub homepage: String,
    pub docs_url: String,
    pub kind: IntegrationKind,
    /// Detection means something for this tool (otherwise `installed` is always false).
    pub detectable: bool,
    pub installed: bool,
    pub configured: bool,
    pub can_apply: bool,
    /// Smallest server context the tool works with (0 = no requirement).
    pub min_ctx: u32,
    pub config_paths: Vec<String>,
    /// Model id the tool's own config currently uses for Harness Studio (read-only parse;
    /// `None` when not configured or not readable). Compared with the served id to spot stale names.
    pub configured_model: Option<String>,
    /// The tool's own default model as written in its config (OpenCode / Kilo `model`, e.g.
    /// `anthropic/claude-sonnet-4` or `harness/qwen3-4b`); `None` when unset or not applicable.
    /// Harness changes it only when it is unset or already points at Harness, unless the
    /// user asks ([`ApplyRequest::set_default_model`]).
    pub default_model: Option<String>,
    pub snippet: String,
    /// `harness launch <tool>` works for this integration.
    pub launch: Option<LaunchTool>,
    /// The API "Test connection" uses.
    pub test_api: WireApi,
    /// Apply needs a file path from the user (LibreChat).
    pub needs_path: bool,
    /// Files changed after Harness last wrote them (hash differs from the ledger).
    pub drifted: Vec<String>,
    /// Timestamped backups of the integration's files, oldest first.
    pub backups: Vec<String>,
    /// Unix seconds of the last apply (from the ledger).
    pub applied_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub before: Option<String>,
    pub after: String,
}

#[derive(Debug, Clone, Default)]
pub struct Detection {
    pub installed: bool,
    pub configured: bool,
    pub config_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ApplyRequest {
    /// OpenAI-compatible base URL (`http://host:port/v1`).
    pub base_url: String,
    pub api_key: Option<String>,
    pub model_id: String,
    pub model_name: String,
    pub context_size: u32,
    /// A config file the user picked (LibreChat's `librechat.yaml`).
    #[serde(default)]
    pub target_path: Option<String>,
    /// Every model the endpoint serves that the tool should list (OpenCode-format providers:
    /// OpenCode, Kilo Code, Open Design), so the user can switch inside the tool. `model_id` is
    /// always listed, first. Empty: only `model_id`.
    #[serde(default)]
    pub models: Vec<ProviderModel>,
    /// Point the tool's default model (OpenCode / Kilo `model`) at `model_id`. `None`: only when
    /// it is unset or already points at Harness (never override a model the user picked);
    /// `Some(true)`: always; `Some(false)`: leave it as it is.
    #[serde(default)]
    pub set_default_model: Option<bool>,
}

/// What the ledger keeps of an apply ([`keyed::LedgerEntry::request`]): the request without its
/// key, so the config can be written again for another endpoint port (see
/// [`drift::follow_gateway_port`]).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct StoredRequest {
    pub base_url: String,
    pub model_id: String,
    pub model_name: String,
    pub context_size: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<ProviderModel>,
    /// The apply wrote an API key (it is not stored: such configs are not rewritten on their
    /// own).
    #[serde(default)]
    pub had_key: bool,
}

impl StoredRequest {
    pub fn of(request: &ApplyRequest) -> Self {
        Self {
            base_url: request.base_url.clone(),
            model_id: request.model_id.clone(),
            model_name: request.model_name.clone(),
            context_size: request.context_size,
            target_path: request.target_path.clone(),
            models: request.models.clone(),
            had_key: request.has_key(),
        }
    }

    /// The same apply against `base_url`, leaving the tool's default model as it is.
    pub fn to_request(&self, base_url: &str) -> ApplyRequest {
        ApplyRequest {
            base_url: base_url.to_string(),
            api_key: None,
            model_id: self.model_id.clone(),
            model_name: self.model_name.clone(),
            context_size: self.context_size,
            target_path: self.target_path.clone(),
            models: self.models.clone(),
            set_default_model: Some(false),
        }
    }
}

/// One model listed in an OpenCode-format provider block.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProviderModel {
    pub id: String,
    /// Per-request context (0 = the request's `context_size`).
    #[serde(default)]
    pub context_size: u32,
}

impl ApplyRequest {
    /// `http://host:port` (the base URL without `/v1`): Anthropic-style clients add the path.
    pub fn host_url(&self) -> String {
        let b = self.base_url.trim_end_matches('/');
        b.strip_suffix("/v1").unwrap_or(b).to_string()
    }

    fn ctx(&self) -> u32 {
        if self.context_size == 0 {
            32_768
        } else {
            self.context_size
        }
    }

    fn key_or_placeholder(&self) -> String {
        self.api_key
            .clone()
            .filter(|k| !k.trim().is_empty())
            .unwrap_or_else(|| launch::PLACEHOLDER_KEY.to_string())
    }

    /// `(alias, context)` for the provider block: `model_id` first, then `models` (no
    /// duplicates, case-insensitive), each context at least `min_ctx`.
    fn provider_models(&self, min_ctx: u32) -> Vec<(String, u32)> {
        let mut out: Vec<(String, u32)> = vec![(self.model_id.clone(), self.ctx().max(min_ctx))];
        for m in &self.models {
            let id = m.id.trim();
            if id.is_empty() || out.iter().any(|(a, _)| a.eq_ignore_ascii_case(id)) {
                continue;
            }
            let ctx = if m.context_size == 0 {
                self.ctx()
            } else {
                m.context_size
            };
            out.push((id.to_string(), ctx.max(min_ctx)));
        }
        out
    }

    fn has_key(&self) -> bool {
        self.api_key
            .as_deref()
            .is_some_and(|k| !k.trim().is_empty())
    }
}

/// Base directories used to locate client configs. Injectable so tests can point them at a
/// temporary directory instead of the real user profile.
#[derive(Debug, Clone, Default)]
pub struct Roots {
    pub home: Option<PathBuf>,
    /// Windows `%APPDATA%`, macOS `~/Library/Application Support`, Linux `$XDG_CONFIG_HOME`.
    pub appdata: Option<PathBuf>,
    pub localappdata: Option<PathBuf>,
    /// `CLAUDE_CONFIG_DIR` (default `~/.claude`).
    pub claude_config_dir: Option<PathBuf>,
    /// `CODEX_HOME` (default `~/.codex`).
    pub codex_home: Option<PathBuf>,
    /// The ledger of what Harness wrote (`<data>/config/integrations.json`).
    pub ledger: Option<PathBuf>,
}

impl Roots {
    pub fn from_env() -> Self {
        let env = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let home = env(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
        let appdata = if cfg!(windows) {
            env("APPDATA")
        } else if cfg!(target_os = "macos") {
            home.as_ref().map(|h| h.join("Library/Application Support"))
        } else {
            env("XDG_CONFIG_HOME").or_else(|| home.as_ref().map(|h| h.join(".config")))
        };
        let localappdata = if cfg!(windows) {
            env("LOCALAPPDATA")
        } else if cfg!(target_os = "macos") {
            home.as_ref().map(|h| h.join("Library/Application Support"))
        } else {
            env("XDG_DATA_HOME").or_else(|| home.as_ref().map(|h| h.join(".local/share")))
        };
        Roots {
            home,
            appdata,
            localappdata,
            claude_config_dir: env("CLAUDE_CONFIG_DIR"),
            codex_home: env("CODEX_HOME"),
            ledger: Some(crate::paths::config_dir().join("integrations.json")),
        }
    }

    fn need_home(&self) -> Result<&Path, String> {
        self.home
            .as_deref()
            .ok_or_else(|| err("integrations.home_not_found").into())
    }

    /// `~/.config` on macOS/Windows (tools that follow XDG everywhere), `$XDG_CONFIG_HOME` on
    /// Linux.
    fn xdg_config(&self) -> Option<PathBuf> {
        if cfg!(target_os = "linux") {
            self.appdata.clone()
        } else {
            self.home.as_ref().map(|h| h.join(".config"))
        }
    }

    fn ledger(&self) -> Option<&Path> {
        self.ledger.as_deref()
    }
}

// ---------------------------------------------------------------------------
// Tolerant JSON (JSONC) reading
// ---------------------------------------------------------------------------

/// Removes `//` and `/* */` comments (outside strings). Returns the cleaned text and whether
/// any comment was found.
fn strip_comments(input: &str) -> (String, bool) {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut found = false;
    let mut i = 0;
    let mut in_str = false;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 1;
            } else if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
                i += 1;
            }
            '/' if chars.get(i + 1) == Some(&'/') => {
                found = true;
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                found = true;
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                i = (i + 2).min(chars.len());
                out.push(' ');
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    (out, found)
}

/// Removes commas that directly precede `}` or `]` (outside strings).
fn strip_trailing_commas(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut in_str = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 1;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if c == ',' {
            let next = chars[i + 1..].iter().find(|ch| !ch.is_whitespace());
            if !matches!(next, Some('}') | Some(']')) {
                out.push(c);
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// Parses JSON that may contain a BOM, comments and trailing commas. Returns the value and
/// whether the source contained comments.
fn parse_tolerant(raw: &str) -> Result<(Value, bool), serde_json::Error> {
    let text = raw.trim_start_matches('\u{feff}');
    let (no_comments, had_comments) = strip_comments(text);
    let clean = strip_trailing_commas(&no_comments);
    serde_json::from_str(&clean).map(|v| (v, had_comments))
}

fn unparseable_msg(path: &Path) -> String {
    err("integrations.config_unparseable")
        .with("path", path.display())
        .into()
}

/// Reads an existing config. `Ok(None)` when the file does not exist. Errors (Turkish) if
/// unreadable, unparseable or the root is not an object; the file is never touched then.
fn read_config(path: &Path) -> Result<Option<(Value, bool)>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(path).map_err(|e| {
        err("fs.read_failed")
            .with("path", path.display())
            .with("error", e)
    })?;
    let raw = String::from_utf8(bytes).map_err(|_| unparseable_msg(path))?;
    match parse_tolerant(&raw) {
        Ok((v, c)) if v.is_object() => Ok(Some((v, c))),
        _ => Err(unparseable_msg(path)),
    }
}

/// Reads a JSON(C), YAML or TOML config as a JSON value (for detection; never errors).
fn read_value(path: &Path) -> Option<Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    match path.extension().and_then(|e| e.to_str()) {
        Some("yaml") | Some("yml") => parse_yaml(&raw).ok(),
        Some("toml") => toml::from_str::<toml::Value>(&raw)
            .ok()
            .and_then(|v| serde_json::to_value(v).ok()),
        _ => parse_tolerant(&raw).ok().map(|(v, _)| v),
    }
}

/// Parses YAML into a JSON value (an empty file is an empty object).
fn parse_yaml(raw: &str) -> Result<Value, String> {
    let raw = raw.trim_start_matches('\u{feff}');
    if raw.trim().is_empty() {
        return Ok(json!({}));
    }
    let value: Value = serde_norway::from_str(raw).map_err(|e| e.to_string())?;
    if value.is_null() {
        Ok(json!({}))
    } else {
        Ok(value)
    }
}

fn file_change(path: &Path, after: String) -> FileChange {
    FileChange {
        path: path.to_string_lossy().to_string(),
        before: std::fs::read_to_string(path).ok(),
        after,
    }
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Existing OpenCode config (`opencode.json` preferred, then `opencode.jsonc`), or the default
/// `opencode.json` path when neither exists.
fn opencode_config_path(home: &Path) -> PathBuf {
    let dir = home.join(".config").join("opencode");
    let json = dir.join("opencode.json");
    let jsonc = dir.join("opencode.jsonc");
    if !json.exists() && jsonc.exists() {
        jsonc
    } else {
        json
    }
}

fn continue_config_path(home: &Path) -> (PathBuf, bool) {
    let dir = home.join(".continue");
    let yaml = dir.join("config.yaml");
    let json = dir.join("config.json");
    if yaml.exists() || !json.exists() {
        (yaml, true)
    } else {
        (json, false)
    }
}

/// Open Design's bundled opencode CLI. Its local "opencode" agent looks for `opencode` on
/// PATH, which OpenCode desktop does not provide; this binary works in its place.
fn open_design_bundled_opencode(localappdata: Option<&Path>) -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        "opencode.exe"
    } else {
        "opencode"
    };
    let path = localappdata?
        .join("Programs")
        .join("Open Design")
        .join("resources")
        .join("open-design")
        .join("bin")
        .join("libexec")
        .join("opencode")
        .join(exe);
    path.is_file().then_some(path)
}

fn on_path(names: &[&str]) -> bool {
    names.iter().any(|n| launch::find_program(n).is_some())
}

/// The opencode binary Open Design should use, when it would not find one by itself.
fn open_design_opencode_bin(roots: &Roots) -> Option<PathBuf> {
    if on_path(&["opencode", "opencode-cli"]) {
        return None;
    }
    open_design_bundled_opencode(roots.localappdata.as_deref())
}

fn open_design_config_paths(data_root: &Path) -> Vec<PathBuf> {
    let namespaces = data_root.join("Open Design").join("namespaces");
    let mut paths: Vec<_> = std::fs::read_dir(&namespaces)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path().join("data"))
        // A fresh or reset install has the data folder but no app-config.json yet.
        .filter(|data| data.is_dir())
        .map(|data| data.join("app-config.json"))
        .collect();
    paths.sort();
    paths
}

fn claude_dir(r: &Roots) -> Result<PathBuf, String> {
    match &r.claude_config_dir {
        Some(d) => Ok(d.clone()),
        None => Ok(r.need_home()?.join(".claude")),
    }
}

fn codex_dir(r: &Roots) -> Result<PathBuf, String> {
    match &r.codex_home {
        Some(d) => Ok(d.clone()),
        None => Ok(r.need_home()?.join(".codex")),
    }
}

fn vscode_user_dir(r: &Roots) -> Option<PathBuf> {
    r.appdata.as_ref().map(|a| a.join("Code").join("User"))
}

fn zed_settings_path(r: &Roots) -> Option<PathBuf> {
    if cfg!(windows) {
        r.appdata
            .as_ref()
            .map(|a| a.join("Zed").join("settings.json"))
    } else {
        r.xdg_config().map(|c| c.join("zed").join("settings.json"))
    }
}

fn kilo_config_path(r: &Roots) -> Option<PathBuf> {
    let dir = r.xdg_config()?.join("kilo");
    ["kilo.jsonc", "kilo.json", "opencode.json", "opencode.jsonc"]
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_file())
        .or_else(|| Some(dir.join("kilo.json")))
}

fn goose_config_dir(r: &Roots) -> Option<PathBuf> {
    if cfg!(windows) {
        r.appdata
            .as_ref()
            .map(|a| a.join("Block").join("goose").join("config"))
    } else {
        r.xdg_config().map(|c| c.join("goose"))
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

type PlanFn = fn(&Roots, &ApplyRequest) -> Result<Vec<FilePlan>, String>;

#[derive(Clone, Copy)]
pub struct IntegrationMeta {
    pub name: &'static str,
    pub homepage: &'static str,
    pub docs_url: &'static str,
    pub kind: IntegrationKind,
    pub min_ctx: u32,
    pub launch: Option<LaunchTool>,
    pub test_api: WireApi,
    pub needs_path: bool,
    pub snippet_template: &'static str,
}

/// One tool. Guide-only tools have no plan.
pub trait Integration: Send + Sync {
    fn id(&self) -> &'static str;
    fn meta(&self) -> IntegrationMeta;
    fn detect(&self, roots: &Roots) -> Detection;
    /// The files and keys Harness writes.
    fn plan(&self, roots: &Roots, request: &ApplyRequest) -> Result<Vec<FilePlan>, String>;
    /// Keys that name Harness (removed on rollback when there is no ledger entry).
    fn markers(&self, roots: &Roots) -> Markers;
    fn configured_model(&self, roots: &Roots) -> Option<String>;
    fn writes_files(&self) -> bool;
}

struct Spec {
    id: &'static str,
    meta: IntegrationMeta,
    detect: fn(&Roots) -> Detection,
    plan: Option<PlanFn>,
    markers: fn(&Roots) -> Markers,
    configured_model: fn(&Roots) -> Option<String>,
}

impl Integration for Spec {
    fn id(&self) -> &'static str {
        self.id
    }
    fn meta(&self) -> IntegrationMeta {
        self.meta
    }
    fn detect(&self, roots: &Roots) -> Detection {
        (self.detect)(roots)
    }
    fn plan(&self, roots: &Roots, request: &ApplyRequest) -> Result<Vec<FilePlan>, String> {
        match self.plan {
            Some(f) => f(roots, request),
            None => Err(err("integrations.guide_only").into()),
        }
    }
    fn markers(&self, roots: &Roots) -> Markers {
        (self.markers)(roots)
    }
    fn configured_model(&self, roots: &Roots) -> Option<String> {
        (self.configured_model)(roots)
    }
    fn writes_files(&self) -> bool {
        self.plan.is_some()
    }
}

fn no_markers(_: &Roots) -> Markers {
    Vec::new()
}

fn no_model(_: &Roots) -> Option<String> {
    None
}

fn nothing_detected(_: &Roots) -> Detection {
    Detection::default()
}

const fn meta(
    name: &'static str,
    homepage: &'static str,
    docs_url: &'static str,
    kind: IntegrationKind,
    min_ctx: u32,
    launch: Option<LaunchTool>,
    test_api: WireApi,
) -> IntegrationMeta {
    IntegrationMeta {
        name,
        homepage,
        docs_url,
        kind,
        min_ctx,
        launch,
        test_api,
        needs_path: false,
        snippet_template: "Base URL: {base_url}\nModel: {model_id}",
    }
}

fn guide(
    id: &'static str,
    name: &'static str,
    homepage: &'static str,
    docs_url: &'static str,
    test_api: WireApi,
    detect: fn(&Roots) -> Detection,
) -> Spec {
    Spec {
        id,
        meta: meta(
            name,
            homepage,
            docs_url,
            IntegrationKind::Guide,
            0,
            None,
            test_api,
        ),
        detect,
        plan: None,
        markers: no_markers,
        configured_model: no_model,
    }
}

fn registry() -> Vec<Spec> {
    use IntegrationKind::{Config, Guide};
    vec![
        Spec {
            id: "claude-code",
            meta: meta(
                "Claude Code",
                "https://code.claude.com",
                "https://code.claude.com/docs/en/llm-gateway-connect",
                Config,
                LaunchTool::Claude.min_ctx(),
                Some(LaunchTool::Claude),
                WireApi::Anthropic,
            ),
            detect: detect_claude,
            plan: Some(plan_claude),
            markers: no_markers,
            configured_model: model_claude,
        },
        Spec {
            id: "codex",
            meta: meta(
                "Codex",
                "https://developers.openai.com/codex",
                "https://developers.openai.com/codex/config-advanced",
                Config,
                LaunchTool::Codex.min_ctx(),
                Some(LaunchTool::Codex),
                WireApi::Responses,
            ),
            detect: detect_codex,
            plan: Some(plan_codex),
            markers: markers_codex,
            configured_model: model_codex,
        },
        Spec {
            id: "opencode",
            meta: IntegrationMeta {
                snippet_template:
                    "Provider: harness\nBase URL: {base_url}\nModel: harness/{model_id}",
                // ~29k-token prompts plus the 8k output reserve: at 32k OpenCode compacts every turn.
                ..meta(
                    "OpenCode",
                    "https://opencode.ai",
                    "https://opencode.ai/docs/providers/",
                    Config,
                    LaunchTool::OpenCode.min_ctx(),
                    Some(LaunchTool::OpenCode),
                    WireApi::Chat,
                )
            },
            detect: detect_opencode,
            plan: Some(plan_opencode),
            markers: markers_opencode,
            configured_model: model_opencode,
        },
        Spec {
            id: "opendesign",
            meta: meta(
                "Open Design",
                "https://open-design.ai",
                "https://opencode.ai/docs/providers/",
                Config,
                32_768,
                None,
                WireApi::Chat,
            ),
            detect: detect_opendesign,
            plan: Some(plan_opendesign),
            markers: markers_opencode,
            configured_model: model_opendesign,
        },
        Spec {
            id: "continue",
            meta: meta(
                "Continue",
                "https://continue.dev",
                "https://docs.continue.dev/customize/model-providers/top-level/openai",
                Config,
                0,
                None,
                WireApi::Chat,
            ),
            detect: detect_continue,
            plan: Some(plan_continue),
            markers: markers_continue,
            configured_model: model_continue,
        },
        Spec {
            id: "vscode-copilot",
            meta: meta(
                "VS Code Copilot (BYOK)",
                "https://code.visualstudio.com",
                "https://code.visualstudio.com/docs/agent-customization/language-models",
                Config,
                0,
                None,
                WireApi::Chat,
            ),
            detect: detect_vscode,
            plan: Some(plan_vscode),
            markers: markers_vscode,
            configured_model: model_vscode,
        },
        Spec {
            id: "zed",
            meta: meta(
                "Zed",
                "https://zed.dev",
                "https://zed.dev/docs/ai/use-api-access",
                Config,
                0,
                None,
                WireApi::Chat,
            ),
            detect: detect_zed,
            plan: Some(plan_zed),
            markers: markers_zed,
            configured_model: model_zed,
        },
        Spec {
            id: "kilo",
            meta: meta(
                "Kilo Code",
                "https://kilo.ai",
                "https://kilo.ai/docs/ai-providers/openai-compatible",
                Config,
                32_768,
                None,
                WireApi::Chat,
            ),
            detect: detect_kilo,
            plan: Some(plan_kilo),
            markers: markers_kilo,
            configured_model: model_kilo,
        },
        Spec {
            id: "goose",
            meta: meta(
                "Goose",
                "https://goose-docs.ai",
                "https://github.com/aaif-goose/goose/blob/main/documentation/docs/getting-started/providers.md",
                Config,
                LaunchTool::Goose.min_ctx(),
                Some(LaunchTool::Goose),
                WireApi::Chat,
            ),
            detect: detect_goose,
            plan: Some(plan_goose),
            markers: no_markers,
            configured_model: model_goose,
        },
        Spec {
            id: "librechat",
            meta: IntegrationMeta {
                needs_path: true,
                ..meta(
                    "LibreChat",
                    "https://www.librechat.ai",
                    "https://www.librechat.ai/docs/configuration/librechat_yaml/object_structure/custom_endpoint",
                    Config,
                    0,
                    None,
                    WireApi::Chat,
                )
            },
            detect: nothing_detected,
            plan: Some(plan_librechat),
            markers: no_markers,
            configured_model: no_model,
        },
        Spec {
            id: "aider",
            meta: IntegrationMeta {
                snippet_template: "OPENAI_API_BASE={base_url}\naider --model openai/{model_id}",
                ..meta(
                    "Aider",
                    "https://aider.chat",
                    "https://aider.chat/docs/llms/openai-compat.html",
                    Guide,
                    LaunchTool::Aider.min_ctx(),
                    Some(LaunchTool::Aider),
                    WireApi::Chat,
                )
            },
            detect: detect_aider,
            plan: None,
            markers: no_markers,
            configured_model: no_model,
        },
        guide(
            "claude-desktop",
            "Claude Desktop",
            "https://claude.ai/download",
            "https://claude.com/docs/third-party/claude-desktop/gateway",
            WireApi::Anthropic,
            detect_claude_desktop,
        ),
        guide(
            "cursor",
            "Cursor",
            "https://cursor.com",
            "https://cursor.com/docs/settings/api-keys",
            WireApi::Chat,
            detect_cursor,
        ),
        guide(
            "cline",
            "Cline",
            "https://cline.bot",
            "https://docs.cline.bot/provider-config/openai-compatible",
            WireApi::Chat,
            detect_cline,
        ),
        guide(
            "roo-code",
            "Roo Code",
            "https://roocode.com",
            "https://docs.roocode.com/sunset",
            WireApi::Chat,
            detect_roo,
        ),
        guide(
            "jetbrains",
            "JetBrains AI Assistant",
            "https://www.jetbrains.com/ai/",
            "https://www.jetbrains.com/help/ai-assistant/use-custom-models.html",
            WireApi::Chat,
            nothing_detected,
        ),
        guide(
            "jan",
            "Jan",
            "https://jan.ai",
            "https://www.jan.ai/docs/desktop/remote-models/custom-endpoint",
            WireApi::Chat,
            nothing_detected,
        ),
        guide(
            "open-webui",
            "Open WebUI",
            "https://openwebui.com",
            "https://docs.openwebui.com/getting-started/quick-start/connect-a-provider/starting-with-openai-compatible",
            WireApi::Chat,
            nothing_detected,
        ),
        guide(
            "obsidian-copilot",
            "Obsidian Copilot",
            "https://www.obsidiancopilot.com",
            "https://docs.obsidiancopilot.com/settings/",
            WireApi::Chat,
            nothing_detected,
        ),
    ]
}

fn find(id: &str) -> Result<Spec, String> {
    registry()
        .into_iter()
        .find(|i| i.id == id)
        .ok_or_else(|| err("integrations.unsupported_client").with("id", id).into())
}

/// Tools with detection worth showing (the rest are UI-only tools Harness cannot see).
fn detectable(id: &str) -> bool {
    !matches!(
        id,
        "librechat" | "jetbrains" | "jan" | "open-webui" | "obsidian-copilot"
    )
}

fn render_snippet(template: &str, request: &ApplyRequest) -> String {
    template
        .replace("{base_url}", &request.base_url)
        .replace("{model_id}", &request.model_id)
        .replace("{context_size}", &request.context_size.to_string())
}

pub fn list_integrations(
    base_url: &str,
    model_id: &str,
    context_size: u32,
) -> Vec<IntegrationInfo> {
    list_integrations_in(&Roots::from_env(), base_url, model_id, context_size)
}

fn list_integrations_in(
    roots: &Roots,
    base_url: &str,
    model_id: &str,
    context_size: u32,
) -> Vec<IntegrationInfo> {
    let request = ApplyRequest {
        base_url: base_url.to_string(),
        model_id: model_id.to_string(),
        model_name: model_id.to_string(),
        context_size,
        ..Default::default()
    };
    let ledger = keyed::Ledger::load(roots.ledger());
    registry()
        .into_iter()
        .map(|spec| {
            let meta = spec.meta();
            let detection = spec.detect(roots);
            let entry = ledger.entry(spec.id);
            let configured = detection.configured || entry.is_some();
            let mut paths = detection.config_paths.clone();
            for f in entry.iter().flat_map(|e| e.files.iter()) {
                let p = PathBuf::from(&f.path);
                if !paths.contains(&p) {
                    paths.push(p);
                }
            }
            let backups: Vec<String> = paths
                .iter()
                .flat_map(|p| keyed::backups_of(p))
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            IntegrationInfo {
                id: spec.id.to_string(),
                name: meta.name.to_string(),
                homepage: meta.homepage.to_string(),
                docs_url: meta.docs_url.to_string(),
                kind: meta.kind,
                detectable: detectable(spec.id),
                installed: detection.installed,
                configured,
                can_apply: spec.writes_files() && context_size >= meta.min_ctx,
                min_ctx: meta.min_ctx,
                config_paths: paths
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect(),
                configured_model: if configured {
                    spec.configured_model(roots)
                } else {
                    None
                },
                default_model: tool_default_model(roots, spec.id),
                snippet: render_snippet(meta.snippet_template, &request),
                launch: meta.launch,
                test_api: meta.test_api,
                needs_path: meta.needs_path,
                drifted: keyed::drifted_files(spec.id, roots.ledger()),
                backups,
                applied_at: entry.map(|e| e.applied_at),
            }
        })
        .collect()
}

/// The files that would change, without writing anything.
pub fn preview_integration(id: &str, request: &ApplyRequest) -> Result<Vec<FileChange>, String> {
    let request = chat_request(request)?;
    preview_in(&Roots::from_env(), id, &request)
}

/// Sanitize once before dispatch to any writer (OpenCode, Zed, VS Code, Continue, Kilo...).
fn chat_request(request: &ApplyRequest) -> Result<ApplyRequest, String> {
    let defs = crate::domain::store::Domain::global().definitions()?;
    filter_chat_request(request, &defs, |p| crate::gguf::read_gguf_metadata(p).ok())
}

fn filter_chat_request(
    request: &ApplyRequest,
    defs: &[crate::domain::ModelDefinition],
    meta: impl Fn(&Path) -> Option<crate::gguf::GgufMetadata>,
) -> Result<ApplyRequest, String> {
    use crate::domain::ModelRef;
    let capabilities: Vec<_> = defs
        .iter()
        .map(|d| {
            let m = match &d.base {
                ModelRef::Local { path } => meta(path),
                _ => None,
            };
            (d, agent_models::chat_capability(d, m.as_ref()))
        })
        .collect();
    let is_chat = |id: &str| {
        // Direct/router endpoints may name a file without a saved definition.
        let path = Path::new(id);
        let direct = crate::domain::ModelDefinition::new(id, ModelRef::Local { path: path.into() });
        let direct_meta = path.is_file().then(|| meta(path)).flatten();
        if agent_models::chat_capability(&direct, direct_meta.as_ref()) == Some(false) {
            return false;
        }
        let stem = |s: &str| {
            let file = s
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(s)
                .to_ascii_lowercase();
            file.strip_suffix(".gguf")
                .or_else(|| file.strip_suffix(".bin"))
                .unwrap_or(&file)
                .to_string()
        };
        !capabilities.iter().any(|(d, chat)| {
            let file = match &d.base {
                ModelRef::Local { path } => path.to_string_lossy().to_string(),
                ModelRef::Hf { file, .. } => file.clone(),
            };
            (d.alias.eq_ignore_ascii_case(id)
                || file.eq_ignore_ascii_case(id)
                || stem(&file) == stem(id))
                && *chat == Some(false)
        })
    };
    if !is_chat(&request.model_id) {
        return Err(err("integrations.chat_model_required")
            .with("model", &request.model_id)
            .into());
    }
    let mut filtered = request.clone();
    filtered.models.retain(|m| is_chat(&m.id));
    Ok(filtered)
}

/// Result of an apply for the UI.
#[derive(Debug, Clone, Serialize)]
pub struct ApplyResult {
    pub message: String,
    #[serde(flatten)]
    pub outcome: ApplyOutcome,
}

pub fn apply_integration(id: &str, request: &ApplyRequest) -> Result<ApplyResult, String> {
    let request = chat_request(request)?;
    apply_in(&Roots::from_env(), id, &request)
}

/// Removes (or restores) only the keys Harness set; see [`keyed`].
pub fn revert_integration(id: &str) -> Result<RevertOutcome, String> {
    revert_in(&Roots::from_env(), id)
}

pub(crate) fn preview_in(
    roots: &Roots,
    id: &str,
    request: &ApplyRequest,
) -> Result<Vec<FileChange>, String> {
    let spec = find(id)?;
    let plans = spec.plan(roots, request)?;
    let ledger = keyed::Ledger::load(roots.ledger());
    plans
        .iter()
        .map(|p| {
            keyed::render(id, p, &ledger).map(|r| FileChange {
                path: r.path.to_string_lossy().to_string(),
                before: r.before,
                after: r.after,
            })
        })
        .collect()
}

pub(crate) fn apply_in(
    roots: &Roots,
    id: &str,
    request: &ApplyRequest,
) -> Result<ApplyResult, String> {
    let spec = find(id)?;
    let plans = spec.plan(roots, request)?;
    let outcome = keyed::apply(id, &plans, roots.ledger())?;
    remember_request(roots, id, request);
    let mut message = format!(
        "{} yapılandırıldı. Model: {} (bağlam {}).",
        spec.meta.name,
        request.model_id,
        request.ctx()
    );
    if let Some(extra) = after_apply_note(id, request) {
        message.push(' ');
        message.push_str(extra);
    }
    Ok(ApplyResult { message, outcome })
}

/// Keeps the request of a successful apply in its ledger entry (best effort: without it the
/// config only misses the automatic port update).
fn remember_request(roots: &Roots, id: &str, request: &ApplyRequest) {
    let mut ledger = keyed::Ledger::load(roots.ledger());
    let Some(entry) = ledger.entries.iter_mut().find(|e| e.id == id) else {
        return;
    };
    entry.request = serde_json::to_value(StoredRequest::of(request)).ok();
    if let Err(e) = ledger.save(roots.ledger()) {
        eprintln!("[Harness Studio] Entegrasyon kaydı yazılamadı: {}", e);
    }
}

fn after_apply_note(id: &str, request: &ApplyRequest) -> Option<&'static str> {
    Some(match id {
        "codex" => "Başlatmak için: codex --profile harness",
        "goose" => "Goose'ta sağlayıcı olarak \"Harness Studio\" (harness) seçin: goose configure ya da GOOSE_PROVIDER=harness.",
        "zed" if request.has_key() => "Anahtarı Zed'in Agent panelinde Harness Studio sağlayıcısına girin ya da HARNESS_STUDIO_API_KEY ortam değişkenini ayarlayıp Zed'i yeniden başlatın.",
        "zed" => "Modelleri görmek için Zed Agent panelinde Harness Studio sağlayıcısının API anahtarına harness-local yazın. Alternatif: HARNESS_STUDIO_API_KEY ortam değişkenini ayarlayıp Zed'i yeniden başlatın.",
        "vscode-copilot" => "VS Code'da Chat: Manage Language Models ile modeli seçin.",
        "opendesign" => "Open Design yerel OpenCode ajanını kullanacak; açıksa yeniden başlatın.",
        "librechat" => "LibreChat'i yeniden başlatın. Docker'da çalışıyorsa adreste 127.0.0.1 yerine host.docker.internal kullanın.",
        "claude-code" => "Claude Code bu ayarlarla açılışta Harness'a bağlanır; Anthropic hesabına dönmek için Geri al.",
        _ => return None,
    })
}

pub(crate) fn revert_in(roots: &Roots, id: &str) -> Result<RevertOutcome, String> {
    let spec = find(id)?;
    if !spec.writes_files() {
        return Err(err("integrations.no_file_changes").into());
    }
    keyed::revert(id, &spec.markers(roots), roots.ledger())
}

/// Detects which tools are installed (registry order).
pub fn detect_all_clients() -> Vec<ClientStatus> {
    let roots = Roots::from_env();
    registry()
        .into_iter()
        .map(|spec| {
            let d = spec.detect(&roots);
            ClientStatus {
                id: spec.id.to_string(),
                name: spec.meta.name.to_string(),
                installed: d.installed,
                configured: d.configured,
                config_path: d
                    .config_paths
                    .first()
                    .map(|p| p.to_string_lossy().to_string()),
                details: None,
            }
        })
        .collect()
}

/// A config file of `format` with Harness's keys.
fn keys_plan(path: PathBuf, format: Format, edits: Vec<Edit>, new_doc: Option<Value>) -> FilePlan {
    FilePlan::Keys {
        path,
        format,
        edits,
        new_doc,
    }
}

fn exists(p: &Option<PathBuf>) -> bool {
    p.as_deref().is_some_and(Path::exists)
}

fn detection(installed: bool, paths: Vec<PathBuf>, configured: bool) -> Detection {
    Detection {
        installed,
        configured,
        config_paths: paths.into_iter().filter(|p| p.exists()).collect(),
    }
}

// ---------------------------------------------------------------------------
// Claude Code: ~/.claude/settings.json `env`
// ---------------------------------------------------------------------------

fn claude_settings(r: &Roots) -> Result<PathBuf, String> {
    Ok(claude_dir(r)?.join("settings.json"))
}

fn detect_claude(r: &Roots) -> Detection {
    let dir = claude_dir(r).ok();
    let settings = claude_settings(r).ok();
    detection(
        exists(&dir) || on_path(&["claude"]),
        settings.into_iter().collect(),
        false,
    )
}

fn plan_claude(r: &Roots, q: &ApplyRequest) -> Result<Vec<FilePlan>, String> {
    let env = launch::claude_env(
        &q.host_url(),
        &q.model_id,
        &q.key_or_placeholder(),
        Some(q.ctx()),
    );
    let edits = env
        .into_iter()
        .map(|(k, v)| Edit::set(keyed::keys(&["env", &k]), Value::String(v)))
        .collect();
    Ok(vec![keys_plan(
        claude_settings(r)?,
        Format::Json,
        edits,
        None,
    )])
}

fn model_claude(r: &Roots) -> Option<String> {
    read_value(&claude_settings(r).ok()?)?["env"]["ANTHROPIC_MODEL"]
        .as_str()
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// Codex: config.toml [model_providers.harness] + harness.config.toml profile
// ---------------------------------------------------------------------------

fn detect_codex(r: &Roots) -> Detection {
    let dir = codex_dir(r).ok();
    let paths = dir
        .iter()
        .flat_map(|d| [d.join("config.toml"), d.join("harness.config.toml")])
        .collect();
    let configured = dir
        .as_ref()
        .is_some_and(|d| d.join("harness.config.toml").is_file());
    detection(exists(&dir) || on_path(&["codex"]), paths, configured)
}

fn plan_codex(r: &Roots, q: &ApplyRequest) -> Result<Vec<FilePlan>, String> {
    let dir = codex_dir(r)?;
    let mut headers = json!({ "X-Harness-Client": "codex" });
    if let Some(k) = q.api_key.as_deref().filter(|k| !k.trim().is_empty()) {
        headers["Authorization"] = json!(format!("Bearer {}", k));
    }
    let provider = json!({
        "name": ENTRY_NAME,
        "base_url": q.base_url,
        "wire_api": "responses",
        "http_headers": headers,
    });
    let profile = format!(
        "# Harness Studio profile (written by Harness Studio). Start Codex with:\n#   codex --profile harness\nmodel_provider = \"harness\"\nmodel = {}\nmodel_context_window = {}\n# Hosted web search cannot run on llama.cpp (the gateway rejects it instead of dropping it).\nweb_search = \"disabled\"\n",
        toml_edit::Value::from(q.model_id.as_str()),
        q.ctx()
    );
    Ok(vec![
        keys_plan(
            dir.join("config.toml"),
            Format::Toml,
            vec![Edit::marker(
                keyed::keys(&["model_providers", "harness"]),
                provider,
            )],
            None,
        ),
        FilePlan::Whole {
            path: dir.join("harness.config.toml"),
            content: profile,
        },
    ])
}

fn markers_codex(r: &Roots) -> Markers {
    codex_dir(r)
        .map(|d| {
            vec![(
                d.join("config.toml"),
                Format::Toml,
                vec![keyed::keys(&["model_providers", "harness"])],
            )]
        })
        .unwrap_or_default()
}

fn model_codex(r: &Roots) -> Option<String> {
    read_value(&codex_dir(r).ok()?.join("harness.config.toml"))?["model"]
        .as_str()
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// OpenCode, Open Design, Continue
// ---------------------------------------------------------------------------

fn opencode_is_configured(root: &Value) -> bool {
    root.get("provider")
        .and_then(|p| p.get("harness"))
        .map(|h| h.is_object())
        .unwrap_or(false)
}

/// `harness/<id>` (or the old `harness:<id>`) -> `<id>`.
fn strip_harness_prefix(model: &str) -> Option<&str> {
    model
        .strip_prefix("harness/")
        .or_else(|| model.strip_prefix("harness:"))
}

fn is_harness_model(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|m| strip_harness_prefix(m).is_some())
}

/// The model OpenCode uses for Harness: the `model` field when it points at the `harness`
/// provider, else the (single) model the provider block lists.
fn opencode_configured_model(root: &Value) -> Option<String> {
    root.get("model")
        .and_then(Value::as_str)
        .and_then(strip_harness_prefix)
        .map(str::to_string)
        .or_else(|| {
            root.get("provider")?
                .get("harness")?
                .get("models")?
                .as_object()?
                .keys()
                .next()
                .cloned()
        })
}

fn detect_opencode(r: &Roots) -> Detection {
    let Some(home) = r.home.as_deref() else {
        return Detection::default();
    };
    let cfg = opencode_config_path(home);
    let configured = read_value(&cfg).is_some_and(|v| opencode_is_configured(&v));
    let desktop = r.localappdata.as_ref().is_some_and(|la| {
        la.join("OpenCode").join("opencode-cli.exe").exists()
            || la
                .join("Programs")
                .join("@opencode-aidesktop")
                .join("OpenCode.exe")
                .exists()
    });
    detection(
        cfg.exists() || desktop || on_path(&["opencode"]),
        vec![cfg],
        configured,
    )
}

fn opencode_plan(path: PathBuf, q: &ApplyRequest, schema: &str) -> FilePlan {
    let mut edits = vec![Edit::marker(
        keyed::keys(&["provider", "harness"]),
        launch::opencode_provider(
            &q.base_url,
            &q.provider_models(16_384),
            &q.key_or_placeholder(),
        ),
    )];
    let model = Edit::set(
        vec![Seg::key("model")],
        json!(format!("harness/{}", q.model_id)),
    );
    match q.set_default_model {
        // Never override a model the user chose; do refresh our own.
        None => edits.push(model.only_if(is_harness_model)),
        Some(true) => edits.push(model),
        Some(false) => {}
    }
    keys_plan(
        path,
        Format::Json,
        edits,
        Some(json!({ "$schema": schema })),
    )
}

/// The tool's own default model (OpenCode / Kilo `model`), as written.
fn tool_default_model(r: &Roots, id: &str) -> Option<String> {
    let path = match id {
        "opencode" | "opendesign" => opencode_config_path(r.home.as_deref()?),
        "kilo" => kilo_config_path(r)?,
        _ => return None,
    };
    read_value(&path)?
        .get("model")?
        .as_str()
        .filter(|m| !m.trim().is_empty())
        .map(str::to_string)
}

fn plan_opencode(r: &Roots, q: &ApplyRequest) -> Result<Vec<FilePlan>, String> {
    Ok(vec![opencode_plan(
        opencode_config_path(r.need_home()?),
        q,
        "https://opencode.ai/config.json",
    )])
}

fn markers_opencode(r: &Roots) -> Markers {
    r.home
        .as_deref()
        .map(|h| {
            vec![(
                opencode_config_path(h),
                Format::Json,
                vec![keyed::keys(&["provider", "harness"])],
            )]
        })
        .unwrap_or_default()
}

fn model_opencode(r: &Roots) -> Option<String> {
    opencode_configured_model(&read_value(&opencode_config_path(r.home.as_deref()?))?)
}

fn opendesign_is_configured(root: &Value) -> bool {
    root.get("agentId").and_then(|a| a.as_str()) == Some("opencode")
        && root["agentModels"]["opencode"]["model"]
            .as_str()
            .is_some_and(|m| strip_harness_prefix(m).is_some())
}

fn detect_opendesign(r: &Roots) -> Detection {
    let exe = r.localappdata.as_ref().is_some_and(|la| {
        la.join("Programs")
            .join("Open Design")
            .join("Open Design.exe")
            .exists()
    });
    let paths = r
        .appdata
        .as_deref()
        .map(open_design_config_paths)
        .unwrap_or_default();
    let configured = paths
        .iter()
        .filter_map(|p| read_value(p))
        .any(|v| opendesign_is_configured(&v));
    Detection {
        installed: exe || !paths.is_empty(),
        configured,
        config_paths: paths,
    }
}

fn plan_opendesign(r: &Roots, q: &ApplyRequest) -> Result<Vec<FilePlan>, String> {
    let mut plans = plan_opencode(r, q)?;
    let bin = open_design_opencode_bin(r);
    for path in r
        .appdata
        .as_deref()
        .map(open_design_config_paths)
        .unwrap_or_default()
    {
        let mut edits = vec![
            Edit::set(
                keyed::keys(&["agentModels", "opencode"]),
                json!({ "model": format!("harness/{}", q.model_id) }),
            ),
            Edit::set(vec![Seg::key("agentId")], json!("opencode")),
        ];
        if let Some(b) = &bin {
            // Keep a binary the user picked themselves.
            edits.push(
                Edit::set(
                    keyed::keys(&["agentCliEnv", "opencode", "OPENCODE_BIN"]),
                    json!(b.to_string_lossy()),
                )
                .only_if(|_| false),
            );
        }
        plans.push(keys_plan(path, Format::Json, edits, None));
    }
    Ok(plans)
}

fn model_opendesign(r: &Roots) -> Option<String> {
    open_design_config_paths(r.appdata.as_deref()?)
        .iter()
        .find_map(|p| {
            read_value(p)?["agentModels"]["opencode"]["model"]
                .as_str()
                .and_then(strip_harness_prefix)
                .map(str::to_string)
        })
}

/// Title of the entry in Continue's `models`.
const CONTINUE_TITLE: &str = "Harness Studio (llama.cpp)";

fn continue_entry(root: &Value) -> Option<&Value> {
    root.get("models")?.as_array()?.iter().find(|m| {
        m.get("title")
            .or_else(|| m.get("name"))
            .and_then(Value::as_str)
            .is_some_and(|t| t.contains(ENTRY_NAME))
    })
}

fn detect_continue(r: &Roots) -> Detection {
    let Some(home) = r.home.as_deref() else {
        return Detection::default();
    };
    let (path, _) = continue_config_path(home);
    let installed = home.join(".continue").exists();
    let configured = read_value(&path).is_some_and(|v| continue_entry(&v).is_some());
    Detection {
        installed,
        configured,
        config_paths: if installed { vec![path] } else { Vec::new() },
    }
}

fn continue_item() -> Vec<Seg> {
    vec![Seg::key("models"), Seg::item("title", CONTINUE_TITLE)]
}

fn plan_continue(r: &Roots, q: &ApplyRequest) -> Result<Vec<FilePlan>, String> {
    let (path, yaml) = continue_config_path(r.need_home()?);
    Ok(vec![keys_plan(
        path,
        if yaml { Format::Yaml } else { Format::Json },
        vec![Edit::marker(
            continue_item(),
            json!({
                "provider": "openai",
                "model": q.model_id,
                "apiBase": q.base_url,
                "apiKey": q.key_or_placeholder(),
            }),
        )],
        None,
    )])
}

fn markers_continue(r: &Roots) -> Markers {
    let Some(home) = r.home.as_deref() else {
        return Vec::new();
    };
    let (path, yaml) = continue_config_path(home);
    vec![(
        path,
        if yaml { Format::Yaml } else { Format::Json },
        vec![continue_item()],
    )]
}

fn model_continue(r: &Roots) -> Option<String> {
    let (path, _) = continue_config_path(r.home.as_deref()?);
    continue_entry(&read_value(&path)?)?["model"]
        .as_str()
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// VS Code Copilot BYOK: chatLanguageModels.json
// ---------------------------------------------------------------------------

fn vscode_models_path(r: &Roots) -> Option<PathBuf> {
    vscode_user_dir(r).map(|d| d.join("chatLanguageModels.json"))
}

fn detect_vscode(r: &Roots) -> Detection {
    let dir = vscode_user_dir(r);
    detection(
        exists(&dir),
        vscode_models_path(r).into_iter().collect(),
        false,
    )
}

fn plan_vscode(r: &Roots, q: &ApplyRequest) -> Result<Vec<FilePlan>, String> {
    let path = vscode_models_path(r)
        .ok_or_else(|| err("integrations.config_dir_not_found").with("tool", "VS Code"))?;
    let ctx = q.ctx();
    let out = (ctx / 4).clamp(1024, 8192);
    let mut entry = json!({
        "vendor": "customendpoint",
        "apiType": "chat-completions",
        "models": [{
            "id": q.model_id,
            "name": format!("{} (Harness Studio)", q.model_id),
            "url": format!("{}/chat/completions", q.base_url.trim_end_matches('/')),
            "toolCalling": true,
            "vision": false,
            "maxInputTokens": ctx.saturating_sub(out),
            "maxOutputTokens": out
        }]
    });
    if q.has_key() {
        // VS Code asks for the value once and keeps it in its secret storage.
        entry["apiKey"] = json!("${input:harnessStudioApiKey}");
    }
    Ok(vec![keys_plan(
        path,
        Format::Json,
        vec![Edit::marker(vec![Seg::item("name", ENTRY_NAME)], entry)],
        None,
    )])
}

fn markers_vscode(r: &Roots) -> Markers {
    vscode_models_path(r)
        .map(|p| (p, Format::Json, vec![vec![Seg::item("name", ENTRY_NAME)]]))
        .into_iter()
        .collect()
}

fn model_vscode(r: &Roots) -> Option<String> {
    let v = read_value(&vscode_models_path(r)?)?;
    v.as_array()?.iter().find(|e| e["name"] == ENTRY_NAME)?["models"][0]["id"]
        .as_str()
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// Zed: language_models.openai_compatible."Harness Studio"
// ---------------------------------------------------------------------------

fn detect_zed(r: &Roots) -> Detection {
    let path = zed_settings_path(r);
    let dir = path
        .as_ref()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    detection(exists(&dir), path.into_iter().collect(), false)
}

fn zed_path() -> Vec<Seg> {
    keyed::keys(&["language_models", "openai_compatible", ENTRY_NAME])
}

fn plan_zed(r: &Roots, q: &ApplyRequest) -> Result<Vec<FilePlan>, String> {
    let path = zed_settings_path(r)
        .ok_or_else(|| err("integrations.config_dir_not_found").with("tool", "Zed"))?;
    let ctx = q.ctx();
    Ok(vec![keys_plan(
        path,
        Format::Json,
        vec![Edit::marker(
            zed_path(),
            json!({
                "api_url": q.base_url,
                "available_models": [{
                    "name": q.model_id,
                    "display_name": format!("{} (Harness Studio)", q.model_id),
                    "max_tokens": ctx,
                    "max_output_tokens": (ctx / 4).clamp(1024, 8192),
                    "capabilities": {
                        "tools": true,
                        "images": false,
                        "parallel_tool_calls": false,
                        "prompt_cache_key": false
                    }
                }]
            }),
        )],
        None,
    )])
}

fn markers_zed(r: &Roots) -> Markers {
    zed_settings_path(r)
        .map(|p| (p, Format::Json, vec![zed_path()]))
        .into_iter()
        .collect()
}

fn model_zed(r: &Roots) -> Option<String> {
    read_value(&zed_settings_path(r)?)?["language_models"]["openai_compatible"][ENTRY_NAME]
        ["available_models"][0]["name"]
        .as_str()
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// Kilo Code: ~/.config/kilo/kilo.json provider (OpenCode format)
// ---------------------------------------------------------------------------

fn detect_kilo(r: &Roots) -> Detection {
    let path = kilo_config_path(r);
    let dir = path
        .as_ref()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let ext = r.home.as_ref().is_some_and(|h| {
        std::fs::read_dir(h.join(".vscode").join("extensions"))
            .into_iter()
            .flatten()
            .flatten()
            .any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with("kilocode.kilo-code")
            })
    });
    let configured = path
        .as_ref()
        .and_then(|p| read_value(p))
        .is_some_and(|v| opencode_is_configured(&v));
    detection(
        exists(&dir) || ext || on_path(&["kilo", "kilocode"]),
        path.into_iter().collect(),
        configured,
    )
}

fn plan_kilo(r: &Roots, q: &ApplyRequest) -> Result<Vec<FilePlan>, String> {
    let path = kilo_config_path(r)
        .ok_or_else(|| err("integrations.config_dir_not_found").with("tool", "Kilo Code"))?;
    Ok(vec![opencode_plan(
        path,
        q,
        "https://app.kilo.ai/config.json",
    )])
}

fn markers_kilo(r: &Roots) -> Markers {
    kilo_config_path(r)
        .map(|p| (p, Format::Json, vec![keyed::keys(&["provider", "harness"])]))
        .into_iter()
        .collect()
}

fn model_kilo(r: &Roots) -> Option<String> {
    opencode_configured_model(&read_value(&kilo_config_path(r)?)?)
}

// ---------------------------------------------------------------------------
// Goose: custom_providers/harness.json (a file Harness owns)
// ---------------------------------------------------------------------------

fn goose_provider_path(r: &Roots) -> Option<PathBuf> {
    goose_config_dir(r).map(|d| d.join("custom_providers").join("harness.json"))
}

fn detect_goose(r: &Roots) -> Detection {
    let dir = goose_config_dir(r);
    let path = goose_provider_path(r);
    detection(
        exists(&dir) || on_path(&["goose"]),
        path.clone().into_iter().collect(),
        exists(&path),
    )
}

fn plan_goose(r: &Roots, q: &ApplyRequest) -> Result<Vec<FilePlan>, String> {
    let path = goose_provider_path(r)
        .ok_or_else(|| err("integrations.config_dir_not_found").with("tool", "Goose"))?;
    let mut provider = json!({
        "name": "harness",
        "engine": "openai",
        "display_name": ENTRY_NAME,
        "description": "Local models served by Harness Studio (llama.cpp)",
        "base_url": format!("{}/chat/completions", q.base_url.trim_end_matches('/')),
        "models": [{ "name": q.model_id, "context_limit": q.ctx() }],
        "headers": { "X-Harness-Client": "goose" },
        "supports_streaming": true,
        "requires_auth": q.has_key()
    });
    if q.has_key() {
        provider["api_key_env"] = json!(launch::TOKEN_ENV);
    }
    Ok(vec![FilePlan::Whole {
        path,
        content: format!(
            "{}\n",
            serde_json::to_string_pretty(&provider)
                .map_err(|e| err("json.serialize_failed").with("error", e))?
        ),
    }])
}

fn model_goose(r: &Roots) -> Option<String> {
    read_value(&goose_provider_path(r)?)?["models"][0]["name"]
        .as_str()
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// LibreChat: librechat.yaml endpoints.custom (path picked by the user)
// ---------------------------------------------------------------------------

fn plan_librechat(_: &Roots, q: &ApplyRequest) -> Result<Vec<FilePlan>, String> {
    let raw = q
        .target_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .ok_or_else(|| err("integrations.librechat_path_missing"))?;
    let path = PathBuf::from(raw);
    let yaml = matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("yaml") | Some("yml")
    );
    if !yaml || !path.is_absolute() {
        return Err(err("integrations.librechat_path_invalid")
            .with("path", raw)
            .into());
    }
    Ok(vec![keys_plan(
        path,
        Format::Yaml,
        vec![Edit::marker(
            vec![
                Seg::key("endpoints"),
                Seg::key("custom"),
                Seg::item("name", ENTRY_NAME),
            ],
            json!({
                "apiKey": q.key_or_placeholder(),
                "baseURL": q.base_url,
                "models": { "default": [q.model_id], "fetch": true },
                "titleConvo": true,
                "titleModel": q.model_id,
                "modelDisplayLabel": ENTRY_NAME
            }),
        )],
        Some(json!({ "version": "1.3.17" })),
    )])
}

// ---------------------------------------------------------------------------
// Guide-only tools and Aider: detection
// ---------------------------------------------------------------------------

fn detect_aider(r: &Roots) -> Detection {
    let conf = r.home.as_ref().map(|h| h.join(".aider.conf.yml"));
    detection(
        exists(&conf) || on_path(&["aider"]),
        conf.into_iter().collect(),
        false,
    )
}

fn detect_claude_desktop(r: &Roots) -> Detection {
    let dir = r.appdata.as_ref().map(|a| a.join("Claude"));
    detection(exists(&dir), Vec::new(), false)
}

fn detect_cursor(r: &Roots) -> Detection {
    let installed = r
        .appdata
        .as_ref()
        .is_some_and(|a| a.join("Cursor").exists())
        || r.localappdata
            .as_ref()
            .is_some_and(|l| l.join("Programs").join("cursor").exists());
    detection(installed, Vec::new(), false)
}

fn vscode_extension_storage(r: &Roots, id: &str) -> bool {
    r.appdata.as_ref().is_some_and(|a| {
        a.join("Code")
            .join("User")
            .join("globalStorage")
            .join(id)
            .exists()
    })
}

fn detect_cline(r: &Roots) -> Detection {
    detection(
        vscode_extension_storage(r, "saoudrizwan.claude-dev"),
        Vec::new(),
        false,
    )
}

fn detect_roo(r: &Roots) -> Detection {
    detection(
        vscode_extension_storage(r, "rooveterinaryinc.roo-cline"),
        Vec::new(),
        false,
    )
}

#[cfg(test)]
mod tests;
