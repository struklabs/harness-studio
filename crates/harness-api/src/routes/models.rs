//! Library and downloads.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use harness_core::config::AppSettings;
use harness_core::domain::ModelDefinition;
use harness_core::downloader::{DownloadProgress, HfModelVariant};
use harness_core::library::ops::DeleteReport;
use harness_core::library::{self, LibraryEntry};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::{blocking, ApiError, ApiResult, Body, ErrorBody, P, Q};
use crate::AppState;

/// The library: every local model (all kinds: LLM, embedding, reranker, projector, adapter)
/// plus the model definitions built on them.
#[derive(Serialize, ToSchema)]
pub struct ModelsResponse {
    pub entries: Vec<LibraryEntry>,
    /// `state.db` unavailable: favorites / last use missing.
    pub state_error: Option<String>,
    pub definitions: Vec<ModelDefinition>,
}

async fn library_view() -> ApiResult<ModelsResponse> {
    blocking(|| {
        let (db, err) = match harness_core::state::shared() {
            Ok(db) => (Some(db), None),
            Err(e) => (None, Some(e)),
        };
        let view = library::list(db.as_deref(), err);
        let definitions = harness_core::domain::store::Domain::global()
            .definitions()
            .unwrap_or_default();
        ModelsResponse {
            entries: view.entries,
            state_error: view.state_error,
            definitions,
        }
    })
    .await
}

#[utoipa::path(get, path = "/models", tag = "models",
    responses((status = 200, body = ModelsResponse), (status = 401, body = ErrorBody)))]
pub async fn list_models() -> ApiResult<Json<ModelsResponse>> {
    Ok(Json(library_view().await?))
}

#[utoipa::path(get, path = "/models/{id}", tag = "models",
    params(("id" = String, Path, description = "Model id (the launch path), percent-encoded")),
    responses((status = 200, body = LibraryEntry), (status = 404, body = ErrorBody)))]
pub async fn get_model(P(id): P<String>) -> ApiResult<Json<LibraryEntry>> {
    library_view()
        .await?
        .entries
        .into_iter()
        .find(|e| e.model.id == id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("model", &id))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct FitQuery {
    /// `-c`; 0 or absent = the default context of the settings.
    pub context_size: Option<u32>,
    /// `-ngl`; absent = full offload.
    pub gpu_layers: Option<u32>,
    /// KV cache types; both absent = the automatic precision the launch uses (f16 unless
    /// the context does not fit, see `harness_core::kv_policy`; the result's `kv` says which).
    pub cache_type_k: Option<String>,
    pub cache_type_v: Option<String>,
    /// `-np`; absent = Harness's automatic rule (`harness_core::slots`).
    pub parallel_slots: Option<u32>,
    /// Projector path; absent = the one known for the model (imported models).
    pub mmproj_path: Option<String>,
}

/// Memory estimate of a local model on this machine (`harness_core::vram`, the estimate the
/// GUI and the CLI use): weights, KV cache (sliding-window aware), compute buffer, projector,
/// corrected with measured loads of the same file.
#[utoipa::path(get, operation_id = "get_model_fit", path = "/models/{id}/fit", tag = "models",
    params(("id" = String, Path, description = "Model id (the launch path), percent-encoded"), FitQuery),
    responses((status = 200, body = harness_core::vram::FitResult), (status = 404, body = ErrorBody)))]
pub async fn model_fit(
    P(id): P<String>,
    Q(q): Q<FitQuery>,
) -> ApiResult<Json<harness_core::vram::FitResult>> {
    let entry = library_view()
        .await?
        .entries
        .into_iter()
        .find(|e| e.model.id == id)
        .ok_or_else(|| ApiError::not_found("model", &id))?;
    let result = blocking(move || {
        let settings = AppSettings::load();
        let input = harness_core::vram::FitInput {
            model_path: Some(entry.model.path.to_string_lossy().to_string()),
            model_size_bytes: entry.model.size_bytes,
            context_size: q.context_size.filter(|&c| c > 0).unwrap_or(
                match settings.server.default_ctx {
                    // Automatic: what a chat launch would get.
                    0 => harness_core::prepare::default_chat_context(entry.model.context_length),
                    n => n,
                },
            ),
            gpu_layers: q.gpu_layers.unwrap_or(999),
            kv_auto: q.cache_type_k.is_none() && q.cache_type_v.is_none(),
            cache_type_k: q.cache_type_k,
            cache_type_v: q.cache_type_v,
            parallel_slots: q.parallel_slots,
            mmproj_path: q.mmproj_path.or_else(|| {
                entry
                    .model
                    .mmproj_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
            }),
            ..Default::default()
        };
        harness_core::vram::fit(input, &harness_core::hardware::detect_hardware())
    })
    .await?;
    Ok(Json(result))
}

/// What the default load of a model (context, `-ngl`, slots and KV cache all automatic) starts
/// with on this machine: the choice the launch itself makes, so `harness show` and the
/// Dashboard's form display what Start does. `null` when the rule does not apply (not a chat
/// model).
#[utoipa::path(get, operation_id = "get_model_automatic", path = "/models/{id}/automatic", tag = "models",
    params(("id" = String, Path, description = "Model id (the launch path), percent-encoded")),
    responses((status = 200, body = Option<harness_core::full_offload::AutomaticLaunch>),
        (status = 404, body = ErrorBody)))]
