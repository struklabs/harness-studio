//! Runtime registry (P2 alternative runtimes): list, add (GitHub release / local folder),
//! verify, remove, and the per-model runtime advice. Reads need `read`; every change needs
//! `admin` (a runtime is third-party code the servers execute).

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use harness_core::config::AppSettings;
use harness_core::runtimes::github::{self, GithubInstall, ReleasePick, ReleaseQuery};
use harness_core::runtimes::{
    self, AddLocal, RuntimeAdvice, RuntimeEntry, RuntimeRegistry, RuntimeView, VerifyReport,
};
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use crate::error::{blocking, ApiResult, Body, ErrorBody, OptBody, P, Q};
use crate::AppState;

#[utoipa::path(get, path = "/runtimes", tag = "runtimes",
    responses((status = 200, body = Vec<RuntimeView>), (status = 401, body = ErrorBody)))]
pub async fn list_runtimes(State(s): State<AppState>) -> ApiResult<Json<Vec<RuntimeView>>> {
    let mgr = s.ctx.runtime.clone();
    Ok(Json(
        blocking(move || runtimes::list_in(&RuntimeRegistry::global(), &mgr, &AppSettings::load()))
            .await??,
    ))
}

#[utoipa::path(get, path = "/runtimes/{id}", tag = "runtimes",
    params(("id" = String, Path, description = "Runtime id (`official` = Settings > Runtime)")),
    responses((status = 200, body = RuntimeView), (status = 404, body = ErrorBody)))]
pub async fn get_runtime(
    State(s): State<AppState>,
    P(id): P<String>,
) -> ApiResult<Json<RuntimeView>> {
    let mgr = s.ctx.runtime.clone();
    Ok(Json(
        blocking(move || {
            runtimes::view_in(&RuntimeRegistry::global(), &mgr, &AppSettings::load(), &id)
        })
        .await??,
    ))
}

/// Add request: a GitHub release (`github`) or a local folder (`local`).
#[derive(Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AddRuntimeRequest {
    Github(GithubInstall),
    Local(AddLocal),
}

/// Adds a runtime. `github`: downloads the release archive (checked against GitHub's SHA-256
/// digest; an archive without one is refused with 409 `runtime.integrity_unverified` unless
/// `allow_unverified`), extracts it under the data directory and runs `--version`. `local`:
/// runs `--version` and hashes the folder's executables (in place, or copied with `copy`).
#[utoipa::path(post, path = "/runtimes", tag = "runtimes", request_body = AddRuntimeRequest,
    responses((status = 201, body = RuntimeEntry), (status = 409, body = ErrorBody),
        (status = 422, body = ErrorBody), (status = 403, body = ErrorBody)))]
pub async fn add_runtime(
    Body(req): Body<AddRuntimeRequest>,
) -> ApiResult<(StatusCode, Json<RuntimeEntry>)> {
    let entry = match req {
        AddRuntimeRequest::Github(g) => github::install(&RuntimeRegistry::global(), &g).await?,
        AddRuntimeRequest::Local(l) => {
            blocking(move || RuntimeRegistry::global().add_local(&l)).await??
        }
    };
    Ok((StatusCode::CREATED, Json(entry)))
}

/// The archive(s) a GitHub install would download (name, size, URL, digest), without
/// downloading anything.
#[utoipa::path(get, path = "/runtimes/release", tag = "runtimes", params(ReleaseParams),
    responses((status = 200, body = ReleasePick), (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody)))]
