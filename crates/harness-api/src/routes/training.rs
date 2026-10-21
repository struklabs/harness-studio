//! Training jobs: LoRA / QLoRA supervised fine-tuning of a library model on a JSONL dataset.
//! Reads (`GET`) need `read`; creating, starting, cancelling, resuming, exporting and deleting
//! jobs, checking a dataset and building the environment need `admin`. Job changes are also
//! published as `training` / `training_removed` events.

use axum::http::StatusCode;
use axum::Json;
use harness_core::training::{
    self, Compatibility, CreateRequest, DatasetReport, Estimate, ExportRequest, Metric,
    TrainingConfig, TrainingJob,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{blocking, ApiError, ApiResult, Body, ErrorBody, P};

fn missing(id: u64) -> ApiError {
    ApiError::not_found("training", &id.to_string())
}

/// What the wizard shows before a job is created: the full configuration (defaults filled in),
/// the dataset report (format, split, size, licence) and the memory and time estimate.
#[derive(Serialize, ToSchema)]
pub struct TrainingPreview {
    pub config: TrainingConfig,
    pub dataset: DatasetReport,
    pub estimate: Estimate,
}

#[derive(Deserialize, ToSchema)]
pub struct DatasetCheck {
    /// Path of a `.jsonl` file.
    pub path: String,
    /// Share of rows held out for validation (default 0.1).
    pub validation_fraction: Option<f64>,
}

/// Checks a dataset file: chat (`messages`) or instruction (`instruction`/`output`) rows, the
/// train/validation split, size and the licence field. Findings are in `issues`.
#[utoipa::path(post, operation_id = "check_training_dataset", path = "/training/dataset", tag = "training", request_body = DatasetCheck,
    responses((status = 200, body = DatasetReport)))]
pub async fn check_dataset(Body(req): Body<DatasetCheck>) -> ApiResult<Json<DatasetReport>> {
    let fraction = req.validation_fraction.unwrap_or(0.1);
    Ok(Json(
        blocking(move || {
            training::dataset::inspect(std::path::Path::new(req.path.trim()), fraction)
        })
        .await?,
    ))
}

/// Validates a job request and returns its configuration, dataset report and estimate.
#[utoipa::path(post, operation_id = "preview_training", path = "/training/preview", tag = "training", request_body = CreateRequest,
    responses((status = 200, body = TrainingPreview), (status = 422, body = ErrorBody)))]
pub async fn preview(Body(req): Body<CreateRequest>) -> ApiResult<Json<TrainingPreview>> {
    let (config, dataset, estimate) = blocking(move || training::preview(&req))
        .await?
        .map_err(ApiError::core)?;
    Ok(Json(TrainingPreview {
        config,
        dataset,
        estimate,
    }))
}

/// Which tuning methods (LoRA, QLoRA, SFT, DPO, full fine-tuning) this machine can run, and why.
#[utoipa::path(get, operation_id = "training_compatibility", path = "/training/compatibility", tag = "training",
    responses((status = 200, body = Compatibility)))]
pub async fn compatibility() -> Json<Compatibility> {
    Json(training::compatibility().await)
}

/// The Python environment training runs in (the `python-hf` one).
#[utoipa::path(get, operation_id = "get_training_env", path = "/training/env", tag = "training",
    responses((status = 200, body = harness_core::pyenv::envs::EnvStatus)))]
pub async fn env_status() -> ApiResult<Json<harness_core::pyenv::envs::EnvStatus>> {
    Ok(Json(blocking(training::env_status).await?))
}

/// Builds the training environment (about 1.5 GB on CPU, 3 GB with CUDA). Returns when ready.
#[utoipa::path(post, operation_id = "install_training_env", path = "/training/env", tag = "training",
    responses((status = 201, body = harness_core::pyenv::envs::EnvStatus), (status = 422, body = ErrorBody)))]
pub async fn env_install() -> ApiResult<(StatusCode, Json<harness_core::pyenv::envs::EnvStatus>)> {
    Ok((
        StatusCode::CREATED,
        Json(training::install_env().await.map_err(ApiError::core)?),
    ))
}

#[utoipa::path(get, operation_id = "list_training_jobs", path = "/training", tag = "training",
    responses((status = 200, body = Vec<TrainingJob>)))]
pub async fn list() -> Json<Vec<TrainingJob>> {
    Json(training::jobs())
}

/// Creates a draft job: the dataset is checked and snapshotted, nothing runs yet.
#[utoipa::path(post, operation_id = "create_training_job", path = "/training", tag = "training", request_body = CreateRequest,
    responses((status = 201, body = TrainingJob), (status = 422, body = ErrorBody)))]
