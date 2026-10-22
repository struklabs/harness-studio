// Agents > Paperclip: the user's own Paperclip install (its REST API only; Harness never runs
// Paperclip here). The board key goes to the OS keychain and is never returned.

use harness_core::paperclip::{self, PaperclipClient};

#[tauri::command]
pub(crate) async fn paperclip_get_settings() -> Result<paperclip::PaperclipView, String> {
    blocking(paperclip::view).await?
}

/// Saves the address and key (`api_key`: `None` keeps the saved one, `""` removes it).
#[tauri::command]
pub(crate) async fn paperclip_save_settings(
    url: String,
    api_key: Option<String>,
) -> Result<paperclip::PaperclipView, String> {
    blocking(move || paperclip::save(&url, api_key.as_deref())).await?
}

/// Tests the form's (possibly unsaved) values; a missing address falls back to the saved one.
#[tauri::command]
pub(crate) async fn paperclip_test_connection(
    url: Option<String>,
    api_key: Option<String>,
) -> Result<paperclip::Connection, String> {
    let client =
        blocking(move || PaperclipClient::from_form(url.as_deref(), api_key.as_deref())).await??;
    client.test().await
}

#[tauri::command]
pub(crate) async fn paperclip_list_companies() -> Result<Vec<paperclip::Company>, String> {
    blocking(PaperclipClient::saved).await??.companies().await
}

#[tauri::command]
pub(crate) async fn paperclip_set_company(company_id: String) -> Result<paperclip::PaperclipView, String> {
    blocking(move || paperclip::set_company(&company_id)).await?
}

/// Teams, agents, tasks, runs and budgets of the saved company.
#[tauri::command]
pub(crate) async fn paperclip_overview() -> Result<paperclip::Overview, String> {
    PaperclipClient::saved_overview().await
}

/// Links a Paperclip task to a Harness project (`project_id: None` removes the link).
#[tauri::command]
pub(crate) async fn paperclip_link_task(
    store: State<'_, ConversationStore>,
    task_id: String,
    project_id: Option<String>,
) -> Result<paperclip::PaperclipView, String> {
    if let Some(id) = project_id.clone().filter(|p| !p.is_empty()) {
        with_db(&store, move |db| db.project(&id)).await?;
    }
    blocking(move || paperclip::set_task_link(&task_id, project_id.as_deref())).await?
}

/// Points an `opencode_local` Paperclip agent at a Harness model (scoped gateway token).
#[tauri::command]
pub(crate) async fn paperclip_use_model(
    gw: State<'_, GatewayState>,
    agent_id: String,
    model: String,
) -> Result<paperclip::ModelApplied, String> {
    let status = gw.server.status().await;
    if !status.running || !status.require_token {
        return Err("agents.token_required".into());
    }
    let base = status.endpoint.ok_or("agents.gateway_required")?.base_url;
    paperclip::use_harness_model(&agent_id, &model, &base).await
}
