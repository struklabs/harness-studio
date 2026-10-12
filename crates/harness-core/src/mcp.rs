//! MCP server definitions for llama-server's built-in MCP client
//! (`--mcp-servers-config`, Cursor-compatible `{"mcpServers": {...}}` format).
//! The server exposes the discovered tools at `GET /tools` and runs them via
//! `POST /tools {"tool", "params"}`; the Playground drives the tool-call loop.

use crate::error_key::err;
use crate::{paths, secrets::SecretStore};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Value returned over IPC for a secret which is present in the OS keychain.
pub const SECRET_MARKER: &str = "••••••••";
/// Value persisted in mcp_servers.json. It is deliberately different from the UI marker so an
/// empty input can unambiguously mean "delete" and legacy plaintext can be migrated.
const DISK_PLACEHOLDER: &str = "__HARNESS_STUDIO_KEYCHAIN__";

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct McpServerDef {
    /// stdio servers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// HTTP servers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// Kept in the list but not passed to llama-server.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    /// HTTP servers: `"http"` (Streamable HTTP), `"sse"` (the older HTTP+SSE transport of the
    /// 2024-11-05 revision) or absent/`"auto"` (Streamable HTTP first, HTTP+SSE when the server
    /// answers the first request with a 4xx).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
}

/// The values of [`McpServerDef::transport`].
pub const TRANSPORTS: [&str; 3] = ["auto", "http", "sse"];

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct McpConfig {
    #[serde(rename = "mcpServers", default)]
    pub mcp_servers: BTreeMap<String, McpServerDef>,
}

/// `mcp_servers.json`: Claude Desktop's `{"mcpServers": {...}}` plus `schema_version`.
/// (The per-server runtime copies handed to llama-server stay unversioned.)
pub const SCHEMA: crate::store::Schema = crate::store::Schema {
    file: "mcp_servers.json",
    current: 1,
    envelope: crate::store::Envelope::Merged,
    migrations: &[],
};

fn config_file() -> PathBuf {
    paths::config_dir().join("mcp_servers.json")
}

/// Launch copy of the config for one server instance (it carries the same secrets as the
/// main file, so it only exists while that server runs).
fn runtime_file(server_id: &str) -> PathBuf {
    paths::config_dir().join(format!("mcp_servers.runtime.{}.json", server_id))
}

/// Deletes the runtime copy of a server instance (called when it stops).
pub fn remove_runtime_config(server_id: &str) {
    let _ = fs::remove_file(runtime_file(server_id));
}

