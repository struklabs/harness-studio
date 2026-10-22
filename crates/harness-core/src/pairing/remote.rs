//! Browser protocol adapter. Only authenticated Noise sessions may call this adapter.
//! The legacy HTTP pairing contract remains unchanged.
use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use blake2::{Blake2s256, Digest};
use serde_json::{json, Value};

pub mod apps;
pub mod frames;
mod json_strict;
pub mod noise;
pub mod relay;
pub mod runtime;
#[cfg(test)]
mod tests;

pub const ERR_PROTOCOL: &str = "pairing.remote_protocol";
pub const ERR_KEYCHAIN: &str = "pairing.remote_keychain";
pub const ERR_REVOKED: &str = "pairing.remote_revoked";
/// The reveal does not open the commitment; the claim is burned (HTTP 403).
pub const ERR_INVALID_REVEAL: &str = "pairing.invalid_reveal";
/// A pairing body carried a key field. Keys come only from the Noise session (HTTP 400).
pub const ERR_KEY_IN_BODY: &str = "pairing.key_in_body";

/// A pending claim lives this long from the claim itself (APP_PROTOCOL 4.1).
pub const REMOTE_CLAIM_TTL_SECS: u64 = 180;
/// An approved token is re-delivered by the same poll for this long after the first delivery,
/// until the browser acknowledges it.
pub const TOKEN_REDELIVERY_SECS: u64 = 60;
/// An approved claim waits this long for its first poll before the token copy is wiped.
pub const TOKEN_DELIVERY_WINDOW_SECS: u64 = 300;
pub(super) const MAX_OFFER_FAILURES: usize = 15;
pub const POLL_INTERVAL_MS: u64 = 2000;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteAccessSettingsV1 {
    pub enabled: bool,
}

impl Hub {
    pub fn remote_settings(&self) -> RemoteAccessSettingsV1 {
        crate::store::JsonStore::new(self.dir.join("remote_access_v1.json")).load_or_default()
    }
    pub fn set_remote_settings(&self, settings: &RemoteAccessSettingsV1) -> Result<(), String> {
        if settings.enabled && !cfg!(feature = "remote-access") {
            return Err("pairing.remote_not_built".into());
        }
        crate::store::JsonStore::new(self.dir.join("remote_access_v1.json")).save(settings)
    }
}

pub fn encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn decode32(value: &str) -> Result<[u8; 32], String> {
    let bytes = URL_SAFE_NO_PAD.decode(value).map_err(|_| ERR_PROTOCOL)?;
    let out: [u8; 32] = bytes.try_into().map_err(|_| ERR_PROTOCOL)?;
    if encode(&out) != value {
        return Err(ERR_PROTOCOL.into());
    }
    Ok(out)
}

fn lp(h: &mut Blake2s256, part: &[u8]) {
    h.update((part.len() as u16).to_be_bytes());
    h.update(part);
}

/// SAS commitment (review C1). Covers nc, Kc, the Noise handshake hash and the device name.
/// `claim_id` is server-assigned and never an input.
pub fn commitment(nc: &[u8; 32], kc: &[u8; 32], hash: &[u8], name: &str) -> [u8; 32] {
    let mut h = Blake2s256::new();
    h.update(b"struklabs/pairing/sas-commit/v1");
    for part in [&nc[..], &kc[..], hash, name.as_bytes()] {
        lp(&mut h, part);
    }
    h.finalize().into()
}

pub(super) fn poll_digest(id: &str, secret: &str) -> String {
    let mut h = Blake2s256::new();
    h.update(b"struklabs/pairing/poll/v1");
    h.update(id.as_bytes());
    h.update([0]);
    h.update(secret.as_bytes());
    encode(&h.finalize())
}

pub fn fingerprint(key: &[u8; 32]) -> String {
    let mut h = Blake2s256::new();
    h.update(b"struklabs/pairing/fingerprint/v1");
    h.update(key);
    encode(&h.finalize()[..16])
}

