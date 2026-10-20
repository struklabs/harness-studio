//! Automations: the user's own n8n instance (its public REST API only). Everything here needs
//! `admin`: the calls go out with the stored n8n API key, a webhook trigger runs a workflow,
//! and the key itself is never returned (it lives in the OS keychain).

use axum::extract::{Path, Query};
use axum::Json;
use harness_core::automations::{
    self, Execution, ExecutionError, N8nClient, N8nConnection, N8nView, Workflow,
};
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use crate::error::{blocking, ApiResult, Body, ErrorBody};

/// Saved n8n address and whether an API key is stored.
#[utoipa::path(get, path = "/automations/n8n", tag = "automations",
    responses((status = 200, body = N8nView), (status = 401, body = ErrorBody)))]
pub async fn get_n8n() -> ApiResult<Json<N8nView>> {
    Ok(Json(blocking(automations::view).await??))
}

#[derive(Deserialize, ToSchema)]
pub struct SaveN8nRequest {
    /// Empty disconnects (and removes the key).
    pub url: String,
    /// Absent keeps the saved key, `""` removes it.
    #[serde(default)]
    pub api_key: Option<String>,
}

#[utoipa::path(put, path = "/automations/n8n", tag = "automations", request_body = SaveN8nRequest,
    responses((status = 200, body = N8nView), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn put_n8n(Body(req): Body<SaveN8nRequest>) -> ApiResult<Json<N8nView>> {
    Ok(Json(
        blocking(move || automations::save(&req.url, req.api_key.as_deref())).await??,
    ))
}

#[derive(Deserialize, ToSchema, Default)]
pub struct TestN8nRequest {
    /// Unsaved address; absent uses the saved one.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
}

/// Checks the address and key against n8n's public API.
#[utoipa::path(post, path = "/automations/n8n/test", tag = "automations", request_body = TestN8nRequest,
    responses((status = 200, body = N8nConnection), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn test_n8n(Body(req): Body<TestN8nRequest>) -> ApiResult<Json<N8nConnection>> {
    let client = blocking(move || N8nClient::from_form(req.url.as_deref(), req.api_key.as_deref()))
        .await??;
    Ok(Json(client.test().await?))
}

/// Workflows with active state and triggers.
#[utoipa::path(get, path = "/automations/n8n/workflows", tag = "automations",
    responses((status = 200, body = Vec<Workflow>), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn n8n_workflows() -> ApiResult<Json<Vec<Workflow>>> {
    let client = blocking(N8nClient::saved).await??;
    Ok(Json(client.workflows().await?))
}

#[derive(Deserialize, IntoParams)]
pub struct ExecutionsQuery {
    pub workflow_id: Option<String>,
    pub limit: Option<u32>,
}

/// Recent executions with their status.
#[utoipa::path(get, path = "/automations/n8n/executions", tag = "automations", params(ExecutionsQuery),
    responses((status = 200, body = Vec<Execution>), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn n8n_executions(Query(q): Query<ExecutionsQuery>) -> ApiResult<Json<Vec<Execution>>> {
    let client = blocking(N8nClient::saved).await??;
    Ok(Json(
        client
            .executions(q.workflow_id.as_deref(), q.limit.unwrap_or(25))
            .await?,
    ))
}

/// Why an execution failed.
#[utoipa::path(get, path = "/automations/n8n/executions/{id}/error", tag = "automations",
    params(("id" = String, Path)),
    responses((status = 200, body = ExecutionError), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn n8n_execution_error(Path(id): Path<String>) -> ApiResult<Json<ExecutionError>> {
    let client = blocking(N8nClient::saved).await??;
    Ok(Json(client.execution_error(&id).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct TriggerN8nRequest {
    /// Must be `true`: starting a workflow is a deliberate act.
    pub confirm: bool,
    /// JSON body sent to the webhook (not for GET webhooks).
    #[serde(default)]
    pub payload: Option<serde_json::Value>,
}

/// Starts a workflow through its webhook trigger (active workflows only).
#[utoipa::path(post, path = "/automations/n8n/workflows/{id}/trigger", tag = "automations",
    params(("id" = String, Path)), request_body = TriggerN8nRequest,
    responses((status = 200, body = automations::TriggerResult), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn trigger_n8n(
    Path(id): Path<String>,
    Body(req): Body<TriggerN8nRequest>,
) -> ApiResult<Json<automations::TriggerResult>> {
    if !req.confirm {
        return Err(String::from(harness_core::error_key::err(
            "automations.n8n_confirm_required",
        ))
        .into());
    }
    let client = blocking(N8nClient::saved).await??;
    Ok(Json(client.trigger(&id, req.payload.as_ref()).await?))
}

/// The gateway's OpenAI-compatible base URL for n8n's OpenAI credentials.
#[utoipa::path(get, path = "/automations/n8n/openai", tag = "automations",
    responses((status = 200, body = automations::OpenAiConnection), (status = 403, body = ErrorBody)))]
pub async fn n8n_openai() -> ApiResult<Json<automations::OpenAiConnection>> {
    Ok(Json(automations::openai_connection()))
}
