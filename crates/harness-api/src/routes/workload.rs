//! Consent for heavy workloads and for the microphone and camera (`harness_core::workload`).
//! Thin adapters: the rules, the storage and the audit rows live in the core. Every route
//! needs `admin`.

use axum::http::StatusCode;
use axum::Json;
use harness_core::workload::{Choice, Consent, Verdict, WorkloadKind, WorkloadView, Workloads};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{blocking, ApiResult, Body, ErrorBody, P};

/// Every workload kind with the operating system's switch (devices) and the remembered
/// answers.
#[utoipa::path(get, operation_id = "list_workloads", path = "/workloads", tag = "permissions",
    responses((status = 200, body = Vec<WorkloadView>)))]
pub async fn list() -> ApiResult<Json<Vec<WorkloadView>>> {
    Ok(Json(blocking(|| Workloads::shared().list()).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct CheckRequest {
    pub client: String,
    pub kind: WorkloadKind,
}

#[derive(Serialize, ToSchema)]
pub struct CheckResponse {
    pub verdict: Verdict,
}

/// What would happen if `client` started a workload of this kind now: allowed, denied, not
/// asked yet (`ask`: show the prompt) or blocked by the operating system.
#[utoipa::path(post, operation_id = "check_workload", path = "/workloads/check", tag = "permissions",
    request_body = CheckRequest, responses((status = 200, body = CheckResponse)))]
pub async fn check(Body(req): Body<CheckRequest>) -> ApiResult<Json<CheckResponse>> {
    let verdict = blocking(move || Workloads::shared().verdict(&req.client, req.kind)).await?;
    Ok(Json(CheckResponse { verdict }))
}

#[derive(Deserialize, ToSchema)]
pub struct RememberRequest {
    pub client: String,
    pub kind: WorkloadKind,
    pub choice: Choice,
}

/// Remembers the user's answer for a client and kind (replacing an earlier one).
#[utoipa::path(post, operation_id = "remember_workload_consent", path = "/workloads/consents",
    tag = "permissions", request_body = RememberRequest,
    responses((status = 200, body = Consent), (status = 400, body = ErrorBody)))]
pub async fn remember(Body(req): Body<RememberRequest>) -> ApiResult<Json<Consent>> {
    let c =
        blocking(move || Workloads::shared().remember("api", &req.client, req.kind, req.choice))
            .await??;
    Ok(Json(c))
}

/// Removes one remembered answer: the client is asked again next time.
#[utoipa::path(delete, operation_id = "revoke_workload_consent",
    path = "/workloads/consents/{id}", tag = "permissions",
    params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody)))]
pub async fn revoke(P(id): P<String>) -> ApiResult<StatusCode> {
    blocking(move || Workloads::shared().revoke("api", &id)).await??;
    Ok(StatusCode::NO_CONTENT)
}

/// Refuses a heavy workload started by an external client that was not asked yet (`428`) or
/// was refused (`403`). The desktop app and the local owner credential are never asked about
/// their own jobs: only a named API token is a "client" in the sense of this page.
pub async fn require_for(caller: &crate::auth::Caller, kind: WorkloadKind) -> ApiResult<()> {
    use harness_core::control_api::Principal;
    let Principal::Named { name, .. } = &caller.principal else {
        return Ok(());
    };
    let client = format!("api:{name}");
    blocking(move || Workloads::shared().require(&client, kind))
        .await?
        .map_err(crate::error::ApiError::core)
}
