//! Control API credentials and settings (ARCHITECTURE §13.6). The HTTP server itself lives in
//! the `harness-api` crate; everything a front end needs without it is here (the GUI and the
//! future CLI manage tokens and settings through these functions).
//!
//! * **Session token** (`hss_<43 chars>`): random, created on every API start, kept in memory
//!   and written to `<data>/api-token` readable only by the current user
//!   ([`crate::instance::write_private_file`]). Full access (admin).
//! * **Named tokens** (`hs_<id>_<43 chars>`): created with scopes, shown once. Only a salted
//!   SHA-256 of the secret part is stored (`config/api_tokens.json`, versioned). The secret is
//!   256 random bits, so a fast hash is enough: a slow password hash (argon2) only helps
//!   against guessing low-entropy secrets and would cost every request tens of milliseconds.
//! * Comparisons of secrets and hashes are constant-time ([`ct_eq`]).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub use crate::token_policy::{GatewayEndpoint, RateLimits, TokenPolicy};

pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 11500;
/// Base path of every endpoint.
pub const BASE_PATH: &str = "/api/harness/v1";
/// Session token file inside the data directory.
pub const SESSION_TOKEN_FILE: &str = "api-token";

/// Error keys (i18n) of this module.
pub const ERR_NETWORK_NOT_ALLOWED: &str = "api.network_not_allowed";
pub const ERR_NETWORK_WITHOUT_TOKEN: &str = "api.network_without_token";
pub const ERR_INVALID_PORT: &str = "api.invalid_port";
pub const ERR_INVALID_ORIGIN: &str = "api.invalid_origin";
pub const ERR_TOKEN_NAME: &str = "api.token_name_invalid";
pub const ERR_TOKEN_SCOPES: &str = "api.token_scopes_empty";
pub const ERR_TOKEN_NOT_FOUND: &str = "api.token_not_found";
pub const ERR_RANDOM: &str = "api.random_failed";

/// `settings.json` → `control_api`. Changed only through [`validate_settings`] (the GUI's
/// general settings save keeps the stored block).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ControlApiSettings {
    /// Off by default; the user turns it on in Settings.
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    /// A non-loopback bind must be allowed explicitly (and needs a named token).
    pub allow_network: bool,
    /// Browser origins allowed by CORS. Empty = no CORS headers (no browser access).
    pub cors_origins: Vec<String>,
}

impl Default for ControlApiSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
            allow_network: false,
            cors_origins: Vec::new(),
        }
    }
}

/// Canonical settings or the i18n error key. `named_tokens` = number of named tokens (a
/// network bind needs at least one: the session token file only exists on this computer).
pub fn validate_settings(
    incoming: &ControlApiSettings,
    named_tokens: usize,
) -> Result<ControlApiSettings, String> {
    let mut s = incoming.clone();
    s.host = crate::network::normalize_bind_host(&s.host)?;
    if s.port < 1024 {
        return Err(ERR_INVALID_PORT.to_string());
    }
    if !crate::network::is_loopback_host(&s.host) {
        if !s.allow_network {
            return Err(ERR_NETWORK_NOT_ALLOWED.to_string());
        }
        if named_tokens == 0 {
            return Err(ERR_NETWORK_WITHOUT_TOKEN.to_string());
        }
    }
    let mut origins = Vec::new();
    for raw in s.cors_origins.iter().filter(|o| !o.trim().is_empty()) {
        let o =
            crate::network::normalize_origin(raw).map_err(|_| ERR_INVALID_ORIGIN.to_string())?;
        // Only explicit `http(s)://host[:port]` origins; no wildcard.
        if o == "*" || o == "localhost" {
            return Err(ERR_INVALID_ORIGIN.to_string());
        }
        if !origins.contains(&o) {
            origins.push(o);
        }
    }
    s.cors_origins = origins;
    Ok(s)
}

