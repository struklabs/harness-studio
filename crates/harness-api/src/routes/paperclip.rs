//! Paperclip: the user's own Paperclip install (its REST API only). Everything here needs
//! `admin`: the calls go out with the stored board key, a model handoff writes into a
//! Paperclip agent, and the key itself is never returned (it lives in the OS keychain).

use axum::extract::Path;
use axum::Json;
use harness_core::paperclip::{
    self, Connection, ModelApplied, Overview, PaperclipClient, PaperclipView,
};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::error::{blocking, ApiResult, Body, ErrorBody};

/// Saved Paperclip address, company, task links and whether a key is stored.
#[utoipa::path(get, path = "/paperclip", tag = "paperclip",
    responses((status = 200, body = PaperclipView), (status = 401, body = ErrorBody)))]
pub async fn get_paperclip() -> ApiResult<Json<PaperclipView>> {
    Ok(Json(blocking(paperclip::view).await??))
}

#[derive(Deserialize, ToSchema)]
pub struct SavePaperclipRequest {
    /// Empty disconnects (and removes the key, company and links).
    pub url: String,
    /// Absent keeps the saved key, `""` removes it. Not needed for a local trusted Paperclip.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Company to show; absent keeps the saved one.
    #[serde(default)]
    pub company_id: Option<String>,
}

#[utoipa::path(put, path = "/paperclip", tag = "paperclip", request_body = SavePaperclipRequest,
    responses((status = 200, body = PaperclipView), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn put_paperclip(
    Body(req): Body<SavePaperclipRequest>,
) -> ApiResult<Json<PaperclipView>> {
    Ok(Json(
        blocking(move || {
            let view = paperclip::save(&req.url, req.api_key.as_deref())?;
            match req.company_id.as_deref().filter(|c| !c.is_empty()) {
                Some(c) if !view.url.is_empty() => paperclip::set_company(c),
                _ => Ok(view),
            }
        })
        .await??,
    ))
}

#[derive(Deserialize, ToSchema, Default)]
pub struct TestPaperclipRequest {
    /// Unsaved address; absent uses the saved one.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
}

/// Checks the address and key against Paperclip's health and company list.
#[utoipa::path(post, path = "/paperclip/test", tag = "paperclip", request_body = TestPaperclipRequest,
    responses((status = 200, body = Connection), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn test_paperclip(Body(req): Body<TestPaperclipRequest>) -> ApiResult<Json<Connection>> {
    let client =
        blocking(move || PaperclipClient::from_form(req.url.as_deref(), req.api_key.as_deref()))
            .await??;
    Ok(Json(client.test().await?))
}

/// Agents, teams, tasks, runs and budgets of the saved company.
#[utoipa::path(get, path = "/paperclip/overview", tag = "paperclip",
    responses((status = 200, body = Overview), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn paperclip_overview() -> ApiResult<Json<Overview>> {
    Ok(Json(PaperclipClient::saved_overview().await?))
}

#[derive(Deserialize, ToSchema)]
pub struct UseModelRequest {
    /// Gateway model alias (a local model or a Harness Link model).
    pub model: String,
}

/// Points an `opencode_local` Paperclip agent at a Harness model through the gateway: a token
/// for exactly that model (chat and model-list endpoints) is created and written into the
/// agent's adapter config.
#[utoipa::path(post, path = "/paperclip/agents/{id}/model", tag = "paperclip",
    params(("id" = String, Path)), request_body = UseModelRequest,
    responses((status = 200, body = ModelApplied), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn paperclip_use_model(
    Path(id): Path<String>,
    Body(req): Body<UseModelRequest>,
) -> ApiResult<Json<ModelApplied>> {
    let status = harness_core::gateway::status();
    if !status.running || !status.require_token {
        return Err(String::from("agents.token_required").into());
    }
    let base = status
        .endpoint
        .ok_or_else(|| String::from("agents.gateway_required"))?
        .base_url;
    Ok(Json(
        paperclip::use_harness_model(&id, &req.model, &base).await?,
    ))
}
