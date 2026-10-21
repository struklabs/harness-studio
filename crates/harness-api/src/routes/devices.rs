//! Paired devices: one-time offers, approval prompts, the device registry, the audit log,
//! revocation (also "revoke all" for a lost phone) and host suggestions. The devices' own
//! public steps (claim, poll, report) and the phone chat page are on the gateway
//! (`harness_gateway::pairing`). Protocol and threat model: `harness_core::pairing`.
//! Everything here needs `admin`.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use harness_core::pairing::client::ClientClaim;
use harness_core::pairing::service::{self, ConnectProgress, OfferCreated, RoleInfo, Suggestion};
use harness_core::pairing::{
    self, AccessPreset, AuditEntry, DeviceRole, DeviceView, OfferInfo, OfferSpec, PairingSettings,
    PendingClaim,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{blocking, ApiError, ApiResult, Body, ErrorBody, P, Q};
use crate::AppState;

#[utoipa::path(post, path = "/pairing/offers", tag = "devices", request_body = OfferSpec,
    responses((status = 201, body = OfferCreated), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn create_offer(
    State(s): State<AppState>,
    Body(spec): Body<OfferSpec>,
) -> ApiResult<(StatusCode, Json<OfferCreated>)> {
    let tokens = s.tokens.clone();
    let created = blocking(move || service::create_offer(&tokens, &spec)).await??;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Open offers (the codes are only returned at creation).
#[utoipa::path(get, path = "/pairing/offers", tag = "devices",
    responses((status = 200, body = Vec<OfferInfo>), (status = 403, body = ErrorBody)))]
pub async fn list_offers(State(s): State<AppState>) -> Json<Vec<OfferInfo>> {
    Json(pairing::hub_for(&s.tokens).offers())
}

#[utoipa::path(delete, path = "/pairing/offers/{id}", tag = "devices",
    params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody)))]
pub async fn cancel_offer(State(s): State<AppState>, P(id): P<String>) -> ApiResult<StatusCode> {
    if pairing::hub_for(&s.tokens).cancel_offer(&id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("offer", &id))
    }
}

/// Claims waiting for approval (device name, verification code).
#[utoipa::path(get, path = "/pairing/pending", tag = "devices",
    responses((status = 200, body = Vec<PendingClaim>), (status = 403, body = ErrorBody)))]
pub async fn pending(State(s): State<AppState>) -> Json<Vec<PendingClaim>> {
    Json(pairing::hub_for(&s.tokens).pending())
}

/// Approves a pending claim: the scoped token is created and handed to the device on its
/// next poll.
#[utoipa::path(post, path = "/pairing/pending/{id}/approve", tag = "devices",
    params(("id" = String, Path)),
    responses((status = 200, body = DeviceView), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody), (status = 410, body = ErrorBody)))]
pub async fn approve(State(s): State<AppState>, P(id): P<String>) -> ApiResult<Json<DeviceView>> {
    let tokens = s.tokens.clone();
    Ok(Json(
        blocking(move || pairing::hub_for(&tokens).approve(&id, &tokens)).await??,
    ))
}

#[utoipa::path(post, path = "/pairing/pending/{id}/deny", tag = "devices",
    params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody)))]
pub async fn deny(State(s): State<AppState>, P(id): P<String>) -> ApiResult<StatusCode> {
    let tokens = s.tokens.clone();
    blocking(move || pairing::hub_for(&tokens).deny(&id)).await??;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct ConnectRequest {
    /// The other Harness host (`http://192.168.1.20:11435`; no `/v1` needed).
    pub url: String,
    /// The code its owner created.
    pub code: String,
    /// How this computer is called on that host.
    pub device_name: String,
    #[serde(default)]
    pub allow_insecure_http: bool,
}

/// Starts pairing this computer with another Harness host. Show `verification` to the
/// owner of that host, then call `/pairing/connect/poll`.
#[utoipa::path(post, path = "/pairing/connect", tag = "devices", request_body = ConnectRequest,
    responses((status = 200, body = ClientClaim), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody), (status = 429, body = ErrorBody)))]
pub async fn connect(Body(req): Body<ConnectRequest>) -> ApiResult<Json<ClientClaim>> {
    Ok(Json(
        pairing::client::start(
            &req.url,
            &req.code,
            &req.device_name,
            std::env::consts::OS,
            req.allow_insecure_http,
        )
        .await?,
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct ConnectPollRequest {
    pub claim: ClientClaim,
    /// Name of the remote link to create (lowercase letters, digits, dashes).
    pub link_name: String,
}

/// Checks whether the other host approved; when it did, saves the token as a remote link
/// and returns an `opencode.json` block for that Gateway.
#[utoipa::path(post, path = "/pairing/connect/poll", tag = "devices", request_body = ConnectPollRequest,
    responses((status = 200, body = ConnectProgress), (status = 404, body = ErrorBody)))]
pub async fn connect_poll(Body(req): Body<ConnectPollRequest>) -> ApiResult<Json<ConnectProgress>> {
    Ok(Json(
        service::connect_progress(&req.link_name, &req.claim).await?,
    ))
}

#[utoipa::path(get, path = "/devices", tag = "devices",
    responses((status = 200, body = Vec<DeviceView>), (status = 403, body = ErrorBody)))]
