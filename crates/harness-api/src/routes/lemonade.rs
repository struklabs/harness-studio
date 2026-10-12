//! Lemonade, the optional managed engine for Ryzen AI NPUs and its own llama.cpp backends
//!: state, install, start / stop, the model registry and backends. Reads need
//! `read`; changes need `admin` (Lemonade is third-party code Harness runs and downloads
//! models and backends for).

use axum::http::StatusCode;
use axum::Json;
use harness_core::lemonade::{
    self, InstallRequest, LemonadeSettings, LemonadeStatus, ModelList, PullProgress,
};
use harness_core::runtimes::github::ReleasePick;
use harness_core::runtimes::RuntimeEntry;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use crate::error::{blocking, ApiResult, Body, ErrorBody, OptBody, P, Q};

/// State of the engine: installed builds, whether it runs, the NPU, backends and downloads.
#[utoipa::path(get, path = "/lemonade", tag = "lemonade",
    responses((status = 200, body = LemonadeStatus), (status = 401, body = ErrorBody)))]
pub async fn lemonade_status() -> ApiResult<Json<LemonadeStatus>> {
    Ok(Json(blocking(lemonade::status).await?))
}

#[derive(Deserialize, IntoParams)]
pub struct ReleaseParams {
    /// Release tag (default: the newest stable release).
    pub tag: Option<String>,
}

/// The archive an install would download (name, size, URL, SHA-256), without downloading.
#[utoipa::path(get, path = "/lemonade/release", tag = "lemonade", params(ReleaseParams),
    responses((status = 200, body = ReleasePick), (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody)))]
pub async fn lemonade_release(Q(q): Q<ReleaseParams>) -> ApiResult<Json<ReleasePick>> {
    Ok(Json(lemonade::preview_release(q.tag).await?))
}

/// Downloads the Lemonade release (checked against GitHub's SHA-256), extracts it and
/// registers it as a runtime of the `lemonade` family. A newer release is a new entry; the
/// older one stays as the rollback.
#[utoipa::path(post, path = "/lemonade/install", tag = "lemonade",
    request_body(content = Option<InstallRequest>),
    responses((status = 201, body = RuntimeEntry), (status = 409, body = ErrorBody),
        (status = 422, body = ErrorBody), (status = 403, body = ErrorBody)))]
pub async fn lemonade_install(
    OptBody(req): OptBody<InstallRequest>,
) -> ApiResult<(StatusCode, Json<RuntimeEntry>)> {
    let entry = lemonade::install(req.unwrap_or_default()).await?;
    Ok((StatusCode::CREATED, Json(entry)))
}

/// Removes an installed build (409 while it runs).
#[utoipa::path(delete, path = "/lemonade/installs/{id}", tag = "lemonade",
    params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody)))]
pub async fn lemonade_remove(P(id): P<String>) -> ApiResult<StatusCode> {
    blocking(move || lemonade::remove_install(&id)).await??;
    Ok(StatusCode::NO_CONTENT)
}

/// Saves the settings (the llama.cpp backend, the pinned build); a running `lemond` is stopped
/// and starts again with them on its next use.
#[utoipa::path(put, path = "/lemonade/settings", tag = "lemonade",
    request_body = LemonadeSettings,
    responses((status = 200, body = LemonadeSettings), (status = 422, body = ErrorBody)))]
pub async fn lemonade_settings(
    Body(s): Body<LemonadeSettings>,
) -> ApiResult<Json<LemonadeSettings>> {
    Ok(Json(lemonade::save_settings(s).await?))
}

/// Starts `lemond` on a port Harness picks (loopback, locked with a per-run key).
#[utoipa::path(post, path = "/lemonade/start", tag = "lemonade",
    responses((status = 200, body = LemonadeStatus), (status = 409, body = ErrorBody),
        (status = 422, body = ErrorBody)))]
