//! Secure pairing of remote devices (phones, laptops, other Harness hosts) with this
//! computer, and the registry behind the Devices page.
//!
//! # Protocol
//!
//! 1. **Offer.** The desktop creates a one-time offer: an 8-digit code, valid for minutes,
//!    for one device, with the role, the access preset and the allowed models already
//!    chosen by the owner ([`Hub::create_offer`]). The code is shown as text and as a link
//!    (QR) the phone opens.
//! 2. **Claim.** The new device sends the code with its name and a random nonce to the
//!    *public* claim route of the gateway ([`Hub::claim`]). A wrong code counts towards a
//!    lockout; a right one is consumed (single use). No token is issued yet. The claim gets
//!    a *verification code* (6 digits derived from the code, both nonces and the claim id)
//!    that both screens show.
//! 3. **Approval.** The desktop lists the pending claim with the device name and the
//!    verification code. Only an explicit approval by the owner ([`Hub::approve`]) mints
//!    the scoped token; a denial or the timeout drops the claim.
//! 4. **Delivery.** The device polls with the claim's poll secret and receives the token
//!    exactly once ([`Hub::poll`]); the token is kept in memory until then and never on disk
//!    in clear text (only its salted hash, in the named token store).
//!
//! Everything after that is an ordinary named token: scopes and gateway policy
//! (`token_policy`), rate limits, revocation. Every step lands in the audit log.
//!
//! The channel is as private as the connection it runs over: use HTTPS (reverse proxy,
//! Tailscale, a TLS-terminating tunnel) outside a trusted LAN. See `docs/en/remote-access.md`.
//!
//! StrukLabs identity, discovery and signaling are modelled as [`SignalingProvider`]; it
//! never relays inference traffic.

use crate::control_api::{self, ct_eq, GatewayEndpoint, Scope, TokenInfo, TokenPolicy, TokenStore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub mod client;
pub mod service;
#[cfg(test)]
mod tests;

/// How long an offer's code can be claimed.
pub const OFFER_TTL_SECS: u64 = 300;
pub const MAX_OFFER_TTL_SECS: u64 = 900;
/// How long a claim waits for the owner's decision.
pub const CLAIM_TTL_SECS: u64 = 300;
const MAX_OFFERS: usize = 5;
const MAX_CLAIMS: usize = 8;
/// Wrong codes tolerated per [`FAIL_WINDOW_SECS`] before claims are refused for the rest of it.
const MAX_FAILURES: usize = 5;
const FAIL_WINDOW_SECS: u64 = 60;
/// A device that spoke to the gateway this recently counts as online.
pub const ONLINE_WINDOW_SECS: u64 = 120;
const AUDIT_FILE: &str = "pairing_audit.jsonl";
const AUDIT_KEEP: usize = 1000;
const AUDIT_ROTATE_BYTES: u64 = 256 * 1024;
const LIST_CAP: usize = 256;

pub const ERR_INVALID_CODE: &str = "pairing.invalid_code";
pub const ERR_LOCKED: &str = "pairing.locked";
pub const ERR_EXPIRED: &str = "pairing.expired";
pub const ERR_NOT_FOUND: &str = "pairing.not_found";
pub const ERR_DEVICE_NOT_FOUND: &str = "pairing.device_not_found";
pub const ERR_DEVICE_NAME: &str = "pairing.device_name_invalid";
pub const ERR_TOO_MANY: &str = "pairing.too_many";
pub const ERR_ADMIN_ACK: &str = "pairing.admin_ack_required";
pub const ERR_NOT_PENDING: &str = "pairing.not_pending";
pub const ERR_NONCE: &str = "pairing.nonce_invalid";

// --- Roles and access ---

/// What a paired device does for this computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum DeviceRole {
    /// Runs models for others (engine, gateway, models).
    InferenceHost,
    /// Uses a gateway (phone browser chat, OpenCode on a laptop).
    Client,
    /// Keeps model files for others.
    Storage,
}

impl DeviceRole {
    pub const ALL: [DeviceRole; 3] = [
        DeviceRole::InferenceHost,
        DeviceRole::Client,
        DeviceRole::Storage,
    ];

    /// The components a device in this role needs, and nothing else.
    pub fn components(self) -> &'static [&'static str] {
        match self {
            DeviceRole::InferenceHost => &["runtime", "gateway", "models"],
            DeviceRole::Client => &["chat_client"],
            DeviceRole::Storage => &["model_storage"],
        }
    }

    /// Components of the role that the device did not report as installed.
    pub fn missing(self, installed: &[String]) -> Vec<String> {
        self.components()
            .iter()
            .filter(|c| !installed.iter().any(|i| i == **c))
            .map(|c| c.to_string())
            .collect()
    }
}

