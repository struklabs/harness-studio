//! Connectors: the user's own Gmail, Google Drive, Google Calendar and Excel (Microsoft Graph)
//! made available to the Playground and agents as scoped tools.
//!
//! * Sign-in is OAuth 2.0 authorization code + PKCE with a loopback redirect ([`oauth`]); the
//!   user consents in the app first and in the provider's own page second.
//! * Tokens live only in the OS keychain (`secrets::connector_*`); `connectors.json` in the
//!   config directory holds the non-secret parts: the OAuth client ids the user registered and
//!   which connector is connected with which access level.
//! * Client ids come from that file (Settings) or from the environment, never from the source:
//!   `HARNESS_GOOGLE_OAUTH_CLIENT_ID` / `HARNESS_MICROSOFT_OAUTH_CLIENT_ID`. Google desktop
//!   clients also have a client secret; it is optional here and kept in the keychain.
//! * Read and search tools need only read scopes. Tools that send or write are registered
//!   only when the user granted write access, and are `Dangerous`, so the tool policy asks the
//!   user before each call.

mod api;
pub mod oauth;
mod tools;

use crate::secrets;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;

pub use tools::{sync, tool_names};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Google,
    Microsoft,
}

impl Family {
    pub fn as_str(self) -> &'static str {
        match self {
            Family::Google => "google",
            Family::Microsoft => "microsoft",
        }
    }

    fn id_env(self) -> &'static str {
        match self {
            Family::Google => "HARNESS_GOOGLE_OAUTH_CLIENT_ID",
            Family::Microsoft => "HARNESS_MICROSOFT_OAUTH_CLIENT_ID",
        }
    }

    fn secret_env(self) -> &'static str {
        match self {
            Family::Google => "HARNESS_GOOGLE_OAUTH_CLIENT_SECRET",
            Family::Microsoft => "HARNESS_MICROSOFT_OAUTH_CLIENT_SECRET",
        }
    }
}

/// One connector: where its OAuth endpoints and API are and which scopes it asks for.
#[derive(Debug, Clone)]
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub family: Family,
    pub auth_url: String,
    pub token_url: String,
    /// Token revocation endpoint; Microsoft has none (the user revokes in their account).
    pub revoke_url: Option<String>,
    pub api_base: String,
    pub read_scopes: Vec<String>,
    pub write_scopes: Vec<String>,
    /// Host of the loopback redirect (`127.0.0.1`, or `localhost` where the provider only
    /// accepts that for public clients).
    pub redirect_host: &'static str,
}

impl Provider {
    pub fn scopes(&self, write: bool) -> Vec<String> {
        let mut scopes = self.read_scopes.clone();
        if write {
            scopes.extend(self.write_scopes.iter().cloned());
        }
        scopes
    }
}

fn google(
    id: &'static str,
    name: &'static str,
    api_base: &str,
    read: &[&str],
    write: &[&str],
) -> Provider {
    let scope = |s: &&str| format!("https://www.googleapis.com/auth/{s}");
    Provider {
        id,
        name,
        family: Family::Google,
        auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
        token_url: "https://oauth2.googleapis.com/token".into(),
        revoke_url: Some("https://oauth2.googleapis.com/revoke".into()),
        api_base: api_base.into(),
        read_scopes: read.iter().map(scope).collect(),
        write_scopes: write.iter().map(scope).collect(),
        redirect_host: "127.0.0.1",
    }
}

/// The connectors Harness ships, in display order.
pub fn builtin_providers() -> Vec<Provider> {
    vec![
        google(
            "gmail",
            "Gmail",
            "https://gmail.googleapis.com",
            &["gmail.readonly"],
            &["gmail.send"],
        ),
        google(
            "drive",
            "Google Drive",
            "https://www.googleapis.com",
            &["drive.readonly"],
            &["drive.file"],
        ),
        google(
            "calendar",
            "Google Calendar",
            "https://www.googleapis.com",
            &["calendar.readonly"],
            &["calendar.events"],
        ),
        Provider {
            id: "excel",
            name: "Excel (Microsoft Graph)",
            family: Family::Microsoft,
            auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".into(),
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token".into(),
            revoke_url: None,
            api_base: "https://graph.microsoft.com".into(),
            read_scopes: vec!["Files.Read".into(), "offline_access".into()],
            write_scopes: vec!["Files.ReadWrite".into()],
            redirect_host: "localhost",
        },
    ]
}

#[cfg(test)]
static TEST_PROVIDERS: Mutex<Vec<Provider>> = Mutex::new(Vec::new());

/// Points a connector at a mock server (tests only).
#[cfg(test)]
pub(crate) fn use_test_provider(p: Provider) {
    let mut all = TEST_PROVIDERS.lock().unwrap();
    all.retain(|x| x.id != p.id);
    all.push(p);
}

