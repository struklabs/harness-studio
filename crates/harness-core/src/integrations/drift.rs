//! Alias drift: client configs that Harness wrote but whose model name no longer exists.
//!
//! An integration stores a model name (a definition alias, or the id a server reported). When
//! the alias is renamed, the definition deleted, or a profile gets its own definition (the
//! served name then changes from the file name to `<file>-<profile>`), the tool keeps asking for
//! the old name and fails with "model not found". [`detect_alias_drift`] reads the tools'
//! configs (read-only) and lists the aliases they name that Harness no longer serves, with the
//! replacement to offer; the fix itself is the normal apply flow (preview, then write).

use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;

use super::keyed::{self, Format, Seg};
use super::launch::PLACEHOLDER_KEY;
use super::{
    claude_settings, codex_dir, continue_config_path, continue_entry, find, goose_provider_path,
    kilo_config_path, opencode_config_path, read_value, registry, strip_harness_prefix,
    vscode_models_path, zed_path, zed_settings_path, Integration, Roots, Spec, ENTRY_NAME,
};

/// One configured integration whose model names have drifted.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AliasDrift {
    /// Integration id (`opencode`, `claude-code`, ...).
    pub id: String,
    pub name: String,
    /// Every model name the tool's config holds for Harness (default model first).
    pub configured: Vec<String>,
    /// The ones that no definition (and no server) answers to.
    pub missing: Vec<String>,
    /// The alias to move the default model to, when one matches unambiguously.
    pub suggestion: Option<String>,
    /// Provider blocks the user wrote by hand that point at the Harness gateway (their model
    /// names are in `configured` too). Harness never rewrites these; the fix writes its own
    /// block and leaves them for the user.
    pub user_blocks: Vec<String>,
}

/// Model names an OpenCode-format config holds for Harness: the default `model` when it
/// points at the `harness` provider, then the provider block's model keys.
fn opencode_format_models(root: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(m) = root
        .get("model")
        .and_then(Value::as_str)
        .and_then(strip_harness_prefix)
    {
        out.push(m.to_string());
    }
    if let Some(models) = root
        .pointer("/provider/harness/models")
        .and_then(Value::as_object)
    {
        for k in models.keys() {
            if !out.iter().any(|o| o == k) {
                out.push(k.clone());
            }
        }
    }
    out
}

/// Every model name the tool `id` has configured for Harness (read-only).
fn configured_models(id: &str, roots: &Roots) -> Vec<String> {
    let from_file = match id {
        "opencode" => roots
            .home
            .as_deref()
            .and_then(|h| read_value(&opencode_config_path(h))),
        "kilo" => kilo_config_path(roots).and_then(|p| read_value(&p)),
        _ => None,
    };
    if let Some(root) = from_file {
        return opencode_format_models(&root);
    }
    find(id)
        .ok()
        .and_then(|spec| spec.configured_model(roots))
        .into_iter()
        .collect()
}

/// The valid alias a missing one most likely became: exactly one valid alias that starts with it
/// (`<file>` -> `<file>-<profile>`) or that it starts with (a shortened alias).
fn suggest(missing: &str, valid: &[String]) -> Option<String> {
    let m = missing.to_lowercase();
    let mut hits = valid.iter().filter(|v| {
        let v = v.to_lowercase();
        v != m && (v.starts_with(&m) || m.starts_with(&v))
    });
    match (hits.next(), hits.next()) {
        (Some(one), None) => Some(one.clone()),
        _ => None,
    }
}

/// The integrations whose config Harness manages: they write files and the tool is configured
/// (or the ledger holds an apply).
fn managed(roots: &Roots) -> Vec<Spec> {
    let ledger = keyed::Ledger::load(roots.ledger());
    registry()
        .into_iter()
        .filter(|spec| spec.writes_files())
        .filter(|spec| spec.detect(roots).configured || ledger.entry(spec.id).is_some())
        .collect()
}

/// Whether `url` is a loopback address on one of the gateway's ports.
fn targets_gateway(url: &str, gateway_ports: &[u16]) -> bool {
    address(url).is_some_and(|(loopback, _, p)| loopback && gateway_ports.contains(&p))
}