/// What a device may do with its token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AccessPreset {
    /// Chat completions and the model list only.
    ChatOnly,
    /// Every gateway endpoint (embeddings, audio, images, pipelines...), narrowed by models.
    GatewayModels,
    /// The whole Control API. Needs an explicit acknowledgement.
    Admin,
}

impl AccessPreset {
    pub fn scopes(self) -> Vec<Scope> {
        match self {
            AccessPreset::ChatOnly | AccessPreset::GatewayModels => vec![Scope::Inference],
            AccessPreset::Admin => vec![Scope::Admin],
        }
    }

    /// Gateway policy: allowed endpoints and model patterns (empty = every model).
    pub fn policy(self, models: &[String]) -> TokenPolicy {
        let endpoints = match self {
            AccessPreset::ChatOnly => vec![GatewayEndpoint::Chat, GatewayEndpoint::Models],
            AccessPreset::GatewayModels => GatewayEndpoint::ALL
                .into_iter()
                .filter(|e| *e != GatewayEndpoint::Metrics)
                .collect(),
            AccessPreset::Admin => Vec::new(),
        };
        TokenPolicy {
            models: models.to_vec(),
            endpoints,
            ..Default::default()
        }
    }
}

// --- Stored devices ---

/// What a device reports about itself (`POST /v1/harness/device/report`). Informational:
/// nothing here grants access.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DeviceReport {
    pub models: Vec<String>,
    pub engines: Vec<String>,
    /// Installed components (see [`DeviceRole::components`]).
    pub components: Vec<String>,
    pub disk_total_bytes: Option<u64>,
    pub disk_free_bytes: Option<u64>,
    pub ram_total_bytes: Option<u64>,
    pub vram_total_bytes: Option<u64>,
    pub vram_free_bytes: Option<u64>,
    pub running_servers: u32,
    pub queued_requests: u32,
    /// Where other devices reach its gateway (`http://192.168.1.20:11434`).
    pub address: Option<String>,
    /// Unix seconds; set by the host when the report arrives.
    #[serde(skip_deserializing)]
    pub reported_at: u64,
}

impl DeviceReport {
    fn sanitize(mut self, now: u64) -> Self {
        fn list(v: &mut Vec<String>, max: usize) {
            v.retain(|s| !s.trim().is_empty());
            v.iter_mut().for_each(|s| {
                *s = s.chars().filter(|c| !c.is_control()).take(128).collect();
            });
            v.truncate(max);
        }
        list(&mut self.models, LIST_CAP);
        list(&mut self.engines, 32);
        list(&mut self.components, 16);
        self.address = self
            .address
            .map(|a| a.chars().filter(|c| !c.is_control()).take(200).collect())
            .filter(|a: &String| !a.is_empty());
        self.reported_at = now;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoredDevice {
    id: String,
    name: String,
    platform: String,
    role: DeviceRole,
    preset: AccessPreset,
    token_id: String,
    created_at: u64,
    #[serde(default)]
    report: Option<DeviceReport>,
}

const DEVICES_SCHEMA: crate::store::Schema = crate::store::Schema {
    file: "paired_devices.json",
    current: 1,
    envelope: crate::store::Envelope::Wrapped("devices"),
    migrations: &[],
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum Reachability {
    /// Spoke to the gateway within [`ONLINE_WINDOW_SECS`].
    Online,
    /// Seen before, not lately.
    Offline,
    /// Never seen since pairing.
    Never,
}

/// A paired device as the Devices page shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub role: DeviceRole,
    pub preset: AccessPreset,
    pub created_at: u64,
    pub last_seen: Option<u64>,
    pub reachability: Reachability,
    /// Control API scopes of its token.
    pub scopes: Vec<Scope>,
    /// Gateway endpoint groups of its token (empty = all).
    pub endpoints: Vec<GatewayEndpoint>,
    /// Allowed model patterns (empty = every model).
    pub models: Vec<String>,
    pub report: Option<DeviceReport>,
    /// Components its role needs.
    pub components: Vec<String>,
    /// Components of its role it did not report as installed.
    pub missing_components: Vec<String>,
}

// --- Offers and claims (memory only) ---

/// What the owner decides when creating an offer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OfferSpec {
    pub role: DeviceRole,
    pub preset: AccessPreset,
    /// Allowed model patterns (empty = every model).
    #[serde(default)]
    pub models: Vec<String>,
    /// Seconds the code stays valid (default 300, at most 900).
    #[serde(default)]
    pub ttl_secs: Option<u64>,
    /// Required for the `admin` preset.
    #[serde(default)]
    pub acknowledge_admin: bool,
}

/// A fresh offer: the only time its code is returned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OfferInfo {
    pub id: String,
    /// `1234-5678` (empty in listings).
    pub code: String,
    pub role: DeviceRole,
    pub preset: AccessPreset,
    pub models: Vec<String>,
    pub expires_at: u64,
}

