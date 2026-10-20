//! BYOK cloud providers (admin scope). The API key is accepted when a provider is added or
//! updated and is never returned: views only say whether one is saved (`key_set`). Provider
//! models are served by the gateway as `cloud:<provider>/<model>`.

use axum::http::StatusCode;
use axum::Json;
use harness_core::cloud::{self, ModelView, ProviderInput, ProviderStatus, ProviderView};
use serde::Serialize;
use utoipa::ToSchema;

use crate::error::{ApiResult, Body, ErrorBody, P};

/// The configured providers with their connection state (no keys).
#[utoipa::path(get, path = "/cloud/providers", tag = "cloud",
    responses((status = 200, body = Vec<ProviderView>), (status = 403, body = ErrorBody)))]
pub async fn list_cloud_providers() -> Json<Vec<ProviderView>> {
    Json(cloud::list())
}

/// Adds a provider. `key` is saved in the OS keychain; the answer carries `key_set` only.
#[utoipa::path(post, path = "/cloud/providers", tag = "cloud", request_body = ProviderInput,
    responses((status = 201, body = ProviderView), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody), (status = 409, body = ErrorBody)))]
pub async fn add_cloud_provider(
    Body(input): Body<ProviderInput>,
) -> ApiResult<(StatusCode, Json<ProviderView>)> {
    Ok((StatusCode::CREATED, Json(cloud::add(input).await?)))
}

/// Changes a provider. `key` omitted/null keeps the saved key, `""` removes it. A saved key
/// is not carried to a different host: changing the address needs a new key.
#[utoipa::path(put, path = "/cloud/providers/{id}", tag = "cloud", request_body = ProviderInput,
    params(("id" = String, Path)),
    responses((status = 200, body = ProviderView), (status = 400, body = ErrorBody),
        (status = 404, body = ErrorBody), (status = 403, body = ErrorBody),
        (status = 409, body = ErrorBody)))]
pub async fn update_cloud_provider(
    P(id): P<String>,
    Body(input): Body<ProviderInput>,
) -> ApiResult<Json<ProviderView>> {
    Ok(Json(cloud::update(id, input).await?))
}

#[derive(Serialize, ToSchema)]
pub struct RemovedProvider {
    /// Set when the keychain could not be reached: the key may still be stored there.
    pub warning: Option<String>,
}

/// Removes a provider and its key.
#[utoipa::path(delete, path = "/cloud/providers/{id}", tag = "cloud",
    params(("id" = String, Path)),
    responses((status = 200, body = RemovedProvider), (status = 404, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn remove_cloud_provider(P(id): P<String>) -> ApiResult<Json<RemovedProvider>> {
    Ok(Json(RemovedProvider {
        warning: cloud::remove(&id).await?,
    }))
}

/// Connection test: lists the provider's models with the saved key (costs no tokens).
#[utoipa::path(post, path = "/cloud/providers/{id}/test", tag = "cloud",
    params(("id" = String, Path)),
    responses((status = 200, body = ProviderStatus), (status = 404, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn test_cloud_provider(P(id): P<String>) -> ApiResult<Json<ProviderStatus>> {
    Ok(Json(cloud::test(&id).await?))
}

/// Chat and embedding models of the enabled providers (`embedding` tells them apart).
#[utoipa::path(get, path = "/cloud/models", tag = "cloud",
    responses((status = 200, body = Vec<ModelView>), (status = 403, body = ErrorBody)))]
pub async fn list_cloud_models() -> Json<Vec<ModelView>> {
    Json(cloud::all_models())
}
