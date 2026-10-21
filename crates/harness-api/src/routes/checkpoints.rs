//! Undo for agent file changes (`harness_core::tools::checkpoint`). Every route needs `admin`:
//! the list shows file paths, and an undo writes files.

use axum::http::StatusCode;
use axum::Json;
use harness_core::tools::checkpoint::{Checkpoint, Checkpoints};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::error::{blocking, ApiResult, Body, ErrorBody, OptBody, P};

/// File changes the agent tools made, newest first.
#[utoipa::path(get, operation_id = "list_checkpoints", path = "/checkpoints", tag = "tools",
    responses((status = 200, body = Vec<Checkpoint>)))]
pub async fn list() -> ApiResult<Json<Vec<Checkpoint>>> {
    Ok(Json(blocking(|| Checkpoints::shared().list()).await?))
}

#[derive(Deserialize, ToSchema, Default)]
pub struct UndoRequest {
    /// Restore even if the file changed after the agent's write.
    #[serde(default)]
    pub force: bool,
}

/// Undoes one change: the file gets its previous content back (a file the agent created is
/// deleted). `409` when the file changed since, the change was undone already, or its
/// previous content was too large to keep.
#[utoipa::path(post, operation_id = "undo_checkpoint", path = "/checkpoints/{id}/undo",
    tag = "tools", params(("id" = String, Path)), request_body = Option<UndoRequest>,
    responses((status = 200, body = Checkpoint), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody)))]
pub async fn undo(
    P(id): P<String>,
    OptBody(body): OptBody<UndoRequest>,
) -> ApiResult<Json<Checkpoint>> {
    let force = body.unwrap_or_default().force;
    Ok(Json(
        blocking(move || Checkpoints::shared().undo(&id, force)).await??,
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct UndoRunRequest {
    pub group: String,
    #[serde(default)]
    pub force: bool,
}

/// Undoes every change of one run, newest first; stops at the first refusal.
#[utoipa::path(post, operation_id = "undo_checkpoint_run", path = "/checkpoints/undo-run",
    tag = "tools", request_body = UndoRunRequest,
    responses((status = 200, body = Vec<Checkpoint>), (status = 409, body = ErrorBody)))]
pub async fn undo_run(Body(req): Body<UndoRunRequest>) -> ApiResult<Json<Vec<Checkpoint>>> {
    Ok(Json(
        blocking(move || Checkpoints::shared().undo_group(&req.group, req.force)).await??,
    ))
}

/// Deletes every checkpoint and its kept content (the files themselves are not touched).
#[utoipa::path(delete, operation_id = "clear_checkpoints", path = "/checkpoints", tag = "tools",
    responses((status = 204)))]
pub async fn clear() -> ApiResult<StatusCode> {
    blocking(|| Checkpoints::shared().clear()).await??;
    Ok(StatusCode::NO_CONTENT)
}