pub async fn preview_release(Q(q): Q<ReleaseParams>) -> ApiResult<Json<ReleasePick>> {
    let query = ReleaseQuery {
        family: q
            .family
            .unwrap_or_else(harness_core::engine::default_family),
        repo: q.repo,
        tag: q.tag,
        asset: q.asset,
        backend: q.backend,
    };
    Ok(Json(github::find_release(&query).await?))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ReleaseParams {
    /// Runtime family whose release naming picks the archive (default: `llama.cpp`).
    pub family: Option<String>,
    /// `owner/repo`.
    pub repo: String,
    pub tag: Option<String>,
    pub asset: Option<String>,
    pub backend: Option<String>,
}

#[derive(Deserialize, ToSchema, Default)]
pub struct VerifyRequest {
    /// Local runtimes: take the current files as the new baseline.
    #[serde(default)]
    pub accept_changes: bool,
}

/// Re-hashes the runtime's files against the hashes recorded when it was added and runs
/// `--version`.
#[utoipa::path(post, path = "/runtimes/{id}/verify", tag = "runtimes",
    params(("id" = String, Path)), request_body(content = Option<VerifyRequest>),
    responses((status = 200, body = VerifyReport), (status = 404, body = ErrorBody)))]
pub async fn verify_runtime(
    P(id): P<String>,
    OptBody(req): OptBody<VerifyRequest>,
) -> ApiResult<Json<VerifyReport>> {
    let accept = req.unwrap_or_default().accept_changes;
    Ok(Json(
        blocking(move || RuntimeRegistry::global().verify(&id, accept)).await??,
    ))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RemoveParams {
    /// Remove even when definitions/profiles still choose it.
    #[serde(default)]
    pub force: bool,
}

/// Unregisters a runtime; a folder Harness created (download/copy) is deleted, a local folder
/// used in place is kept. 409 `runtime.in_use` while a server runs it (or, without `force`,
/// while definitions/profiles choose it).
#[utoipa::path(delete, path = "/runtimes/{id}", tag = "runtimes",
    params(("id" = String, Path), RemoveParams),
    responses((status = 204), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody)))]
pub async fn remove_runtime(
    State(s): State<AppState>,
    P(id): P<String>,
    Q(q): Q<RemoveParams>,
) -> ApiResult<StatusCode> {
    harness_core::orchestrator::remove_runtime(&s.ctx.registry, &s.ctx.gate, id, q.force).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AdviceParams {
    /// Model file path, library model id, or definition alias.
    pub model: String,
    /// The runtime currently chosen (preferred when it fits).
    pub runtime: Option<String>,
}

/// Which runtimes can load a model (from its GGUF tensor types) and what to use: the
/// suggested registered runtime, or the fork to add (e.g. PrismML for Bonsai `PQ2_0`).
#[utoipa::path(get, path = "/runtimes/advice", tag = "runtimes", params(AdviceParams),
    responses((status = 200, body = RuntimeAdvice), (status = 404, body = ErrorBody)))]
pub async fn runtime_advice(
    State(s): State<AppState>,
    Q(q): Q<AdviceParams>,
) -> ApiResult<Json<RuntimeAdvice>> {
    let mgr = s.ctx.runtime.clone();
    let advice = blocking(move || {
        let path = model_path(&q.model)?;
        runtimes::advise(&mgr, &AppSettings::load(), &path, q.runtime.as_deref())
    })
    .await??;
    Ok(Json(advice))
}

/// A path, a library model id, or a definition alias -> the GGUF path.
fn model_path(model: &str) -> Result<std::path::PathBuf, String> {
    let p = std::path::PathBuf::from(model);
    if p.is_file() {
        return Ok(p);
    }
    if let Some(m) = harness_core::models::ModelManager.find_model(model) {
        return Ok(m.path);
    }
    let id = harness_core::orchestrator::resolve_definition(model)?;
    let def = harness_core::domain::store::Domain::global().definition(&id)?;
    def.base
        .local_path()
        .map(std::path::Path::to_path_buf)
        .ok_or_else(|| {
            format!(
                "{}: {}",
                harness_core::orchestrator::ERR_MODEL_NOT_FOUND,
                model
            )
        })
}

/// Download progress of a runtime install (`null` when idle).
#[utoipa::path(get, path = "/runtimes/progress", tag = "runtimes",
    responses((status = 200, body = Option<Object>)))]
pub async fn install_progress() -> Json<serde_json::Value> {
    Json(serde_json::to_value(github::progress()).unwrap_or(serde_json::Value::Null))
}