/// Deletes every runtime copy (app start after a crash, app exit).
pub fn remove_all_runtime_configs() {
    let Ok(entries) = fs::read_dir(paths::config_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("mcp_servers.runtime.") && name.ends_with(".json") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Writes a file readable by the owner only (0600 on Unix; on Windows the file inherits the
/// ACL of the per-user app data directory).
fn write_private(path: &Path, data: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        // `mode` only applies to newly created files.
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(data.as_bytes())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, data)
    }
}

fn read_file(path: &Path) -> McpConfig {
    crate::store::JsonStore::sensitive(path.to_path_buf())
        .load_versioned::<McpConfig>(&SCHEMA)
        .ok()
        .flatten()
        .map(|v| v.data)
        .unwrap_or_default()
}

fn secret_account(server: &str, kind: &str, key: &str) -> String {
    crate::secrets::mcp_account(server, kind, key)
}

fn secret_get(
    store: &dyn SecretStore,
    server: &str,
    kind: &str,
    key: &str,
) -> Result<Option<String>, String> {
    store.get(&secret_account(server, kind, key))
}

fn rewrite(path: &Path, config: &McpConfig) -> Result<(), String> {
    crate::store::JsonStore::sensitive(path.to_path_buf()).save_versioned(&SCHEMA, config)
}

/// Moves legacy plaintext values into the keychain and rewrites the file with placeholders.
fn migrate(path: &Path, store: &dyn SecretStore) -> Result<McpConfig, String> {
    let mut config = read_file(path);
    let mut changed = false;
    for (server, def) in &mut config.mcp_servers {
        for (kind, values) in [("env", &mut def.env), ("header", &mut def.headers)] {
            for (key, value) in values.iter_mut() {
                if value != DISK_PLACEHOLDER {
                    if !value.is_empty() {
                        store
                            .set(&secret_account(server, kind, key), value)
                            .map_err(|e| err("mcp.secret_migrate_failed").with("error", e))?;
                    }
                    *value = DISK_PLACEHOLDER.to_string();
                    changed = true;
                }
            }
        }
    }
    if changed {
        rewrite(path, &config)?;
    }
    Ok(config)
}

fn mask_config(mut config: McpConfig, store: &dyn SecretStore) -> Result<McpConfig, String> {
    for (server, def) in &mut config.mcp_servers {
        for (kind, values) in [("env", &mut def.env), ("header", &mut def.headers)] {
            for (key, value) in values.iter_mut() {
                *value = if secret_get(store, server, kind, key)?.is_some() {
                    SECRET_MARKER.to_string()
                } else {
                    String::new()
                };
            }
        }
    }
    Ok(config)
}

fn load_with(path: &Path, store: &dyn SecretStore) -> Result<McpConfig, String> {
    mask_config(migrate(path, store)?, store)
}

pub fn load() -> Result<McpConfig, String> {
    load_with(&config_file(), crate::secrets::store())
}

pub fn validate(config: &McpConfig) -> Result<(), String> {
    for (name, def) in &config.mcp_servers {
        let name_ok = !name.is_empty()
            && name.len() <= 48
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !name_ok {
            return Err(err("mcp.invalid_server_name").with("name", name).into());
        }
        match (&def.command, &def.url) {
            (Some(c), None) if !c.trim().is_empty() => {}
            (None, Some(u)) if u.starts_with("http://") || u.starts_with("https://") => {}
            _ => return Err(err("mcp.command_or_url").with("name", name).into()),
        }
        if let Some(t) = &def.transport {
            if !TRANSPORTS.contains(&t.as_str()) {
                return Err(err("mcp.invalid_transport")
                    .with("name", name)
                    .with("transport", t)
                    .into());
            }
        }
    }
    Ok(())
}

fn oauth_account(server: &str) -> String {
    crate::secrets::mcp_oauth_account(server)
}

fn same_server_shape(a: &McpServerDef, b: &McpServerDef) -> bool {
    a.command == b.command && a.args == b.args && a.url == b.url
}

#[derive(Clone)]
struct PendingImport {
    name: String,
    shape: McpServerDef,
    env: BTreeMap<String, String>,
    headers: BTreeMap<String, String>,
}

fn pending_imports() -> &'static Mutex<Vec<PendingImport>> {
    static PENDING: OnceLock<Mutex<Vec<PendingImport>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(Vec::new()))
}

fn pending_value(server: &str, def: &McpServerDef, kind: &str, key: &str) -> Option<String> {
    let pending = pending_imports().lock().ok()?;
    let mut matches = pending.iter().filter(|p| {
        (server == p.name
            || server.strip_prefix(&p.name).is_some_and(|s| {
                s.strip_prefix('-')
                    .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
            }))
            && same_server_shape(def, &p.shape)
    });
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    match kind {
        "env" => first.env.get(key).cloned(),
        "header" => first.headers.get(key).cloned(),
        _ => None,
    }
}