/// Provider blocks the user wrote by hand (not Harness's own `harness` block) whose base URL is
/// the Harness gateway, with the model names each holds: `(block, models)`. Read-only. Covers
/// the OpenCode format (OpenCode, Kilo Code), Codex `model_providers` and Continue `models`.
fn user_blocks(id: &str, roots: &Roots, gateway_ports: &[u16]) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    match id {
        "opencode" | "kilo" => {
            let path = if id == "opencode" {
                roots.home.as_deref().map(opencode_config_path)
            } else {
                kilo_config_path(roots)
            };
            let Some(root) = path.and_then(|p| read_value(&p)) else {
                return out;
            };
            let default = root.get("model").and_then(Value::as_str);
            for (name, block) in root["provider"].as_object().into_iter().flatten() {
                let url = block["options"]["baseURL"].as_str();
                if name == "harness" || !url.is_some_and(|u| targets_gateway(u, gateway_ports)) {
                    continue;
                }
                let mut models: Vec<String> = Vec::new();
                if let Some(m) = default.and_then(|m| m.strip_prefix(&format!("{name}/"))) {
                    models.push(m.to_string());
                }
                for k in block["models"]
                    .as_object()
                    .into_iter()
                    .flat_map(|m| m.keys())
                {
                    if !models.contains(k) {
                        models.push(k.clone());
                    }
                }
                out.push((name.clone(), models));
            }
        }
        "codex" => {
            let Ok(dir) = codex_dir(roots) else {
                return out;
            };
            let Some(root) = read_value(&dir.join("config.toml")) else {
                return out;
            };
            for (name, block) in root["model_providers"].as_object().into_iter().flatten() {
                let url = block["base_url"].as_str();
                if name == "harness" || !url.is_some_and(|u| targets_gateway(u, gateway_ports)) {
                    continue;
                }
                let models = (root["model_provider"].as_str() == Some(name.as_str()))
                    .then(|| root["model"].as_str())
                    .flatten()
                    .map(|m| vec![m.to_string()])
                    .unwrap_or_default();
                out.push((name.clone(), models));
            }
        }
        "continue" => {
            let Some(home) = roots.home.as_deref() else {
                return out;
            };
            let Some(root) = read_value(&continue_config_path(home).0) else {
                return out;
            };
            let own = continue_entry(&root);
            for entry in root["models"].as_array().into_iter().flatten() {
                let url = entry["apiBase"].as_str();
                if own == Some(entry) || !url.is_some_and(|u| targets_gateway(u, gateway_ports)) {
                    continue;
                }
                let label = ["title", "name", "model"]
                    .iter()
                    .find_map(|k| entry[*k].as_str())
                    .unwrap_or("?");
                out.push((
                    label.to_string(),
                    entry["model"]
                        .as_str()
                        .map(str::to_string)
                        .into_iter()
                        .collect(),
                ));
            }
        }
        _ => {}
    }
    out
}

/// Configured integrations whose names are not in `valid` (case-insensitive). Without any valid
/// name (nothing defined, nothing served) nothing is reported: it cannot be told what is stale.
#[cfg(test)]
pub(crate) fn detect_in(roots: &Roots, valid: &[String]) -> Vec<AliasDrift> {
    detect_with(roots, valid, &[])
}

/// [`detect_in`], plus the provider blocks the user wrote by hand that point at the gateway
/// (`gateway_ports`: its current and earlier ports; empty = hand-written blocks are not looked
/// at).
pub(crate) fn detect_with(
    roots: &Roots,
    valid: &[String],
    gateway_ports: &[u16],
) -> Vec<AliasDrift> {
    if valid.is_empty() {
        return Vec::new();
    }
    let known = |name: &str| valid.iter().any(|v| v.eq_ignore_ascii_case(name));
    let managed: Vec<&'static str> = managed(roots).into_iter().map(|s| s.id).collect();
    registry()
        .into_iter()
        .filter(|spec| spec.writes_files())
        .filter_map(|spec| {
            let mut configured = if managed.contains(&spec.id) {
                configured_models(spec.id, roots)
            } else {
                Vec::new()
            };
            let mut blocks = Vec::new();
            if !gateway_ports.is_empty() {
                for (name, models) in user_blocks(spec.id, roots, gateway_ports) {
                    for m in models {
                        if !configured.contains(&m) {
                            configured.push(m);
                        }
                    }
                    blocks.push(name);
                }
            }
            let missing: Vec<String> = configured.iter().filter(|m| !known(m)).cloned().collect();
            if missing.is_empty() {
                return None;
            }
            let suggestion = configured
                .first()
                .filter(|first| missing.contains(first))
                .and_then(|first| suggest(first, valid));
            Some(AliasDrift {
                id: spec.id.to_string(),
                name: spec.meta().name.to_string(),
                configured,
                missing,
                suggestion,
                user_blocks: blocks,
            })
        })
        .collect()
}

/// [`detect_in`] on the real user profile; `valid` = every definition alias plus `extra`
/// (names a running server or the gateway answers to).
pub fn detect_alias_drift(extra: &[String]) -> Vec<AliasDrift> {
    let mut valid: Vec<String> = crate::domain::store::Domain::global()
        .definitions()
        .unwrap_or_default()
        .into_iter()
        .map(|d| d.alias)
        .collect();
    valid.extend(extra.iter().filter(|e| !e.trim().is_empty()).cloned());
    let mut ports = known_gateway_ports(
        &crate::ports::PortState::load(&crate::paths::data_root()).gateway_history,
    );
    ports.extend(crate::gateway::status().port);
    detect_with(&Roots::from_env(), &valid, &ports)
}

// --- endpoint drift (the gateway moved to another port) ---

/// A configured integration whose saved address is not the gateway's current one (the gateway
/// started on a fallback port, or the tool was set up for a server's own port).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EndpointDrift {
    /// Integration id (`opencode`, `claude-code`, ...).
    pub id: String,
    pub name: String,
    /// The base URL Harness wrote into the tool's config.
    pub configured: String,
    /// The gateway's base URL now.
    pub current: String,
}

/// `(loopback, host, port)` of an `http://host:port/...` URL.
pub(super) fn address(url: &str) -> Option<(bool, String, u16)> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split('/').next()?;
    let (host, port) = authority.rsplit_once(':')?;
    let port = port.parse().ok()?;
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_lowercase();
    let loopback = host == "localhost" || crate::network::is_loopback_host(&host);
    Some((loopback, host, port))
}

