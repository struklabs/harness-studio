//! Browser protocol adapter. Only authenticated Noise sessions may call this adapter.
//! The legacy HTTP pairing contract remains unchanged.
use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use blake2::{Blake2s256, Digest};
use serde_json::{json, Value};

pub mod apps;
pub mod noise;
pub mod relay;
pub mod runtime;
#[cfg(test)]
mod tests;

pub const ERR_PROTOCOL: &str = "pairing.remote_protocol";
pub const ERR_KEYCHAIN: &str = "pairing.remote_keychain";
pub const ERR_REVOKED: &str = "pairing.remote_revoked";

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

pub fn commitment(nonce: &[u8; 32], key: &[u8; 32]) -> [u8; 32] {
    let mut h = Blake2s256::new();
    h.update(b"struklabs/pairing/commit/v1");
    h.update(nonce);
    h.update(key);
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

pub fn sas(id: &str, nc: &[u8; 32], nd: &[u8; 32], kc: &[u8; 32], kd: &[u8; 32]) -> String {
    let mut h = Blake2s256::new();
    h.update(b"struklabs/pairing/verification/v1");
    for part in [id.as_bytes(), nc, nd, kc, kd] {
        h.update((part.len() as u16).to_be_bytes());
        h.update(part);
    }
    let d = h.finalize();
    format!(
        "{:06}",
        u32::from_be_bytes(d[..4].try_into().unwrap()) % 1_000_000
    )
}

pub(super) struct CommittedClaim {
    commit: [u8; 32],
    nonce_d: [u8; 32],
    desktop_key: [u8; 32],
    pub client_key: [u8; 32],
    pub revealed: bool,
}

fn field<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v[key].as_str().ok_or_else(|| ERR_PROTOCOL.into())
}

impl Hub {
    /// Atomically consumes an offer and installs a commitment before exposing desktop entropy.
    fn remote_claim(
        &self,
        body: &Value,
        client_key: [u8; 32],
        desktop_key: [u8; 32],
    ) -> Result<Value, String> {
        let commit = decode32(field(body, "commit")?)?;
        let name = field(body, "name")?.trim();
        if name.is_empty() || name.encode_utf16().count() > 64 || name.chars().any(char::is_control)
        {
            return Err(ERR_DEVICE_NAME.into());
        }
        let name = name.to_string();
        let code = field(body, "code")?;
        if code.len() != 8 || !code.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ERR_INVALID_CODE.into());
        }
        let nonce_d = control_api::random_bytes::<32>()?;
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
            st.failures.push(now);
            return Err(ERR_INVALID_CODE.into());
        };
        if st.claims.len() >= MAX_CLAIMS {
            return Err(ERR_TOO_MANY.into());
        }
        let id = control_api::hex(&control_api::random_bytes::<8>()?);
        let secret = control_api::new_secret()?;
        let offer = st.offers.remove(i);
        st.claims.push(Claim {
            id: id.clone(),
            poll_secret: poll_digest(&id, &secret),
            name,
            platform: "browser".into(),
            role: offer.info.role,
            preset: offer.info.preset,
            models: offer.info.models,
            verification: String::new(),
            requested_at: now,
            expires_at: now + CLAIM_TTL_SECS,
            stage: ClaimStage::Pending,
            remote: Some(CommittedClaim {
                commit,
                nonce_d,
                desktop_key,
                client_key,
                revealed: false,
            }),
        });
        Ok(
            json!({"claim_id":id,"poll_secret":secret,"nonce_d":encode(&nonce_d),
            "static_pub_d":encode(&desktop_key),"expires_at":chrono::DateTime::from_timestamp((now + CLAIM_TTL_SECS) as i64,0).ok_or(ERR_PROTOCOL)?.to_rfc3339_opts(chrono::SecondsFormat::Secs,true),"poll_interval_ms":2000}),
        )
    }

    fn remote_reveal(&self, body: &Value, peer: &[u8; 32]) -> Result<Value, String> {
        let nc = decode32(field(body, "nonce_c")?)?;
        let kc = decode32(field(body, "static_pub_c")?)?;
        let mut st = self.state.lock().unwrap();
        let c = st
            .claims
            .iter_mut()
            .find(|c| {
                c.id == body["claim_id"].as_str().unwrap_or("")
                    && c.matches_poll(body["poll_secret"].as_str().unwrap_or(""))
            })
            .ok_or(ERR_NOT_FOUND)?;
        if c.expires_at <= self.now() {
            return Err(ERR_EXPIRED.into());
        }
        let r = c.remote.as_mut().ok_or(ERR_PROTOCOL)?;
        // A failed reveal burns the claim. It cannot be retried with another nonce/key.
        if r.revealed || !matches!(c.stage, ClaimStage::Pending) {
            return Err(ERR_NOT_PENDING.into());
        }
        if !ct_eq(&kc, peer)
            || !ct_eq(&kc, &r.client_key)
            || !ct_eq(&commitment(&nc, &kc), &r.commit)
        {
            c.stage = ClaimStage::Denied;
            return Err(ERR_PROTOCOL.into());
        }
        r.revealed = true;
        c.verification = sas(&c.id, &nc, &r.nonce_d, &kc, &r.desktop_key);
        Ok(json!({"verification_code":c.verification}))
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
        desktop_id: &str,
        desktop_name: &str,
        tokens: &TokenStore,
    ) -> (u16, Value) {
        let result = (|| -> Result<(u16, Value), String> {
            match op {
                "claim" => Ok((201, self.remote_claim(body, peer, desktop_key)?)),
                "reveal" => Ok((200, self.remote_reveal(body, &peer)?)),
                "poll" => {
                    let id = field(body, "claim_id")?;
                    let secret = field(body, "poll_secret")?;
                    {
                        let st = self.state.lock().unwrap();
                        let c = st.claims.iter().find(|c| c.id == id).ok_or(ERR_NOT_FOUND)?;
                        let r = c.remote.as_ref().ok_or(ERR_NOT_FOUND)?;
                        if !ct_eq(&r.client_key, &peer) || !r.revealed {
                            return Err(ERR_NOT_FOUND.into());
                        }
                    }
                    let p = self.poll_internal(id, secret, tokens, true)?;
                    match p.status {
                        ClaimState::Pending => Ok((202, json!({"status":"pending"}))),
                        ClaimState::Denied => Ok((
                            403,
                            json!({"error":{"code":"pairing.pending_denied","message":"The owner denied this device."}}),
                        )),
                        ClaimState::Expired => Err(ERR_EXPIRED.into()),
                        ClaimState::Approved => Ok((
                            200,
                            json!({"status":"approved","token":p.token,"token_type":"bearer","device_id":p.device_id,
                            "desktop_id":desktop_id,"desktop_name":desktop_name,"scopes":["status.read","models.read","chat.create"],
                            "token_policy":{"allowed_models":if p.models.is_empty(){json!("*")}else{json!(p.models)},"rate_limit":{"requests_per_minute":60,"max_concurrent_chats":1}},
                            "noise_static_pub":encode(&desktop_key)}),
                        )),
                    }
                }
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
                    ERR_INVALID_CODE => 403,
                    ERR_NOT_FOUND | ERR_EXPIRED => 410,
                    ERR_LOCKED | ERR_TOO_MANY => 429,
                    _ => 400,
                };
                let mut error = json!({"code":code,"message":"Pairing could not be completed."});
                if code == ERR_LOCKED {
                    error["retry_after_s"] = json!(60);
                }
                (status, json!({"error":error}))
            }
        }
    }
}
