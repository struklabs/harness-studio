//! Managed ONNX Runtime (the library `harness-onnx` loads): per-build state, install (pinned
//! SHA-256, downloaded on demand), verify, remove. Reads need `read`; changes need `admin`.

use axum::http::StatusCode;
use axum::Json;
use harness_core::onnx_runtime::{self, InstallProgress, Installed, OrtVariant, VariantView};
use serde::Serialize;
use utoipa::ToSchema;

use crate::error::{blocking, ApiResult, ErrorBody, P};

fn variant(id: &str) -> Result<OrtVariant, String> {
    OrtVariant::parse(id).ok_or_else(|| {
        String::from(harness_core::error_key::err("onnx.unsupported_platform").with("variant", id))
    })
}

/// Every ONNX Runtime build of this platform with its state.
#[utoipa::path(get, path = "/onnx-runtime", tag = "runtimes",
    responses((status = 200, body = Vec<VariantView>), (status = 401, body = ErrorBody)))]
pub async fn list_onnx_runtime() -> ApiResult<Json<Vec<VariantView>>> {
    Ok(Json(blocking(onnx_runtime::view).await?))
}

/// Download progress of the running install (`null` when idle).
#[utoipa::path(get, path = "/onnx-runtime/progress", tag = "runtimes",
    responses((status = 200, body = Option<InstallProgress>)))]
pub async fn onnx_runtime_progress() -> Json<Option<InstallProgress>> {
    Json(onnx_runtime::progress())
}

/// Downloads and installs a build (`directml`, `cuda`, `cpu`); every file is checked against
/// its pinned SHA-256.
#[utoipa::path(post, path = "/onnx-runtime/{variant}", tag = "runtimes",
    params(("variant" = String, Path)),
    responses((status = 201, body = Installed), (status = 422, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn install_onnx_runtime(P(id): P<String>) -> ApiResult<(StatusCode, Json<Installed>)> {
    let v = variant(&id)?;
    Ok((StatusCode::CREATED, Json(onnx_runtime::install(v).await?)))
}

#[derive(Serialize, ToSchema)]
pub struct OnnxVerifyReport {
    pub ok: bool,
    /// Files whose hash differs or that are missing.
    pub problems: Vec<String>,
}

/// Re-hashes the installed files against the pinned digests.
#[utoipa::path(post, path = "/onnx-runtime/{variant}/verify", tag = "runtimes",
    params(("variant" = String, Path)),
    responses((status = 200, body = OnnxVerifyReport), (status = 422, body = ErrorBody)))]
pub async fn verify_onnx_runtime(P(id): P<String>) -> ApiResult<Json<OnnxVerifyReport>> {
    let v = variant(&id)?;
    let problems = blocking(move || onnx_runtime::verify_in(&onnx_runtime::root(), v)).await??;
    Ok(Json(OnnxVerifyReport {
        ok: problems.is_empty(),
        problems,
    }))
}

/// Removes an installed build (it is downloaded again on the next use).
#[utoipa::path(delete, path = "/onnx-runtime/{variant}", tag = "runtimes",
    params(("variant" = String, Path)),
    responses((status = 204), (status = 422, body = ErrorBody)))]
pub async fn remove_onnx_runtime(P(id): P<String>) -> ApiResult<StatusCode> {
    let v = variant(&id)?;
    blocking(move || onnx_runtime::remove(v)).await??;
    Ok(StatusCode::NO_CONTENT)
}