/// 6-digit SAS. Both static keys and the handshake hash come from the Noise session, never
/// from message bodies. `claim_id` is deliberately excluded (a relay could grind it).
pub fn sas(
    nc: &[u8; 32],
    nd: &[u8; 32],
    kc: &[u8; 32],
    kd: &[u8; 32],
    hash: &[u8],
    name: &str,
) -> String {
    let mut h = Blake2s256::new();
    h.update(b"struklabs/pairing/sas/v1");
    for part in [&nc[..], &nd[..], &kc[..], &kd[..], hash, name.as_bytes()] {
        lp(&mut h, part);
    }
    let d = h.finalize();
    format!(
        "{:06}",
        u64::from_be_bytes(d[..8].try_into().unwrap()) % 1_000_000
    )
}

pub(super) struct CommittedClaim {
    commit: [u8; 32],
    hash: Vec<u8>,
    name: String,
    nd: [u8; 32],
    desktop_key: [u8; 32],
    pub client_key: [u8; 32],
    pub revealed: bool,
    /// Set when the owner approves; the token copy is wiped `TOKEN_DELIVERY_WINDOW_SECS` later
    /// if it was never polled.
    pub approved_at: Option<u64>,
    /// Set on the first delivery; the same poll re-delivers until `ack` or the redelivery window.
    pub delivered_at: Option<u64>,
}

fn field<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v[key].as_str().ok_or_else(|| ERR_PROTOCOL.into())
}

/// Static keys come only from the Noise session. A body that names one is refused outright:
/// it is never compared and never used (APP_PROTOCOL 4.3, review C1).
fn reject_key_fields(body: &Value) -> Result<(), String> {
    if let Some(map) = body.as_object() {
        let forbidden = |k: &str| {
            let k = k.to_ascii_lowercase();
            k.contains("static") || k.contains("pub") || k == "kc" || k == "kd" || k.contains("key")
        };
        if map.keys().any(|k| forbidden(k)) {
            return Err(ERR_KEY_IN_BODY.into());
        }
    }
    Ok(())
}

