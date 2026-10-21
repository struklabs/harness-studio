//! What the Control API and the GUI share on top of the [`super::Hub`]: the offer with the
//! addresses a phone can open, the local capacity and host suggestions, and the client-side
//! connect step.

use super::{
    client, hub_for, suggest_hosts, ClaimState, DeviceRole, DeviceView, HostSuggestion, OfferInfo,
    OfferSpec,
};
use crate::control_api::TokenStore;
use serde::{Deserialize, Serialize};

/// A new offer with where to reach this computer's gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OfferCreated {
    pub offer: OfferInfo,
    /// Links the phone can open (`http://192.168.1.20:11435/m#code=1234-5678`); one per LAN
    /// address when the gateway listens on the network, else the loopback link.
    pub pair_urls: Vec<String>,
    /// The gateway runs and accepts connections from other devices.
    pub reachable_from_lan: bool,
}

pub fn create_offer(tokens: &TokenStore, spec: &OfferSpec) -> Result<OfferCreated, String> {
    let offer = hub_for(tokens).create_offer(spec)?;
    let status = crate::gateway::status();
    let host = status.host.clone().unwrap_or_default();
    let port = status.port;
    let any_host = host == "0.0.0.0" || host == "::";
    let reachable =
        status.running && port.is_some() && (any_host || !crate::network::is_loopback_host(&host));
    let mut pair_urls = Vec::new();
    if let Some(port) = port.filter(|_| status.running) {
        let hash = format!("#code={}", offer.code);
        if reachable {
            let hosts: Vec<String> = if any_host {
                crate::network::lan_ipv4_addresses()
                    .into_iter()
                    .map(|a| a.ip)
                    .collect()
            } else {
                vec![host.clone()]
            };
            for h in hosts {
                pair_urls.push(format!("http://{h}:{port}/m{hash}"));
            }
        }
        if pair_urls.is_empty() {
            pair_urls.push(format!("http://127.0.0.1:{port}/m{hash}"));
        }
    }
    Ok(OfferCreated {
        offer,
        pair_urls,
        reachable_from_lan: reachable,
    })
}

/// Memory a model can use here: the unified budget, else the biggest GPU, else system RAM.
pub fn local_capacity_bytes() -> u64 {
    let hw = crate::hardware::detect_hardware();
    if let Some(b) = hw.unified_memory_budget_bytes {
        return b;
    }
    hw.gpus
        .iter()
        .filter_map(|g| g.vram_bytes)
        .max()
        .unwrap_or(hw.total_ram_bytes)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Suggestion {
    pub required_bytes: u64,
    pub local_bytes: u64,
    /// This computer can run the model itself.
    pub fits_locally: bool,
    /// Paired devices that could run it instead, best first (empty when it fits locally).
    pub hosts: Vec<HostSuggestion>,
}

pub fn suggest(
    tokens: &TokenStore,
    required_bytes: u64,
    local_bytes: u64,
) -> Result<Suggestion, String> {
    let devices = hub_for(tokens).list(tokens)?;
    Ok(suggest_with(required_bytes, local_bytes, &devices))
}

pub fn suggest_with(required_bytes: u64, local_bytes: u64, devices: &[DeviceView]) -> Suggestion {
    let hosts = suggest_hosts(required_bytes, local_bytes, devices);
    Suggestion {
        required_bytes,
        local_bytes,
        fits_locally: hosts.is_none(),
        hosts: hosts.unwrap_or_default(),
    }
}

/// A role and the components it needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RoleInfo {
    pub role: DeviceRole,
    pub components: Vec<String>,
}

pub fn roles() -> Vec<RoleInfo> {
    DeviceRole::ALL
        .into_iter()
        .map(|role| RoleInfo {
            role,
            components: role.components().iter().map(|c| c.to_string()).collect(),
        })
        .collect()
}

/// Progress of connecting this computer to another host.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConnectProgress {
    pub status: ClaimState,
    pub connected: Option<client::Connected>,
}

/// Polls the remote host once; when the owner approved, saves the token as a link.
pub async fn connect_progress(
    link_name: &str,
    claim: &client::ClientClaim,
) -> Result<ConnectProgress, String> {
    let poll = client::poll(claim).await?;
    let connected = if poll.status == ClaimState::Approved {
        Some(client::finish(link_name, claim, &poll).await?)
    } else {
        None
    };
    Ok(ConnectProgress {
        status: poll.status,
        connected,
    })
}
