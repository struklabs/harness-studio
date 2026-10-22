//! Secret storage behind a small abstraction: the OS keychain in production,
//! an in-memory map in unit tests. Nothing secret is ever written to JSON files.

use std::collections::HashMap;
use std::sync::Mutex;

const SERVICE: &str = "com.harnessstudio.app";
static SERVICE_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Desktop test builds have their own credential namespace as well as their own data root.
pub fn init_service(identifier: &str) {
    let _ = SERVICE_OVERRIDE.set(identifier.to_string());
}

fn service() -> &'static str {
    SERVICE_OVERRIDE.get().map_or(SERVICE, String::as_str)
}
const HF_ACCOUNT: &str = "huggingface_token";
/// OAuth session of "Sign in with Hugging Face" (JSON: tokens, expiry, client id, profile).
const HF_OAUTH_ACCOUNT: &str = "huggingface_oauth";
const API_KEY_PREFIX: &str = "server-api-key:";
const MCP_PREFIX: &str = "mcp-secret:";
const LINK_PREFIX: &str = "harness-link:";
const WEB_SEARCH_PREFIX: &str = "web-search:";
/// API key of the user's own n8n instance (Settings > Automations).
const N8N_ACCOUNT: &str = "n8n-api-key";
/// Board API key of the user's own Paperclip instance.
const PAPERCLIP_ACCOUNT: &str = "paperclip-api-key";
/// OAuth session of a remote MCP server (`mcp-oauth:<server name>`).
const MCP_OAUTH_PREFIX: &str = "mcp-oauth:";
/// API key of a BYOK cloud provider (`cloud-provider:[<datadir-hash>:]<id>`).
const CLOUD_PREFIX: &str = "cloud-provider:";

/// OAuth session of a connector (`connector:[<datadir-hash>:]<id>`).
const CONNECTOR_PREFIX: &str = "connector:";
/// Optional OAuth client secret of a connector family (`connector-client:[<hash>:]<family>`).
const CONNECTOR_CLIENT_PREFIX: &str = "connector-client:";

pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    fn set(&self, account: &str, value: &str) -> Result<(), String>;
    fn delete(&self, account: &str) -> Result<(), String>;
}

/// OS keychain (Windows Credential Manager / macOS Keychain / Secret Service).
pub struct KeyringStore;

impl KeyringStore {
    fn entry(account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(service(), account).map_err(|e| e.to_string())
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        match Self::entry(account)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        // Record the account before writing, so even a crash cannot leave an untracked secret.
        // No secret values go into this ledger. Keep deleted accounts for idempotent cleanup.
        static LEDGER: Mutex<()> = Mutex::new(());
        let _guard = LEDGER.lock().map_err(|e| e.to_string())?;
        let ledger =
            crate::store::JsonStore::new(crate::paths::config_dir().join("secret-accounts.json"));
        let mut accounts: Vec<String> = ledger.load_or_default();
        if !accounts.iter().any(|a| a == account) {
            accounts.push(account.to_string());
            ledger.save(&accounts)?;
        }
        Self::entry(account)?
            .set_password(value)
            .map_err(|e| e.to_string())
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Removes credentials before deleting settings. Windows also enumerates old/orphaned
/// accounts from versions predating the ledger, using the exact keyring service suffix.
pub fn delete_all_owned() -> Result<(), String> {
    let mut accounts: Vec<String> =
        crate::store::JsonStore::new(crate::paths::config_dir().join("secret-accounts.json"))
            .load_or_default();
    accounts.extend([HF_ACCOUNT, HF_OAUTH_ACCOUNT, OTEL_HEADERS_ACCOUNT].map(str::to_string));
    // Legacy accounts still referenced by saved settings, profiles and presets.
    fn refs(value: &serde_json::Value, accounts: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, value) in map {
                    if key == "api_key_ref" {
                        if let Some(r) = value.as_str().filter(|r| valid_ref(r)) {
                            accounts.push(api_account(r));
                        }
                    }
                    refs(value, accounts);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    refs(value, accounts);
                }
            }
            _ => {}
        }
    }
    if let Ok(entries) = std::fs::read_dir(crate::paths::config_dir()) {
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|e| e == "json") {
                if let Ok(text) = std::fs::read_to_string(entry.path()) {
                    if let Ok(value) = serde_json::from_str(&text) {
                        refs(&value, &mut accounts);
                    }
                }
            }
        }
    }
    let settings = crate::config::AppSettings::load();
    for link in &settings.links {
        accounts.push(link_account(&link.id));
    }
    for provider in ["brave", "tavily", "exa", "serper"] {
        accounts.push(format!("{WEB_SEARCH_PREFIX}{provider}"));
    }
    // Integration keys of this data directory (the plain names in the default one).
    for legacy in [N8N_ACCOUNT, PAPERCLIP_ACCOUNT] {
        accounts.push(scoped_key_account(legacy));
    }
    #[cfg(windows)]
    if MEMORY_OVERRIDE.get().is_none() && !cfg!(test) {
        accounts.extend(windows_accounts()?);
    }
    accounts.sort();
    accounts.dedup();
    for account in accounts {
        store().delete(&account)?;
    }
    Ok(())
}