fn rfc3339(secs: u64) -> Result<String, String> {
    Ok(chrono::DateTime::from_timestamp(secs as i64, 0)
        .ok_or(ERR_PROTOCOL)?
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

impl Hub {
    /// Atomically consumes an offer and installs a commitment before exposing desktop entropy.
    fn remote_claim(
        &self,
        body: &Value,
        client_key: [u8; 32],
        desktop_key: [u8; 32],
        hash: &[u8],
    ) -> Result<Value, String> {
        let commit = decode32(field(body, "commit")?)?;
        let name = field(body, "name")?.trim();
        if name.is_empty() || name.encode_utf16().count() > 64 || name.chars().any(char::is_control)
        {
            return Err(ERR_DEVICE_NAME.into());
        }
        let name = name.to_string();
        let code = field(body, "code")?;
        // The desktop nonce is generated only after the commitment was decoded and is sent back
        // in this answer.
        let nd = control_api::random_bytes::<32>()?;
        // `claim` installs a legacy pending record; hold an outer transaction lock so owner
        // approval cannot see it before the commitment has been attached.
        let now = self.now();
        let mut st = self.state.lock().unwrap();
        self.expire(&mut st, now);
        if st.failures.len() >= MAX_FAILURES {
            return Err(ERR_LOCKED.into());
        }
        let mut hit = None;
        for (i, offer) in st.offers.iter().enumerate() {
            if ct_eq(offer.digits.as_bytes(), code.as_bytes()) {
                hit = Some(i);
            }
        }
        let Some(i) = hit else {
            let burned = st.remote_code_failure(now);
            drop(st);
            if burned {
                self.audit("offer_burned", None, None, Some("wrong_code_limit"));
            }
            return Err(ERR_INVALID_CODE.into());
        };
        if st.claims.len() >= MAX_CLAIMS {
            return Err(ERR_TOO_MANY.into());
        }
        let id = control_api::hex(&control_api::random_bytes::<8>()?);
        let secret = control_api::new_secret()?;
        let offer = st.offers.remove(i);
        let expires_at = now + REMOTE_CLAIM_TTL_SECS;
        st.claims.push(Claim {
            id: id.clone(),
            poll_secret: poll_digest(&id, &secret),
            name: name.clone(),
            platform: "browser".into(),
            role: offer.info.role,
            preset: offer.info.preset,
            models: offer.info.models,
            verification: String::new(),
            requested_at: now,
            expires_at,
            stage: ClaimStage::Pending,
            remote: Some(CommittedClaim {
                commit,
                hash: hash.to_vec(),
                name,
                nd,
                desktop_key,
                client_key,
                revealed: false,
                approved_at: None,
                delivered_at: None,
            }),
        });
        Ok(json!({"claim_id":id,"poll_secret":secret,"nd":encode(&nd),
            "expires_at":rfc3339(expires_at)?,"poll_interval_ms":POLL_INTERVAL_MS}))
    }

    /// Opens the commitment with `nc`. The claim is found by id and must belong to the calling
    /// Noise session's key; keys and `h` come from the session, never from the body.
    fn remote_reveal(&self, body: &Value, peer: &[u8; 32]) -> Result<Value, String> {
        let nc = decode32(field(body, "nc")?)?;
        let id = field(body, "claim_id")?;
        let now = self.now();
        let mut st = self.state.lock().unwrap();
        self.expire(&mut st, now);
        let idx = st
            .claims
            .iter()
            .position(|c| {
                c.id == id
                    && c.remote
                        .as_ref()
                        .is_some_and(|r| ct_eq(&r.client_key, peer))
            })
            .ok_or(ERR_NOT_FOUND)?;
        let c = &mut st.claims[idx];
        if c.expires_at <= now {
            return Err(ERR_EXPIRED.into());
        }
        let r = c.remote.as_mut().ok_or(ERR_PROTOCOL)?;
        // One reveal per claim.
        if r.revealed || !matches!(c.stage, ClaimStage::Pending) {
            return Err(ERR_NOT_PENDING.into());
        }
        if !ct_eq(&commitment(&nc, peer, &r.hash, &r.name), &r.commit) {
            // A failed reveal erases the claim and counts a strike. It cannot be retried.
            st.claims.remove(idx);
            st.failures.push(now);
            return Err(ERR_INVALID_REVEAL.into());
        }
        r.revealed = true;
        c.verification = sas(&nc, &r.nd, peer, &r.desktop_key, &r.hash, &r.name);
        Ok(json!({"status":"revealed"}))
    }

    /// Index of a revealed claim that belongs to this Noise key and whose poll secret matches.
    fn remote_owned(&self, st: &HubState, body: &Value, peer: &[u8; 32]) -> Result<usize, String> {
        let id = field(body, "claim_id")?;
        let secret = field(body, "poll_secret")?;
        st.claims
            .iter()
            .position(|c| {
                c.id == id
                    && c.matches_poll(secret)
                    && c.remote
                        .as_ref()
                        .is_some_and(|r| r.revealed && ct_eq(&r.client_key, peer))
            })
            .ok_or_else(|| ERR_NOT_FOUND.to_string())
    }

    fn remote_poll(
        &self,
        body: &Value,
        peer: &[u8; 32],
        desktop_key: [u8; 32],
        desktop_id: &str,
        desktop_name: &str,
    ) -> Result<(u16, Value), String> {
        let now = self.now();
        let mut st = self.state.lock().unwrap();
        self.expire(&mut st, now);
        let idx = self.remote_owned(&st, body, peer)?;
        let expired = st.claims[idx].expires_at <= now;
        match &st.claims[idx].stage {
            ClaimStage::Pending if expired => {
                st.claims.remove(idx);
                Err(ERR_EXPIRED.into())
            }
            ClaimStage::Pending => Ok((202, json!({"status":"pending"}))),
            ClaimStage::Denied => {
                st.claims.remove(idx);
                Ok((
                    403,
                    json!({"error":{"code":"pairing.pending_denied","message":"The owner denied this device."}}),
                ))
            }
            ClaimStage::Approved(grant) => {
                let grant = (**grant).clone();
                let claim = &mut st.claims[idx];
                // The token stays redeliverable until `ack` (or the redelivery window ends).
                if let Some(r) = claim.remote.as_mut() {
                    r.delivered_at.get_or_insert(now);
                }
                let models = claim.models.clone();
                Ok((
                    200,
                    json!({"status":"approved","token":grant.token,"token_type":"bearer","device_id":grant.device_id,
                    "desktop_id":desktop_id,"desktop_name":desktop_name,"scopes":["status.read","models.read","chat.create"],
                    "token_policy":{"allowed_models":if models.is_empty(){json!("*")}else{json!(models)},"rate_limit":{"requests_per_minute":60,"max_concurrent_chats":1}},
                    "noise_static_pub":encode(&desktop_key)}),
                ))
            }
        }
    }

    /// The browser stored the token and acknowledges it; the desktop wipes its redelivery copy.
    fn remote_ack(&self, body: &Value, peer: &[u8; 32]) -> Result<Value, String> {
        let now = self.now();
        let mut st = self.state.lock().unwrap();
        self.expire(&mut st, now);
        let idx = self.remote_owned(&st, body, peer)?;
        let delivered = matches!(st.claims[idx].stage, ClaimStage::Approved(_))
            && st.claims[idx]
                .remote
                .as_ref()
                .is_some_and(|r| r.delivered_at.is_some());
        if !delivered {
            return Err(ERR_NOT_FOUND.into());
        }
        st.claims.remove(idx);
        Ok(json!({"status":"acknowledged"}))
    }

    pub fn remote_authorize(
        &self,
        token: &str,
        key: &[u8; 32],
        tokens: &TokenStore,
    ) -> Result<(String, TokenInfo), String> {
        let (info, _) = tokens.verify_noise(token, key).ok_or(ERR_REVOKED)?;
        let device = self
            .load_devices()?
            .into_iter()
            .find(|d| {
                d.token_id == info.id && d.noise_static_pub.as_deref() == Some(encode(key).as_str())
            })
            .ok_or(ERR_REVOKED)?;
        Ok((device.id, info))
    }

    /// Browser wire bodies and status mapping; runs exclusively inside a Noise pair session.
    #[allow(clippy::too_many_arguments)]
    pub fn remote_pair(
        &self,
        op: &str,
        body: &Value,
        peer: [u8; 32],
        desktop_key: [u8; 32],
        hash: &[u8],
        desktop_id: &str,
        desktop_name: &str,
        _tokens: &TokenStore,
    ) -> (u16, Value) {
        let result = (|| -> Result<(u16, Value), String> {
            reject_key_fields(body)?;
            match op {
                "claim" => Ok((201, self.remote_claim(body, peer, desktop_key, hash)?)),
                "reveal" => Ok((200, self.remote_reveal(body, &peer)?)),
                "poll" => self.remote_poll(body, &peer, desktop_key, desktop_id, desktop_name),
                "ack" => Ok((200, self.remote_ack(body, &peer)?)),
                _ => Err(ERR_PROTOCOL.into()),
            }
        })();
        match result {
            Ok(answer) => answer,
            Err(code) => {
                let code = if code == ERR_TOO_MANY {
                    ERR_LOCKED.to_string()
                } else {
                    code
                };
                let status = match code.as_str() {
                    ERR_INVALID_CODE | ERR_INVALID_REVEAL => 403,
                    ERR_NOT_FOUND | ERR_EXPIRED => 410,
                    ERR_LOCKED | ERR_TOO_MANY => 429,
                    _ => 400,
                };
                let message = match code.as_str() {
                    ERR_KEY_IN_BODY => {
                        "Keys are taken from the Noise session; a message body must not carry one."
                    }
                    ERR_INVALID_REVEAL => {
                        "The reveal does not open the commitment; the claim was burned."
                    }
                    _ => "Pairing could not be completed.",
                };
                let mut error = json!({"code":code,"message":message});
                if code == ERR_LOCKED {
                    error["retry_after_s"] = json!(60);
                }
                (status, json!({"error":error}))
            }
        }
    }
}
