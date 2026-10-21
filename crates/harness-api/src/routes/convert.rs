//! Safetensors to GGUF conversion: plan a Hugging Face repo, queue a conversion,
//! follow and cancel jobs, and manage the conversion environment (a throwaway uv + Python
//! venv). Reads need `read`; queueing, cancelling and removing jobs need `downloads:write`;
//! building and removing the environment need `admin`. Job changes are also published as
//! `convert` / `convert_removed` events.

use axum::http::StatusCode;
use axum::Json;
use harness_core::convert::{self, ConvertJob, ConvertPlan, EnvStatus};
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use crate::error::{blocking, ApiError, ApiResult, Body, ErrorBody, P, Q};

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PlanQuery {
    /// Hugging Face repo (`owner/name`).
    pub repo: String,
}

/// What converting a repo would take: whether the converter handles its architecture, the
/// files to download, the size of each result type and the disk the run needs.
#[utoipa::path(get, operation_id = "plan_conversion", path = "/convert/plan", tag = "models", params(PlanQuery),
    responses((status = 200, body = ConvertPlan), (status = 403, body = ErrorBody),
        (status = 422, body = ErrorBody)))]
pub async fn plan(Q(q): Q<PlanQuery>) -> ApiResult<Json<ConvertPlan>> {
    Ok(Json(convert::plan(q.repo.trim(), None).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct ConvertRequest {
    /// Hugging Face repo (`owner/name`) with safetensors weights.
    pub repo: String,
    /// Target type (`Q4_K_M` default, `Q5_K_M`, `Q6_K`, `Q8_0`, `F16`, `BF16`, or any type
    /// the runtime's `llama-quantize` lists).
    pub quant: Option<String>,
    /// Keep the downloaded safetensors in the library after the conversion.
    #[serde(default)]
    pub keep_original: bool,
}

/// Queues a conversion (download, convert, quantize, place in the library). A named API token
/// is asked once for consent to heavy CPU work (`428 workload.consent_required`, answered on the
/// Permissions page or with `POST /workloads/consents`).
#[utoipa::path(post, operation_id = "start_conversion", path = "/convert", tag = "models", request_body = ConvertRequest,
    responses((status = 202, body = ConvertJob), (status = 403, body = ErrorBody),
        (status = 409, body = ErrorBody), (status = 422, body = ErrorBody),
        (status = 428, body = ErrorBody)))]
pub async fn start(
    axum::Extension(caller): axum::Extension<crate::auth::Caller>,
    Body(req): Body<ConvertRequest>,
) -> ApiResult<(StatusCode, Json<ConvertJob>)> {
    super::workload::require_for(&caller, harness_core::workload::WorkloadKind::HeavyCpu).await?;
    let job = convert::start(req.repo.trim(), req.quant.as_deref(), req.keep_original).await?;
    Ok((StatusCode::ACCEPTED, Json(job)))
}

#[utoipa::path(get, operation_id = "list_conversions", path = "/convert", tag = "models",
    responses((status = 200, body = Vec<ConvertJob>)))]
pub async fn list() -> Json<Vec<ConvertJob>> {
    Json(convert::jobs())
}

#[utoipa::path(get, operation_id = "get_conversion", path = "/convert/{id}", tag = "models", params(("id" = u64, Path)),
    responses((status = 200, body = ConvertJob), (status = 404, body = ErrorBody)))]
pub async fn get_one(P(id): P<u64>) -> ApiResult<Json<ConvertJob>> {
    convert::job(id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("convert", &id.to_string()))
}

/// Stops a job; a running one is killed, its partial files are kept for a retry.
#[utoipa::path(post, operation_id = "cancel_conversion", path = "/convert/{id}/cancel", tag = "models", params(("id" = u64, Path)),
    responses((status = 204), (status = 404, body = ErrorBody)))]
pub async fn cancel(P(id): P<u64>) -> ApiResult<StatusCode> {
    convert::cancel(id).map_err(|_| ApiError::not_found("convert", &id.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Removes a finished job and the partial files it kept.
#[utoipa::path(delete, operation_id = "delete_conversion", path = "/convert/{id}", tag = "models", params(("id" = u64, Path)),
    responses((status = 204), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody)))]
pub async fn remove(P(id): P<u64>) -> ApiResult<StatusCode> {
    if convert::job(id).is_none() {
        return Err(ApiError::not_found("convert", &id.to_string()));
    }
    blocking(move || convert::remove(id))
        .await?
        .map_err(ApiError::core)?;
    Ok(StatusCode::NO_CONTENT)
}

/// The conversion environment: installed or not, its size and the build in progress.
#[utoipa::path(get, operation_id = "get_convert_env", path = "/convert/env", tag = "models",
    responses((status = 200, body = EnvStatus)))]
pub async fn env_status() -> ApiResult<Json<EnvStatus>> {
    Ok(Json(blocking(convert::env::status).await?))
}

/// Builds the conversion environment for the managed runtime's llama.cpp tag (uv, Python and
/// the converter's packages, about 700 MB). Returns when it is complete.
#[utoipa::path(post, operation_id = "install_convert_env", path = "/convert/env", tag = "models",
    responses((status = 201, body = EnvStatus), (status = 422, body = ErrorBody)))]
pub async fn env_install() -> ApiResult<(StatusCode, Json<EnvStatus>)> {
    Ok((StatusCode::CREATED, Json(convert::install_env().await?)))
}

/// Deletes the conversion environment (it is built again on the next conversion).
#[utoipa::path(delete, operation_id = "remove_convert_env", path = "/convert/env", tag = "models",
    responses((status = 204), (status = 409, body = ErrorBody)))]
pub async fn env_remove() -> ApiResult<StatusCode> {
    blocking(convert::remove_env).await??;
    Ok(StatusCode::NO_CONTENT)
}
