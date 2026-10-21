fn connector_family(family: &str) -> Result<harness_core::connectors::Family, String> {
    match family {
        "google" => Ok(harness_core::connectors::Family::Google),
        "microsoft" => Ok(harness_core::connectors::Family::Microsoft),
        _ => Err("Unknown connector family".into()),
    }
}
fn connectors_sync_tools() {
    harness_core::connectors::sync(harness_core::tools::ToolService::shared().registry());
}
#[tauri::command]
pub(crate) async fn connectors_status() -> Result<Vec<harness_core::connectors::ConnectorStatus>, String> {
    blocking(harness_core::connectors::status).await
}
#[tauri::command]
pub(crate) async fn connectors_set_client(family: String, client_id: String, client_secret: Option<String>) -> Result<(), String> {
    let family = connector_family(&family)?;
    blocking(move || harness_core::connectors::set_client(family, &client_id, client_secret.as_deref())).await?
}
#[tauri::command]
pub(crate) async fn connectors_connect(id: String, write: bool, consent: bool) -> Result<(), String> {
    harness_core::connectors::oauth::connect(&id, write, consent, harness_core::tools::mcp_client::oauth::open_in_browser).await?;
    blocking(connectors_sync_tools).await
}
#[tauri::command]
pub(crate) fn connectors_cancel(id: String) -> bool {
    harness_core::connectors::oauth::cancel_login(&id)
}
#[tauri::command]
pub(crate) async fn connectors_disconnect(id: String) -> Result<harness_core::connectors::oauth::Disconnected, String> {
    let done = harness_core::connectors::oauth::disconnect(&id).await?;
    blocking(connectors_sync_tools).await?;
    Ok(done)
}