fn save_with(path: &Path, store: &dyn SecretStore, config: &McpConfig) -> Result<(), String> {
    validate(config)?;
    let old = migrate(path, store)?;
    let removed: Vec<String> = old
        .mcp_servers
        .keys()
        .filter(|name| !config.mcp_servers.contains_key(*name))
        .cloned()
        .collect();
    let added: Vec<String> = config
        .mcp_servers
        .keys()
        .filter(|name| !old.mcp_servers.contains_key(*name))
        .cloned()
        .collect();
    let mut renames = BTreeMap::<String, String>::new();
    if removed.len() == 1 && added.len() == 1 {
        renames.insert(added[0].clone(), removed[0].clone());
    } else {
        for new_name in &added {
            let def = &config.mcp_servers[new_name];
            let candidates: Vec<_> = removed
                .iter()
                .filter(|old_name| same_server_shape(&old.mcp_servers[*old_name], def))
                .collect();
            if candidates.len() == 1 {
                renames.insert(new_name.clone(), candidates[0].to_string());
            }
        }
    }

    let mut persisted = config.clone();
    for (server, def) in &mut persisted.mcp_servers {
        let input_def = &config.mcp_servers[server];
        for (kind, values) in [("env", &mut def.env), ("header", &mut def.headers)] {
            let input_values = if kind == "env" {
                &input_def.env
            } else {
                &input_def.headers
            };
            values.retain(|_, value| !value.is_empty());
            for (key, value) in values.iter_mut() {
                let account = secret_account(server, kind, key);
                if value == SECRET_MARKER || value == DISK_PLACEHOLDER {
                    if store.get(&account)?.is_none() {
                        let inherited = renames
                            .get(server)
                            .and_then(|old_name| {
                                secret_get(store, old_name, kind, key).ok().flatten()
                            })
                            .or_else(|| pending_value(server, input_def, kind, key));
                        let inherited = inherited.ok_or_else(|| {
                            err("mcp.secret_missing")
                                .with("secret", format!("{server}:{kind}:{key}"))
                        })?;
                        store.set(&account, &inherited)?;
                    }
                } else {
                    store
                        .set(&account, value)
                        .map_err(|e| err("mcp.secret_save_failed").with("error", e))?;
                }
                *value = DISK_PLACEHOLDER.to_string();
            }
            // A key omitted from the incoming map (or sent empty and retained above) is deleted.
            let old_values =
                old.mcp_servers
                    .get(server)
                    .map(|d| if kind == "env" { &d.env } else { &d.headers });
            if let Some(old_values) = old_values {
                for key in old_values
                    .keys()
                    .filter(|k| !input_values.contains_key(*k) || input_values[*k].is_empty())
                {
                    store.delete(&secret_account(server, kind, key))?;
                }
            }
        }
    }

    rewrite(path, &persisted)?;
    // A renamed server keeps its OAuth sign-in; a removed one loses it.
    for (new_name, old_name) in &renames {
        if let Ok(Some(session)) = store.get(&oauth_account(old_name)) {
            store.set(&oauth_account(new_name), &session)?;
        }
    }
    for old_name in removed {
        store.delete(&oauth_account(&old_name))?;
        if let Some(def) = old.mcp_servers.get(&old_name) {
            for key in def.env.keys() {
                store.delete(&secret_account(&old_name, "env", key))?;
            }
            for key in def.headers.keys() {
                store.delete(&secret_account(&old_name, "header", key))?;
            }
        }
    }
    if let Ok(mut pending) = pending_imports().lock() {
        pending.clear();
    }
    crate::events::publish(crate::events::CoreEvent::ToolsChanged);
    Ok(())
}

pub fn save(config: &McpConfig) -> Result<(), String> {
    save_with(&config_file(), crate::secrets::store(), config)
}

/// Windows can't spawn `npx`/`uvx`-style `.cmd` shims directly; wrap them in `cmd /c`.
pub(crate) fn windows_wrap(def: &McpServerDef) -> McpServerDef {
    let mut def = def.clone();
    let Some(cmd) = def.command.clone() else {
        return def;
    };
    if !cfg!(windows) || Path::new(&cmd).extension().is_some() {
        return def;
    }
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let dirs: Vec<PathBuf> = std::env::split_paths(&path_var).collect();
    let exists = |ext: &str| {
        dirs.iter()
            .any(|d| d.join(format!("{}.{}", cmd, ext)).is_file())
    };
    if !exists("exe") && (exists("cmd") || exists("bat")) {
        let mut args = vec!["/c".to_string(), cmd];
        args.append(&mut def.args);
        def.command = Some("cmd".to_string());
        def.args = args;
    }
    def
}

/// Writes the launch-ready config (enabled servers only, Windows-wrapped) and returns
/// its path, or `None` if no server is enabled.
pub fn runtime_config(server_id: &str) -> Result<Option<PathBuf>, String> {
    let config = runtime_config_data(&config_file(), crate::secrets::store())?;
    let servers: BTreeMap<String, McpServerDef> = config
        .mcp_servers
        .iter()
        .filter(|(_, d)| !d.disabled)
        .map(|(n, d)| {
            let mut d = windows_wrap(d);
            d.disabled = false;
            // llama-server's own client knows nothing of Harness's transport choice.
            d.transport = None;
            (n.clone(), d)
        })
        .collect();
    if servers.is_empty() {
        return Ok(None);
    }
    let path = runtime_file(server_id);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| {
            err("fs.create_dir_failed")
                .with("path", dir.display())
                .with("error", e)
        })?;
    }
    let json = serde_json::to_string_pretty(&McpConfig {
        mcp_servers: servers,
    })
    .map_err(|e| err("json.serialize_failed").with("error", e))?;
    write_private(&path, &json).map_err(|e| {
        err("fs.write_failed")
            .with("path", path.display())
            .with("error", e)
    })?;
    Ok(Some(path))
}