struct Offer {
    info: OfferInfo,
    digits: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ClaimState {
    Pending,
    Approved,
    Denied,
    Expired,
}

#[derive(Clone)]
struct ClaimGrant {
    token: String,
    device_id: String,
    token_id: String,
}

enum ClaimStage {
    Pending,
    Approved(Box<ClaimGrant>),
    Denied,
}

struct Claim {
    id: String,
    poll_secret: String,
    name: String,
    platform: String,
    role: DeviceRole,
    preset: AccessPreset,
    models: Vec<String>,
    verification: String,
    requested_at: u64,
    expires_at: u64,
    stage: ClaimStage,
}

/// What the new device gets back from a successful claim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ClaimStarted {
    pub claim_id: String,
    /// Secret for polling; shown to no one else.
    pub poll_secret: String,
    /// 6 digits; the owner compares them with the desktop prompt.
    pub verification: String,
    pub server_nonce: String,
    pub expires_at: u64,
}

/// A claim waiting for the owner (the approval prompt).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PendingClaim {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub role: DeviceRole,
    pub preset: AccessPreset,
    pub models: Vec<String>,
    pub verification: String,
    pub requested_at: u64,
    pub expires_at: u64,
}

/// The answer to a poll. `token` is present once, on the poll that finds the approval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ClaimPoll {
    pub status: ClaimState,
    pub token: Option<String>,
    pub device_id: Option<String>,
    pub role: Option<DeviceRole>,
    pub preset: Option<AccessPreset>,
    pub scopes: Vec<Scope>,
    pub endpoints: Vec<GatewayEndpoint>,
    pub models: Vec<String>,
}

impl ClaimPoll {
    fn bare(status: ClaimState) -> Self {
        Self {
            status,
            token: None,
            device_id: None,
            role: None,
            preset: None,
            scopes: Vec::new(),
            endpoints: Vec::new(),
            models: Vec::new(),
        }
    }
}

/// One audit log line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AuditEntry {
    pub at: u64,
    /// `offer_created`, `claim_failed`, `claim_pending`, `approved`, `denied`, `revoked`,
    /// `revoke_all`, `token_removed`, `access_changed`.
    pub event: String,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub detail: Option<String>,
}

#[derive(Default)]
struct HubState {
    offers: Vec<Offer>,
    claims: Vec<Claim>,
    failures: Vec<u64>,
    seen: HashMap<String, u64>,
}

/// The pairing state of one config directory.
pub struct Hub {
    dir: PathBuf,
    state: Mutex<HubState>,
    audit_lock: Mutex<()>,
    /// Added to the wall clock (tests move time forward).
    skew: AtomicU64,
}

/// The hub of a config directory (one per directory, shared by the gateway, the Control API
/// and the GUI of the process).
pub fn hub(config_dir: &Path) -> Arc<Hub> {
    static HUBS: OnceLock<Mutex<HashMap<PathBuf, Arc<Hub>>>> = OnceLock::new();
    let mut map = HUBS.get_or_init(Default::default).lock().unwrap();
    map.entry(config_dir.to_path_buf())
        .or_insert_with(|| {
            Arc::new(Hub {
                dir: config_dir.to_path_buf(),
                state: Mutex::default(),
                audit_lock: Mutex::new(()),
                skew: AtomicU64::new(0),
            })
        })
        .clone()
}

/// The hub next to a token store.
pub fn hub_for(tokens: &TokenStore) -> Arc<Hub> {
    hub(&tokens.dir())
}

fn digits_of(raw: &str) -> String {
    raw.chars().filter(|c| c.is_ascii_digit()).collect()
}

fn random_digits(n: usize) -> Result<String, String> {
    // Rejection sampling keeps every digit uniform.
    let mut out = String::with_capacity(n);
    while out.len() < n {
        for b in control_api::random_bytes::<16>()? {
            if b < 250 && out.len() < n {
                out.push(char::from(b'0' + b % 10));
            }
        }
    }
    Ok(out)
}

/// The 6-digit code both screens show: binds the one-time code, both nonces and the claim id.
pub fn verification_code(
    code_digits: &str,
    client_nonce: &str,
    server_nonce: &str,
    claim_id: &str,
) -> String {
    let mut h = Sha256::new();
    for part in [code_digits, client_nonce, server_nonce, claim_id] {
        h.update(part.as_bytes());
        h.update([0u8]);
    }
    let d = h.finalize();
    let n = u32::from_be_bytes([d[0], d[1], d[2], d[3]]) % 1_000_000;
    format!("{n:06}")
}

fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 48 || name.chars().any(|c| c.is_control()) {
        return Err(ERR_DEVICE_NAME.to_string());
    }
    Ok(name.to_string())
}

fn clean_platform(p: &str) -> String {
    p.chars()
        .filter(|c| !c.is_control())
        .take(32)
        .collect::<String>()
        .trim()
        .to_string()
}

impl Hub {
    pub fn now(&self) -> u64 {
        control_api::now_secs() + self.skew.load(Ordering::Relaxed)
    }

    /// Moves this hub's clock forward (tests of expiry and lockout).
    #[doc(hidden)]
    pub fn advance_clock(&self, secs: u64) {
        self.skew.fetch_add(secs, Ordering::Relaxed);
    }

    fn devices_store(&self) -> crate::store::JsonStore {
        crate::store::JsonStore::sensitive(self.dir.join(DEVICES_SCHEMA.file))
    }

    fn load_devices(&self) -> Result<Vec<StoredDevice>, String> {
        Ok(self
            .devices_store()
            .load_versioned::<Vec<StoredDevice>>(&DEVICES_SCHEMA)?
            .map(|l| l.data)
            .unwrap_or_default())
    }

    fn update_devices<R>(
        &self,
        f: impl FnOnce(&mut Vec<StoredDevice>) -> Result<R, String>,
    ) -> Result<R, String> {
        self.devices_store()
            .update_versioned::<Vec<StoredDevice>, R>(&DEVICES_SCHEMA, f)
    }

    fn expire(&self, st: &mut HubState, now: u64) {
        st.offers.retain(|o| o.info.expires_at > now);
        // An expired claim is kept for one more window so its device learns it expired.
        st.claims.retain(|c| c.expires_at + CLAIM_TTL_SECS > now);
        st.failures.retain(|t| t + FAIL_WINDOW_SECS > now);
        st.seen.retain(|_, t| *t + 7 * 24 * 3600 > now);
    }

    // --- Audit ---