pub fn provider(id: &str) -> Option<Provider> {
    #[cfg(test)]
    if let Some(p) = TEST_PROVIDERS
        .lock()
        .unwrap()
        .iter()
        .find(|p| p.id == id)
        .cloned()
    {
        return Some(p);
    }
    builtin_providers().into_iter().find(|p| p.id == id)
}

// --- non-secret settings ---

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Connected {
    /// The user granted write access (send, create, update).
    pub write: bool,
    pub scopes: Vec<String>,
    pub since: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Config {
    google_client_id: String,
    microsoft_client_id: String,
    connected: BTreeMap<String, Connected>,
}

static CONFIG_LOCK: Mutex<()> = Mutex::new(());

fn config_store() -> crate::store::JsonStore {
    crate::store::JsonStore::new(crate::paths::config_dir().join("connectors.json"))
}

fn config() -> Config {
    config_store().load_or_default()
}

fn update_config<R>(f: impl FnOnce(&mut Config) -> R) -> Result<R, String> {
    let _guard = CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    config_store().update(|c: &mut Config| Ok(f(c)))
}

pub(crate) fn connected(id: &str) -> Option<Connected> {
    config().connected.get(id).cloned()
}

pub(crate) fn mark_connected(id: &str, value: Connected) -> Result<(), String> {
    update_config(|c| {
        c.connected.insert(id.to_string(), value);
    })
}

pub(crate) fn mark_disconnected(id: &str) -> Result<(), String> {
    update_config(|c| {
        c.connected.remove(id);
    })
}

pub fn valid_client_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.~@".contains(&b))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientSource {
    Settings,
    Env,
}

/// The OAuth client id: the user's saved one, then the environment.
pub fn client_id(family: Family) -> Option<(String, ClientSource)> {
    let c = config();
    let saved = match family {
        Family::Google => c.google_client_id,
        Family::Microsoft => c.microsoft_client_id,
    };
    if valid_client_id(saved.trim()) {
        return Some((saved.trim().to_string(), ClientSource::Settings));
    }
    std::env::var(family.id_env())
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| valid_client_id(v))
        .map(|v| (v, ClientSource::Env))
}

/// The optional client secret: the keychain, then the environment.
pub fn client_secret(family: Family) -> Result<Option<String>, String> {
    if let Some(secret) = secrets::connector_client_secret_get(family.as_str())? {
        return Ok(Some(secret));
    }
    Ok(std::env::var(family.secret_env())
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty()))
}

/// Saves the client id (empty removes it) and the optional client secret (`None` keeps the
/// current one, an empty string removes it).
pub fn set_client(
    family: Family,
    client_id: &str,
    client_secret: Option<&str>,
) -> Result<(), String> {
    let id = client_id.trim();
    if !id.is_empty() && !valid_client_id(id) {
        return Err("The client ID contains characters an OAuth client ID never has".into());
    }
    if let Some(secret) = client_secret {
        secrets::connector_client_secret_set(family.as_str(), Some(secret))?;
    }
    update_config(|c| match family {
        Family::Google => c.google_client_id = id.to_string(),
        Family::Microsoft => c.microsoft_client_id = id.to_string(),
    })
}

// --- what the screens show ---

#[derive(Debug, Clone, Serialize)]
pub struct ConnectorStatus {
    pub id: &'static str,
    pub name: &'static str,
    pub family: Family,
    pub connected: bool,
    /// Write access granted: send / create / update tools are available (with approval).
    pub write: bool,
    pub scopes: Vec<String>,
    pub read_scopes: Vec<String>,
    pub write_scopes: Vec<String>,
    pub client_id_set: bool,
    pub client_source: Option<ClientSource>,
    /// The redirect URI to register on the OAuth client (the port is chosen per sign-in).
    pub redirect_uri: String,
    pub revocable_here: bool,
    pub tools: Vec<String>,
}

pub fn status() -> Vec<ConnectorStatus> {
    let cfg = config();
    builtin_providers()
        .into_iter()
        .map(|p| {
            let conn = cfg.connected.get(p.id);
            let client = client_id(p.family);
            ConnectorStatus {
                id: p.id,
                name: p.name,
                family: p.family,
                connected: conn.is_some(),
                write: conn.is_some_and(|c| c.write),
                scopes: conn.map(|c| c.scopes.clone()).unwrap_or_default(),
                read_scopes: p.read_scopes.clone(),
                write_scopes: p.write_scopes.clone(),
                client_id_set: client.is_some(),
                client_source: client.map(|(_, s)| s),
                redirect_uri: format!("http://{}/callback", p.redirect_host),
                revocable_here: p.revoke_url.is_some(),
                tools: tool_names(p.id, true),
            }
        })
        .collect()
}