#[cfg(windows)]
fn windows_accounts() -> Result<Vec<String>, String> {
    use windows::Win32::Security::Credentials::{CredEnumerateW, CredFree, CREDENTIALW};
    let mut count = 0;
    let mut credentials: *mut *mut CREDENTIALW = std::ptr::null_mut();
    let result = unsafe { CredEnumerateW(None, None, &mut count, &mut credentials) };
    if let Err(e) = result {
        if e.code().0 as u32 == 0x80070490 {
            return Ok(Vec::new());
        }
        return Err(e.to_string());
    }
    let mut accounts = Vec::new();
    let suffix = format!(".{}", service());
    // The API owns this allocation until CredFree; no credential values are read.
    unsafe {
        for credential in std::slice::from_raw_parts(credentials, count as usize) {
            if let Ok(target) = (**credential).TargetName.to_string() {
                if let Some(account) = target.strip_suffix(&suffix) {
                    accounts.push(account.to_string());
                }
            }
        }
        CredFree(credentials.cast());
    }
    Ok(accounts)
}

/// In-memory store (unit tests; integration tests and isolated instances via
/// [`use_memory_store`]).
#[derive(Default)]
pub struct MemoryStore {
    map: Mutex<HashMap<String, String>>,
}

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        Ok(self.map.lock().unwrap().get(account).cloned())
    }

    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        self.map
            .lock()
            .unwrap()
            .insert(account.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        self.map.lock().unwrap().remove(account);
        Ok(())
    }
}

static MEMORY_OVERRIDE: std::sync::OnceLock<MemoryStore> = std::sync::OnceLock::new();

/// Switches this process to an in-memory secret store: nothing is read from or written to
/// the OS keychain afterwards. For integration tests (and other isolated instances) that must
/// never touch the user's real keychain entries. Cannot be undone.
pub fn use_memory_store() {
    MEMORY_OVERRIDE.get_or_init(MemoryStore::default);
}

#[cfg(not(test))]
pub fn store() -> &'static dyn SecretStore {
    static STORE: KeyringStore = KeyringStore;
    match MEMORY_OVERRIDE.get() {
        Some(memory) => memory,
        None => &STORE,
    }
}

#[cfg(test)]
pub fn store() -> &'static MemoryStore {
    static STORE: std::sync::OnceLock<MemoryStore> = std::sync::OnceLock::new();
    STORE.get_or_init(MemoryStore::default)
}

// --- Hugging Face token ---

pub fn hf_get() -> Option<String> {
    store().get(HF_ACCOUNT).ok().flatten()
}

pub fn hf_set(token: &str) -> Result<(), String> {
    store().set(HF_ACCOUNT, token).map_err(|e| {
        crate::error_key::err("secrets.hf_save_failed")
            .with("error", e)
            .into()
    })
}

pub fn hf_delete() -> Result<(), String> {
    store().delete(HF_ACCOUNT)
}

fn hf_read_failed(e: String) -> String {
    crate::error_key::err("secrets.hf_read_failed")
        .with("error", e)
        .into()
}