    /// Appends one line to the audit log (best effort: a full disk must not block a revoke).
    pub fn audit(
        &self,
        event: &str,
        device_id: Option<&str>,
        device_name: Option<&str>,
        detail: Option<&str>,
    ) {
        let entry = AuditEntry {
            at: self.now(),
            event: event.to_string(),
            device_id: device_id.map(str::to_string),
            device_name: device_name.map(str::to_string),
            detail: detail.map(|d| d.chars().filter(|c| !c.is_control()).take(200).collect()),
        };
        let Ok(line) = serde_json::to_string(&entry) else {
            return;
        };
        let _guard = self.audit_lock.lock().unwrap();
        let path = self.dir.join(AUDIT_FILE);
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "{line}");
        }
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > AUDIT_ROTATE_BYTES) {
            if let Ok(text) = std::fs::read_to_string(&path) {
                let lines: Vec<&str> = text.lines().collect();
                let keep = &lines[lines.len().saturating_sub(AUDIT_KEEP)..];
                let _ = std::fs::write(&path, format!("{}\n", keep.join("\n")));
            }
        }
    }

    /// The newest `limit` audit entries, newest first.
    pub fn audit_log(&self, limit: usize) -> Vec<AuditEntry> {
        let _guard = self.audit_lock.lock().unwrap();
        let text = std::fs::read_to_string(self.dir.join(AUDIT_FILE)).unwrap_or_default();
        text.lines()
            .filter_map(|l| serde_json::from_str::<AuditEntry>(l).ok())
            .rev()
            .take(limit.clamp(1, AUDIT_KEEP))
            .collect()
    }

    // --- Offers ---

    pub fn create_offer(&self, spec: &OfferSpec) -> Result<OfferInfo, String> {
        if spec.preset == AccessPreset::Admin && !spec.acknowledge_admin {
            return Err(ERR_ADMIN_ACK.to_string());
        }
        // Same validation the token will get; fail now, not at approval.
        let policy = spec.preset.policy(&spec.models).validate()?;
        let now = self.now();
        let ttl = spec
            .ttl_secs
            .unwrap_or(OFFER_TTL_SECS)
            .clamp(30, MAX_OFFER_TTL_SECS);
        let digits = random_digits(8)?;
        let info = OfferInfo {
            id: control_api::hex(&control_api::random_bytes::<6>()?),
            code: format!("{}-{}", &digits[..4], &digits[4..]),
            role: spec.role,
            preset: spec.preset,
            models: policy.models,
            expires_at: now + ttl,
        };
        {
            let mut st = self.state.lock().unwrap();
            self.expire(&mut st, now);
            if st.offers.len() >= MAX_OFFERS {
                return Err(ERR_TOO_MANY.to_string());
            }
            st.offers.push(Offer {
                info: info.clone(),
                digits,
            });
        }
        self.audit(
            "offer_created",
            None,
            None,
            Some(&format!("{:?}/{:?}", spec.role, spec.preset)),
        );
        Ok(info)
    }

    /// Offers that are still claimable, without their codes.
    pub fn offers(&self) -> Vec<OfferInfo> {
        let now = self.now();
        let mut st = self.state.lock().unwrap();
        self.expire(&mut st, now);
        st.offers
            .iter()
            .map(|o| OfferInfo {
                code: String::new(),
                ..o.info.clone()
            })
            .collect()
    }

    pub fn cancel_offer(&self, id: &str) -> bool {
        let mut st = self.state.lock().unwrap();
        let before = st.offers.len();
        st.offers.retain(|o| o.info.id != id);
        st.offers.len() != before
    }

    // --- Claims ---

    /// The public claim step (see the module docs). The one-time code is consumed.
    pub fn claim(
        &self,
        code: &str,
        name: &str,
        platform: &str,
        client_nonce: &str,
    ) -> Result<ClaimStarted, String> {
        let now = self.now();
        let name = clean_name(name)?;
        if client_nonce.len() < 16
            || client_nonce.len() > 128
            || !client_nonce.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return Err(ERR_NONCE.to_string());
        }
        let digits = digits_of(code);
        let mut st = self.state.lock().unwrap();
        self.expire(&mut st, now);
        if st.failures.len() >= MAX_FAILURES {
            drop(st);
            self.audit("claim_failed", None, Some(&name), Some("locked"));
            return Err(ERR_LOCKED.to_string());
        }
        // Compare against every offer so timing does not reveal which one matched.
        let mut hit = None;
        for (i, o) in st.offers.iter().enumerate() {
            if ct_eq(o.digits.as_bytes(), digits.as_bytes()) {
                hit = Some(i);
            }
        }
        let Some(i) = hit else {
            st.failures.push(now);
            drop(st);
            self.audit("claim_failed", None, Some(&name), Some("invalid_code"));
            return Err(ERR_INVALID_CODE.to_string());
        };
        let pending = st
            .claims
            .iter()
            .filter(|c| matches!(c.stage, ClaimStage::Pending))
            .count();
        if pending >= MAX_CLAIMS {
            return Err(ERR_TOO_MANY.to_string());
        }
        let offer = st.offers.remove(i);
        let claim_id = control_api::hex(&control_api::random_bytes::<8>()?);
        let server_nonce = control_api::new_secret()?;
        let claim = Claim {
            verification: verification_code(&offer.digits, client_nonce, &server_nonce, &claim_id),
            id: claim_id.clone(),
            poll_secret: control_api::new_secret()?,
            name: name.clone(),
            platform: clean_platform(platform),
            role: offer.info.role,
            preset: offer.info.preset,
            models: offer.info.models.clone(),
            requested_at: now,
            expires_at: now + CLAIM_TTL_SECS,
            stage: ClaimStage::Pending,
        };
        let started = ClaimStarted {
            claim_id,
            poll_secret: claim.poll_secret.clone(),
            verification: claim.verification.clone(),
            server_nonce,
            expires_at: claim.expires_at,
        };
        st.claims.push(claim);
        drop(st);
        self.audit("claim_pending", None, Some(&name), None);
        Ok(started)
    }

    /// Claims waiting for the owner.
    pub fn pending(&self) -> Vec<PendingClaim> {
        let now = self.now();
        let mut st = self.state.lock().unwrap();
        self.expire(&mut st, now);
        st.claims
            .iter()
            .filter(|c| matches!(c.stage, ClaimStage::Pending) && c.expires_at > now)
            .map(|c| PendingClaim {
                id: c.id.clone(),
                name: c.name.clone(),
                platform: c.platform.clone(),
                role: c.role,
                preset: c.preset,
                models: c.models.clone(),
                verification: c.verification.clone(),
                requested_at: c.requested_at,
                expires_at: c.expires_at,
            })
            .collect()
    }

    /// The owner approves a pending claim: mints the scoped token and registers the device.
    pub fn approve(&self, claim_id: &str, tokens: &TokenStore) -> Result<DeviceView, String> {
        let now = self.now();
        let (name, platform, role, preset, models) = {
            let mut st = self.state.lock().unwrap();
            self.expire(&mut st, now);
            let c = st
                .claims
                .iter()
                .find(|c| c.id == claim_id)
                .ok_or_else(|| ERR_NOT_FOUND.to_string())?;
            if !matches!(c.stage, ClaimStage::Pending) {
                return Err(ERR_NOT_PENDING.to_string());
            }
            if c.expires_at <= now {
                return Err(ERR_EXPIRED.to_string());
            }
            (
                c.name.clone(),
                c.platform.clone(),
                c.role,
                c.preset,
                c.models.clone(),
            )
        };
        let created = tokens.create_with_policy(
            &format!("device: {name}"),
            &preset.scopes(),
            &preset.policy(&models),
        )?;
        let device = StoredDevice {
            id: control_api::hex(&control_api::random_bytes::<6>()?),
            name: name.clone(),
            platform,
            role,
            preset,
            token_id: created.info.id.clone(),
            created_at: now,
            report: None,
        };
        let stored = device.clone();
        if let Err(e) = self.update_devices(|list| {
            list.push(stored);
            Ok(())
        }) {
            // Never leave a token nobody tracks.
            let _ = tokens.revoke(&created.info.id);
            return Err(e);
        }
        {
            let mut st = self.state.lock().unwrap();
            if let Some(c) = st.claims.iter_mut().find(|c| c.id == claim_id) {
                c.stage = ClaimStage::Approved(Box::new(ClaimGrant {
                    token: created.token,
                    device_id: device.id.clone(),
                    token_id: created.info.id.clone(),
                }));
            }
        }
        self.audit("approved", Some(&device.id), Some(&name), None);
        Ok(self.view(&device, Some(&created.info), now))
    }

    /// The owner refuses a pending claim.
    pub fn deny(&self, claim_id: &str) -> Result<(), String> {
        let now = self.now();
        let name = {
            let mut st = self.state.lock().unwrap();
            self.expire(&mut st, now);
            let c = st
                .claims
                .iter_mut()
                .find(|c| c.id == claim_id)
                .ok_or_else(|| ERR_NOT_FOUND.to_string())?;
            if !matches!(c.stage, ClaimStage::Pending) {
                return Err(ERR_NOT_PENDING.to_string());
            }
            c.stage = ClaimStage::Denied;
            c.name.clone()
        };
        self.audit("denied", None, Some(&name), None);
        Ok(())
    }

    /// The device polls its claim. An approval hands out the token once.
    pub fn poll(
        &self,
        claim_id: &str,
        poll_secret: &str,
        tokens: &TokenStore,
    ) -> Result<ClaimPoll, String> {
        let now = self.now();
        let mut st = self.state.lock().unwrap();
        self.expire(&mut st, now);
        let idx = st
            .claims
            .iter()
            .position(|c| {
                c.id == claim_id && ct_eq(c.poll_secret.as_bytes(), poll_secret.as_bytes())
            })
            .ok_or_else(|| ERR_NOT_FOUND.to_string())?;
        let expired = st.claims[idx].expires_at <= now;
        match &st.claims[idx].stage {
            ClaimStage::Pending if expired => {
                st.claims.remove(idx);
                Ok(ClaimPoll::bare(ClaimState::Expired))
            }
            ClaimStage::Pending => Ok(ClaimPoll::bare(ClaimState::Pending)),
            ClaimStage::Denied => {
                st.claims.remove(idx);
                Ok(ClaimPoll::bare(ClaimState::Denied))
            }
            ClaimStage::Approved(grant) => {
                let grant = (**grant).clone();
                let claim = st.claims.remove(idx);
                drop(st);
                let info = tokens.get(&grant.token_id).ok();
                Ok(ClaimPoll {
                    status: ClaimState::Approved,
                    token: Some(grant.token),
                    device_id: Some(grant.device_id),
                    role: Some(claim.role),
                    preset: Some(claim.preset),
                    scopes: info.as_ref().map(|i| i.scopes.clone()).unwrap_or_default(),
                    endpoints: info
                        .as_ref()
                        .map(|i| i.policy.endpoints.clone())
                        .unwrap_or_default(),
                    models: claim.models,
                })
            }
        }
    }

    // --- Devices ---

    fn view(&self, d: &StoredDevice, info: Option<&TokenInfo>, now: u64) -> DeviceView {
        let seen = self.state.lock().unwrap().seen.get(&d.token_id).copied();
        let last_seen = seen.or_else(|| d.report.as_ref().map(|r| r.reported_at));
        let reachability = match last_seen {
            Some(t) if t + ONLINE_WINDOW_SECS >= now => Reachability::Online,
            Some(_) => Reachability::Offline,
            None => Reachability::Never,
        };
        let installed = d
            .report
            .as_ref()
            .map(|r| r.components.clone())
            .unwrap_or_default();
        DeviceView {
            id: d.id.clone(),
            name: d.name.clone(),
            platform: d.platform.clone(),
            role: d.role,
            preset: d.preset,
            created_at: d.created_at,
            last_seen,
            reachability,
            scopes: info.map(|i| i.scopes.clone()).unwrap_or_default(),
            endpoints: info.map(|i| i.policy.endpoints.clone()).unwrap_or_default(),
            models: info.map(|i| i.policy.models.clone()).unwrap_or_default(),
            report: d.report.clone(),
            components: d.role.components().iter().map(|c| c.to_string()).collect(),
            missing_components: d.role.missing(&installed),
        }
    }

    /// Paired devices. A device whose token was removed elsewhere (the Tokens list) is
    /// dropped here too.
    pub fn list(&self, tokens: &TokenStore) -> Result<Vec<DeviceView>, String> {
        let now = self.now();
        let infos = tokens.list()?;
        let devices = self.load_devices()?;
        let (live, gone): (Vec<_>, Vec<_>) = devices
            .into_iter()
            .partition(|d| infos.iter().any(|i| i.id == d.token_id));
        if !gone.is_empty() {
            let ids: Vec<String> = gone.iter().map(|d| d.id.clone()).collect();
            let _ = self.update_devices(|list| {
                list.retain(|d| !ids.contains(&d.id));
                Ok(())
            });
            for d in &gone {
                self.audit("token_removed", Some(&d.id), Some(&d.name), None);
            }
        }
        Ok(live
            .iter()
            .map(|d| self.view(d, infos.iter().find(|i| i.id == d.token_id), now))
            .collect())
    }

    /// Notes that the token spoke to the gateway (memory only: no disk write per request).
    pub fn touch(&self, token_id: &str) {
        let now = self.now();
        self.state
            .lock()
            .unwrap()
            .seen
            .insert(token_id.to_string(), now);
    }

    /// A device's self-report, keyed by the token it presented. `Ok(false)` = that token
    /// belongs to no paired device (a plain named token).
    pub fn report(&self, token_id: &str, report: DeviceReport) -> Result<bool, String> {
        let report = report.sanitize(self.now());
        let found = self.update_devices(|list| {
            Ok(match list.iter_mut().find(|d| d.token_id == token_id) {
                Some(d) => {
                    d.report = Some(report.clone());
                    true
                }
                None => false,
            })
        })?;
        self.touch(token_id);
        Ok(found)
    }

    /// Renames a device and/or changes its role.
    pub fn update(
        &self,
        id: &str,
        name: Option<&str>,
        role: Option<DeviceRole>,
        tokens: &TokenStore,
    ) -> Result<DeviceView, String> {
        let name = name.map(clean_name).transpose()?;
        let d = self.update_devices(|list| {
            let d = list
                .iter_mut()
                .find(|d| d.id == id)
                .ok_or_else(|| ERR_DEVICE_NOT_FOUND.to_string())?;
            if let Some(n) = &name {
                d.name = n.clone();
            }
            if let Some(r) = role {
                d.role = r;
            }
            Ok(d.clone())
        })?;
        let info = tokens.get(&d.token_id).ok();
        Ok(self.view(&d, info.as_ref(), self.now()))
    }

    /// Changes what a paired device may do (applies from its next request).
    pub fn set_access(
        &self,
        id: &str,
        preset: AccessPreset,
        models: &[String],
        acknowledge_admin: bool,
        tokens: &TokenStore,
    ) -> Result<DeviceView, String> {
        if preset == AccessPreset::Admin && !acknowledge_admin {
            return Err(ERR_ADMIN_ACK.to_string());
        }
        let device = self
            .load_devices()?
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| ERR_DEVICE_NOT_FOUND.to_string())?;
        let policy = preset.policy(models);
        tokens.update(
            &device.token_id,
            &control_api::TokenUpdate {
                scopes: Some(preset.scopes()),
                models: Some(policy.models.clone()),
                endpoints: Some(policy.endpoints.clone()),
                ..Default::default()
            },
        )?;
        let d = self.update_devices(|list| {
            let d = list
                .iter_mut()
                .find(|d| d.id == id)
                .ok_or_else(|| ERR_DEVICE_NOT_FOUND.to_string())?;
            d.preset = preset;
            Ok(d.clone())
        })?;
        self.audit(
            "access_changed",
            Some(&d.id),
            Some(&d.name),
            Some(&format!("{preset:?}")),
        );
        let info = tokens.get(&d.token_id).ok();
        Ok(self.view(&d, info.as_ref(), self.now()))
    }

    /// Unpairs a device: its token stops working immediately (the next request gets 401).
    pub fn revoke(&self, id: &str, tokens: &TokenStore) -> Result<(), String> {
        let d = self.update_devices(|list| {
            let pos = list
                .iter()
                .position(|d| d.id == id)
                .ok_or_else(|| ERR_DEVICE_NOT_FOUND.to_string())?;
            Ok(list.remove(pos))
        })?;
        let _ = tokens.revoke(&d.token_id);
        self.state.lock().unwrap().seen.remove(&d.token_id);
        self.audit("revoked", Some(&d.id), Some(&d.name), None);
        Ok(())
    }

    /// Lost-phone recovery: unpairs every device, cancels open offers and pending claims.
    /// Returns how many devices were unpaired. Named tokens that pairing did not make are
    /// left alone.
    pub fn revoke_all(&self, tokens: &TokenStore) -> Result<usize, String> {
        let removed = self.update_devices(|list| Ok(std::mem::take(list)))?;
        for d in &removed {
            let _ = tokens.revoke(&d.token_id);
        }
        {
            let mut st = self.state.lock().unwrap();
            st.offers.clear();
            st.claims.clear();
            st.seen.clear();
        }
        self.audit(
            "revoke_all",
            None,
            None,
            Some(&format!("{} devices", removed.len())),
        );
        Ok(removed.len())
    }
}