// --- Scopes ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum Scope {
    /// Every GET endpoint.
    #[serde(rename = "read")]
    Read,
    /// Gateway inference requests (`/v1/*` on the managed endpoint). What a token may
    /// do there is narrowed further by its [`TokenPolicy`] (models, endpoints, limits).
    #[serde(rename = "inference")]
    Inference,
    /// Downloads, model deletion, definitions and presets.
    #[serde(rename = "models:write")]
    ModelsWrite,
    /// Only downloads: start (`pull`), pause, resume, cancel. Implied by `models:write`.
    #[serde(rename = "downloads:write")]
    DownloadsWrite,
    /// Server profiles, start/stop/load/unload, benchmarks.
    #[serde(rename = "servers:write")]
    ServersWrite,
    /// Only running servers: start, stop, retry, load, unload (no profile edits, no
    /// benchmarks). Implied by `servers:write`.
    #[serde(rename = "servers:control")]
    ServersControl,
    /// Stored conversations: list, read, export, their documents' metadata, and compaction
    /// (`/conversations`). Chats are private, so `read` does not cover them.
    #[serde(rename = "conversations")]
    Conversations,
    /// Running Harness's built-in tools (files, shell, network) through `POST /tools/{name}/run`
    /// and the MCP server. The tool policy still applies and a call that needs the user's
    /// approval is refused; `approved` and a working directory of the caller's choosing need
    /// `admin`. Not part of `read`.
    #[serde(rename = "tools")]
    Tools,
    /// Everything, including tokens and the "open anyway" network confirmation.
    #[serde(rename = "admin")]
    Admin,
}

impl Scope {
    pub const ALL: [Scope; 9] = [
        Scope::Read,
        Scope::Inference,
        Scope::ModelsWrite,
        Scope::DownloadsWrite,
        Scope::ServersWrite,
        Scope::ServersControl,
        Scope::Conversations,
        Scope::Tools,
        Scope::Admin,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Read => "read",
            Scope::Inference => "inference",
            Scope::ModelsWrite => "models:write",
            Scope::DownloadsWrite => "downloads:write",
            Scope::ServersWrite => "servers:write",
            Scope::ServersControl => "servers:control",
            Scope::Conversations => "conversations",
            Scope::Tools => "tools",
            Scope::Admin => "admin",
        }
    }

    /// Case-insensitive name to scope.
    pub fn parse(s: &str) -> Option<Scope> {
        let s = s.trim();
        Scope::ALL
            .into_iter()
            .find(|x| x.as_str().eq_ignore_ascii_case(s))
    }

    /// Every scope name, comma-separated (help and error texts).
    pub fn names() -> String {
        Scope::ALL.map(Scope::as_str).join(", ")
    }
}

/// Whether `granted` covers `needed`: admin covers everything; `models:write` covers
/// `downloads:write`; `servers:write` covers `servers:control`; any write scope implies read.
pub fn allows(granted: &[Scope], needed: Scope) -> bool {
    granted.contains(&Scope::Admin)
        || granted.contains(&needed)
        || (needed == Scope::DownloadsWrite && granted.contains(&Scope::ModelsWrite))
        || (needed == Scope::ServersControl && granted.contains(&Scope::ServersWrite))
        || (needed == Scope::Read
            && granted.iter().any(|s| {
                matches!(
                    s,
                    Scope::ModelsWrite
                        | Scope::DownloadsWrite
                        | Scope::ServersWrite
                        | Scope::ServersControl
                )
            }))
}

// --- Secrets ---