/// The saved token; `Err` (`secrets.hf_read_failed`) when the keychain cannot be read, which
/// is not the same as "no token".
pub fn hf_try_get() -> Result<Option<String>, String> {
    store()
        .get(HF_ACCOUNT)
        .map(|t| t.filter(|t| !t.trim().is_empty()))
        .map_err(hf_read_failed)
}

pub fn hf_oauth_get() -> Result<Option<String>, String> {
    store().get(HF_OAUTH_ACCOUNT).map_err(hf_read_failed)
}

pub fn hf_oauth_set(session_json: &str) -> Result<(), String> {
    store().set(HF_OAUTH_ACCOUNT, session_json).map_err(|e| {
        crate::error_key::err("secrets.hf_save_failed")
            .with("error", e)
            .into()
    })
}

pub fn hf_oauth_delete() -> Result<(), String> {
    store().delete(HF_OAUTH_ACCOUNT).map_err(|e| {
        crate::error_key::err("secrets.hf_delete_failed")
            .with("error", e)
            .into()
    })
}

// --- llama-server API keys ---
//
// A key lives under the account `server-api-key:<ref>`. JSON files (last session,
// profiles, model presets) only carry the `<ref>` string, never the key.

pub fn valid_ref(r: &str) -> bool {
    !r.is_empty()
        && r.len() <= 100
        && r.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
}

fn api_account(r: &str) -> String {
    format!("{}{}", API_KEY_PREFIX, r)
}

/// The saved key under `r`; `Err` (`secrets.api_key_read_failed`) when the keychain cannot be
/// read (e.g. no Secret Service on Linux, or it is locked). Unlike [`api_key_get`] a failed
/// read is not the same as "no key": a server must not silently start without its key.
pub fn api_key_try_get(r: &str) -> Result<Option<String>, String> {
    if !valid_ref(r) {
        return Ok(None);
    }
    store()
        .get(&api_account(r))
        .map(|k| k.filter(|k| !k.is_empty()))
        .map_err(|e| {
            crate::error_key::err("secrets.api_key_read_failed")
                .with("error", e)
                .into()
        })
}

pub fn api_key_get(r: &str) -> Option<String> {
    if !valid_ref(r) {
        return None;
    }
    store()
        .get(&api_account(r))
        .ok()
        .flatten()
        .filter(|k| !k.is_empty())
}

pub fn api_key_delete(r: &str) {
    if valid_ref(r) {
        let _ = store().delete(&api_account(r));
    }
}

/// Reference for a model preset (the model id is a path, so it is hashed).
pub fn preset_ref(model_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(model_id.as_bytes());
    let hex: String = digest
        .iter()
        .take(12)
        .map(|b| format!("{:02x}", b))
        .collect();
    format!("preset:{}", hex)
}

pub fn profile_ref(profile_id: &str) -> String {
    format!("profile:{}", profile_id)
}

pub const SESSION_REF: &str = "session";

/// Applies an incoming (frontend or legacy-file) API key to the owner `owner_ref` and returns
/// the reference to persist in JSON.
///
/// * `new_key` non-empty: stored under `owner_ref`.
/// * `new_key` empty string: the key is cleared.
/// * `new_key` None: the existing key is kept; if it is referenced through another owner
///   (e.g. a profile saved from a preset-derived form) its value is copied to `owner_ref`.
pub fn adopt_api_key(
    owner_ref: &str,
    new_key: Option<&str>,
    incoming_ref: Option<&str>,
) -> Result<Option<String>, String> {
    let fail = |e: String| -> String {
        crate::error_key::err("secrets.api_key_save_failed")
            .with("error", e)
            .into()
    };
    match new_key.map(str::trim) {
        Some("") => {
            api_key_delete(owner_ref);
            Ok(None)
        }
        Some(key) => {
            store().set(&api_account(owner_ref), key).map_err(fail)?;
            Ok(Some(owner_ref.to_string()))
        }
        None => match incoming_ref.filter(|r| valid_ref(r)) {
            Some(r) if r == owner_ref => Ok(api_key_try_get(r)?.map(|_| owner_ref.to_string())),
            Some(r) => match api_key_try_get(r)? {
                Some(key) => {
                    store().set(&api_account(owner_ref), &key).map_err(fail)?;
                    Ok(Some(owner_ref.to_string()))
                }
                None => {
                    api_key_delete(owner_ref);
                    Ok(None)
                }
            },
            None => {
                api_key_delete(owner_ref);
                Ok(None)
            }
        },
    }
}