// --- Host suggestion ---

/// A device that could run a model this computer cannot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct HostSuggestion {
    pub device_id: String,
    pub name: String,
    /// Memory it reported free for a model (free VRAM, else its RAM).
    pub available_bytes: u64,
    /// Components to install for the role it will play (only those it lacks).
    pub missing_components: Vec<String>,
    pub reachability: Reachability,
}

/// Suggests paired inference hosts for a model of `required_bytes` when the local capacity
/// `local_bytes` is not enough (`None` = this computer fits it). Best fit first: online
/// devices with the most headroom. Only devices that reported their memory are considered.
pub fn suggest_hosts(
    required_bytes: u64,
    local_bytes: u64,
    devices: &[DeviceView],
) -> Option<Vec<HostSuggestion>> {
    if local_bytes >= required_bytes {
        return None;
    }
    let mut out: Vec<HostSuggestion> = devices
        .iter()
        .filter(|d| d.role == DeviceRole::InferenceHost && d.reachability != Reachability::Never)
        .filter_map(|d| {
            let r = d.report.as_ref()?;
            let available = r.vram_free_bytes.or(r.ram_total_bytes)?;
            (available >= required_bytes).then(|| HostSuggestion {
                device_id: d.id.clone(),
                name: d.name.clone(),
                available_bytes: available,
                missing_components: d.missing_components.clone(),
                reachability: d.reachability,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        (b.reachability == Reachability::Online)
            .cmp(&(a.reachability == Reachability::Online))
            .then(b.available_bytes.cmp(&a.available_bytes))
    });
    Some(out)
}

// --- OpenCode ---

/// An `opencode.json` provider block that reaches a paired Gateway with a scoped token. The
/// token itself is read from `HARNESS_REMOTE_TOKEN`, so the file holds no secret.
pub fn opencode_config(base_url: &str, models: &[String]) -> serde_json::Value {
    let base = base_url.trim_end_matches('/');
    let base = if base.ends_with("/v1") {
        base.to_string()
    } else {
        format!("{base}/v1")
    };
    let models: serde_json::Map<String, serde_json::Value> = models
        .iter()
        .map(|m| (m.clone(), serde_json::json!({ "name": m })))
        .collect();
    serde_json::json!({
        "$schema": "https://opencode.ai/config.json",
        "provider": {
            "harness-remote": {
                "npm": "@ai-sdk/openai-compatible",
                "name": "Harness Studio (remote)",
                "options": { "baseURL": base, "apiKey": "{env:HARNESS_REMOTE_TOKEN}" },
                "models": models
            }
        }
    })
}

// --- StrukLabs signaling (interface only) ---

/// A device announcing itself to a discovery service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalAnnouncement {
    pub device_name: String,
    pub role: DeviceRole,
    /// Where peers can reach its gateway directly (LAN or VPN address).
    pub address: Option<String>,
}

/// A device found by a discovery service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalPeer {
    pub device_name: String,
    pub role: DeviceRole,
    pub address: Option<String>,
}

/// Identity, discovery and signaling for pairing across networks. StrukLabs may implement
/// this later; it exchanges addresses and offers only. Inference traffic always goes
/// directly between the paired devices, never through the provider: there is no relay method
/// on purpose.
pub trait SignalingProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn announce(&self, device: &SignalAnnouncement) -> Result<(), String>;
    fn discover(&self) -> Result<Vec<SignalPeer>, String>;
}

/// The default: no service; pairing happens by code on a reachable address.
pub struct LocalOnlySignaling;

impl SignalingProvider for LocalOnlySignaling {
    fn name(&self) -> &'static str {
        "local"
    }
    fn announce(&self, _device: &SignalAnnouncement) -> Result<(), String> {
        Ok(())
    }
    fn discover(&self) -> Result<Vec<SignalPeer>, String> {
        Ok(Vec::new())
    }
}