/// Constant-time equality (length is not secret: every token of a kind has the same length).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|e| format!("{}: {}", ERR_RANDOM, e))?;
    Ok(buf)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 2);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let chars = chunk.len() + 1;
        for i in 0..chars {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// A fresh 256-bit secret (43 base64url chars).
fn new_secret() -> Result<String, String> {
    Ok(base64url(&random_bytes::<32>()?))
}

fn hash_secret(salt: &str, secret: &str) -> String {
    let mut h = Sha256::new();
    h.update(salt.as_bytes());
    h.update([0u8]);
    h.update(secret.as_bytes());
    hex(&h.finalize())
}

// --- Session token ---

/// A new session token (`hss_...`).
pub fn new_session_token() -> Result<String, String> {
    Ok(format!("hss_{}", new_secret()?))
}

pub fn session_token_path(data_dir: &Path) -> PathBuf {
    data_dir.join(SESSION_TOKEN_FILE)
}

/// Writes the session token file (owner-only) and returns its path.
pub fn write_session_token(data_dir: &Path, token: &str) -> Result<PathBuf, String> {
    let path = session_token_path(data_dir);
    crate::instance::write_private_file(&path, token.as_bytes())?;
    Ok(path)
}

/// Removes the session token file (API stopped).
pub fn remove_session_token(data_dir: &Path) {
    let _ = std::fs::remove_file(session_token_path(data_dir));
}

// --- Named tokens ---

/// A stored named token (never the secret).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredToken {
    /// Public id (also the prefix of the token value).
    pub id: String,
    pub name: String,
    pub scopes: Vec<Scope>,
    pub salt: String,
    /// `sha256(salt ‖ 0 ‖ secret)`, hex.
    pub hash: String,
    /// Unix seconds.
    pub created_at: u64,
    /// Gateway policy (schema v2; a v1 token has none = no restriction).
    #[serde(flatten, default)]
    pub policy: TokenPolicy,
}

/// What lists and the API show about a named token.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TokenInfo {
    pub id: String,
    pub name: String,
    pub scopes: Vec<Scope>,
    pub created_at: u64,
    /// Gateway policy: allowed models and endpoints, rate limits (empty = unrestricted).
    #[serde(flatten, default)]
    pub policy: TokenPolicy,
}

impl From<&StoredToken> for TokenInfo {
    fn from(t: &StoredToken) -> Self {
        Self {
            id: t.id.clone(),
            name: t.name.clone(),
            scopes: t.scopes.clone(),
            created_at: t.created_at,
            policy: t.policy.clone(),
        }
    }
}

/// A change to a named token (`None` = keep). The secret never changes; to rotate it,
/// create a new token and revoke the old one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TokenUpdate {
    pub name: Option<String>,
    pub scopes: Option<Vec<Scope>>,
    /// Model alias patterns (`*`, `?`); `[]` = every model.
    pub models: Option<Vec<String>>,
    /// Gateway endpoint groups; `[]` = every endpoint.
    pub endpoints: Option<Vec<GatewayEndpoint>>,
    /// Replaces all three limits (a missing or `null` field = unlimited).
    pub limits: Option<RateLimits>,
}

/// A token just created: the only time its value is returned.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreatedToken {
    pub info: TokenInfo,
    /// `hs_<id>_<secret>`; not stored anywhere.
    pub token: String,
}

/// Who a request authenticated as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    /// Local owner credential read from the private session token file.
    Session,
    /// Password-authenticated browser administrator; never a local owner credential.
    Browser,
    Named {
        id: String,
        name: String,
    },
}

/// `config/api_tokens.json`: `{schema_version: 2, tokens: [...]}`.
///
/// * v2: every token carries its gateway policy (`models`, `endpoints`, `limits`). The
///   migration writes the empty policy, i.e. exactly the old behaviour; the v1 file is kept
///   as `api_tokens.json.bak-v1`.
pub const TOKENS_SCHEMA: crate::store::Schema = crate::store::Schema {
    file: "api_tokens.json",
    current: 2,
    envelope: crate::store::Envelope::Wrapped("tokens"),
    migrations: &[tokens_v1_to_v2],
};