// --- OpenTelemetry export headers (e.g. an observability vendor's API key) ---

const OTEL_HEADERS_ACCOUNT: &str = "otel-headers";

/// The OTLP export headers (`name=value` lines), `Ok(None)` when none are saved.
pub fn otel_headers_get() -> Result<Option<String>, String> {
    store().get(OTEL_HEADERS_ACCOUNT)
}

pub fn otel_headers_set(value: &str) -> Result<(), String> {
    store().set(OTEL_HEADERS_ACCOUNT, value).map_err(|e| {
        crate::error_key::err("secrets.otel_save_failed")
            .with("error", e)
            .into()
    })
}

pub fn otel_headers_delete() -> Result<(), String> {
    store().delete(OTEL_HEADERS_ACCOUNT)
}

// --- MCP secrets (env values / HTTP header values) ---

pub(crate) fn mcp_account(server: &str, kind: &str, key: &str) -> String {
    format!("{}{server}:{kind}:{key}", MCP_PREFIX)
}

pub fn mcp_get(name: &str) -> Option<String> {
    store()
        .get(&format!("{}{}", MCP_PREFIX, name))
        .ok()
        .flatten()
}

pub fn mcp_set(name: &str, value: &str) -> Result<(), String> {
    store()
        .set(&format!("{}{}", MCP_PREFIX, name), value)
        .map_err(|e| {
            crate::error_key::err("secrets.mcp_save_failed")
                .with("error", e)
                .into()
        })
}

pub fn mcp_delete(name: &str) {
    let _ = store().delete(&format!("{}{}", MCP_PREFIX, name));
}

// --- MCP OAuth sessions (remote servers behind the MCP authorization flow) ---

pub(crate) fn mcp_oauth_account(server: &str) -> String {
    format!("{MCP_OAUTH_PREFIX}{server}")
}

/// The OAuth session (JSON: tokens, client, endpoints) of one MCP server, if signed in.
pub fn mcp_oauth_get(server: &str) -> Result<Option<String>, String> {
    store().get(&mcp_oauth_account(server))
}

pub fn mcp_oauth_set(server: &str, session_json: &str) -> Result<(), String> {
    store()
        .set(&mcp_oauth_account(server), session_json)
        .map_err(|e| {
            crate::error_key::err("secrets.mcp_save_failed")
                .with("error", e)
                .into()
        })
}

pub fn mcp_oauth_delete(server: &str) {
    let _ = store().delete(&mcp_oauth_account(server));
}

pub fn web_search_key_get(provider: &str) -> Result<Option<String>, String> {
    store()
        .get(&format!("{WEB_SEARCH_PREFIX}{provider}"))
        .map(|v| v.filter(|v| !v.trim().is_empty()))
        .map_err(|e| format!("could not read the web search key from the OS keychain: {e}"))
}

pub fn web_search_key_set(provider: &str, value: Option<&str>) -> Result<(), String> {
    let account = format!("{WEB_SEARCH_PREFIX}{provider}");
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(value) => store().set(&account, value),
        None => store().delete(&account),
    }
    .map_err(|e| format!("could not update the web search key in the OS keychain: {e}"))
}

/// The account of a key kept per data directory: the plain `legacy` name for the real app's
/// default directory, a hashed one for any other (isolated instances, tests).
fn scoped_key_account(legacy: &str) -> String {
    scoped_account("", legacy)
}

/// Legacy accounts this data directory has already taken over (or set or cleared itself).
fn scoped_ledger() -> crate::store::JsonStore {
    crate::store::JsonStore::new(crate::paths::config_dir().join("scoped-secrets.json"))
}

fn scoped_done(legacy: &str) -> bool {
    scoped_ledger()
        .load_or_default::<Vec<String>>()
        .iter()
        .any(|a| a == legacy)
}