fn runtime_config_data(path: &Path, store: &dyn SecretStore) -> Result<McpConfig, String> {
    let mut config = migrate(path, store)?;
    for (server, def) in &mut config.mcp_servers {
        for (kind, values) in [("env", &mut def.env), ("header", &mut def.headers)] {
            for (key, value) in values.iter_mut() {
                *value = secret_get(store, server, kind, key)?.ok_or_else(|| {
                    err("mcp.secret_missing").with("secret", format!("{server}:{kind}:{key}"))
                })?;
            }
        }
    }
    Ok(config)
}

/// The saved servers with their secrets read from the keychain: what Harness's own MCP client
/// connects with (never handed to the UI).
pub fn load_resolved() -> Result<McpConfig, String> {
    runtime_config_data(&config_file(), crate::secrets::store())
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Known config locations of other MCP clients.
pub fn import_source_path(source: &str) -> Option<PathBuf> {
    let home = home()?;
    Some(match source {
        "claude" => {
            if cfg!(windows) {
                PathBuf::from(std::env::var_os("APPDATA")?)
                    .join("Claude")
                    .join("claude_desktop_config.json")
            } else if cfg!(target_os = "macos") {
                home.join("Library/Application Support/Claude/claude_desktop_config.json")
            } else {
                home.join(".config/Claude/claude_desktop_config.json")
            }
        }
        "cursor" => home.join(".cursor").join("mcp.json"),
        "lmstudio" => home.join(".lmstudio").join("mcp.json"),
        // Claude Code keeps user-scope servers (and the list of its projects) here.
        "claude-code" => home.join(".claude.json"),
        "codex" => codex_home(&home).join("config.toml"),
        _ => return None,
    })
}

/// `CODEX_HOME`, else `~/.codex`.
fn codex_home(home: &Path) -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| home.join(".codex"))
}

/// A server definition from a client's JSON entry (extra keys such as `type` are ignored).
fn def_from_json(v: &serde_json::Value) -> Option<McpServerDef> {
    serde_json::from_value::<McpServerDef>(v.clone()).ok()
}

fn servers_of(value: &serde_json::Value) -> BTreeMap<String, McpServerDef> {
    let mut out = BTreeMap::new();
    if let Some(map) = value.get("mcpServers").and_then(|v| v.as_object()) {
        for (name, def) in map {
            let Some(mut parsed) = def_from_json(def) else {
                continue;
            };
            // The older HTTP+SSE transport is marked `"type": "sse"` by other clients.
            if parsed.url.is_some() && def.get("type").and_then(|t| t.as_str()) == Some("sse") {
                parsed.transport = Some("sse".into());
            }
            out.insert(name.clone(), parsed);
        }
    }
    out
}

fn read_json_file(path: &Path) -> Result<serde_json::Value, String> {
    let text = fs::read_to_string(path)
        .map_err(|_| err("mcp.import_config_not_found").with("path", path.display()))?;
    serde_json::from_str(&text).map_err(|e| {
        err("json.parse_failed")
            .with("path", path.display())
            .with("error", e)
            .into()
    })
}

/// Claude Code's servers: `mcpServers` of `~/.claude.json` (user scope), the `mcpServers` of each
/// project in its `projects` table (local scope) and the `.mcp.json` of those project
/// directories (project scope). A project's server whose name is taken by a different
/// definition gets the project folder's name as a suffix. Read-only.
fn claude_code_servers(file: &Path) -> Result<BTreeMap<String, McpServerDef>, String> {
    let root = read_json_file(file)?;
    let mut out = servers_of(&root);
    let mut add = |name: &str, def: McpServerDef, project: &str| {
        match out.get(name) {
            Some(existing) if *existing == def => {}
            Some(_) => {
                let folder: String = Path::new(project)
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default();
                out.insert(format!("{name}-{folder}"), def);
            }
            None => {
                out.insert(name.to_string(), def);
            }
        };
    };
    if let Some(projects) = root.get("projects").and_then(|p| p.as_object()) {
        for (dir, p) in projects {
            for (n, d) in servers_of(p) {
                add(&n, d, dir);
            }
            if let Ok(v) = read_json_file(&Path::new(dir).join(".mcp.json")) {
                for (n, d) in servers_of(&v) {
                    add(&n, d, dir);
                }
            }
        }
    }
    Ok(out)
}

