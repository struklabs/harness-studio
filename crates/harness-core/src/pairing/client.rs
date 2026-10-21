//! The claiming side of pairing: this computer (or a script) pairs with another Harness
//! host by its one-time code, then keeps the scoped token as a remote link so the local
//! gateway, the model lists and OpenCode can use that host.

use super::{ClaimPoll, ClaimStarted, ClaimState};
use crate::control_api;
use crate::error_key::err;
use crate::link;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const ANSWER_LIMIT: usize = 256 * 1024;

pub const ERR_UNREACHABLE: &str = "pairing.unreachable";
pub const ERR_BAD_ANSWER: &str = "pairing.bad_answer";

/// A claim in progress on a remote host.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ClientClaim {
    /// Validated address of the host (no `/v1`).
    pub base_url: String,
    pub claim_id: String,
    /// Kept by the claiming side only.
    pub poll_secret: String,
    /// Shown here and on the host's prompt; the owner compares them.
    pub verification: String,
    pub allow_insecure_http: bool,
}

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| ERR_UNREACHABLE.to_string())
}

async fn answer(resp: reqwest::Response) -> Result<serde_json::Value, String> {
    let status = resp.status();
    let bytes = resp
        .bytes()
        .await
        .map_err(|_| ERR_UNREACHABLE.to_string())?;
    if bytes.len() > ANSWER_LIMIT {
        return Err(ERR_BAD_ANSWER.to_string());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| ERR_BAD_ANSWER.to_string())?;
    if status.is_success() {
        return Ok(value);
    }
    let code = value["error"]["code"].as_str().unwrap_or("");
    Err(if code.starts_with("pairing.") {
        code.to_string()
    } else {
        ERR_UNREACHABLE.to_string()
    })
}

/// Starts a claim with the host's code. Private and loopback hosts may be plain HTTP when
/// `allow_insecure_http` is set; everything else needs HTTPS (the same rule as remote links).
pub async fn start(
    base_url: &str,
    code: &str,
    device_name: &str,
    platform: &str,
    allow_insecure_http: bool,
) -> Result<ClientClaim, String> {
    let base = link::validate_url(base_url, allow_insecure_http).await?;
    let nonce = control_api::hex(&control_api::random_bytes::<16>()?);
    let resp = http()?
        .post(format!("{base}/v1/harness/pair/claim"))
        .json(&serde_json::json!({
            "code": code,
            "device_name": device_name,
            "platform": platform,
            "client_nonce": nonce,
        }))
        .send()
        .await
        .map_err(|_| ERR_UNREACHABLE.to_string())?;
    let started: ClaimStarted =
        serde_json::from_value(answer(resp).await?).map_err(|_| ERR_BAD_ANSWER.to_string())?;
    Ok(ClientClaim {
        base_url: base,
        claim_id: started.claim_id,
        poll_secret: started.poll_secret,
        verification: started.verification,
        allow_insecure_http,
    })
}

/// Asks whether the owner decided yet.
pub async fn poll(claim: &ClientClaim) -> Result<ClaimPoll, String> {
    // The address was validated at the start; validate again so a changed DNS answer is
    // checked before the poll secret leaves this computer.
    let base = link::validate_url(&claim.base_url, claim.allow_insecure_http).await?;
    let resp = http()?
        .get(format!("{base}/v1/harness/pair/claim/{}", claim.claim_id))
        .header("x-harness-pair-secret", &claim.poll_secret)
        .send()
        .await
        .map_err(|_| ERR_UNREACHABLE.to_string())?;
    serde_json::from_value(answer(resp).await?).map_err(|_| ERR_BAD_ANSWER.to_string())
}

/// What a finished pairing leaves behind.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Connected {
    pub link_id: String,
    pub link_name: String,
    /// `opencode.json` block for the paired Gateway (the token comes from
    /// `HARNESS_REMOTE_TOKEN`; it is not written into the config).
    pub opencode_config: serde_json::Value,
}

/// Saves the approved token as a remote link named `link_name`.
pub async fn finish(
    link_name: &str,
    claim: &ClientClaim,
    poll: &ClaimPoll,
) -> Result<Connected, String> {
    let token = match (poll.status, &poll.token) {
        (ClaimState::Approved, Some(t)) => t.clone(),
        _ => return Err(err("pairing.not_approved").into()),
    };
    let view = link::add(
        link_name.to_string(),
        claim.base_url.clone(),
        token,
        claim.allow_insecure_http,
    )
    .await?;
    Ok(Connected {
        link_id: view.link.id,
        link_name: view.link.name,
        opencode_config: super::opencode_config(&claim.base_url, &[]),
    })
}