fn mark_scoped(legacy: &str) -> Result<(), String> {
    let store = scoped_ledger();
    let mut done: Vec<String> = store.load_or_default();
    if !done.iter().any(|a| a == legacy) {
        done.push(legacy.to_string());
        store.save(&done)?;
    }
    Ok(())
}

/// Reads a per-data-directory key. Older versions kept it under one `legacy` account for every
/// data directory; another directory that has the integration configured copies it into its own
/// account once. The original stays: it belongs to the default directory (or whichever wrote
/// it). After that (or after this directory set or cleared the key) only its own account counts.
fn scoped_key_get(
    legacy: &str,
    configured: impl FnOnce() -> bool,
) -> Result<Option<String>, String> {
    let account = scoped_key_account(legacy);
    let value = store().get(&account)?;
    if value.is_some() || account == legacy || scoped_done(legacy) || !configured() {
        return Ok(value);
    }
    let old = store().get(legacy)?;
    if let Some(key) = old.as_deref().filter(|k| !k.trim().is_empty()) {
        store().set(&account, key)?;
    }
    mark_scoped(legacy)?;
    Ok(old)
}

fn scoped_key_set(legacy: &str, value: Option<&str>) -> Result<(), String> {
    let account = scoped_key_account(legacy);
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(value) => store().set(&account, value),
        None => store().delete(&account),
    }?;
    if account != legacy {
        mark_scoped(legacy)?;
    }
    Ok(())
}

pub fn n8n_key_get() -> Result<Option<String>, String> {
    scoped_key_get(N8N_ACCOUNT, || {
        !crate::config::AppSettings::load().n8n.url.is_empty()
    })
    .map(|v| v.filter(|v| !v.trim().is_empty()))
    .map_err(|e| {
        crate::error_key::err("automations.keychain_unavailable")
            .with("error", e)
            .into()
    })
}

/// Stores the n8n API key; `None` or an empty value removes it.
pub fn n8n_key_set(value: Option<&str>) -> Result<(), String> {
    scoped_key_set(N8N_ACCOUNT, value).map_err(|e| {
        crate::error_key::err("automations.keychain_unavailable")
            .with("error", e)
            .into()
    })
}

pub fn paperclip_key_get() -> Result<Option<String>, String> {
    scoped_key_get(PAPERCLIP_ACCOUNT, || {
        !crate::config::AppSettings::load().paperclip.url.is_empty()
    })
    .map(|v| v.filter(|v| !v.trim().is_empty()))
    .map_err(|e| {
        crate::error_key::err("paperclip.keychain_unavailable")
            .with("error", e)
            .into()
    })
}

/// Stores the Paperclip key; `None` or an empty value removes it.
pub fn paperclip_key_set(value: Option<&str>) -> Result<(), String> {
    scoped_key_set(PAPERCLIP_ACCOUNT, value).map_err(|e| {
        crate::error_key::err("paperclip.keychain_unavailable")
            .with("error", e)
            .into()
    })
}

/// `<prefix><id>` for the real app's default data directory, `<prefix><hash>:<id>` for any
/// other one, so isolated instances never read or overwrite the installed app's entries.
fn scoped_account_for(
    prefix: &str,
    data_dir: &std::path::Path,
    default_dir: Option<&std::path::Path>,
    id: &str,
) -> String {
    use sha2::{Digest, Sha256};
    let canonical = crate::paths::canonicalize_lenient(data_dir);
    // Preserve installed tokens only for the real app's default data directory.
    if default_dir.is_some_and(|dir| crate::paths::canonicalize_lenient(dir) == canonical) {
        return format!("{prefix}{id}");
    }
    let digest = Sha256::digest(canonical.as_os_str().as_encoded_bytes());
    let hash: String = digest.iter().take(12).map(|b| format!("{b:02x}")).collect();
    format!("{prefix}{hash}:{id}")
}

#[cfg(test)]
fn link_account_for(
    data_dir: &std::path::Path,
    default_dir: Option<&std::path::Path>,
    id: &str,
) -> String {
    scoped_account_for(LINK_PREFIX, data_dir, default_dir, id)
}

