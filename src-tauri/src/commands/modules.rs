#[tauri::command]
pub(crate) async fn list_modules() -> Result<Vec<harness_core::modules::ModuleView>, String> {
    blocking(harness_core::modules::list).await?
}
#[tauri::command]
pub(crate) async fn install_module(id: String) -> Result<(), String> {
    harness_core::modules::install(&id).await
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
pub(crate) async fn detect_agent() -> Result<harness_core::modules::agents::Detection, String> {
    blocking(harness_core::modules::agents::detect).await?
}
#[tauri::command]
pub(crate) async fn connect_agent(gw: State<'_, GatewayState>, model: String) -> Result<(), String> {
    let status = gw.server.status().await;
    if !status.running { return Err("agents.gateway_required".into()); }
    let port = status.port.ok_or("agents.gateway_required")?;
    let host = match status.host.as_deref() {
        Some("127.0.0.1" | "localhost" | "0.0.0.0") => "127.0.0.1",
        Some("::1" | "::") => "[::1]",
        _ => return Err("agents.gateway_required".into()),
    };
    blocking(move || harness_core::modules::agents::connect(&format!("http://{host}:{port}/v1"), &model)).await?
}
#[tauri::command]
pub(crate) async fn disconnect_agent() -> Result<(), String> {
    blocking(harness_core::modules::agents::disconnect).await?
}
#[tauri::command]
pub(crate) fn agent_task_status() -> harness_core::modules::agents::TaskStatus {
    harness_core::modules::agents::task_status()
}
#[tauri::command]
pub(crate) async fn start_agent_task(prompt: String, grants: harness_core::modules::agents::RunGrants) -> Result<(), String> {
    blocking(move || harness_core::modules::agents::start_task_with_grants(prompt, grants)).await?
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
