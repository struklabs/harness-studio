//! Named API tokens (admin scope). The value is returned once, at creation. Besides the
//! scopes, a token carries its gateway policy: allowed models and endpoints, rate limits.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use harness_core::control_api::{CreatedToken, Scope, TokenInfo, TokenPolicy, TokenUpdate};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::error::{blocking, ApiResult, Body, ErrorBody, P};
use crate::AppState;

#[utoipa::path(get, path = "/tokens", tag = "tokens",
    responses((status = 200, body = Vec<TokenInfo>), (status = 403, body = ErrorBody)))]
pub async fn list_tokens(State(s): State<AppState>) -> ApiResult<Json<Vec<TokenInfo>>> {
    let store = s.tokens.clone();
    Ok(Json(blocking(move || store.list()).await??))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateTokenRequest {
    pub name: String,
    pub scopes: Vec<Scope>,
    /// Gateway policy (`models`, `endpoints`, `limits`); omitted = unrestricted.
    #[serde(flatten, default)]
    pub policy: TokenPolicy,
}

#[utoipa::path(post, path = "/tokens", tag = "tokens", request_body = CreateTokenRequest,
    responses((status = 201, body = CreatedToken), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn create_token(
    State(s): State<AppState>,
    Body(req): Body<CreateTokenRequest>,
) -> ApiResult<(StatusCode, Json<CreatedToken>)> {
    let store = s.tokens.clone();
    let created =
        blocking(move || store.create_with_policy(&req.name, &req.scopes, &req.policy)).await??;
    Ok((StatusCode::CREATED, Json(created)))
}

/// One token (never its value).
#[utoipa::path(get, path = "/tokens/{id}", tag = "tokens",
    params(("id" = String, Path)),
    responses((status = 200, body = TokenInfo), (status = 404, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn get_token(State(s): State<AppState>, P(id): P<String>) -> ApiResult<Json<TokenInfo>> {
    let store = s.tokens.clone();
    Ok(Json(blocking(move || store.get(&id)).await??))
}

/// Changes a token's name, scopes, allowed models/endpoints or rate limits (fields left out
/// are kept). Applies from the next request; the token value stays the same.
#[utoipa::path(patch, path = "/tokens/{id}", tag = "tokens", request_body = TokenUpdate,
    params(("id" = String, Path)),
    responses((status = 200, body = TokenInfo), (status = 400, body = ErrorBody),
        (status = 404, body = ErrorBody), (status = 403, body = ErrorBody)))]
pub async fn update_token(
    State(s): State<AppState>,
    P(id): P<String>,
    Body(change): Body<TokenUpdate>,
) -> ApiResult<Json<TokenInfo>> {
    let store = s.tokens.clone();
    Ok(Json(blocking(move || store.update(&id, &change)).await??))
}

/// Revokes a token immediately (the next request with it gets 401).
#[utoipa::path(delete, path = "/tokens/{id}", tag = "tokens",
    params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody), (status = 403, body = ErrorBody)))]
pub async fn revoke_token(State(s): State<AppState>, P(id): P<String>) -> ApiResult<StatusCode> {
    let store = s.tokens.clone();
    blocking(move || store.revoke(&id)).await??;
    Ok(StatusCode::NO_CONTENT)
}