/// The stored apply request of every configured integration (`(spec, request)`).
fn stored_requests(roots: &Roots) -> Vec<(super::Spec, super::StoredRequest)> {
    let ledger = super::keyed::Ledger::load(roots.ledger());
    registry()
        .into_iter()
        .filter(|spec| spec.writes_files())
        .filter_map(|spec| {
            let stored = ledger.entry(spec.id)?.request.clone()?;
            let stored: super::StoredRequest = serde_json::from_value(stored).ok()?;
            Some((spec, stored))
        })
        .collect()
}

/// One address (or a block holding addresses) Harness wrote into a tool's config file.
struct UrlSlot {
    path: PathBuf,
    format: Format,
    /// Key path of the value that holds the address(es).
    keys: Vec<Seg>,
    value: Value,
    /// The config holds an API key or token (not Harness's placeholder): it is not rewritten on
    /// its own.
    secret: bool,
}

impl UrlSlot {
    fn urls(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect_urls(&self.value, &mut out);
        out
    }
}

fn collect_urls(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) if s.starts_with("http") => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| collect_urls(x, out)),
        Value::Object(m) => m.values().for_each(|x| collect_urls(x, out)),
        _ => {}
    }
}

/// `url` with its port replaced.
pub(super) fn with_port(url: &str, port: u16) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let (authority, tail) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let (host, _) = authority.rsplit_once(':')?;
    Some(format!("{scheme}://{host}:{port}{tail}"))
}

/// Whether `url` is a loopback address on one of the gateway's earlier ports (not `current`).
fn is_old_gateway_url(url: &str, current: u16, gateway_ports: &[u16]) -> bool {
    address(url)
        .is_some_and(|(loopback, _, p)| loopback && p != current && gateway_ports.contains(&p))
}

/// `v` with every such address moved to `current`.
fn move_urls(v: &Value, current: u16, gateway_ports: &[u16]) -> Value {
    match v {
        Value::String(s)
            if s.starts_with("http") && is_old_gateway_url(s, current, gateway_ports) =>
        {
            with_port(s, current).map_or_else(|| v.clone(), Value::String)
        }
        Value::Array(a) => Value::Array(
            a.iter()
                .map(|x| move_urls(x, current, gateway_ports))
                .collect(),
        ),
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, x)| (k.clone(), move_urls(x, current, gateway_ports)))
                .collect(),
        ),
        _ => v.clone(),
    }
}

/// A key that is not Harness's placeholder (it names a real secret).
fn is_secret(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|k| !k.trim().is_empty() && k != PLACEHOLDER_KEY)
}

/// The address in the `harness` provider block of an OpenCode-format file (OpenCode, Kilo
/// Code, Open Design).
fn opencode_slot(path: Option<PathBuf>) -> Vec<UrlSlot> {
    let Some(path) = path else {
        return Vec::new();
    };
    let Some(root) = read_value(&path) else {
        return Vec::new();
    };
    let opts = &root["provider"]["harness"]["options"];
    let Some(url) = opts.get("baseURL").filter(|u| u.is_string()) else {
        return Vec::new();
    };
    vec![UrlSlot {
        path,
        format: Format::Json,
        keys: keyed::keys(&["provider", "harness", "options", "baseURL"]),
        value: url.clone(),
        secret: is_secret(&opts["apiKey"]),
    }]
}

fn slot(
    path: PathBuf,
    format: Format,
    keys: Vec<Seg>,
    value: Option<&Value>,
    secret: bool,
) -> Vec<UrlSlot> {
    value
        .filter(|v| !v.is_null())
        .map(|v| UrlSlot {
            path,
            format,
            keys,
            value: v.clone(),
            secret,
        })
        .into_iter()
        .collect()
}

