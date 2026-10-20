//! Trust levels and capability grants of MCP servers, agent runtimes and the training backend
//! (`harness_core::permissions`). Thin adapters: the rules, the storage and the audit rows (the
//! tool log, tool `permissions.*`) live in the core. Every route needs `admin`.

use axum::http::StatusCode;
use axum::Json;
use harness_core::permissions::{Capability, Grant, Permissions, SubjectKind, SubjectView};
use harness_core::tools::PermissionClass;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::error::{blocking, ApiResult, Body, ErrorBody, P};

/// Every subject with its trust level and grants.
#[utoipa::path(get, operation_id = "list_permissions", path = "/permissions", tag = "permissions",
    responses((status = 200, body = Vec<SubjectView>)))]
pub async fn list() -> ApiResult<Json<Vec<SubjectView>>> {
    Ok(Json(blocking(|| Permissions::shared().list()).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct GrantRequest {
    pub kind: SubjectKind,
    pub id: String,
    pub capability: Capability,
    /// Existing absolute folders (`file` only).
    #[serde(default)]
    pub paths: Vec<String>,
}

/// Grants one capability to a subject. The training backend takes none (`409`).
#[utoipa::path(post, operation_id = "grant_permission", path = "/permissions/grants", tag = "permissions",
    request_body = GrantRequest,
    responses((status = 200, body = Grant), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody), (status = 422, body = ErrorBody)))]
pub async fn grant(Body(req): Body<GrantRequest>) -> ApiResult<Json<Grant>> {
    let g = blocking(move || {
        Permissions::shared().grant("api", req.kind, &req.id, req.capability, req.paths)
    })
    .await??;
    Ok(Json(g))
}

/// Revokes one grant; it stops applying at the next agent run or MCP call.
#[utoipa::path(delete, operation_id = "revoke_permission_grant",
    path = "/permissions/{kind}/{id}/grants/{grant_id}", tag = "permissions",
    params(("kind" = SubjectKind, Path), ("id" = String, Path), ("grant_id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody)))]
pub async fn revoke_grant(
    P((kind, id, grant_id)): P<(SubjectKind, String, String)>,
) -> ApiResult<StatusCode> {
    blocking(move || Permissions::shared().revoke_grant("api", kind, &id, &grant_id)).await??;
    Ok(StatusCode::NO_CONTENT)
}

/// Revokes every grant of a subject and resets its trust level to the default.
#[utoipa::path(delete, operation_id = "revoke_permission_subject",
    path = "/permissions/{kind}/{id}", tag = "permissions",
    params(("kind" = SubjectKind, Path), ("id" = String, Path)),
    responses((status = 204)))]
pub async fn revoke_subject(P((kind, id)): P<(SubjectKind, String)>) -> ApiResult<StatusCode> {
    blocking(move || Permissions::shared().revoke_subject("api", kind, &id)).await??;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct TrustRequest {
    /// `safe`, `restricted`, `dangerous`; `null` = the default.
    pub trust: Option<PermissionClass>,
}

/// Sets (or, with `null`, clears) a subject's trust level.
#[utoipa::path(put, operation_id = "set_permission_trust",
    path = "/permissions/{kind}/{id}/trust", tag = "permissions", request_body = TrustRequest,
    params(("kind" = SubjectKind, Path), ("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody)))]
pub async fn set_trust(
    P((kind, id)): P<(SubjectKind, String)>,
    Body(req): Body<TrustRequest>,
) -> ApiResult<StatusCode> {
    blocking(move || Permissions::shared().set_trust("api", kind, &id, req.trust)).await??;
    Ok(StatusCode::NO_CONTENT)
}