pub async fn create(Body(req): Body<CreateRequest>) -> ApiResult<(StatusCode, Json<TrainingJob>)> {
    let job = blocking(move || training::create(&req))
        .await?
        .map_err(ApiError::core)?;
    Ok((StatusCode::CREATED, Json(job)))
}

#[utoipa::path(get, operation_id = "get_training_job", path = "/training/{id}", tag = "training", params(("id" = u64, Path)),
    responses((status = 200, body = TrainingJob), (status = 404, body = ErrorBody)))]
pub async fn get_one(P(id): P<u64>) -> ApiResult<Json<TrainingJob>> {
    training::job(id).map(Json).ok_or_else(|| missing(id))
}

/// Deletes a job that is not running, with its checkpoints. An exported model stays.
#[utoipa::path(delete, operation_id = "delete_training_job", path = "/training/{id}", tag = "training", params(("id" = u64, Path)),
    responses((status = 204), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody)))]
pub async fn remove(P(id): P<u64>) -> ApiResult<StatusCode> {
    blocking(move || training::remove(id))
        .await?
        .map_err(ApiError::core)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Loss and eval loss of every step (the loss chart).
#[utoipa::path(get, operation_id = "get_training_metrics", path = "/training/{id}/metrics", tag = "training", params(("id" = u64, Path)),
    responses((status = 200, body = Vec<Metric>), (status = 404, body = ErrorBody)))]
pub async fn metrics(P(id): P<u64>) -> ApiResult<Json<Vec<Metric>>> {
    blocking(move || training::metrics(id))
        .await?
        .map(Json)
        .map_err(ApiError::core)
}

/// The tail of the worker's output.
#[utoipa::path(get, operation_id = "get_training_log", path = "/training/{id}/log", tag = "training", params(("id" = u64, Path)),
    responses((status = 200, body = String), (status = 404, body = ErrorBody)))]
pub async fn log(P(id): P<u64>) -> ApiResult<Json<String>> {
    blocking(move || training::log_tail(id))
        .await?
        .map(Json)
        .map_err(ApiError::core)
}

/// Queues a draft job: it waits for the training slot and the GPU scheduler, then runs.
#[utoipa::path(post, operation_id = "start_training_job", path = "/training/{id}/start", tag = "training", params(("id" = u64, Path)),
    responses((status = 202, body = TrainingJob), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody), (status = 422, body = ErrorBody)))]
pub async fn start(P(id): P<u64>) -> ApiResult<(StatusCode, Json<TrainingJob>)> {
    let job = training::start(id).map_err(ApiError::core)?;
    Ok((StatusCode::ACCEPTED, Json(job)))
}

/// Cancels a job at any point. A running worker saves a checkpoint first.
#[utoipa::path(post, operation_id = "cancel_training_job", path = "/training/{id}/cancel", tag = "training", params(("id" = u64, Path)),
    responses((status = 204), (status = 404, body = ErrorBody)))]
pub async fn cancel(P(id): P<u64>) -> ApiResult<StatusCode> {
    training::cancel(id).map_err(ApiError::core)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Continues a failed or cancelled job from its last complete checkpoint.
#[utoipa::path(post, operation_id = "resume_training_job", path = "/training/{id}/resume", tag = "training", params(("id" = u64, Path)),
    responses((status = 202, body = TrainingJob), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody), (status = 422, body = ErrorBody)))]
pub async fn resume(P(id): P<u64>) -> ApiResult<(StatusCode, Json<TrainingJob>)> {
    let job = training::resume(id).map_err(ApiError::core)?;
    Ok((StatusCode::ACCEPTED, Json(job)))
}

/// Merges the LoRA into the base model and writes safetensors into the model library (and a
/// GGUF when asked and the conversion environment is installed). Progress is the job's `export`.
#[utoipa::path(post, operation_id = "export_training_job", path = "/training/{id}/export", tag = "training", params(("id" = u64, Path)),
    request_body = ExportRequest,
    responses((status = 202, body = TrainingJob), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody), (status = 422, body = ErrorBody)))]
pub async fn export(
    P(id): P<u64>,
    Body(req): Body<ExportRequest>,
) -> ApiResult<(StatusCode, Json<TrainingJob>)> {
    let job = training::export(id, &req).map_err(ApiError::core)?;
    Ok((StatusCode::ACCEPTED, Json(job)))
}