/// The addresses Harness wrote for the tool `id` (read-only). Only blocks that name Harness
/// count: an unrelated address of the user's is never listed.
fn url_slots(id: &str, roots: &Roots) -> Vec<UrlSlot> {
    match id {
        "claude-code" => {
            let Ok(path) = claude_settings(roots) else {
                return Vec::new();
            };
            let Some(root) = read_value(&path) else {
                return Vec::new();
            };
            let env = &root["env"];
            if !env["ANTHROPIC_CUSTOM_HEADERS"]
                .as_str()
                .is_some_and(|h| h.contains("X-Harness-Client"))
            {
                return Vec::new();
            }
            slot(
                path,
                Format::Json,
                keyed::keys(&["env", "ANTHROPIC_BASE_URL"]),
                env.get("ANTHROPIC_BASE_URL"),
                is_secret(&env["ANTHROPIC_AUTH_TOKEN"]),
            )
        }
        "codex" => {
            let Ok(dir) = codex_dir(roots) else {
                return Vec::new();
            };
            let path = dir.join("config.toml");
            let Some(root) = read_value(&path) else {
                return Vec::new();
            };
            let provider = &root["model_providers"]["harness"];
            slot(
                path,
                Format::Toml,
                keyed::keys(&["model_providers", "harness", "base_url"]),
                provider.get("base_url"),
                provider["http_headers"].get("Authorization").is_some(),
            )
        }
        "opencode" | "opendesign" => opencode_slot(roots.home.as_deref().map(opencode_config_path)),
        "kilo" => opencode_slot(kilo_config_path(roots)),
        "continue" => {
            let Some(home) = roots.home.as_deref() else {
                return Vec::new();
            };
            let (path, yaml) = continue_config_path(home);
            let Some(root) = read_value(&path) else {
                return Vec::new();
            };
            let Some(entry) = continue_entry(&root) else {
                return Vec::new();
            };
            let field = if entry.get("title").is_some() {
                "title"
            } else {
                "name"
            };
            let Some(name) = entry[field].as_str() else {
                return Vec::new();
            };
            slot(
                path,
                if yaml { Format::Yaml } else { Format::Json },
                vec![
                    Seg::key("models"),
                    Seg::item(field, name),
                    Seg::key("apiBase"),
                ],
                entry.get("apiBase"),
                is_secret(&entry["apiKey"]),
            )
        }
        "vscode-copilot" => {
            let Some(path) = vscode_models_path(roots) else {
                return Vec::new();
            };
            let Some(root) = read_value(&path) else {
                return Vec::new();
            };
            let Some(entry) = root
                .as_array()
                .and_then(|a| a.iter().find(|e| e["name"] == ENTRY_NAME))
            else {
                return Vec::new();
            };
            slot(
                path,
                Format::Json,
                vec![Seg::item("name", ENTRY_NAME), Seg::key("models")],
                entry.get("models"),
                entry.get("apiKey").is_some(),
            )
        }
        "zed" => {
            let Some(path) = zed_settings_path(roots) else {
                return Vec::new();
            };
            let Some(root) = read_value(&path) else {
                return Vec::new();
            };
            let mut keys = zed_path();
            keys.push(Seg::key("api_url"));
            slot(
                path,
                Format::Json,
                keys,
                root["language_models"]["openai_compatible"][ENTRY_NAME].get("api_url"),
                false,
            )
        }
        "goose" => {
            let Some(path) = goose_provider_path(roots) else {
                return Vec::new();
            };
            let Some(root) = read_value(&path) else {
                return Vec::new();
            };
            slot(
                path,
                Format::Json,
                keyed::keys(&["base_url"]),
                root.get("base_url"),
                root["requires_auth"].as_bool().unwrap_or(false),
            )
        }
        "librechat" => {
            // The file is one the user picked: only the ledger knows it.
            let ledger = keyed::Ledger::load(roots.ledger());
            let Some(file) = ledger.entry(id).and_then(|e| e.files.first()) else {
                return Vec::new();
            };
            let path = PathBuf::from(&file.path);
            let Some(root) = read_value(&path) else {
                return Vec::new();
            };
            let Some(entry) = root["endpoints"]["custom"]
                .as_array()
                .and_then(|a| a.iter().find(|e| e["name"] == ENTRY_NAME))
            else {
                return Vec::new();
            };
            slot(
                path,
                Format::Yaml,
                vec![
                    Seg::key("endpoints"),
                    Seg::key("custom"),
                    Seg::item("name", ENTRY_NAME),
                    Seg::key("baseURL"),
                ],
                entry.get("baseURL"),
                is_secret(&entry["apiKey"]),
            )
        }
        _ => Vec::new(),
    }
}

/// Integrations whose saved loopback address points at another port than `current_base`
/// (the gateway's base URL): the address of the stored apply request (also a server's own
/// port), and the address read from the config file itself when it is one of the gateway's
/// earlier ports (`gateway_ports`; configs written before Harness remembered the request have
/// no stored one). Addresses on other hosts (a LAN setup) and other ports in a config file are
/// left alone.
pub(crate) fn endpoint_drift_in(
    roots: &Roots,
    current_base: &str,
    gateway_ports: &[u16],
) -> Vec<EndpointDrift> {
    let Some((true, _, current_port)) = address(current_base) else {
        return Vec::new();
    };
    let mut out: Vec<EndpointDrift> = stored_requests(roots)
        .into_iter()
        .filter_map(|(spec, stored)| {
            let (loopback, _, port) = address(&stored.base_url)?;
            (loopback && port != current_port).then(|| EndpointDrift {
                id: spec.id.to_string(),
                name: spec.meta().name.to_string(),
                configured: stored.base_url.clone(),
                current: current_base.to_string(),
            })
        })
        .collect();
    for spec in managed(roots) {
        if out.iter().any(|d| d.id == spec.id) {
            continue;
        }
        let old = url_slots(spec.id, roots)
            .iter()
            .flat_map(UrlSlot::urls)
            .find(|u| is_old_gateway_url(u, current_port, gateway_ports));
        if let Some(configured) = old {
            out.push(EndpointDrift {
                id: spec.id.to_string(),
                name: spec.meta().name.to_string(),
                configured,
                current: current_base.to_string(),
            });
        }
    }
    out
}

/// The gateway's earlier ports: `history` (`config/ports.json`) and the default.
fn known_gateway_ports(history: &[u16]) -> Vec<u16> {
    let mut ports = history.to_vec();
    ports.push(crate::gateway::DEFAULT_PORT);
    ports
}

/// [`endpoint_drift_in`] on the real user profile.
pub fn detect_endpoint_drift(current_base: &str) -> Vec<EndpointDrift> {
    let history = crate::ports::PortState::load(&crate::paths::data_root()).gateway_history;
    endpoint_drift_in(
        &Roots::from_env(),
        current_base,
        &known_gateway_ports(&history),
    )
}

/// What [`follow_gateway_port_in`] did (integration ids).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PortFollow {
    /// Rewritten for the current gateway address.
    pub moved: Vec<String>,
    /// Still on another address after it (a key in the file, a server's own port): the card
    /// shows the warning with "Düzelt".
    pub pending: Vec<String>,
}

