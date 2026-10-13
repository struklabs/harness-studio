//! "Harness MCP'yi ekle": registers the Harness MCP server (`harness mcp` over
//! stdio, or the Control API's `/mcp` over streamable HTTP) in coding agents' MCP configs.
//!
//! | Tool        | File                                              | Entry                          |
//! |-------------|---------------------------------------------------|--------------------------------|
//! | Claude Code | `~/.claude.json` (`$CLAUDE_CONFIG_DIR/.claude.json`) | `mcpServers.harness` (user scope) |
//! | Codex       | `~/.codex/config.toml` (`$CODEX_HOME/config.toml`) | `[mcp_servers.harness]`        |
//! | OpenCode    | `~/.config/opencode/opencode.json(c)`             | `mcp.harness`                  |
//! | Cursor      | `~/.cursor/mcp.json`                              | `mcpServers.harness`           |
//!
//! Formats follow the tools' docs (2026-09): code.claude.com/docs/en/mcp,
//! learn.chatgpt.com/docs/extend/mcp, opencode.ai/docs/mcp-servers, cursor.com/docs/context/mcp.
//!
//! Safety (the same machinery as the provider integrations, `keyed`): tolerant JSONC reading, a
//! file that does not parse is never touched, comment-preserving edits of just the `harness`
//! entry, a ledger (`harness-mcp:<tool>` in `integrations.json`) so removal restores the file as
//! it was (a file Harness created is deleted again), rotating backups, atomic replace, a preview
//! (before/after) without writing. Entries written by older versions (no ledger) are removed by
//! the name `harness`; everything else in the file stays.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{file_change, keyed, read_config, FileChange};
use crate::error_key::err;

/// Name of the entry written into every tool's config.
pub const ENTRY: &str = "harness";

/// How the tool reaches the Harness MCP server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpTransport {
    /// The tool starts `harness mcp` itself.
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    /// The Control API's `/api/harness/v1/mcp` with a named token.
    Http {
        url: String,
        #[serde(default)]
        token: Option<String>,
    },
}

/// A tool Harness MCP can be added to.
#[derive(Debug, Clone, Serialize)]
pub struct McpTargetInfo {
    pub id: String,
    pub name: String,
    pub docs_url: String,
    pub config_path: Option<String>,
    /// The tool's config folder (or file) exists.
    pub installed: bool,
    /// A `harness` entry exists.
    pub configured: bool,
    /// The current `harness` entry (read-only, for display).
    pub entry: Option<Value>,
    /// The config does not parse; apply is refused until the user fixes it.
    pub error: Option<String>,
}

/// Config locations. Injectable so tests never touch the real user profile.
#[derive(Debug, Clone, Default)]
pub struct McpRoots {
    pub home: Option<PathBuf>,
    /// `CLAUDE_CONFIG_DIR` (Claude Code keeps `.claude.json` there when set).
    pub claude_config_dir: Option<PathBuf>,
    /// `CODEX_HOME` (default `~/.codex`).
    pub codex_home: Option<PathBuf>,
    /// The ledger of what Harness wrote (`<data>/config/integrations.json`); `None` = none kept.
    pub ledger: Option<PathBuf>,
}

impl McpRoots {
    pub fn from_env() -> Self {
        let env = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        McpRoots {
            home: env(if cfg!(windows) { "USERPROFILE" } else { "HOME" }),
            claude_config_dir: env("CLAUDE_CONFIG_DIR"),
            codex_home: env("CODEX_HOME"),
            ledger: Some(crate::paths::config_dir().join("integrations.json")),
        }
    }
}

struct Target {
    id: &'static str,
    name: &'static str,
    docs_url: &'static str,
}

const TARGETS: [Target; 4] = [
    Target {
        id: "claude-code",
        name: "Claude Code",
        docs_url: "https://code.claude.com/docs/en/mcp",
    },
    Target {
        id: "codex",
        name: "Codex",
        docs_url: "https://learn.chatgpt.com/docs/extend/mcp?surface=cli",
    },
    Target {
        id: "opencode",
        name: "OpenCode",
        docs_url: "https://opencode.ai/docs/mcp-servers/",
    },
    Target {
        id: "cursor",
        name: "Cursor",
        docs_url: "https://cursor.com/docs/context/mcp",
    },
];

fn target(id: &str) -> Result<&'static Target, String> {
    TARGETS
        .iter()
        .find(|t| t.id == id)
        .ok_or_else(|| err("integrations.unsupported_client").with("id", id).into())
}

fn need_home(r: &McpRoots) -> Result<&Path, String> {
    r.home
        .as_deref()
        .ok_or_else(|| err("integrations.home_not_found").into())
}

/// The config file of a tool and the folder whose presence means "installed".
fn paths(r: &McpRoots, id: &str) -> Result<(PathBuf, PathBuf), String> {
    Ok(match id {
        "claude-code" => match &r.claude_config_dir {
            Some(dir) => (dir.join(".claude.json"), dir.clone()),
            None => {
                let home = need_home(r)?;
                (home.join(".claude.json"), home.join(".claude"))
            }
        },
        "codex" => {
            let dir = match &r.codex_home {
                Some(d) => d.clone(),
                None => need_home(r)?.join(".codex"),
            };
            (dir.join("config.toml"), dir)
        }
        "opencode" => {
            let home = need_home(r)?;
            (
                super::opencode_config_path(home),
                home.join(".config").join("opencode"),
            )
        }
        "cursor" => {
            let dir = need_home(r)?.join(".cursor");
            (dir.join("mcp.json"), dir)
        }
        other => {
            return Err(err("integrations.unsupported_client")
                .with("id", other)
                .into())
        }
    })
}

