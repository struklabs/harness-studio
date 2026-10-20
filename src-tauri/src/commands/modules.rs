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
pub(crate) async fn start_agent_task(prompt: String) -> Result<(), String> {
    blocking(move || harness_core::modules::agents::start_task(prompt)).await?
}
#[tauri::command]
pub(crate) fn stop_agent_task() { harness_core::modules::agents::stop(); }