/// Codex's servers: the `[mcp_servers.<name>]` tables of `config.toml`. `bearer_token_env_var`
/// becomes an `Authorization: Bearer` header from that variable's current value (skipped when it
/// is not set), `http_headers` are copied, `enabled = false` becomes `disabled`. Read-only.
fn codex_servers(file: &Path) -> Result<BTreeMap<String, McpServerDef>, String> {
    let text = fs::read_to_string(file)
        .map_err(|_| err("mcp.import_config_not_found").with("path", file.display()))?;
    let root: toml::Table = text.parse().map_err(|e| {
        err("json.parse_failed")
            .with("path", file.display())
            .with("error", e)
    })?;
    let strings = |v: Option<&toml::Value>| -> BTreeMap<String, String> {
        v.and_then(|v| v.as_table())
            .map(|t| {
                t.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut out = BTreeMap::new();
    let Some(servers) = root.get("mcp_servers").and_then(|v| v.as_table()) else {
        return Ok(out);
    };
    for (name, v) in servers {
        let Some(t) = v.as_table() else { continue };
        let mut def = McpServerDef {
            command: t
                .get("command")
                .and_then(|c| c.as_str())
                .map(str::to_string),
            url: t.get("url").and_then(|c| c.as_str()).map(str::to_string),
            args: t
                .get("args")
                .and_then(|a| a.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            env: strings(t.get("env")),
            headers: strings(t.get("http_headers")),
            disabled: t.get("enabled").and_then(|e| e.as_bool()) == Some(false),
            transport: None,
        };
        if let Some(var) = t.get("bearer_token_env_var").and_then(|c| c.as_str()) {
            if let Some(token) = std::env::var(var).ok().filter(|t| !t.is_empty()) {
                def.headers
                    .insert("Authorization".into(), format!("Bearer {token}"));
            }
        }
        if def.command.is_some() || def.url.is_some() {
            out.insert(name.clone(), def);
        }
    }
    Ok(out)
}

/// Reads another client's MCP servers (not saved — the UI merges and saves).
pub fn read_import_source(source: &str) -> Result<McpConfig, String> {
    let path = import_source_path(source)
        .ok_or_else(|| err("mcp.unknown_import_source").with("source", source))?;
    let found = match source {
        "claude-code" => claude_code_servers(&path)?,
        "codex" => codex_servers(&path)?,
        _ => servers_of(&read_json_file(&path)?),
    };
    let mut out = McpConfig::default();
    for (name, def) in found {
        let clean: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .take(48)
            .collect();
        if !clean.is_empty() {
            out.mcp_servers.insert(clean, def);
        }
    }
    let mut pending = Vec::new();
    for (name, def) in &mut out.mcp_servers {
        let env = def.env.clone();
        let headers = def.headers.clone();
        def.env
            .values_mut()
            .for_each(|v| *v = SECRET_MARKER.to_string());
        def.headers
            .values_mut()
            .for_each(|v| *v = SECRET_MARKER.to_string());
        pending.push(PendingImport {
            name: name.clone(),
            shape: def.clone(),
            env,
            headers,
        });
    }
    *pending_imports()
        .lock()
        .map_err(|_| err("mcp.import_state_locked"))? = pending;
    Ok(out)
}

/// Tools/MCP give the model file and process access through the server: never
/// expose that on the network without an API key.
pub fn check_exposure(host: &str, api_key: Option<&str>, uses_tools: bool) -> Result<(), String> {
    // The tools part of `network::check_exposure` (an "open anyway" confirmation never
    // covers tools/MCP, so it is passed as given here).
    let key_set = api_key.is_some_and(|k| !k.trim().is_empty());
    crate::network::check_exposure(host, key_set, uses_tools, true).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_config_is_private_and_removable() {
        let mut c = McpConfig::default();
        c.mcp_servers.insert(
            "api".into(),
            McpServerDef {
                url: Some("https://x.example".into()),
                headers: BTreeMap::from([("Authorization".to_string(), "Bearer t0k".to_string())]),
                ..Default::default()
            },
        );
        let path = runtime_file("test-mcp-priv");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        write_private(&path, &serde_json::to_string(&c).unwrap()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        assert!(path.exists());
        remove_runtime_config("test-mcp-priv");
        assert!(!path.exists());
    }

    #[test]
    fn test_validate() {
        let mut c = McpConfig::default();
        c.mcp_servers.insert(
            "fs".into(),
            McpServerDef {
                command: Some("npx".into()),
                ..Default::default()
            },
        );
        assert!(validate(&c).is_ok());
        c.mcp_servers
            .insert("bad name".into(), McpServerDef::default());
        assert!(validate(&c).is_err());
        c.mcp_servers.remove("bad name");
        c.mcp_servers.insert(
            "both".into(),
            McpServerDef {
                command: Some("x".into()),
                url: Some("https://x".into()),
                ..Default::default()
            },
        );
        assert!(validate(&c).is_err());
    }

    #[test]
    fn the_transport_choice_is_validated_and_round_trips() {
        let mut c = McpConfig::default();
        let remote = |t: Option<&str>| McpServerDef {
            url: Some("https://x.example/sse".into()),
            transport: t.map(str::to_string),
            ..Default::default()
        };
        for ok in [None, Some("auto"), Some("http"), Some("sse")] {
            c.mcp_servers.insert("remote".into(), remote(ok));
            assert!(validate(&c).is_ok(), "{ok:?}");
        }
        c.mcp_servers
            .insert("remote".into(), remote(Some("websocket")));
        assert!(validate(&c).is_err());
        // The field round-trips in the Cursor-style file, and is absent when not chosen.
        let json = serde_json::to_string(&remote(Some("sse"))).unwrap();
        assert!(json.contains(r#""transport":"sse""#), "{json}");
        assert!(!serde_json::to_string(&remote(None))
            .unwrap()
            .contains("transport"));
    }

    #[test]
    fn test_cursor_format_roundtrip() {
        let json = r#"{"mcpServers":{"everything":{"command":"npx","args":["-y","pkg"],"env":{"K":"V"}},"remote":{"url":"https://example.com/mcp","type":"http"}}}"#;
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        let c: McpConfig = serde_json::from_value(v).unwrap();
        assert_eq!(c.mcp_servers["everything"].args, vec!["-y", "pkg"]);
        assert_eq!(
            c.mcp_servers["remote"].url.as_deref(),
            Some("https://example.com/mcp")
        );
        let out = serde_json::to_string(&c).unwrap();
        assert!(!out.contains("disabled"), "{}", out);
    }

    fn test_config(name: &str, value: &str) -> McpConfig {
        McpConfig {
            mcp_servers: BTreeMap::from([(
                name.to_string(),
                McpServerDef {
                    command: Some("npx".into()),
                    env: BTreeMap::from([("TOKEN".into(), value.into())]),
                    headers: BTreeMap::from([("Authorization".into(), value.into())]),
                    ..Default::default()
                },
            )]),
        }
    }

    #[test]
    fn test_secret_save_load_mask_keep_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        let store = crate::secrets::MemoryStore::default();
        let mut changes = crate::events::subscribe();
        save_with(&path, &store, &test_config("roundtrip", "top-secret")).unwrap();
        assert!(std::iter::from_fn(|| changes.try_recv().ok())
            .any(|e| matches!(e, crate::events::CoreEvent::ToolsChanged)));

        let disk = fs::read_to_string(&path).unwrap();
        assert!(!disk.contains("top-secret"));
        assert!(disk.contains(DISK_PLACEHOLDER));
        let masked = load_with(&path, &store).unwrap();
        assert_eq!(masked.mcp_servers["roundtrip"].env["TOKEN"], SECRET_MARKER);
        assert_eq!(
            masked.mcp_servers["roundtrip"].headers["Authorization"],
            SECRET_MARKER
        );

        save_with(&path, &store, &masked).unwrap();
        assert_eq!(
            store
                .get(&secret_account("roundtrip", "env", "TOKEN"))
                .unwrap()
                .as_deref(),
            Some("top-secret")
        );

        let mut cleared = masked;
        cleared
            .mcp_servers
            .get_mut("roundtrip")
            .unwrap()
            .env
            .insert("TOKEN".into(), String::new());
        save_with(&path, &store, &cleared).unwrap();
        assert!(store
            .get(&secret_account("roundtrip", "env", "TOKEN"))
            .unwrap()
            .is_none());
        assert!(!read_file(&path).mcp_servers["roundtrip"]
            .env
            .contains_key("TOKEN"));

        let empty = McpConfig::default();
        save_with(&path, &store, &empty).unwrap();
        assert!(store
            .get(&secret_account("roundtrip", "header", "Authorization"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_secret_server_rename_moves_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        let store = crate::secrets::MemoryStore::default();
        save_with(&path, &store, &test_config("before", "rename-secret")).unwrap();
        let mut renamed = load_with(&path, &store).unwrap();
        let def = renamed.mcp_servers.remove("before").unwrap();
        renamed.mcp_servers.insert("after".into(), def);
        save_with(&path, &store, &renamed).unwrap();

        assert!(store
            .get(&secret_account("before", "env", "TOKEN"))
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .get(&secret_account("after", "env", "TOKEN"))
                .unwrap()
                .as_deref(),
            Some("rename-secret")
        );
    }

    #[test]
    fn test_plaintext_migration_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        let store = crate::secrets::MemoryStore::default();
        fs::write(
            &path,
            serde_json::to_string(&test_config("legacy", "plain-old")).unwrap(),
        )
        .unwrap();

        let first = load_with(&path, &store).unwrap();
        let second = load_with(&path, &store).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.mcp_servers["legacy"].env["TOKEN"], SECRET_MARKER);
        assert!(!fs::read_to_string(&path).unwrap().contains("plain-old"));
        assert_eq!(
            store
                .get(&secret_account("legacy", "env", "TOKEN"))
                .unwrap()
                .as_deref(),
            Some("plain-old")
        );
    }

    #[test]
    fn test_runtime_copy_contains_real_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        let runtime = dir.path().join("mcp.runtime.json");
        let store = crate::secrets::MemoryStore::default();
        save_with(&path, &store, &test_config("runtime", "launch-secret")).unwrap();

        let resolved = runtime_config_data(&path, &store).unwrap();
        write_private(&runtime, &serde_json::to_string(&resolved).unwrap()).unwrap();
        let text = fs::read_to_string(runtime).unwrap();
        assert!(text.contains("launch-secret"));
        assert!(!text.contains(DISK_PLACEHOLDER));
        assert!(!text.contains(SECRET_MARKER));
    }

    #[test]
    fn claude_code_import_reads_user_local_and_project_scopes() {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("proj-a");
        fs::create_dir_all(&proj).unwrap();
        fs::write(
            proj.join(".mcp.json"),
            r#"{"mcpServers":{"shared":{"command":"other"},"from-file":{"type":"http","url":"https://x.example/mcp","headers":{"Authorization":"Bearer t"}}}}"#,
        )
        .unwrap();
        let file = dir.path().join(".claude.json");
        let proj_key = proj.to_string_lossy().replace('\\', "/");
        fs::write(
            &file,
            serde_json::json!({
                "numStartups": 3,
                "mcpServers": {
                    "shared": {"type": "stdio", "command": "npx", "args": ["-y", "pkg"], "env": {"K": "V"}},
                    "legacy-sse": {"type": "sse", "url": "https://old.example/sse"}
                },
                "projects": {
                    (proj_key): {"mcpServers": {"local": {"command": "uvx", "args": ["tool"]}}, "allowedTools": []}
                }
            })
            .to_string(),
        )
        .unwrap();
        let before = fs::read_to_string(&file).unwrap();
        let s = claude_code_servers(&file).unwrap();
        // Read-only.
        assert_eq!(fs::read_to_string(&file).unwrap(), before);
        assert_eq!(s["shared"].command.as_deref(), Some("npx"));
        assert_eq!(s["shared"].env["K"], "V");
        assert_eq!(s["legacy-sse"].transport.as_deref(), Some("sse"));
        assert_eq!(s["from-file"].transport, None);
        assert_eq!(s["local"].command.as_deref(), Some("uvx"));
        // The project's `.mcp.json` entry with a clashing name keeps both, suffixed.
        assert_eq!(s["shared-proj-a"].command.as_deref(), Some("other"));
        assert_eq!(s["from-file"].headers["Authorization"], "Bearer t");
        assert_eq!(s.len(), 5, "{:?}", s.keys().collect::<Vec<_>>());
    }

    #[test]
    fn codex_import_reads_mcp_servers_tables() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.toml");
        fs::write(
            &file,
            r#"
model = "gpt-5"

[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
env = { API_KEY = "k" }

[mcp_servers.remote]
url = "https://mcp.example/mcp"
http_headers = { "X-Team" = "a" }
bearer_token_env_var = "HARNESS_TEST_CODEX_TOKEN_UNSET"

[mcp_servers.off]
command = "x"
enabled = false

[mcp_servers.empty]
startup_timeout_sec = 5
"#,
        )
        .unwrap();
        let s = codex_servers(&file).unwrap();
        assert_eq!(s.len(), 3, "{:?}", s.keys().collect::<Vec<_>>());
        assert_eq!(s["context7"].args, ["-y", "@upstash/context7-mcp"]);
        assert_eq!(s["context7"].env["API_KEY"], "k");
        assert_eq!(s["remote"].url.as_deref(), Some("https://mcp.example/mcp"));
        // The bearer variable is not set: no Authorization header is invented.
        assert_eq!(s["remote"].headers.len(), 1);
        assert!(s["off"].disabled);
    }

    #[test]
    fn test_exposure() {
        assert!(check_exposure("127.0.0.1", None, true).is_ok());
        assert!(check_exposure("0.0.0.0", None, true).is_err());
        assert!(check_exposure("0.0.0.0", Some("k"), true).is_ok());
        assert!(check_exposure("0.0.0.0", None, false).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn test_windows_wrap_npx() {
        let def = McpServerDef {
            command: Some("npx".into()),
            args: vec!["-y".into(), "pkg".into()],
            ..Default::default()
        };
        let wrapped = windows_wrap(&def);
        // Only wrapped when npx.cmd is actually on PATH (Node installed).
        if wrapped.command.as_deref() == Some("cmd") {
            assert_eq!(wrapped.args, vec!["/c", "npx", "-y", "pkg"]);
        }
        let exe = McpServerDef {
            command: Some("C:/tools/server.exe".into()),
            ..Default::default()
        };
        assert_eq!(windows_wrap(&exe), exe);
    }

    /// llama-server + MCP "everything" server (needs Node/npx):
    /// HS_LLAMA_SERVER=… HS_TEST_MODEL=… cargo test e2e_mcp -- --ignored
    #[tokio::test]
    #[ignore]
    async fn e2e_mcp_tools_through_llama_server() {
        use crate::process::{LaunchConfig, ProcessManager, ServerStatus};
        use std::time::{Duration, Instant};
        let (Ok(bin), Ok(model)) = (
            std::env::var("HS_LLAMA_SERVER"),
            std::env::var("HS_TEST_MODEL"),
        ) else {
            eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let mut servers = BTreeMap::new();
        servers.insert(
            "everything".to_string(),
            windows_wrap(&McpServerDef {
                command: Some("npx".into()),
                args: vec![
                    "-y".into(),
                    "@modelcontextprotocol/server-everything".into(),
                ],
                ..Default::default()
            }),
        );
        let cfg_path = dir.path().join("mcp.json");
        fs::write(
            &cfg_path,
            serde_json::to_string(&McpConfig {
                mcp_servers: servers,
            })
            .unwrap(),
        )
        .unwrap();

        let pm = ProcessManager::new();
        pm.start(LaunchConfig {
            binary_path: bin.into(),
            model_path: model.into(),
            context_size: 512,
            host: "127.0.0.1".into(),
            port: 18096,
            mcp_enabled: true,
            mcp_config_path: Some(cfg_path),
            ..Default::default()
        })
        .await
        .unwrap();
        let t = Instant::now();
        while pm.get_status() != ServerStatus::Running {
            assert!(
                t.elapsed() < Duration::from_secs(120),
                "{:?}",
                pm.get_logs(Some(30))
            );
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        let client = reqwest::Client::new();
        let tools: Vec<serde_json::Value> = client
            .get("http://127.0.0.1:18096/tools")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let names: Vec<&str> = tools.iter().filter_map(|t| t["tool"].as_str()).collect();
        assert!(names.contains(&"get_info"), "{:?}", names);
        assert!(names.contains(&"everything_echo"), "{:?}", names);
        let res: serde_json::Value = client
            .post("http://127.0.0.1:18096/tools")
            .json(&serde_json::json!({"tool": "everything_echo", "params": {"message": "hi"}}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(res.to_string().contains("Echo: hi"), "{}", res);
        pm.stop().await.unwrap();
    }
}