// ---------------------------------------------------------------------------
// Entries per tool
// ---------------------------------------------------------------------------

fn bearer(token: &Option<String>) -> Option<String> {
    token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| format!("Bearer {}", t))
}

/// Claude Code / Cursor (`mcpServers` style).
fn json_servers_entry(t: &McpTransport, cursor: bool) -> Value {
    match t {
        McpTransport::Stdio { command, args, env } => {
            let mut e = json!({"type": "stdio", "command": command, "args": args});
            if !env.is_empty() {
                e["env"] = json!(env);
            }
            e
        }
        McpTransport::Http { url, token } => {
            // Cursor infers remote servers from `url`; Claude Code needs `type: http`.
            let mut e = if cursor {
                json!({"url": url})
            } else {
                json!({"type": "http", "url": url})
            };
            if let Some(b) = bearer(token) {
                e["headers"] = json!({"Authorization": b});
            }
            e
        }
    }
}

fn opencode_entry(t: &McpTransport) -> Value {
    match t {
        McpTransport::Stdio { command, args, env } => {
            let mut cmd = vec![command.clone()];
            cmd.extend(args.iter().cloned());
            let mut e = json!({"type": "local", "command": cmd, "enabled": true});
            if !env.is_empty() {
                e["environment"] = json!(env);
            }
            e
        }
        McpTransport::Http { url, token } => {
            let mut e = json!({"type": "remote", "url": url, "enabled": true, "oauth": false});
            if let Some(b) = bearer(token) {
                e["headers"] = json!({"Authorization": b});
            }
            e
        }
    }
}

/// Codex `[mcp_servers.harness]` table as the JSON value keyed writes (`env` and
/// `http_headers` end up as inline tables).
fn codex_value(t: &McpTransport) -> Value {
    match t {
        McpTransport::Stdio { command, args, env } => {
            let mut e = json!({"command": command, "args": args});
            if !env.is_empty() {
                e["env"] = json!(env);
            }
            e
        }
        McpTransport::Http { url, token } => {
            let mut e = json!({"url": url});
            if let Some(b) = bearer(token) {
                e["http_headers"] = json!({"Authorization": b});
            }
            e
        }
    }
}

// ---------------------------------------------------------------------------
// Reading and rendering
// ---------------------------------------------------------------------------

/// The key under which a JSON config keeps its MCP servers.
fn json_key(id: &str) -> &'static str {
    if id == "opencode" {
        "mcp"
    } else {
        "mcpServers"
    }
}

fn read_toml(path: &Path) -> Result<Option<toml_edit::DocumentMut>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path).map_err(|e| {
        err("fs.read_failed")
            .with("path", path.display())
            .with("error", e)
    })?;
    raw.trim_start_matches('\u{feff}')
        .parse::<toml_edit::DocumentMut>()
        .map(Some)
        .map_err(|_| super::unparseable_msg(path))
}

/// The current `harness` entry of a tool (as JSON), if any.
fn current_entry(id: &str, path: &Path) -> Result<Option<Value>, String> {
    if id == "codex" {
        let Some(doc) = read_toml(path)? else {
            return Ok(None);
        };
        let entry = doc
            .get("mcp_servers")
            .and_then(|s| s.as_table_like())
            .and_then(|s| s.get(ENTRY));
        return Ok(entry.map(|item| {
            // Render the table as TOML text for display; parse back as JSON-ish via toml.
            let text = match item.as_table() {
                Some(t) => {
                    let mut d = toml_edit::DocumentMut::new();
                    for (k, v) in t.iter() {
                        d[k] = v.clone();
                    }
                    d.to_string()
                }
                None => item.to_string(),
            };
            toml::from_str::<toml::Value>(&text)
                .ok()
                .and_then(|v| serde_json::to_value(v).ok())
                .unwrap_or(Value::String(text))
        }));
    }
    let Some((root, _)) = read_config(path)? else {
        return Ok(None);
    };
    Ok(root.get(json_key(id)).and_then(|s| s.get(ENTRY)).cloned())
}

/// The key path of the `harness` entry in a tool's config.
fn entry_path(id: &str) -> Vec<keyed::Seg> {
    if id == "codex" {
        keyed::keys(&["mcp_servers", ENTRY])
    } else {
        keyed::keys(&[json_key(id), ENTRY])
    }
}

fn format_of(id: &str) -> keyed::Format {
    if id == "codex" {
        keyed::Format::Toml
    } else {
        keyed::Format::Json
    }
}

/// Ledger id of a tool's Harness MCP entry (`integrations.json`, next to the providers').
fn ledger_id(id: &str) -> String {
    format!("harness-mcp:{id}")
}