/// Rewrites the configs Harness wrote for an earlier gateway port (`gateway_ports`: every port
/// the gateway listened on) for `current_base`. A config with a stored apply request is written
/// again with it (only the address changes, the tool's default model stays) while its files are
/// still as Harness wrote them; any other is changed in place: only the port of the address
/// Harness wrote, with a backup, so a model the user picked afterwards stays. Configs with an
/// API key (at apply time or added since) and configs pointing at anything else (a server's own
/// port) are left for the user ("Düzelt").
pub(crate) fn follow_gateway_port_in(
    roots: &Roots,
    current_base: &str,
    gateway_ports: &[u16],
) -> PortFollow {
    let mut moved = Vec::new();
    for drift in endpoint_drift_in(roots, current_base, gateway_ports) {
        let Some((spec, stored)) = stored_requests(roots)
            .into_iter()
            .find(|(s, _)| s.id == drift.id)
        else {
            continue;
        };
        let old_port = address(&stored.base_url).map(|(_, _, p)| p);
        // A key added after the apply: writing the request again would put the placeholder back.
        let keyed_now = url_slots(spec.id, roots).iter().any(|s| s.secret);
        if stored.had_key || keyed_now || !old_port.is_some_and(|p| gateway_ports.contains(&p)) {
            continue;
        }
        if !untouched_since_apply(roots, spec.id) {
            // The user edited the config since: only its address moves.
            if let Some((true, _, current)) = address(current_base) {
                if rewrite_in_place(roots, &spec, current, gateway_ports) {
                    moved.push(spec.id.to_string());
                }
                // Moved now or by the user already: the request follows the file.
                if on_port(roots, spec.id, current, gateway_ports) {
                    set_stored_base(roots, spec.id, current_base);
                }
            }
            continue;
        }
        match super::apply_in(roots, spec.id, &stored.to_request(current_base)) {
            Ok(_) => moved.push(spec.id.to_string()),
            Err(e) => eprintln!(
                "[Harness Studio] {} yeni gateway adresine taşınamadı: {}",
                spec.meta().name,
                e
            ),
        }
    }
    if let Some((true, _, current)) = address(current_base) {
        for spec in managed(roots) {
            if rewrite_in_place(roots, &spec, current, gateway_ports) {
                moved.push(spec.id.to_string());
            }
        }
    }
    let pending = endpoint_drift_in(roots, current_base, gateway_ports)
        .into_iter()
        .map(|d| d.id)
        .collect();
    PortFollow { moved, pending }
}

/// Whether every file the apply of `id` wrote is still byte for byte as Harness wrote it.
fn untouched_since_apply(roots: &Roots, id: &str) -> bool {
    let ledger = keyed::Ledger::load(roots.ledger());
    ledger.entry(id).is_some_and(|entry| {
        entry.files.iter().all(|f| {
            std::fs::read(&f.path).is_ok_and(|bytes| keyed::sha256_hex(&bytes) == f.sha256)
        })
    })
}

/// Whether the config of `id` names `current` and no earlier gateway port.
fn on_port(roots: &Roots, id: &str, current: u16, gateway_ports: &[u16]) -> bool {
    let urls: Vec<String> = url_slots(id, roots)
        .iter()
        .flat_map(UrlSlot::urls)
        .collect();
    urls.iter()
        .any(|u| address(u).is_some_and(|(_, _, p)| p == current))
        && !urls
            .iter()
            .any(|u| is_old_gateway_url(u, current, gateway_ports))
}

/// Points the stored apply request of `id` at `base_url` after its config was moved in place,
/// so the request no longer reports the old address.
fn set_stored_base(roots: &Roots, id: &str, base_url: &str) {
    let mut ledger = keyed::Ledger::load(roots.ledger());
    let Some(request) = ledger
        .entries
        .iter_mut()
        .find(|e| e.id == id)
        .and_then(|e| e.request.as_mut())
    else {
        return;
    };
    request["base_url"] = serde_json::Value::String(base_url.to_string());
    if let Err(e) = ledger.save(roots.ledger()) {
        eprintln!("[Harness Studio] Bağlantı kaydı güncellenemedi: {e}");
    }
}

/// Moves the earlier gateway ports in the config of `spec` to `current`, unless the config
/// holds a key. `true` when a file was changed.
fn rewrite_in_place(roots: &Roots, spec: &Spec, current: u16, gateway_ports: &[u16]) -> bool {
    let stale: Vec<UrlSlot> = url_slots(spec.id, roots)
        .into_iter()
        .filter(|s| {
            s.urls()
                .iter()
                .any(|u| is_old_gateway_url(u, current, gateway_ports))
        })
        .collect();
    if stale.is_empty() || stale.iter().any(|s| s.secret) {
        return false;
    }
    let mut changed = false;
    for slot in stale {
        let value = move_urls(&slot.value, current, gateway_ports);
        match keyed::retarget(
            spec.id,
            &slot.path,
            slot.format,
            &slot.keys,
            &value,
            roots.ledger(),
        ) {
            Ok(saved) => changed |= saved.is_some(),
            Err(e) => eprintln!(
                "[Harness Studio] {} yeni gateway adresine taşınamadı: {}",
                spec.meta().name,
                e
            ),
        }
    }
    changed
}