fn scoped_account(prefix: &str, id: &str) -> String {
    let default_dir = dirs::data_dir().map(|dir| dir.join(SERVICE));
    scoped_account_for(
        prefix,
        &crate::paths::data_root(),
        default_dir.as_deref(),
        id,
    )
}

/// Stable per-data-directory identity for encrypted remote app connections.
pub fn remote_noise_account() -> String {
    scoped_account("remote-noise:", "static-v1")
}

fn link_account(id: &str) -> String {
    scoped_account(LINK_PREFIX, id)
}

pub fn link_get(id: &str) -> Result<Option<String>, String> {
    store()
        .get(&link_account(id))
        .map_err(|_| crate::error_key::err("link.keychain_unavailable").into())
}

pub fn link_set(id: &str, token: &str) -> Result<(), String> {
    store()
        .set(&link_account(id), token)
        .map_err(|_| crate::error_key::err("link.keychain_unavailable").into())
}

pub fn link_delete(id: &str) -> Result<(), String> {
    store()
        .delete(&link_account(id))
        .map_err(|_| crate::error_key::err("link.keychain_unavailable").into())
}

// --- Cloud provider API keys (BYOK) ---

fn cloud_account(id: &str) -> String {
    scoped_account(CLOUD_PREFIX, id)
}

pub fn cloud_key_get(id: &str) -> Result<Option<String>, String> {
    store()
        .get(&cloud_account(id))
        .map(|k| k.filter(|k| !k.trim().is_empty()))
        .map_err(|_| crate::error_key::err("cloud.keychain_unavailable").into())
}

pub fn cloud_key_set(id: &str, key: &str) -> Result<(), String> {
    store()
        .set(&cloud_account(id), key)
        .map_err(|_| crate::error_key::err("cloud.keychain_unavailable").into())
}

pub fn cloud_key_delete(id: &str) -> Result<(), String> {
    store()
        .delete(&cloud_account(id))
        .map_err(|_| crate::error_key::err("cloud.keychain_unavailable").into())
}

// --- Connectors (Gmail, Google Drive, Calendar, Excel via Microsoft Graph) ---

fn connector_unavailable<E>(_: E) -> String {
    crate::error_key::err("connector.keychain_unavailable").into()
}

/// The OAuth session (JSON: tokens, expiry, scopes) of one connector, if connected.
pub fn connector_session_get(id: &str) -> Result<Option<String>, String> {
    store()
        .get(&scoped_account(CONNECTOR_PREFIX, id))
        .map_err(connector_unavailable)
}

pub fn connector_session_set(id: &str, session_json: &str) -> Result<(), String> {
    store()
        .set(&scoped_account(CONNECTOR_PREFIX, id), session_json)
        .map_err(connector_unavailable)
}

pub fn connector_session_delete(id: &str) -> Result<(), String> {
    store()
        .delete(&scoped_account(CONNECTOR_PREFIX, id))
        .map_err(connector_unavailable)
}

/// The OAuth client secret the user saved for a connector family (Google desktop clients have
/// one even with PKCE), `Ok(None)` when there is none.
pub fn connector_client_secret_get(family: &str) -> Result<Option<String>, String> {
    store()
        .get(&scoped_account(CONNECTOR_CLIENT_PREFIX, family))
        .map(|v| v.filter(|v| !v.trim().is_empty()))
        .map_err(connector_unavailable)
}