/// What Harness writes into a tool's config: the one `harness` entry.
fn plan(id: &str, path: &Path, t: &McpTransport) -> keyed::FilePlan {
    let value = match id {
        "codex" => codex_value(t),
        "opencode" => opencode_entry(t),
        "cursor" => json_servers_entry(t, true),
        _ => json_servers_entry(t, false),
    };
    keyed::FilePlan::Keys {
        path: path.to_path_buf(),
        format: format_of(id),
        edits: vec![keyed::Edit::marker(entry_path(id), value)],
        new_doc: (id == "opencode").then(|| json!({"$schema": "https://opencode.ai/config.json"})),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn list_targets_in(r: &McpRoots) -> Vec<McpTargetInfo> {
    TARGETS
        .iter()
        .map(|t| {
            let (config, marker) = match paths(r, t.id) {
                Ok(p) => (Some(p.0), Some(p.1)),
                Err(_) => (None, None),
            };
            let (entry, error) = match &config {
                Some(p) => match current_entry(t.id, p) {
                    Ok(e) => (e, None),
                    Err(e) => (None, Some(e)),
                },
                None => (None, None),
            };
            McpTargetInfo {
                id: t.id.to_string(),
                name: t.name.to_string(),
                docs_url: t.docs_url.to_string(),
                installed: marker.as_deref().is_some_and(Path::exists)
                    || config.as_deref().is_some_and(Path::exists),
                config_path: config.map(|p| p.to_string_lossy().to_string()),
                configured: entry.is_some(),
                entry,
                error,
            }
        })
        .collect()
}

fn validate(t: &McpTransport) -> Result<(), String> {
    match t {
        McpTransport::Stdio { command, .. } if command.trim().is_empty() => {
            Err(err("harness_mcp.command_empty").into())
        }
        McpTransport::Stdio { command, .. } => {
            super::cli_install::refuse_transient(Path::new(command.trim()))
        }
        McpTransport::Http { url, .. }
            if !(url.starts_with("http://") || url.starts_with("https://")) =>
        {
            Err(err("harness_mcp.url_invalid").with("url", url).into())
        }
        _ => Ok(()),
    }
}

pub fn preview_in(r: &McpRoots, id: &str, t: &McpTransport) -> Result<Vec<FileChange>, String> {
    target(id)?;
    validate(t)?;
    let (path, _) = paths(r, id)?;
    let ledger = keyed::Ledger::load(r.ledger.as_deref());
    let rendered = keyed::render(&ledger_id(id), &plan(id, &path, t), &ledger)?;
    Ok(vec![file_change(&path, rendered.after)])
}

pub fn apply_in(r: &McpRoots, id: &str, t: &McpTransport) -> Result<String, String> {
    let target = target(id)?;
    validate(t)?;
    let (path, _) = paths(r, id)?;
    let lid = ledger_id(id);
    let known = keyed::Ledger::load(r.ledger.as_deref())
        .entry(&lid)
        .is_some();
    keyed::apply(&lid, &[plan(id, &path, t)], r.ledger.as_deref())?;
    if !known {
        adopt_entry(r, &lid);
    }
    Ok(format!(
        "Harness MCP, {} yapılandırmasına eklendi ({}). Aracı yeniden başlatın.",
        target.name,
        path.display()
    ))
}

/// A `harness` entry that was already in the file before the first ledgered apply (written by
/// an older version) is Harness's own: removal deletes it instead of restoring it.
fn adopt_entry(r: &McpRoots, lid: &str) {
    let mut ledger = keyed::Ledger::load(r.ledger.as_deref());
    let Some(entry) = ledger.entries.iter_mut().find(|e| e.id == lid) else {
        return;
    };
    let mut changed = false;
    for k in entry
        .files
        .iter_mut()
        .flat_map(|f| f.keys.iter_mut())
        .filter(|k| k.marker && k.prior.is_some())
    {
        k.prior = None;
        k.created_from = Some(k.path.len() - 1);
        changed = true;
    }
    if changed {
        let _ = ledger.save(r.ledger.as_deref());
    }
}

/// Removes only the `harness` entry: what the ledger recorded is restored (a file Harness
/// created is deleted again), an entry without a ledger record is removed by name; a backup
/// is written first.
pub fn remove_in(r: &McpRoots, id: &str) -> Result<String, String> {
    let target = target(id)?;
    let (path, _) = paths(r, id)?;
    if current_entry(id, &path)?.is_none() {
        return Err(err("harness_mcp.not_configured")
            .with("tool", target.name)
            .into());
    }
    let lid = ledger_id(id);
    let markers = [(path.clone(), format_of(id), vec![entry_path(id)])];
    let first = keyed::revert(&lid, &markers, r.ledger.as_deref());
    // The entry was changed after Harness wrote it (the revert left it alone): it still is the
    // `harness` entry, so remove it by name (the ledger entry is gone by now).
    if current_entry(id, &path)?.is_some() {
        keyed::revert(&lid, &markers, r.ledger.as_deref())?;
    } else {
        first?;
    }
    scrub_backup_tokens(id, &path);
    Ok(format!(
        "Harness MCP, {} yapılandırmasından kaldırıldı.",
        target.name
    ))
}

/// What a removed token is replaced with in the backups.
const TOKEN_REMOVED: &str = "removed-by-harness";

/// The named token of an HTTP `harness` entry (`Authorization: Bearer <token>`).
fn entry_token(entry: &Value) -> Option<String> {
    ["headers", "http_headers"]
        .iter()
        .filter_map(|k| entry.get(*k))
        .filter_map(|h| h.get("Authorization").and_then(Value::as_str))
        .find_map(|a| a.strip_prefix("Bearer "))
        .map(str::trim)
        // Only Harness's own named tokens (`hs_<id>_<secret>`): a value the user typed into the
        // entry (say `test`) could also occur elsewhere in the backup.
        .filter(|t| t.starts_with("hs_") && t.len() > 3)
        .map(str::to_string)
}

/// After a removal the backups Harness took (`<file>.harness-<stamp>.bak`) would still hold the
/// named token of every HTTP `harness` entry they caught, in plain text. The tokens found in the
/// `harness` entry of those backups are replaced with [`TOKEN_REMOVED`] in all of them; nothing
/// else in a backup changes, and a backup without a `harness` entry (the file as it was before
/// Harness first wrote it) is not touched. Best effort: a backup that cannot be read or written
/// is skipped. Returns the number of backups rewritten.
fn scrub_backup_tokens(id: &str, path: &Path) -> usize {
    let backups = keyed::backups_of(path);
    let tokens: BTreeSet<String> = backups
        .iter()
        .filter_map(|b| current_entry(id, b).ok().flatten())
        .filter_map(|e| entry_token(&e))
        .collect();
    if tokens.is_empty() {
        return 0;
    }
    let mut rewritten = 0;
    for b in &backups {
        let Ok(text) = std::fs::read_to_string(b) else {
            continue;
        };
        let clean = tokens.iter().fold(text.clone(), |acc, t| {
            acc.replace(t.as_str(), TOKEN_REMOVED)
        });
        if clean == text {
            continue;
        }
        match crate::store::JsonStore::sensitive(b).write_bytes(clean.as_bytes()) {
            Ok(()) => rewritten += 1,
            Err(e) => eprintln!(
                "[Harness Studio] Yedekteki Harness MCP token'ı silinemedi ({}): {}",
                b.display(),
                e
            ),
        }
    }
    rewritten
}

/// Moves the HTTP `harness` entries Harness wrote for an earlier Control API port (`ports`:
/// every port it listened on) to `current`. Only the port of a loopback address changes; the
/// token and everything else in the file stay. Stdio entries, remote addresses and ports the
/// Control API never used are left alone. Returns the names of the tools that were rewritten.
pub fn follow_api_port_in(r: &McpRoots, current: u16, ports: &[u16]) -> Vec<String> {
    let mut moved = Vec::new();
    for t in &TARGETS {
        let Ok((path, _)) = paths(r, t.id) else {
            continue;
        };
        let Ok(Some(entry)) = current_entry(t.id, &path) else {
            continue;
        };
        let Some(url) = entry.get("url").and_then(Value::as_str) else {
            continue;
        };
        let stale = super::drift::address(url)
            .is_some_and(|(loopback, _, p)| loopback && p != current && ports.contains(&p));
        let Some(new_url) = stale
            .then(|| super::drift::with_port(url, current))
            .flatten()
        else {
            continue;
        };
        // Claude Code, Cursor and OpenCode keep the header under `headers`, Codex under
        // `http_headers`.
        let token = ["headers", "http_headers"]
            .iter()
            .filter_map(|k| entry.get(*k))
            .filter_map(|h| h.get("Authorization").and_then(Value::as_str))
            .find_map(|a| a.strip_prefix("Bearer "))
            .map(str::to_string);
        let transport = McpTransport::Http {
            url: new_url,
            token,
        };
        match apply_in(r, t.id, &transport) {
            Ok(_) => moved.push(t.name.to_string()),
            Err(e) => eprintln!(
                "[Harness Studio] Harness MCP ({}) yeni Control API adresine taşınamadı: {}",
                t.name, e
            ),
        }
    }
    moved
}

/// [`follow_api_port_in`] on the real user profile, with this instance's own port history
/// (`config/ports.json`) only. The default port is not added: an instance on a separate data
/// directory that never listened on 11500 must not move the entries of the one that does.
pub fn follow_api_port(current: u16, history: &[u16]) -> Vec<String> {
    follow_api_port_in(&McpRoots::from_env(), current, history)
}

pub fn list_targets() -> Vec<McpTargetInfo> {
    list_targets_in(&McpRoots::from_env())
}

pub fn preview(id: &str, t: &McpTransport) -> Result<Vec<FileChange>, String> {
    preview_in(&McpRoots::from_env(), id, t)
}

pub fn apply(id: &str, t: &McpTransport) -> Result<String, String> {
    apply_in(&McpRoots::from_env(), id, t)
}

pub fn remove(id: &str) -> Result<String, String> {
    remove_in(&McpRoots::from_env(), id)
}

/// The data directory `harness` uses without `--data-dir` (the GUI's: the platform data dir +
/// `com.harnessstudio.app`).
pub fn cli_default_data_dir() -> Option<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        env("APPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support"))
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local/share")))
    }?;
    Some(base.join("com.harnessstudio.app"))
}