/// [`follow_gateway_port_in`] on the real user profile. Only the ports this data directory's
/// gateway really listened on (`gateway_ports`, its history) are followed, not the default port
/// on its own: an instance with its own data directory (a test or a measurement on another
/// port) must not pull the user's tools away from the app's gateway on 11435. Detection still
/// counts the default port, so such a config shows up as a warning there instead.
pub fn follow_gateway_port(current_base: &str, gateway_ports: &[u16]) -> PortFollow {
    follow_history_in(&Roots::from_env(), current_base, gateway_ports)
}

/// [`follow_gateway_port`] on `roots`: the gateway's own port history, nothing added.
fn follow_history_in(roots: &Roots, current_base: &str, history: &[u16]) -> PortFollow {
    follow_gateway_port_in(roots, current_base, history)
}

/// The tool's display name (the id when unknown).
pub fn client_name(id: &str) -> String {
    find(id).map_or_else(|_| id.to_string(), |s| s.meta().name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::{apply_in, ApplyRequest, ProviderModel};
    use serde_json::json;

    fn roots(dir: &std::path::Path) -> Roots {
        Roots {
            home: Some(dir.to_path_buf()),
            appdata: Some(dir.join("AppData/Roaming")),
            localappdata: Some(dir.join("AppData/Local")),
            ledger: Some(dir.join("integrations.json")),
            ..Default::default()
        }
    }

    fn request(model: &str, others: &[&str]) -> ApplyRequest {
        ApplyRequest {
            base_url: "http://127.0.0.1:8788/v1".into(),
            model_id: model.into(),
            model_name: model.into(),
            context_size: 32768,
            models: others
                .iter()
                .map(|m| ProviderModel {
                    id: m.to_string(),
                    context_size: 0,
                })
                .collect(),
            set_default_model: Some(true),
            ..Default::default()
        }
    }

    fn valid(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn opencode_config_written_by_harness_is_checked_against_the_aliases() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        apply_in(
            &r,
            "opencode",
            &request("qwen3-4b", &["gpt-oss-20b", "old-name"]),
        )
        .unwrap();

        // Everything still exists: nothing to report.
        assert!(detect_in(&r, &valid(&["qwen3-4b", "gpt-oss-20b", "old-name"])).is_empty());
        // Nothing known at all: cannot judge.
        assert!(detect_in(&r, &[]).is_empty());

        // `old-name` was deleted, the default `qwen3-4b` got a profile alias.
        let d = detect_in(&r, &valid(&["Qwen3-4B-code", "GPT-OSS-20B", "unrelated"]));
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].id, "opencode");
        assert_eq!(d[0].configured[0], "qwen3-4b");
        assert_eq!(d[0].missing, ["qwen3-4b", "old-name"]);
        // Case-insensitive prefix match: `qwen3-4b` -> `Qwen3-4B-code`.
        assert_eq!(d[0].suggestion.as_deref(), Some("Qwen3-4B-code"));

        // Two candidates: no guess.
        let d = detect_in(
            &r,
            &valid(&["qwen3-4b-a", "qwen3-4b-b", "gpt-oss-20b", "old-name"]),
        );
        assert_eq!(d[0].missing, ["qwen3-4b"]);
        assert_eq!(d[0].suggestion, None);
    }

    #[test]
    fn a_moved_gateway_port_is_followed_and_other_addresses_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let gw = |port: u16| format!("http://127.0.0.1:{port}/v1");
        let mut req = request("qwen3-4b", &["gpt-oss-20b"]);
        req.base_url = gw(11435);
        apply_in(&r, "opencode", &req).unwrap();
        // Claude Code set up for a llama-server's own port.
        let mut direct = request("qwen3-4b", &[]);
        direct.base_url = "http://localhost:11440/v1".into();
        apply_in(&r, "claude-code", &direct).unwrap();
        // Codex with a key: never rewritten on its own.
        let mut keyed = request("qwen3-4b", &[]);
        keyed.base_url = gw(11435);
        keyed.api_key = Some("secret".into());
        apply_in(&r, "codex", &keyed).unwrap();

        // Same port: only the direct one differs.
        let d = endpoint_drift_in(&r, &gw(11435), &[11435, 11437]);
        assert_eq!(
            d.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
            ["claude-code"]
        );
        // A LAN or unparsable gateway address is not judged.
        assert!(endpoint_drift_in(&r, "http://192.168.1.5:11435/v1", &[11435]).is_empty());

        // The gateway moved to 11437: OpenCode follows, the others are reported.
        let mut ids: Vec<_> = endpoint_drift_in(&r, &gw(11437), &[11435, 11437])
            .into_iter()
            .map(|d| d.id)
            .collect();
        ids.sort();
        assert_eq!(ids, ["claude-code", "codex", "opencode"]);
        let done = follow_gateway_port_in(&r, &gw(11437), &[11435, 11437]);
        assert_eq!(done.moved, ["opencode"]);
        assert_eq!(done.pending, ["claude-code", "codex"]);
        let text =
            std::fs::read_to_string(crate::integrations::opencode_config_path(dir.path())).unwrap();
        assert!(text.contains("127.0.0.1:11437/v1"), "{text}");
        assert!(!text.contains(":11435"), "{text}");
        let mut left: Vec<_> = endpoint_drift_in(&r, &gw(11437), &[11435, 11437])
            .into_iter()
            .map(|d| d.id)
            .collect();
        left.sort();
        assert_eq!(left, ["claude-code", "codex"]);
    }

    #[test]
    fn following_the_port_keeps_a_key_and_a_model_the_user_set_after_the_apply() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let gw = |port: u16| format!("http://127.0.0.1:{port}/v1");
        let mut req = request("qwen3-4b", &[]);
        req.base_url = gw(11435);
        apply_in(&r, "claude-code", &req).unwrap();
        let path = crate::integrations::claude_settings(&r).unwrap();
        let edit = |f: &dyn Fn(&mut Value)| {
            let mut v: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            f(&mut v);
            std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
        };
        let read =
            || -> Value { serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap() };

        // The user picks another model: the port moves, the model stays, nothing is left over.
        edit(&|v| v["env"]["ANTHROPIC_MODEL"] = json!("my-model"));
        let done = follow_gateway_port_in(&r, &gw(11437), &[11435, 11437]);
        assert_eq!(done.moved, ["claude-code"]);
        assert!(done.pending.is_empty(), "{done:?}");
        let v = read();
        assert_eq!(
            v["env"]["ANTHROPIC_BASE_URL"],
            json!("http://127.0.0.1:11437")
        );
        assert_eq!(v["env"]["ANTHROPIC_MODEL"], json!("my-model"));
        assert!(endpoint_drift_in(&r, &gw(11437), &[11435, 11437]).is_empty());

        // The user moves the address to the next port themselves: nothing is left to fix.
        edit(&|v| v["env"]["ANTHROPIC_BASE_URL"] = json!("http://127.0.0.1:11439"));
        let done = follow_gateway_port_in(&r, &gw(11439), &[11435, 11437, 11439]);
        assert!(done.moved.is_empty() && done.pending.is_empty(), "{done:?}");
        assert!(endpoint_drift_in(&r, &gw(11439), &[11435, 11437, 11439]).is_empty());

        // The user adds a real key: the config is left for "Düzelt", the key is not replaced.
        edit(&|v| v["env"]["ANTHROPIC_AUTH_TOKEN"] = json!("sk-user-real"));
        let done = follow_gateway_port_in(&r, &gw(11438), &[11435, 11437, 11439, 11438]);
        assert!(done.moved.is_empty(), "{done:?}");
        assert_eq!(done.pending, ["claude-code"]);
        assert_eq!(read()["env"]["ANTHROPIC_AUTH_TOKEN"], json!("sk-user-real"));
    }

    #[test]
    fn undo_takes_out_an_address_moved_in_a_config_the_user_edited() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let gw = |port: u16| format!("http://127.0.0.1:{port}/v1");
        let mut req = request("qwen3-4b", &[]);
        req.base_url = gw(11435);
        apply_in(&r, "claude-code", &req).unwrap();
        let path = crate::integrations::claude_settings(&r).unwrap();
        let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        v["env"]["ANTHROPIC_MODEL"] = json!("my-model");
        std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
        let done = follow_gateway_port_in(&r, &gw(11437), &[11435, 11437]);
        assert_eq!(done.moved, ["claude-code"]);

        crate::integrations::revert_in(&r, "claude-code").unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(v["env"].get("ANTHROPIC_BASE_URL").is_none(), "{v}");
        assert_eq!(v["env"]["ANTHROPIC_MODEL"], json!("my-model"));
    }

    #[test]
    fn single_model_tools_and_unconfigured_tools() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        // Nothing written yet: nothing reported.
        assert!(detect_in(&r, &valid(&["a"])).is_empty());
        apply_in(&r, "claude-code", &request("old-alias", &[])).unwrap();
        let d = detect_in(&r, &valid(&["old-alias-v2"]));
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].id, "claude-code");
        assert_eq!(d[0].missing, ["old-alias"]);
        assert_eq!(d[0].suggestion.as_deref(), Some("old-alias-v2"));
        assert!(detect_in(&r, &valid(&["OLD-ALIAS"])).is_empty());
    }

    // --- configs without a stored request (written before Harness remembered it) ---

    fn gw(port: u16) -> String {
        format!("http://127.0.0.1:{port}/v1")
    }

    /// An `opencode.json` as an older Harness wrote it (and a provider of the user's).
    fn old_opencode(dir: &std::path::Path, harness_url: &str, key: &str) -> std::path::PathBuf {
        let path = crate::integrations::opencode_config_path(dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            format!(
                r#"{{
  // my settings
  "$schema": "https://opencode.ai/config.json",
  "model": "harness/qwen3-4b",
  "provider": {{
    "mine": {{ "options": {{ "baseURL": "http://127.0.0.1:11435/v1" }} }},
    "harness": {{
      "npm": "@ai-sdk/openai-compatible",
      "name": "Harness Studio",
      "options": {{ "baseURL": "{harness_url}", "apiKey": "{key}" }},
      "models": {{ "qwen3-4b": {{ "name": "qwen3-4b (Harness Studio)" }} }}
    }}
  }}
}}
"#
            ),
        )
        .unwrap();
        path
    }

    #[test]
    fn an_old_gateway_port_in_a_file_without_a_request_is_rewritten_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let path = old_opencode(dir.path(), &gw(11435), "harness-local");

        let d = endpoint_drift_in(&r, &gw(11436), &[11435, 11436]);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].id, "opencode");
        assert_eq!(d[0].configured, gw(11435));

        let done = follow_gateway_port_in(&r, &gw(11436), &[11435, 11436]);
        assert_eq!(done.moved, ["opencode"]);
        assert!(done.pending.is_empty(), "{done:?}");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains(r#""baseURL": "http://127.0.0.1:11436/v1", "apiKey""#),
            "{text}"
        );
        // The user's own provider on the same port, the comment and the model stay.
        assert!(
            text.contains(r#""mine": { "options": { "baseURL": "http://127.0.0.1:11435/v1" } }"#),
            "{text}"
        );
        assert!(text.contains("// my settings"), "{text}");
        assert!(text.contains(r#""model": "harness/qwen3-4b""#), "{text}");
        // One backup of the file as it was.
        let backups = keyed::backups_of(&path);
        assert_eq!(backups.len(), 1, "{backups:?}");
        assert!(std::fs::read_to_string(&backups[0])
            .unwrap()
            .contains(r#""baseURL": "http://127.0.0.1:11435/v1", "apiKey""#));
        // Nothing left to do, and a second pass changes nothing.
        assert!(endpoint_drift_in(&r, &gw(11436), &[11435, 11436]).is_empty());
        assert_eq!(
            follow_gateway_port_in(&r, &gw(11436), &[11435, 11436]),
            PortFollow::default()
        );
        assert_eq!(keyed::backups_of(&path).len(), 1);
    }

    #[test]
    fn a_port_this_gateway_never_used_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        // The app's config on the default port; another instance's gateway only ever ran on 11535.
        let path = old_opencode(dir.path(), &gw(11435), "harness-local");
        let before = std::fs::read_to_string(&path).unwrap();
        let done = follow_history_in(&r, &gw(11535), &[11535]);
        assert!(done.moved.is_empty(), "{done:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        assert!(keyed::backups_of(&path).is_empty());
    }

    #[test]
    fn the_gateway_back_on_its_default_port_brings_the_configs_back() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let path = old_opencode(dir.path(), &gw(11436), "harness-local");
        let done = follow_gateway_port_in(&r, &gw(11435), &[11435, 11436]);
        assert_eq!(done.moved, ["opencode"]);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains(r#""baseURL": "http://127.0.0.1:11435/v1", "apiKey""#),
            "{text}"
        );
    }

    #[test]
    fn a_user_address_on_another_port_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        // Harness's provider block pointing at a port the gateway never used, and at a LAN host.
        for url in ["http://127.0.0.1:9000/v1", "http://192.168.1.5:11435/v1"] {
            let path = old_opencode(dir.path(), url, "harness-local");
            let before = std::fs::read_to_string(&path).unwrap();
            assert!(endpoint_drift_in(&r, &gw(11436), &[11435, 11436]).is_empty());
            assert_eq!(
                follow_gateway_port_in(&r, &gw(11436), &[11435, 11436]),
                PortFollow::default()
            );
            assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
            assert!(keyed::backups_of(&path).is_empty());
        }
    }

    #[test]
    fn a_config_with_a_key_is_reported_but_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let path = old_opencode(dir.path(), &gw(11435), "sk-secret");
        let before = std::fs::read_to_string(&path).unwrap();

        let d = endpoint_drift_in(&r, &gw(11436), &[11435, 11436]);
        assert_eq!(d.len(), 1, "{d:?}");
        let done = follow_gateway_port_in(&r, &gw(11436), &[11435, 11436]);
        assert!(done.moved.is_empty(), "{done:?}");
        assert_eq!(done.pending, ["opencode"]);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        assert!(keyed::backups_of(&path).is_empty());
    }

    #[test]
    fn codex_toml_without_a_request_is_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let codex = dir.path().join(".codex");
        std::fs::create_dir_all(&codex).unwrap();
        std::fs::write(codex.join("harness.config.toml"), "model = \"qwen3-4b\"\n").unwrap();
        std::fs::write(
            codex.join("config.toml"),
            "# mine\nmodel = \"gpt\"\n\n[model_providers.harness]\nname = \"Harness Studio\"\nbase_url = \"http://127.0.0.1:11435/v1\"\nwire_api = \"responses\"\n",
        )
        .unwrap();
        let done = follow_gateway_port_in(&r, &gw(11436), &[11435, 11436]);
        assert_eq!(done.moved, ["codex"]);
        let text = std::fs::read_to_string(codex.join("config.toml")).unwrap();
        assert!(
            text.contains("base_url = \"http://127.0.0.1:11436/v1\""),
            "{text}"
        );
        assert!(text.starts_with("# mine\nmodel = \"gpt\""), "{text}");
    }

    #[test]
    fn rollback_still_works_after_an_in_place_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let mut req = request("qwen3-4b", &[]);
        req.base_url = gw(11435);
        apply_in(&r, "opencode", &req).unwrap();
        // An entry from before the request was remembered.
        let mut ledger = keyed::Ledger::load(r.ledger());
        ledger.entries[0].request = None;
        ledger.save(r.ledger()).unwrap();

        let done = follow_gateway_port_in(&r, &gw(11436), &[11435, 11436]);
        assert_eq!(done.moved, ["opencode"]);
        let path = crate::integrations::opencode_config_path(dir.path());
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains(":11436/v1"));

        let out = crate::integrations::revert_in(&r, "opencode").unwrap();
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
        let after = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(!after.contains("harness"), "{after}");
    }
}