/// Saves the client secret; `None` or an empty value removes it.
pub fn connector_client_secret_set(family: &str, value: Option<&str>) -> Result<(), String> {
    let account = scoped_account(CONNECTOR_CLIENT_PREFIX, family);
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(value) => store().set(&account, value),
        None => store().delete(&account),
    }
    .map_err(connector_unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_tokens_are_isolated_by_canonical_data_dir() {
        let root = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let default_dir = root.path().join("default");
        let other_dir = root.path().join("rc");
        let isolated_dir = root.path().join("isolated");
        std::fs::create_dir_all(&default_dir).unwrap();
        std::fs::create_dir_all(&other_dir).unwrap();
        let account = |dir: &std::path::Path| link_account_for(dir, Some(&default_dir), "same-id");
        let default = account(&default_dir);
        let other = account(&other_dir);
        let isolated = account(&isolated_dir);
        assert_eq!(default, "harness-link:same-id");
        assert_ne!(default, other);
        assert_ne!(other, isolated);
        assert_eq!(other, account(&other_dir.join(".")));
        assert_eq!(other, account(&root.path().join("default/../rc")));
        let store = MemoryStore::default();
        store.set(&default, "default-token").unwrap();
        assert_eq!(store.get(&other).unwrap(), None);
        store.set(&other, "rc-token").unwrap();
        assert_eq!(store.get(&isolated).unwrap(), None);
        store.set(&isolated, "isolated-token").unwrap();
        assert_eq!(
            store.get(&default).unwrap().as_deref(),
            Some("default-token")
        );
        assert_eq!(store.get(&other).unwrap().as_deref(), Some("rc-token"));
        store.delete(&other).unwrap();
        assert_eq!(store.get(&other).unwrap(), None);
        assert_eq!(
            store.get(&isolated).unwrap().as_deref(),
            Some("isolated-token")
        );
        assert_eq!(
            store.get(&default).unwrap().as_deref(),
            Some("default-token")
        );
    }

    #[test]
    fn integration_keys_are_scoped_and_taken_over_once() {
        // Tests run in a temp data directory, never the default one: the account is hashed.
        let legacy = "t-scoped-integration-key";
        let account = scoped_key_account(legacy);
        assert_ne!(account, legacy);
        store().set(legacy, "shared").unwrap();
        // Not configured here: the shared key is neither read nor copied.
        assert_eq!(scoped_key_get(legacy, || false).unwrap(), None);
        // Configured: copied once, the original stays for the directory that wrote it.
        assert_eq!(
            scoped_key_get(legacy, || true).unwrap().as_deref(),
            Some("shared")
        );
        assert_eq!(store().get(&account).unwrap().as_deref(), Some("shared"));
        assert_eq!(store().get(legacy).unwrap().as_deref(), Some("shared"));
        // A key this directory clears does not come back from the shared account.
        scoped_key_set(legacy, None).unwrap();
        assert_eq!(scoped_key_get(legacy, || true).unwrap(), None);
        scoped_key_set(legacy, Some("own")).unwrap();
        assert_eq!(
            scoped_key_get(legacy, || true).unwrap().as_deref(),
            Some("own")
        );
        assert_eq!(store().get(legacy).unwrap().as_deref(), Some("shared"));
        store().delete(legacy).unwrap();
    }

    #[test]
    fn adopt_sets_clears_and_keeps() {
        let owner = "profile:t-adopt";
        assert_eq!(
            adopt_api_key(owner, Some("k1"), None).unwrap().as_deref(),
            Some(owner)
        );
        assert_eq!(api_key_get(owner).as_deref(), Some("k1"));
        // None + same ref keeps
        assert_eq!(
            adopt_api_key(owner, None, Some(owner)).unwrap().as_deref(),
            Some(owner)
        );
        assert_eq!(api_key_get(owner).as_deref(), Some("k1"));
        // None + other ref copies
        let other = "profile:t-adopt-2";
        assert_eq!(
            adopt_api_key(other, None, Some(owner)).unwrap().as_deref(),
            Some(other)
        );
        assert_eq!(api_key_get(other).as_deref(), Some("k1"));
        // empty string clears
        assert_eq!(adopt_api_key(owner, Some(""), Some(owner)).unwrap(), None);
        assert!(api_key_get(owner).is_none());
        // None without a ref clears (no key)
        assert_eq!(adopt_api_key(other, None, None).unwrap(), None);
        assert!(api_key_get(other).is_none());
    }

    #[test]
    fn ref_validation() {
        assert!(valid_ref("profile:abc-1"));
        assert!(!valid_ref(""));
        assert!(!valid_ref("a b"));
        assert!(!valid_ref(&"a".repeat(101)));
        assert!(preset_ref("C:\\models\\x.gguf").starts_with("preset:"));
    }
}
