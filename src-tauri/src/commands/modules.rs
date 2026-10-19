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
