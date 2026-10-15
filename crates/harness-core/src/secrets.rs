//! Secret storage behind a small abstraction: the OS keychain in production,
//! an in-memory map in unit tests. Nothing secret is ever written to JSON files.

use std::collections::HashMap;
use std::sync::Mutex;

const SERVICE: &str = "com.harnessstudio.app";
const HF_ACCOUNT: &str = "huggingface_token";
/// OAuth session of "Sign in with Hugging Face" (JSON: tokens, expiry, client id, profile).
const HF_OAUTH_ACCOUNT: &str = "huggingface_oauth";
const API_KEY_PREFIX: &str = "server-api-key:";
const MCP_PREFIX: &str = "mcp-secret:";
const LINK_PREFIX: &str = "harness-link:";
const WEB_SEARCH_PREFIX: &str = "web-search:";
/// OAuth session of a remote MCP server (`mcp-oauth:<server name>`).
const MCP_OAUTH_PREFIX: &str = "mcp-oauth:";

pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    fn set(&self, account: &str, value: &str) -> Result<(), String>;
    fn delete(&self, account: &str) -> Result<(), String>;
}

/// OS keychain (Windows Credential Manager / macOS Keychain / Secret Service).
pub struct KeyringStore;

impl KeyringStore {
    fn entry(account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())
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

pub fn link_get(id: &str) -> Result<Option<String>, String> {
    store()
        .get(&format!("{LINK_PREFIX}{id}"))
        .map_err(|_| crate::error_key::err("link.keychain_unavailable").into())
}

pub fn link_set(id: &str, token: &str) -> Result<(), String> {
    store()
        .set(&format!("{LINK_PREFIX}{id}"), token)
        .map_err(|_| crate::error_key::err("link.keychain_unavailable").into())
}

pub fn link_delete(id: &str) -> Result<(), String> {
    store()
        .delete(&format!("{LINK_PREFIX}{id}"))
        .map_err(|_| crate::error_key::err("link.keychain_unavailable").into())
}

#[cfg(test)]
mod tests {
    use super::*;

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