pub async fn model_automatic(
    P(id): P<String>,
) -> ApiResult<Json<Option<harness_core::full_offload::AutomaticLaunch>>> {
    let entry = library_view()
        .await?
        .entries
        .into_iter()
        .find(|e| e.model.id == id)
        .ok_or_else(|| ApiError::not_found("model", &id))?;
    let choice = blocking(move || {
        let config = harness_core::process::LaunchConfig {
            model_path: entry.model.path.clone(),
            gpu_layers: 999,
            mmproj_path: entry.model.mmproj_path.clone(),
            ..Default::default()
        };
        harness_core::full_offload::automatic_choice(
            &config,
            &harness_core::hardware::detect_hardware(),
        )
    })
    .await?;
    Ok(Json(choice))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DeleteModelQuery {
    /// Also delete the model definitions based on this file.
    #[serde(default)]
    pub delete_definitions: bool,
}

/// Same rules as the library screen: refused while a server uses one of its files
/// (`models.in_use`), for linked models and for files outside the library directories.
#[utoipa::path(delete, path = "/models/{id}", tag = "models",
    params(("id" = String, Path, description = "Model id, percent-encoded"), DeleteModelQuery),
    responses((status = 200, body = DeleteReport), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody)))]
pub async fn delete_model(
    State(s): State<AppState>,
    P(id): P<String>,
    Q(q): Q<DeleteModelQuery>,
) -> ApiResult<Json<DeleteReport>> {
    let report = harness_core::orchestrator::delete_library_model(
        &s.ctx.registry,
        &s.ctx.gate,
        id,
        q.delete_definitions,
    )
    .await?;
    Ok(Json(report))
}

#[derive(Deserialize, ToSchema)]
pub struct PullRequest {
    /// Hugging Face repo (`owner/name`).
    pub repo: String,
    /// A file of the variant (or the variant name). Takes precedence over `quant`.
    pub file: Option<String>,
    /// Quantization (`Q4_K_M`...), case-insensitive.
    pub quant: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct PullResponse {
    /// Download job id (`/downloads/{id}`, `download` events).
    pub job_id: u64,
    pub variant: HfModelVariant,
    /// Download jobs of the companion files a known image model needs (VAE, text encoders);
    /// empty for every other model.
    pub companions: Vec<u64>,
}

/// Queues a Hugging Face download into the primary library directory.
#[utoipa::path(post, path = "/models/pull", tag = "models", request_body = PullRequest,
    responses((status = 202, body = PullResponse), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody)))]
pub async fn pull_model(
    State(s): State<AppState>,
    Body(req): Body<PullRequest>,
) -> ApiResult<(StatusCode, Json<PullResponse>)> {
    let token = harness_core::hf_auth::current_token().await;
    let (job_id, variant) = s
        .ctx
        .downloads
        .pull(
            req.repo.trim(),
            req.file.as_deref(),
            req.quant.as_deref(),
            token.as_deref(),
        )
        .await?;
    let main_file = variant
        .shards
        .first()
        .map(|s| s.filename.clone())
        .unwrap_or_default();
    let companions = s
        .ctx
        .downloads
        .pull_companions(&main_file, token.as_deref())
        .await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(PullResponse {
            job_id,
            variant,
            companions,
        }),
    ))
}

#[utoipa::path(get, path = "/downloads", tag = "models",
    responses((status = 200, body = Vec<DownloadProgress>)))]
pub async fn list_downloads(State(s): State<AppState>) -> Json<Vec<DownloadProgress>> {
    Json(s.ctx.downloads.list())
}

fn job(s: &AppState, id: u64) -> ApiResult<DownloadProgress> {
    s.ctx
        .downloads
        .get(id)
        .ok_or_else(|| ApiError::not_found("download", &id.to_string()))
}

#[utoipa::path(get, path = "/downloads/{id}", tag = "models",
    params(("id" = u64, Path)),
    responses((status = 200, body = DownloadProgress), (status = 404, body = ErrorBody)))]
pub async fn get_download(
    State(s): State<AppState>,
    P(id): P<u64>,
) -> ApiResult<Json<DownloadProgress>> {
    Ok(Json(job(&s, id)?))
}

/// Pauses a queued or running download (partial data is kept).
#[utoipa::path(post, path = "/downloads/{id}/pause", tag = "models",
    params(("id" = u64, Path)),
    responses((status = 200, body = DownloadProgress), (status = 404, body = ErrorBody)))]
pub async fn pause_download(
    State(s): State<AppState>,
    P(id): P<u64>,
) -> ApiResult<Json<DownloadProgress>> {
    job(&s, id)?;
    let downloads = s.ctx.downloads.clone();
    blocking(move || downloads.cancel_download(Some(id))).await?;
    Ok(Json(job(&s, id)?))
}

#[utoipa::path(post, path = "/downloads/{id}/resume", tag = "models",
    params(("id" = u64, Path)),
    responses((status = 200, body = DownloadProgress), (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody)))]
pub async fn resume_download(
    State(s): State<AppState>,
    P(id): P<u64>,
) -> ApiResult<Json<DownloadProgress>> {
    job(&s, id)?;
    let downloads = s.ctx.downloads.clone();
    blocking(move || downloads.resume_download(id)).await??;
    Ok(Json(job(&s, id)?))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RemoveDownloadQuery {
    /// Also delete the partial files.
    #[serde(default)]
    pub delete_partial: bool,
}

/// Removes a job that is not running (pause it first).
#[utoipa::path(delete, path = "/downloads/{id}", tag = "models",
    params(("id" = u64, Path), RemoveDownloadQuery),
    responses((status = 204), (status = 404, body = ErrorBody), (status = 422, body = ErrorBody)))]
pub async fn remove_download(
    State(s): State<AppState>,
    P(id): P<u64>,
    Q(q): Q<RemoveDownloadQuery>,
) -> ApiResult<StatusCode> {
    job(&s, id)?;
    let downloads = s.ctx.downloads.clone();
    blocking(move || downloads.remove_download(id, q.delete_partial)).await??;
    Ok(StatusCode::NO_CONTENT)
}