/// The stdio entry for `harness mcp`: `--scope` widened with `models:write` when downloads and
/// deletions are allowed; `HARNESS_DATA_DIR` only when the GUI's data dir is not the CLI's
/// default (so the agent's `harness mcp` finds this instance).
pub fn stdio_transport(command: &str, allow_models_write: bool, data_dir: &Path) -> McpTransport {
    let mut args = vec!["mcp".to_string()];
    if allow_models_write {
        args.push("--scope".into());
        args.push("read,servers:write,tools,models:write".into());
    }
    let mut env = BTreeMap::new();
    let same = |a: &Path, b: &Path| {
        let norm = |p: &Path| {
            p.to_string_lossy()
                .replace('\\', "/")
                .trim_end_matches('/')
                .to_ascii_lowercase()
        };
        norm(a) == norm(b)
    };
    if !cli_default_data_dir().is_some_and(|d| same(&d, data_dir)) {
        env.insert(
            "HARNESS_DATA_DIR".to_string(),
            data_dir.to_string_lossy().to_string(),
        );
    }
    McpTransport::Stdio {
        command: command.to_string(),
        args,
        env,
    }
}

/// The `harness` CLI to put in stdio configs (see `cli_install::locate`: the bundled sidecar
/// next to the app, else `PATH`).
pub fn find_harness_cli() -> Option<PathBuf> {
    super::cli_install::locate(None).map(|l| l.path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(dir: &Path) -> McpRoots {
        McpRoots {
            home: Some(dir.to_path_buf()),
            claude_config_dir: None,
            codex_home: None,
            ledger: Some(dir.join("hs-data").join("integrations.json")),
        }
    }

    fn stdio() -> McpTransport {
        McpTransport::Stdio {
            command: r"C:\Harness\harness.exe".into(),
            args: vec!["mcp".into()],
            env: BTreeMap::new(),
        }
    }

    fn http() -> McpTransport {
        McpTransport::Http {
            url: "http://127.0.0.1:11500/api/harness/v1/mcp".into(),
            token: Some("hs_abc_secret".into()),
        }
    }

    /// Every backup of `path` with its text.
    fn backup_texts(path: &Path) -> Vec<String> {
        keyed::backups_of(path)
            .iter()
            .map(|b| std::fs::read_to_string(b).unwrap())
            .collect()
    }

    #[test]
    fn only_harness_tokens_are_scrubbed_from_backups() {
        let bearer = |t: &str| json!({"headers": {"Authorization": format!("Bearer {t}")}});
        assert_eq!(
            entry_token(&bearer("hs_ab12_cd34")).as_deref(),
            Some("hs_ab12_cd34")
        );
        assert_eq!(
            entry_token(&json!({"http_headers": {"Authorization": "Bearer hs_x_y"}})).as_deref(),
            Some("hs_x_y")
        );
        // A value the user typed could occur anywhere in the backup; it is left alone.
        assert_eq!(entry_token(&bearer("test")), None);
        assert_eq!(entry_token(&bearer("hs_")), None);
        assert_eq!(entry_token(&bearer(TOKEN_REMOVED)), None);
    }

    #[test]
    fn removal_clears_the_token_from_backups_and_keeps_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let token = |t: &str| McpTransport::Http {
            url: "http://127.0.0.1:11500/api/harness/v1/mcp".into(),
            token: Some(t.into()),
        };
        // Cursor (JSON) with a file the user wrote before Harness; Codex (TOML) created by
        // Harness; Claude Code with a token that was replaced by a second one.
        let cursor = dir.path().join(".cursor/mcp.json");
        std::fs::create_dir_all(cursor.parent().unwrap()).unwrap();
        let original = "{
    // mine
    \"mcpServers\": { \"other\": { \"url\": \"http://x\" } }
}
";
        std::fs::write(&cursor, original).unwrap();
        apply_in(&r, "cursor", &token("hs_cur_secret1")).unwrap();
        // A second write (port follow) so a backup holding the entry exists before removal.
        follow_api_port_in(&r, 11501, &[11500]);
        apply_in(&r, "codex", &token("hs_cdx_secret2")).unwrap();
        apply_in(&r, "codex", &stdio()).unwrap();
        apply_in(&r, "claude-code", &token("hs_old_secret3")).unwrap();
        apply_in(&r, "claude-code", &token("hs_new_secret4")).unwrap();

        let leaked = |texts: &[String], t: &str| texts.iter().any(|x| x.contains(t));
        assert!(leaked(&backup_texts(&cursor), "hs_cur_secret1"));

        remove_in(&r, "cursor").unwrap();
        remove_in(&r, "codex").unwrap();
        remove_in(&r, "claude-code").unwrap();

        // The file is back as the user wrote it.
        assert_eq!(std::fs::read_to_string(&cursor).unwrap(), original);
        let codex = dir.path().join(".codex/config.toml");
        let claude = dir.path().join(".claude.json");
        for (path, secrets) in [
            (&cursor, vec!["hs_cur_secret1"]),
            (&codex, vec!["hs_cdx_secret2"]),
            (&claude, vec!["hs_old_secret3", "hs_new_secret4"]),
        ] {
            let texts = backup_texts(path);
            assert!(!texts.is_empty(), "{}", path.display());
            for s in secrets {
                assert!(!leaked(&texts, s), "{} still holds {s}", path.display());
            }
        }
        // The backup of the user's own file (before Harness wrote) is byte-for-byte untouched,
        // and the scrubbed backups still parse.
        assert!(backup_texts(&cursor).iter().any(|t| t == original));
        for b in keyed::backups_of(&cursor) {
            assert!(read_config(&b).is_ok());
        }
        for b in keyed::backups_of(&codex) {
            assert!(read_toml(&b).is_ok());
        }
        assert!(backup_texts(&cursor)
            .iter()
            .any(|t| t.contains(&format!("Bearer {TOKEN_REMOVED}"))));
    }

    #[test]
    fn removal_of_a_stdio_entry_leaves_backups_alone() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let cursor = dir.path().join(".cursor/mcp.json");
        std::fs::create_dir_all(cursor.parent().unwrap()).unwrap();
        std::fs::write(
            &cursor,
            "{\"mcpServers\": {}}
",
        )
        .unwrap();
        apply_in(&r, "cursor", &stdio()).unwrap();
        apply_in(&r, "cursor", &stdio()).unwrap();
        let before = backup_texts(&cursor);
        remove_in(&r, "cursor").unwrap();
        let after = backup_texts(&cursor);
        // Removal adds its own backup; the earlier ones did not change.
        assert!(before.iter().all(|t| after.contains(t)));
        assert_eq!(scrub_backup_tokens("cursor", &cursor), 0);
    }

    fn read_json(p: &Path) -> Value {
        // Tolerant: the files keep their comments now.
        read_config(p).unwrap().unwrap().0
    }

    #[test]
    fn claude_code_keeps_everything_else_and_removes_only_its_entry() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let path = dir.path().join(".claude.json");
        std::fs::write(
            &path,
            r#"{"numStartups": 42, "projects": {"C:/x": {"mcpServers": {}}},
               "mcpServers": {"other": {"type": "stdio", "command": "npx", "args": ["x"]}}}"#,
        )
        .unwrap();
        let before = list_targets_in(&r);
        let cc = before.iter().find(|t| t.id == "claude-code").unwrap();
        assert!(cc.installed && !cc.configured);

        // Preview writes nothing.
        let changes = preview_in(&r, "claude-code", &stdio()).unwrap();
        assert_eq!(changes.len(), 1);
        assert!(changes[0].after.contains("\"harness\""));
        assert!(!std::fs::read_to_string(&path).unwrap().contains("harness"));

        apply_in(&r, "claude-code", &stdio()).unwrap();
        let v = read_json(&path);
        assert_eq!(v["numStartups"], 42);
        assert_eq!(v["mcpServers"]["other"]["command"], "npx");
        assert_eq!(
            v["mcpServers"]["harness"],
            json!({"type": "stdio", "command": r"C:\Harness\harness.exe", "args": ["mcp"]})
        );
        assert_eq!(keyed::backups_of(&path).len(), 1);
        // Idempotent; HTTP replaces it.
        apply_in(&r, "claude-code", &stdio()).unwrap();
        apply_in(&r, "claude-code", &http()).unwrap();
        let v = read_json(&path);
        assert_eq!(v["mcpServers"]["harness"]["type"], "http");
        assert_eq!(
            v["mcpServers"]["harness"]["headers"]["Authorization"],
            "Bearer hs_abc_secret"
        );
        let listed = list_targets_in(&r);
        let cc = listed.iter().find(|t| t.id == "claude-code").unwrap();
        assert!(cc.configured);
        assert_eq!(cc.entry.as_ref().unwrap()["type"], "http");

        remove_in(&r, "claude-code").unwrap();
        let v = read_json(&path);
        assert!(v["mcpServers"].get("harness").is_none());
        assert_eq!(v["mcpServers"]["other"]["command"], "npx");
        assert_eq!(v["numStartups"], 42);
        assert!(remove_in(&r, "claude-code").is_err());
    }

    #[test]
    fn api_port_follow_moves_only_http_entries_on_known_ports() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let ports = [11500, 11501];
        let url = |p: u16| format!("http://127.0.0.1:{p}/api/harness/v1/mcp");
        let http_at = |p: u16| McpTransport::Http {
            url: url(p),
            token: Some("hs_abc_secret".into()),
        };

        // Claude Code (JSON), Codex (TOML) and OpenCode (JSON) hold the old fallback port,
        // Cursor a stdio entry.
        apply_in(&r, "claude-code", &http_at(11501)).unwrap();
        apply_in(&r, "codex", &http_at(11500)).unwrap();
        apply_in(&r, "opencode", &http_at(11501)).unwrap();
        apply_in(&r, "cursor", &stdio()).unwrap();
        let cursor_before = std::fs::read_to_string(dir.path().join(".cursor/mcp.json")).unwrap();

        let moved = follow_api_port_in(&r, 11502, &ports);
        assert_eq!(moved, ["Claude Code", "Codex", "OpenCode"]);
        let v = read_json(&dir.path().join(".claude.json"));
        assert_eq!(v["mcpServers"]["harness"]["url"], url(11502));
        assert_eq!(
            v["mcpServers"]["harness"]["headers"]["Authorization"],
            "Bearer hs_abc_secret"
        );
        let toml = std::fs::read_to_string(dir.path().join(".codex/config.toml")).unwrap();
        assert!(toml.contains(&url(11502)) && toml.contains("Bearer hs_abc_secret"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".cursor/mcp.json")).unwrap(),
            cursor_before
        );
        // Already current: nothing more to do.
        assert!(follow_api_port_in(&r, 11502, &ports).is_empty());

        // Back on the default port.
        let moved = follow_api_port_in(&r, 11500, &[11500, 11501, 11502]);
        assert_eq!(moved.len(), 3);
        assert_eq!(
            read_json(&dir.path().join(".claude.json"))["mcpServers"]["harness"]["url"],
            url(11500)
        );

        // A port the Control API never used, or a remote host, is somebody else's.
        apply_in(&r, "claude-code", &http_at(9999)).unwrap();
        apply_in(
            &r,
            "opencode",
            &McpTransport::Http {
                url: "http://10.0.0.5:11501/api/harness/v1/mcp".into(),
                token: None,
            },
        )
        .unwrap();
        assert_eq!(follow_api_port_in(&r, 11502, &ports), ["Codex"]);
        assert_eq!(
            read_json(&dir.path().join(".claude.json"))["mcpServers"]["harness"]["url"],
            url(9999)
        );
    }

    #[test]
    fn a_port_this_control_api_never_used_is_not_followed() {
        // A second instance on its own data directory listens on 11501 and has never been on
        // 11500: the user's entry for the main instance's 11500 stays.
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let url = "http://127.0.0.1:11500/api/harness/v1/mcp";
        apply_in(
            &r,
            "cursor",
            &McpTransport::Http {
                url: url.into(),
                token: Some("hs_abc_secret".into()),
            },
        )
        .unwrap();
        let path = dir.path().join(".cursor/mcp.json");
        let before = std::fs::read_to_string(&path).unwrap();
        assert!(follow_api_port_in(&r, 11501, &[11501]).is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        assert!(keyed::backups_of(&path).is_empty());
    }

    #[test]
    fn stdio_transport_scopes_and_data_dir() {
        let default = cli_default_data_dir().unwrap();
        match stdio_transport("h", false, &default) {
            McpTransport::Stdio { args, env, .. } => {
                assert_eq!(args, vec!["mcp"]);
                assert!(env.is_empty());
            }
            _ => unreachable!(),
        }
        let other = std::env::temp_dir().join("hs-other");
        match stdio_transport("h", true, &other) {
            McpTransport::Stdio { args, env, .. } => {
                assert_eq!(
                    args,
                    vec!["mcp", "--scope", "read,servers:write,tools,models:write"]
                );
                assert_eq!(env["HARNESS_DATA_DIR"], other.to_string_lossy());
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn claude_config_dir_is_respected() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("claude-cfg");
        let r = McpRoots {
            home: Some(dir.path().to_path_buf()),
            claude_config_dir: Some(cfg.clone()),
            codex_home: None,
            ledger: None,
        };
        apply_in(&r, "claude-code", &stdio()).unwrap();
        assert!(cfg.join(".claude.json").is_file());
        assert!(!dir.path().join(".claude.json").exists());
    }

    #[test]
    fn codex_toml_keeps_comments_and_other_servers() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let path = dir.path().join(".codex").join("config.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "# my settings\nmodel = \"gpt-5\"\n\n[mcp_servers.docs]\ncommand = \"npx\"\nargs = [\"-y\", \"docs\"] # keep\n",
        )
        .unwrap();
        apply_in(&r, "codex", &stdio()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# my settings"));
        assert!(text.contains("# keep"));
        let v: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(v["model"].as_str(), Some("gpt-5"));
        assert_eq!(
            v["mcp_servers"]["harness"]["command"].as_str(),
            Some(r"C:\Harness\harness.exe")
        );
        assert_eq!(
            v["mcp_servers"]["harness"]["args"].as_array().unwrap()[0].as_str(),
            Some("mcp")
        );
        assert_eq!(v["mcp_servers"]["docs"]["command"].as_str(), Some("npx"));

        apply_in(&r, "codex", &http()).unwrap();
        let v: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            v["mcp_servers"]["harness"]["url"].as_str(),
            Some("http://127.0.0.1:11500/api/harness/v1/mcp")
        );
        assert_eq!(
            v["mcp_servers"]["harness"]["http_headers"]["Authorization"].as_str(),
            Some("Bearer hs_abc_secret")
        );
        assert!(v["mcp_servers"]["harness"].get("command").is_none());
        let listed = list_targets_in(&r);
        let cx = listed.iter().find(|t| t.id == "codex").unwrap();
        assert!(cx.configured);
        assert_eq!(
            cx.entry.as_ref().unwrap()["url"],
            "http://127.0.0.1:11500/api/harness/v1/mcp"
        );

        remove_in(&r, "codex").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("harness"));
        assert!(text.contains("# keep") && text.contains("[mcp_servers.docs]"));
    }

    #[test]
    fn codex_home_and_fresh_file() {
        let dir = tempfile::tempdir().unwrap();
        let r = McpRoots {
            home: Some(dir.path().to_path_buf()),
            claude_config_dir: None,
            codex_home: Some(dir.path().join("ch")),
            ledger: None,
        };
        apply_in(&r, "codex", &stdio()).unwrap();
        let text = std::fs::read_to_string(dir.path().join("ch").join("config.toml")).unwrap();
        assert!(text.contains("[mcp_servers.harness]"), "{text}");
    }

    #[test]
    fn opencode_and_cursor_shapes() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let oc = dir.path().join(".config/opencode/opencode.json");
        std::fs::create_dir_all(oc.parent().unwrap()).unwrap();
        std::fs::write(
            &oc,
            "{\n  // comment\n  \"provider\": {\"harness\": {\"npm\": \"x\"}},\n  \"mcp\": {\"gh\": {\"type\": \"remote\", \"url\": \"https://x\"},},\n}",
        )
        .unwrap();
        apply_in(&r, "opencode", &stdio()).unwrap();
        let v = read_json(&oc);
        assert_eq!(
            v["mcp"]["harness"],
            json!({"type": "local", "command": [r"C:\Harness\harness.exe", "mcp"], "enabled": true})
        );
        assert_eq!(v["mcp"]["gh"]["url"], "https://x");
        assert_eq!(v["provider"]["harness"]["npm"], "x");
        apply_in(&r, "opencode", &http()).unwrap();
        let v = read_json(&oc);
        assert_eq!(v["mcp"]["harness"]["type"], "remote");
        assert_eq!(v["mcp"]["harness"]["oauth"], false);
        remove_in(&r, "opencode").unwrap();
        let v = read_json(&oc);
        assert!(v["mcp"].get("harness").is_none());
        // The provider integration's entry is a different key and stays.
        assert_eq!(v["provider"]["harness"]["npm"], "x");

        apply_in(&r, "cursor", &http()).unwrap();
        let v = read_json(&dir.path().join(".cursor/mcp.json"));
        assert_eq!(
            v["mcpServers"]["harness"],
            json!({"url": "http://127.0.0.1:11500/api/harness/v1/mcp",
                   "headers": {"Authorization": "Bearer hs_abc_secret"}})
        );
        apply_in(&r, "cursor", &stdio()).unwrap();
        let v = read_json(&dir.path().join(".cursor/mcp.json"));
        assert_eq!(v["mcpServers"]["harness"]["type"], "stdio");
    }

    #[test]
    fn broken_configs_are_never_touched() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let cursor = dir.path().join(".cursor/mcp.json");
        std::fs::create_dir_all(cursor.parent().unwrap()).unwrap();
        std::fs::write(&cursor, "{ not json").unwrap();
        assert!(apply_in(&r, "cursor", &stdio()).is_err());
        assert_eq!(std::fs::read_to_string(&cursor).unwrap(), "{ not json");
        let listed = list_targets_in(&r);
        assert!(listed
            .iter()
            .find(|t| t.id == "cursor")
            .unwrap()
            .error
            .is_some());
        let codex = dir.path().join(".codex/config.toml");
        std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
        std::fs::write(&codex, "[[[").unwrap();
        assert!(apply_in(&r, "codex", &stdio()).is_err());
        assert_eq!(std::fs::read_to_string(&codex).unwrap(), "[[[");
        // mcpServers of the wrong type.
        std::fs::write(&cursor, r#"{"mcpServers": []}"#).unwrap();
        assert!(apply_in(&r, "cursor", &stdio()).is_err());
        assert!(apply_in(&r, "nope", &stdio()).is_err());
        assert!(apply_in(
            &r,
            "cursor",
            &McpTransport::Http {
                url: "ftp://x".into(),
                token: None
            }
        )
        .is_err());
    }

    #[test]
    fn remove_deletes_a_file_apply_created() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let path = dir.path().join(".cursor/mcp.json");
        assert!(!path.exists());
        apply_in(&r, "cursor", &stdio()).unwrap();
        assert!(path.is_file());
        remove_in(&r, "cursor").unwrap();
        assert!(!path.exists());
        // The ledger forgot the entry too.
        assert!(keyed::Ledger::load(r.ledger.as_deref())
            .entry("harness-mcp:cursor")
            .is_none());
        assert!(remove_in(&r, "cursor").is_err());
    }

    #[test]
    fn jsonc_with_comments_comes_back_byte_for_byte() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let path = dir.path().join(".cursor/mcp.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = "{\n    // my servers\n    \"mcpServers\": {\n        \"docs\": {\n            \"command\": \"npx\", // keep\n            \"args\": [\"-y\", \"docs\"]\n        }\n    },\n    /* other */\n    \"theme\": \"dark\"\n}\n";
        std::fs::write(&path, original).unwrap();

        let preview = preview_in(&r, "cursor", &http()).unwrap();
        apply_in(&r, "cursor", &http()).unwrap();
        let applied = std::fs::read_to_string(&path).unwrap();
        // The preview is exactly what apply wrote; comments and indentation survive.
        assert_eq!(preview[0].after, applied);
        assert!(applied.contains("// my servers") && applied.contains("// keep"));
        assert!(applied.contains("/* other */") && applied.contains("\"harness\""));
        // A second apply (new transport) still keeps the first prior.
        apply_in(&r, "cursor", &stdio()).unwrap();
        remove_in(&r, "cursor").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn codex_toml_with_comments_comes_back_byte_for_byte() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let path = dir.path().join(".codex/config.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = "# my settings\nmodel = \"gpt-5\"\n\n[mcp_servers.docs]\ncommand = \"npx\" # keep\nargs = [\"-y\", \"docs\"]\n";
        std::fs::write(&path, original).unwrap();
        let stdio_env = McpTransport::Stdio {
            command: "h".into(),
            args: vec!["mcp".into()],
            env: BTreeMap::from([("HARNESS_DATA_DIR".to_string(), "/d".to_string())]),
        };
        apply_in(&r, "codex", &stdio_env).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# my settings") && text.contains("# keep"));
        let v: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(
            v["mcp_servers"]["harness"]["env"]["HARNESS_DATA_DIR"].as_str(),
            Some("/d")
        );
        apply_in(&r, "codex", &http()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[mcp_servers.harness]"), "{text}");
        remove_in(&r, "codex").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

        // A new file gets `[mcp_servers.harness]` (no empty `[mcp_servers]`) and is deleted again.
        let fresh = tempfile::tempdir().unwrap();
        let r2 = roots(fresh.path());
        apply_in(&r2, "codex", &stdio()).unwrap();
        let path2 = fresh.path().join(".codex/config.toml");
        let text = std::fs::read_to_string(&path2).unwrap();
        assert!(text.starts_with("[mcp_servers.harness]"), "{text}");
        remove_in(&r2, "codex").unwrap();
        assert!(!path2.exists());
    }

    #[test]
    fn an_entry_without_a_ledger_record_is_removed_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let path = dir.path().join(".cursor/mcp.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"mcpServers": {"harness": {"url": "http://x"}, "other": {"command": "npx"}}}"#,
        )
        .unwrap();
        remove_in(&r, "cursor").unwrap();
        let v = read_json(&path);
        assert!(v["mcpServers"].get("harness").is_none());
        assert_eq!(v["mcpServers"]["other"]["command"], "npx");

        // An older entry that is re-applied (no ledger yet) is still removed, not "restored".
        std::fs::write(&path, r#"{"mcpServers": {"harness": {"url": "http://x"}}}"#).unwrap();
        apply_in(&r, "cursor", &stdio()).unwrap();
        remove_in(&r, "cursor").unwrap();
        assert!(read_json(&path)["mcpServers"].get("harness").is_none());
    }

    #[test]
    fn an_entry_the_user_edited_after_apply_is_still_removed() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let path = dir.path().join(".cursor/mcp.json");
        apply_in(&r, "cursor", &stdio()).unwrap();
        let edited = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\"mcp\"", "\"mcp\", \"--x\"");
        std::fs::write(&path, edited).unwrap();
        remove_in(&r, "cursor").unwrap();
        assert!(read_json(&path)["mcpServers"].get("harness").is_none());
    }

    #[test]
    fn following_the_api_port_keeps_the_original_restorable() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let path = dir.path().join(".cursor/mcp.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = "{\n    // keep me\n    \"mcpServers\": {\n        \"docs\": {\"command\": \"npx\"}\n    }\n}\n";
        std::fs::write(&path, original).unwrap();
        let at = |p: u16| McpTransport::Http {
            url: format!("http://127.0.0.1:{p}/api/harness/v1/mcp"),
            token: Some("hs_abc_secret".into()),
        };
        apply_in(&r, "cursor", &at(11501)).unwrap();
        assert_eq!(follow_api_port_in(&r, 11502, &[11501]), ["Cursor"]);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(":11502/") && text.contains("// keep me"));
        remove_in(&r, "cursor").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }
}
