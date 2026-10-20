// Settings > Automations: the user's own n8n instance (public REST API only; Harness never
// runs n8n). The API key goes to the OS keychain and is never returned.

use harness_core::automations::{self as n8n, N8nClient};

#[tauri::command]
pub(crate) async fn n8n_get_settings() -> Result<n8n::N8nView, String> {
    blocking(n8n::view).await?
}

/// Saves the address and key (`api_key`: `None` keeps the saved one, `""` removes it).
#[tauri::command]
pub(crate) async fn n8n_save_settings(
    url: String,
    api_key: Option<String>,
) -> Result<n8n::N8nView, String> {
    blocking(move || n8n::save(&url, api_key.as_deref())).await?
}

/// Tests the form's (possibly unsaved) values; missing ones fall back to the saved ones.
#[tauri::command]
pub(crate) async fn n8n_test_connection(
    url: Option<String>,
    api_key: Option<String>,
) -> Result<n8n::N8nConnection, String> {
    let client = blocking(move || N8nClient::from_form(url.as_deref(), api_key.as_deref())).await??;
    client.test().await
}

#[tauri::command]
pub(crate) async fn n8n_list_workflows() -> Result<Vec<n8n::Workflow>, String> {
    blocking(N8nClient::saved).await??.workflows().await
}

#[tauri::command]
pub(crate) async fn n8n_list_executions(
    workflow_id: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<n8n::Execution>, String> {
    blocking(N8nClient::saved)
        .await??
        .executions(workflow_id.as_deref(), limit.unwrap_or(25))
        .await
}

#[tauri::command]
pub(crate) async fn n8n_execution_error(id: String) -> Result<n8n::ExecutionError, String> {
    blocking(N8nClient::saved).await??.execution_error(&id).await
}

/// Starts a workflow through its webhook trigger. `confirmed` must be true: the UI asks first.
#[tauri::command]
pub(crate) async fn n8n_trigger_workflow(
    id: String,
    payload: Option<serde_json::Value>,
    confirmed: bool,
) -> Result<n8n::TriggerResult, String> {
    if !confirmed {
        return Err(harness_core::error_key::err("automations.n8n_confirm_required").into());
    }
    blocking(N8nClient::saved)
        .await??
        .trigger(&id, payload.as_ref())
        .await
}

/// The gateway's OpenAI-compatible base URL for n8n's OpenAI credentials.
#[tauri::command]
pub(crate) async fn n8n_openai_connection() -> Result<n8n::OpenAiConnection, String> {
    Ok(n8n::openai_connection())
}

/// Creates an inference-only gateway token named "n8n" (its value is shown once).
#[tauri::command]
pub(crate) async fn n8n_create_gateway_token() -> Result<crate::control_api::CreatedToken, String> {
    blocking(n8n::create_gateway_token).await?
}