fn tokens_v1_to_v2(mut doc: serde_json::Value) -> Result<serde_json::Value, String> {
    let empty = serde_json::to_value(TokenPolicy::default()).map_err(|e| e.to_string())?;
    let empty = empty.as_object().cloned().unwrap_or_default();
    if let Some(list) = doc.get_mut("tokens").and_then(|t| t.as_array_mut()) {
        for token in list.iter_mut().filter_map(|t| t.as_object_mut()) {
            for (k, v) in &empty {
                token.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
    }
    Ok(doc)
}

fn validate_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(|c| c.is_control()) {
        return Err(ERR_TOKEN_NAME.to_string());
    }
    Ok(name.to_string())
}

fn validate_scopes(scopes: &[Scope]) -> Result<Vec<Scope>, String> {
    let mut scopes: Vec<Scope> = scopes.to_vec();
    scopes.sort();
    scopes.dedup();
    if scopes.is_empty() {
        return Err(ERR_TOKEN_SCOPES.to_string());
    }
    Ok(scopes)
}

/// Named token file of a config directory.
#[derive(Debug, Clone)]
pub struct TokenStore {
    path: PathBuf,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl TokenStore {
    pub fn at(config_dir: &Path) -> Self {
        Self {
            path: config_dir.join(TOKENS_SCHEMA.file),
        }
    }

    /// The token file of the app's data root.
    pub fn global() -> Self {
        Self::at(&crate::paths::config_dir())
    }

    fn store(&self) -> crate::store::JsonStore {
        crate::store::JsonStore::sensitive(&self.path)
    }

    fn load(&self) -> Result<Vec<StoredToken>, String> {
        Ok(self
            .store()
            .load_versioned::<Vec<StoredToken>>(&TOKENS_SCHEMA)?
            .map(|l| l.data)
            .unwrap_or_default())
    }

    pub fn list(&self) -> Result<Vec<TokenInfo>, String> {
        Ok(self.load()?.iter().map(TokenInfo::from).collect())
    }

    pub fn count(&self) -> usize {
        self.load().map(|l| l.len()).unwrap_or(0)
    }

    /// Creates a named token without a gateway policy; the returned value is shown once.
    pub fn create(&self, name: &str, scopes: &[Scope]) -> Result<CreatedToken, String> {
        self.create_with_policy(name, scopes, &TokenPolicy::default())
    }

    /// Creates a named token with a gateway policy; the returned value is shown once.
    pub fn create_with_policy(
        &self,
        name: &str,
        scopes: &[Scope],
        policy: &TokenPolicy,
    ) -> Result<CreatedToken, String> {
        let name = validate_name(name)?;
        let scopes = validate_scopes(scopes)?;
        let policy = policy.validate()?;
        let id = hex(&random_bytes::<6>()?);
        let secret = new_secret()?;
        let salt = hex(&random_bytes::<16>()?);
        let stored = StoredToken {
            id: id.clone(),
            name,
            scopes,
            hash: hash_secret(&salt, &secret),
            salt,
            created_at: now_secs(),
            policy,
        };
        let info = TokenInfo::from(&stored);
        self.store()
            .update_versioned::<Vec<StoredToken>, _>(&TOKENS_SCHEMA, |list| {
                list.push(stored);
                Ok(())
            })?;
        Ok(CreatedToken {
            info,
            token: format!("hs_{}_{}", id, secret),
        })
    }

    /// Changes a token's name, scopes or gateway policy (validated like at creation). Takes
    /// effect on the next request: every request reads the token file.
    pub fn update(&self, id: &str, change: &TokenUpdate) -> Result<TokenInfo, String> {
        let name = change.name.as_deref().map(validate_name).transpose()?;
        let scopes = change.scopes.as_deref().map(validate_scopes).transpose()?;
        self.store()
            .update_versioned::<Vec<StoredToken>, _>(&TOKENS_SCHEMA, |list| {
                let token = list
                    .iter_mut()
                    .find(|t| t.id == id)
                    .ok_or_else(|| ERR_TOKEN_NOT_FOUND.to_string())?;
                let mut policy = token.policy.clone();
                if let Some(m) = &change.models {
                    policy.models = m.clone();
                }
                if let Some(e) = &change.endpoints {
                    policy.endpoints = e.clone();
                }
                if let Some(l) = change.limits {
                    policy.limits = l;
                }
                token.policy = policy.validate()?;
                if let Some(n) = &name {
                    token.name = n.clone();
                }
                if let Some(s) = &scopes {
                    token.scopes = s.clone();
                }
                Ok(TokenInfo::from(&*token))
            })
    }

    /// One token's info (never the secret).
    pub fn get(&self, id: &str) -> Result<TokenInfo, String> {
        self.load()?
            .iter()
            .find(|t| t.id == id)
            .map(TokenInfo::from)
            .ok_or_else(|| ERR_TOKEN_NOT_FOUND.to_string())
    }

    /// Revokes (deletes) a named token.
    pub fn revoke(&self, id: &str) -> Result<(), String> {
        self.store()
            .update_versioned::<Vec<StoredToken>, _>(&TOKENS_SCHEMA, |list| {
                let before = list.len();
                list.retain(|t| t.id != id);
                if list.len() == before {
                    Err(ERR_TOKEN_NOT_FOUND.to_string())
                } else {
                    Ok(())
                }
            })
    }

    /// The token's info and scopes when `presented` is a valid named token.
    pub fn verify(&self, presented: &str) -> Option<(TokenInfo, Vec<Scope>)> {
        let rest = presented.strip_prefix("hs_")?;
        let (id, secret) = rest.split_once('_')?;
        let tokens = self.load().ok()?;
        let token = tokens.iter().find(|t| t.id == id)?;
        let computed = hash_secret(&token.salt, secret);
        ct_eq(computed.as_bytes(), token.hash.as_bytes())
            .then(|| (TokenInfo::from(token), token.scopes.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_validation() {
        let ok = validate_settings(&ControlApiSettings::default(), 0).unwrap();
        assert_eq!(ok.host, "127.0.0.1");
        assert_eq!(ok.port, 11500);
        assert!(!ok.enabled, "off by default");

        let lan = ControlApiSettings {
            host: "0.0.0.0".into(),
            ..Default::default()
        };
        assert_eq!(
            validate_settings(&lan, 3).unwrap_err(),
            ERR_NETWORK_NOT_ALLOWED
        );
        let lan = ControlApiSettings {
            allow_network: true,
            ..lan
        };
        assert_eq!(
            validate_settings(&lan, 0).unwrap_err(),
            ERR_NETWORK_WITHOUT_TOKEN
        );
        assert!(validate_settings(&lan, 1).is_ok());

        let bad_host = ControlApiSettings {
            host: "example.com".into(),
            ..Default::default()
        };
        assert_eq!(
            validate_settings(&bad_host, 1).unwrap_err(),
            crate::network::ERR_INVALID_HOST
        );
        let low_port = ControlApiSettings {
            port: 80,
            ..Default::default()
        };
        assert_eq!(
            validate_settings(&low_port, 0).unwrap_err(),
            ERR_INVALID_PORT
        );

        let cors = ControlApiSettings {
            cors_origins: vec![
                "http://localhost:5173/".into(),
                " ".into(),
                "http://localhost:5173".into(),
            ],
            ..Default::default()
        };
        assert_eq!(
            validate_settings(&cors, 0).unwrap().cors_origins,
            vec!["http://localhost:5173"]
        );
        for bad in ["*", "localhost", "file:///x", "http://a b"] {
            let s = ControlApiSettings {
                cors_origins: vec![bad.into()],
                ..Default::default()
            };
            assert_eq!(
                validate_settings(&s, 0).unwrap_err(),
                ERR_INVALID_ORIGIN,
                "{bad}"
            );
        }
    }

    #[test]
    fn scopes_serialize_and_imply() {
        assert_eq!(
            serde_json::to_string(&Scope::ModelsWrite).unwrap(),
            "\"models:write\""
        );
        let parsed: Vec<Scope> =
            serde_json::from_str(r#"["read","servers:write","admin"]"#).unwrap();
        assert_eq!(parsed, vec![Scope::Read, Scope::ServersWrite, Scope::Admin]);
        assert!(allows(&[Scope::Admin], Scope::ModelsWrite));
        assert!(allows(&[Scope::ServersWrite], Scope::Read));
        assert!(!allows(&[Scope::Read], Scope::ServersWrite));
        assert!(!allows(&[Scope::ServersWrite], Scope::ModelsWrite));
        assert!(!allows(&[Scope::Inference], Scope::Read));
        for s in Scope::ALL {
            assert_eq!(serde_json::to_value(s).unwrap(), s.as_str());
            assert_eq!(Scope::parse(&s.as_str().to_uppercase()), Some(s));
            assert!(allows(&[Scope::Admin], s));
        }
        assert_eq!(Scope::parse("root"), None);
        assert!(Scope::names().contains("servers:control"));
    }

    #[test]
    fn narrow_scopes_are_implied_by_the_wide_ones() {
        // servers:control: start/stop/load/unload only; servers:write covers it.
        assert!(allows(&[Scope::ServersWrite], Scope::ServersControl));
        assert!(allows(&[Scope::ServersControl], Scope::ServersControl));
        assert!(!allows(&[Scope::ServersControl], Scope::ServersWrite));
        assert!(allows(&[Scope::ServersControl], Scope::Read));
        // downloads:write: downloads only; models:write covers it.
        assert!(allows(&[Scope::ModelsWrite], Scope::DownloadsWrite));
        assert!(!allows(&[Scope::DownloadsWrite], Scope::ModelsWrite));
        assert!(allows(&[Scope::DownloadsWrite], Scope::Read));
        // No cross-over between the two families, and inference implies nothing.
        assert!(!allows(&[Scope::ServersWrite], Scope::DownloadsWrite));
        assert!(!allows(&[Scope::ModelsWrite], Scope::ServersControl));
        assert!(!allows(&[Scope::Inference], Scope::ServersControl));
        assert!(!allows(&[Scope::Read], Scope::Inference));
    }

    #[test]
    fn v1_token_file_migrates_with_backup_and_keeps_working() {
        let dir = tempfile::tempdir().unwrap();
        // A token file exactly as the v1 build wrote it (one real token created by hand).
        let secret = new_secret().unwrap();
        let salt = hex(&random_bytes::<16>().unwrap());
        let v1 = serde_json::json!({
            "schema_version": 1,
            "tokens": [{
                "id": "0123456789ab", "name": "old", "scopes": ["inference"],
                "salt": salt, "hash": hash_secret(&salt, &secret), "created_at": 1_700_000_000u64
            }]
        });
        let path = dir.path().join("api_tokens.json");
        std::fs::write(&path, serde_json::to_vec_pretty(&v1).unwrap()).unwrap();
        let store = TokenStore::at(dir.path());

        let (info, scopes) = store
            .verify(&format!("hs_0123456789ab_{}", secret))
            .expect("old token still verifies");
        assert_eq!(scopes, vec![Scope::Inference]);
        assert!(
            info.policy.is_unrestricted(),
            "old tokens keep today's behaviour"
        );

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"schema_version\": 2"), "{text}");
        assert!(text.contains("\"models\": []") && text.contains("\"limits\""));
        let backup = std::fs::read_to_string(dir.path().join("api_tokens.json.bak-v1")).unwrap();
        assert!(backup.contains("\"schema_version\": 1"));
        assert!(!backup.contains("\"limits\""));
    }

    #[test]
    fn token_policy_is_stored_and_updated() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::at(dir.path());
        let policy = TokenPolicy {
            models: vec!["qwen3-*".into()],
            endpoints: vec![GatewayEndpoint::Chat],
            limits: RateLimits {
                requests_per_minute: Some(30),
                tokens_per_minute: Some(20_000),
                max_concurrent: Some(1),
            },
        };
        let created = store
            .create_with_policy("agent", &[Scope::Inference], &policy)
            .unwrap();
        assert_eq!(created.info.policy, policy);
        let (info, _) = store.verify(&created.token).unwrap();
        assert_eq!(info.policy, policy);
        // Flat on the wire: models/endpoints/limits next to the scopes.
        let v = serde_json::to_value(&info).unwrap();
        assert_eq!(v["models"][0], "qwen3-*");
        assert_eq!(v["endpoints"][0], "chat");
        assert_eq!(v["limits"]["requests_per_minute"], 30);

        let id = created.info.id.clone();
        let updated = store
            .update(
                &id,
                &TokenUpdate {
                    name: Some(" agent 2 ".into()),
                    endpoints: Some(vec![]),
                    limits: Some(RateLimits {
                        max_concurrent: Some(4),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(updated.name, "agent 2");
        assert_eq!(
            updated.policy.models,
            vec!["qwen3-*"],
            "untouched part kept"
        );
        assert!(updated.policy.endpoints.is_empty());
        assert_eq!(updated.policy.limits.requests_per_minute, None);
        assert_eq!(updated.policy.limits.max_concurrent, Some(4));
        assert_eq!(store.get(&id).unwrap(), updated);
        // The secret did not change.
        assert!(store.verify(&created.token).is_some());

        // Invalid changes leave the token as it was.
        let bad = TokenUpdate {
            limits: Some(RateLimits {
                requests_per_minute: Some(0),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            store.update(&id, &bad).unwrap_err(),
            crate::token_policy::ERR_LIMIT
        );
        assert_eq!(
            store
                .update(
                    &id,
                    &TokenUpdate {
                        scopes: Some(vec![]),
                        ..Default::default()
                    }
                )
                .unwrap_err(),
            ERR_TOKEN_SCOPES
        );
        assert_eq!(store.get(&id).unwrap(), updated);
        assert_eq!(
            store.update("nope", &TokenUpdate::default()).unwrap_err(),
            ERR_TOKEN_NOT_FOUND
        );
        assert_eq!(
            store
                .create_with_policy(
                    "x",
                    &[Scope::Inference],
                    &TokenPolicy {
                        models: vec!["a b".into()],
                        ..Default::default()
                    }
                )
                .unwrap_err(),
            crate::token_policy::ERR_MODEL_PATTERN
        );
    }

    #[test]
    fn named_tokens_are_stored_hashed_and_revocable() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::at(dir.path());
        let created = store
            .create("  CI bot ", &[Scope::Read, Scope::Read])
            .unwrap();
        assert_eq!(created.info.name, "CI bot");
        assert_eq!(created.info.scopes, vec![Scope::Read]);
        assert!(created
            .token
            .starts_with(&format!("hs_{}_", created.info.id)));
        assert_eq!(created.token.len(), 3 + 12 + 1 + 43);

        // The file holds the hash, never the secret.
        let text = std::fs::read_to_string(dir.path().join("api_tokens.json")).unwrap();
        let secret_part = &created.token[16..];
        assert!(!text.contains(secret_part));
        assert!(text.contains("\"schema_version\": 2"));

        let (info, scopes) = store.verify(&created.token).unwrap();
        assert_eq!(info.id, created.info.id);
        assert_eq!(scopes, vec![Scope::Read]);
        // Wrong secret, wrong id, other formats.
        let mut wrong = created.token.clone();
        let last = wrong.pop().unwrap();
        wrong.push(if last == 'A' { 'B' } else { 'A' });
        assert!(store.verify(&wrong).is_none());
        assert!(store.verify("hs_000000000000_abc").is_none());
        assert!(store.verify("Bearer x").is_none());
        assert!(store.verify("").is_none());

        assert_eq!(store.list().unwrap().len(), 1);
        store.revoke(&created.info.id).unwrap();
        assert!(store.verify(&created.token).is_none());
        assert_eq!(
            store.revoke(&created.info.id).unwrap_err(),
            ERR_TOKEN_NOT_FOUND
        );
        assert_eq!(
            store.create(" ", &[Scope::Read]).unwrap_err(),
            ERR_TOKEN_NAME
        );
        assert_eq!(store.create("x", &[]).unwrap_err(), ERR_TOKEN_SCOPES);
    }

    #[test]
    fn session_tokens_are_random_and_private() {
        let a = new_session_token().unwrap();
        let b = new_session_token().unwrap();
        assert_ne!(a, b);
        assert!(a.starts_with("hss_") && a.len() == 4 + 43);
        let dir = tempfile::tempdir().unwrap();
        let path = write_session_token(dir.path(), &a).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), a);
        assert!(crate::instance::is_private_to_current_user(&path).unwrap());
        remove_session_token(dir.path());
        assert!(!path.exists());
    }

    #[test]
    fn constant_time_eq() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
        assert_eq!(base64url(&[0u8; 32]).len(), 43);
    }
}