pub async fn list_devices(State(s): State<AppState>) -> ApiResult<Json<Vec<DeviceView>>> {
    let tokens = s.tokens.clone();
    Ok(Json(
        blocking(move || pairing::hub_for(&tokens).list(&tokens)).await??,
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct DeviceUpdate {
    pub name: Option<String>,
    pub role: Option<DeviceRole>,
    /// Changes the access; `models` then replaces the allowed model patterns.
    pub preset: Option<AccessPreset>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub acknowledge_admin: bool,
}

#[utoipa::path(patch, path = "/devices/{id}", tag = "devices", request_body = DeviceUpdate,
    params(("id" = String, Path)),
    responses((status = 200, body = DeviceView), (status = 400, body = ErrorBody),
        (status = 404, body = ErrorBody)))]
pub async fn update_device(
    State(s): State<AppState>,
    P(id): P<String>,
    Body(change): Body<DeviceUpdate>,
) -> ApiResult<Json<DeviceView>> {
    let tokens = s.tokens.clone();
    Ok(Json(
        blocking(move || {
            let hub = pairing::hub_for(&tokens);
            let mut view = hub.update(&id, change.name.as_deref(), change.role, &tokens)?;
            if let Some(preset) = change.preset {
                view = hub.set_access(
                    &id,
                    preset,
                    &change.models,
                    change.acknowledge_admin,
                    &tokens,
                )?;
            }
            Ok::<_, String>(view)
        })
        .await??,
    ))
}

/// Unpairs a device; its token stops working at once.
#[utoipa::path(delete, path = "/devices/{id}", tag = "devices",
    params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody)))]
pub async fn revoke_device(State(s): State<AppState>, P(id): P<String>) -> ApiResult<StatusCode> {
    let tokens = s.tokens.clone();
    blocking(move || pairing::hub_for(&tokens).revoke(&id, &tokens)).await??;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct RevokedAll {
    pub revoked: usize,
}

/// Lost-phone recovery: unpairs every device and cancels open offers and pending claims.
#[utoipa::path(post, path = "/devices/revoke-all", tag = "devices",
    responses((status = 200, body = RevokedAll), (status = 403, body = ErrorBody)))]
pub async fn revoke_all(State(s): State<AppState>) -> ApiResult<Json<RevokedAll>> {
    let tokens = s.tokens.clone();
    let revoked = blocking(move || pairing::hub_for(&tokens).revoke_all(&tokens)).await??;
    Ok(Json(RevokedAll { revoked }))
}

#[derive(Deserialize)]
pub struct AuditQuery {
    pub limit: Option<usize>,
}

/// The pairing audit log, newest first.
#[utoipa::path(get, path = "/devices/audit", tag = "devices",
    params(("limit" = Option<usize>, Query, description = "At most this many entries (default 100, max 1000)")),
    responses((status = 200, body = Vec<AuditEntry>), (status = 403, body = ErrorBody)))]
pub async fn audit(State(s): State<AppState>, Q(q): Q<AuditQuery>) -> Json<Vec<AuditEntry>> {
    Json(pairing::hub_for(&s.tokens).audit_log(q.limit.unwrap_or(100)))
}

/// Roles and the components each one needs.
#[utoipa::path(get, path = "/devices/roles", tag = "devices",
    responses((status = 200, body = Vec<RoleInfo>)))]
pub async fn roles() -> Json<Vec<RoleInfo>> {
    Json(service::roles())
}

#[derive(Deserialize)]
pub struct SuggestQuery {
    pub required_bytes: u64,
}

/// Whether this computer can run a model of `required_bytes`, and if not, which paired
/// devices could.
#[utoipa::path(get, path = "/devices/suggest", tag = "devices",
    params(("required_bytes" = u64, Query, description = "Memory the model needs")),
    responses((status = 200, body = Suggestion), (status = 400, body = ErrorBody)))]
pub async fn suggest(
    State(s): State<AppState>,
    Q(q): Q<SuggestQuery>,
) -> ApiResult<Json<Suggestion>> {
    let tokens = s.tokens.clone();
    Ok(Json(
        blocking(move || {
            service::suggest(&tokens, q.required_bytes, service::local_capacity_bytes())
        })
        .await??,
    ))
}

/// Pairing settings: the signaling endpoint (empty = off).
#[utoipa::path(get, path = "/devices/settings", tag = "devices",
    responses((status = 200, body = PairingSettings), (status = 403, body = ErrorBody)))]
pub async fn get_settings(State(s): State<AppState>) -> Json<PairingSettings> {
    Json(pairing::hub_for(&s.tokens).settings())
}

#[utoipa::path(put, path = "/devices/settings", tag = "devices", request_body = PairingSettings,
    responses((status = 200, body = PairingSettings), (status = 400, body = ErrorBody)))]
pub async fn put_settings(
    State(s): State<AppState>,
    Body(incoming): Body<PairingSettings>,
) -> ApiResult<Json<PairingSettings>> {
    let tokens = s.tokens.clone();
    Ok(Json(
        blocking(move || pairing::hub_for(&tokens).set_settings(&incoming)).await??,
    ))
}