pub async fn lemonade_start() -> ApiResult<Json<LemonadeStatus>> {
    lemonade::start().await?;
    Ok(Json(blocking(lemonade::status).await?))
}

/// Stops `lemond` and the backends it started.
#[utoipa::path(post, path = "/lemonade/stop", tag = "lemonade",
    responses((status = 200, body = LemonadeStatus)))]
pub async fn lemonade_stop() -> ApiResult<Json<LemonadeStatus>> {
    lemonade::stop().await?;
    Ok(Json(blocking(lemonade::status).await?))
}

/// The last lines of `lemond`'s output.
#[utoipa::path(get, path = "/lemonade/logs", tag = "lemonade", params(LogParams),
    responses((status = 200, body = Vec<String>)))]
pub async fn lemonade_logs(Q(q): Q<LogParams>) -> Json<Vec<String>> {
    Json(lemonade::log_tail(q.lines.unwrap_or(100).min(200)))
}

#[derive(Deserialize, IntoParams)]
pub struct LogParams {
    pub lines: Option<usize>,
}

/// Lemonade's model registry with the downloaded flag: current when `lemond` runs
/// (`live: true`), the last snapshot otherwise. NPU models (FastFlowLM) appear once a backend
/// for them is available on this machine.
#[utoipa::path(get, path = "/lemonade/models", tag = "lemonade",
    responses((status = 200, body = ModelList), (status = 422, body = ErrorBody)))]
pub async fn lemonade_models() -> ApiResult<Json<ModelList>> {
    Ok(Json(lemonade::list_models().await?))
}

/// Progress of the running model pull or backend install (`null` when idle).
#[utoipa::path(get, path = "/lemonade/progress", tag = "lemonade",
    responses((status = 200, body = Option<PullProgress>)))]
pub async fn lemonade_progress() -> Json<Option<PullProgress>> {
    Json(lemonade::pull_progress())
}

#[derive(Deserialize, ToSchema)]
pub struct ModelRequest {
    /// A Lemonade registry name (`Qwen3-0.6B-GGUF`) or a checkpoint (`owner/repo[:variant]`).
    pub model: String,
}

/// Downloads a model into Lemonade (starts `lemond` if needed); returns when it is done.
/// Progress: `GET /lemonade/progress`.
#[utoipa::path(post, path = "/lemonade/models/pull", tag = "lemonade",
    request_body = ModelRequest,
    responses((status = 204), (status = 409, body = ErrorBody), (status = 422, body = ErrorBody)))]
pub async fn lemonade_pull(Body(req): Body<ModelRequest>) -> ApiResult<StatusCode> {
    lemonade::pull_model(&req.model).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Deletes a downloaded model from Lemonade.
#[utoipa::path(post, path = "/lemonade/models/delete", tag = "lemonade",
    request_body = ModelRequest,
    responses((status = 204), (status = 422, body = ErrorBody)))]
pub async fn lemonade_delete(Body(req): Body<ModelRequest>) -> ApiResult<StatusCode> {
    lemonade::delete_model(&req.model).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct BackendRequest {
    /// `llamacpp`, `flm`, `whispercpp`...
    pub recipe: String,
    /// `vulkan`, `rocm`, `cpu`, `npu`...
    pub backend: String,
}

/// Installs a backend Lemonade runs models on (llama.cpp ROCm / Vulkan, FastFlowLM on an
/// NPU...); returns when it is done. Progress: `GET /lemonade/progress`.
#[utoipa::path(post, path = "/lemonade/backends/install", tag = "lemonade",
    request_body = BackendRequest,
    responses((status = 204), (status = 409, body = ErrorBody), (status = 422, body = ErrorBody)))]
pub async fn lemonade_install_backend(Body(req): Body<BackendRequest>) -> ApiResult<StatusCode> {
    lemonade::install_backend(&req.recipe, &req.backend).await?;
    Ok(StatusCode::NO_CONTENT)
}
