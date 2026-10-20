#[tauri::command]
pub(crate) async fn list_modules() -> Result<Vec<harness_core::modules::ModuleView>, String> {
    blocking(harness_core::modules::list).await?
}
#[tauri::command]
pub(crate) async fn install_module(id: String) -> Result<(), String> {
    harness_core::modules::start_install(&id).await
}
#[tauri::command]
pub(crate) fn cancel_module_install(id: String) -> Result<(), String> {
    harness_core::modules::cancel(&id)
}
#[tauri::command]
pub(crate) async fn remove_module(id: String) -> Result<(), String> {
    blocking(move || harness_core::modules::remove(&id)).await?
}

#[tauri::command]
pub(crate) async fn detect_agent(agent: Option<String>) -> Result<harness_core::modules::agents::Detection, String> {
    let kind = agent_kind(agent)?;
    blocking(move || harness_core::modules::agents::detect_kind(kind)).await?
}
fn agent_kind(agent: Option<String>) -> Result<harness_core::modules::agents::AgentKind, String> {
    agent.as_deref().map_or(Ok(Default::default()), harness_core::modules::agents::AgentKind::parse)
}
/// Installs Harness's own pinned OpenClaw with npm (the official method).
#[tauri::command]
pub(crate) async fn install_openclaw() -> Result<(), String> {
    harness_core::modules::agents::install_openclaw().await
}
#[tauri::command]
pub(crate) fn cancel_openclaw_install() {
    harness_core::modules::agents::cancel_install_kind(harness_core::modules::agents::AgentKind::OpenClaw);
}
#[tauri::command]
pub(crate) async fn remove_openclaw() -> Result<(), String> {
    blocking(harness_core::modules::agents::remove_openclaw).await?
}
/// Chat models for agents and Paperclip: local models plus Harness Link models.
#[tauri::command]
pub(crate) async fn list_agent_choices() -> Result<Vec<harness_core::integrations::agent_models::AgentModel>, String> {
    blocking(harness_core::integrations::agent_models::list_with_link).await?
}
#[tauri::command]
pub(crate) async fn connect_agent(gw: State<'_, GatewayState>, model: String, agent: Option<String>) -> Result<(), String> {
    let kind = agent_kind(agent)?;
    let status = gw.server.status().await;
    if !status.running { return Err("agents.gateway_required".into()); }
    let port = status.port.ok_or("agents.gateway_required")?;
    let host = match status.host.as_deref() {
        Some("127.0.0.1" | "localhost" | "0.0.0.0") => "127.0.0.1",
        Some("::1" | "::") => "[::1]",
        _ => return Err("agents.gateway_required".into()),
    };
    blocking(move || harness_core::modules::agents::connect_kind(kind, &format!("http://{host}:{port}/v1"), &model)).await?
}
#[tauri::command]
pub(crate) async fn disconnect_agent(agent: Option<String>) -> Result<(), String> {
    let kind = agent_kind(agent)?;
    blocking(move || harness_core::modules::agents::disconnect_kind(kind)).await?
}
#[tauri::command]
pub(crate) fn agent_task_status() -> harness_core::modules::agents::TaskStatus {
    harness_core::modules::agents::task_status()
}
#[tauri::command]
pub(crate) async fn start_agent_task(prompt: String, grants: harness_core::modules::agents::RunGrants, agent: Option<String>) -> Result<(), String> {
    let kind = agent_kind(agent)?;
    blocking(move || harness_core::modules::agents::start_task_kind(kind, prompt, grants)).await?
}
#[tauri::command]
pub(crate) fn stop_agent_task() { harness_core::modules::agents::stop(); }

#[tauri::command]
pub(crate) async fn agent_task_history() -> Result<Vec<harness_core::modules::agents::TaskStatus>, String> {
    blocking(harness_core::modules::agents::task_history).await?
}
#[tauri::command]
pub(crate) async fn delete_agent_task(id: String) -> Result<(), String> {
    blocking(move || harness_core::modules::agents::delete_task(&id)).await?
}
#[tauri::command]
pub(crate) async fn paperclip_handoff(gw: State<'_, GatewayState>, model: String) -> Result<harness_core::modules::paperclip::Handoff, String> {
    let status = gw.server.status().await;
    if !status.running || !status.require_token { return Err("agents.token_required".into()); }
    let base = status.endpoint.ok_or("agents.gateway_required")?.base_url;
    blocking(move || harness_core::modules::paperclip::generate(&base, &model)).await?
}
#[tauri::command]
pub(crate) async fn revoke_paperclip_token(id: String) -> Result<(), String> {
    blocking(move || harness_core::modules::paperclip::revoke(&id)).await?
}
#[tauri::command]
pub(crate) async fn apply_paperclip_config(instance: String, agent: String, credential: String, config: serde_json::Value) -> Result<(), String> {
    harness_core::modules::paperclip::apply(instance, agent, credential, config).await
}
#[tauri::command]
pub(crate) async fn module_action(id: String, action: String) -> Result<(), String> {
    harness_core::modules::start(&id, &action).await
}
#[tauri::command]
pub(crate) async fn remove_module_data(id: String, options: harness_core::modules::RemovalOptions) -> Result<(), String> {
    blocking(move || harness_core::modules::lifecycle_remove(&id, &options)).await?
}
