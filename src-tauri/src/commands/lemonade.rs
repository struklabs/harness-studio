// Lemonade, the optional engine for Ryzen AI NPUs and its own llama.cpp backends:
// Settings > Model Engine card. The same operations as the Control API's `/lemonade` routes.

#[tauri::command]
pub(crate) async fn get_lemonade_status() -> Result<harness_core::lemonade::LemonadeStatus, String>
{
    blocking(harness_core::lemonade::status).await
}

/// The release an install would download (shown before asking the user).
#[tauri::command]
pub(crate) async fn preview_lemonade_release(
    tag: Option<String>,
) -> Result<crate::runtimes::github::ReleasePick, String> {
    harness_core::lemonade::preview_release(tag).await
}

#[tauri::command]
pub(crate) async fn install_lemonade(
    tag: Option<String>,
) -> Result<crate::runtimes::RuntimeEntry, String> {
    harness_core::lemonade::install(harness_core::lemonade::InstallRequest { tag }).await
}

#[tauri::command]
pub(crate) async fn remove_lemonade_install(id: String) -> Result<(), String> {
    blocking(move || harness_core::lemonade::remove_install(&id)).await?
}

#[tauri::command]
pub(crate) async fn save_lemonade_settings(
    settings: harness_core::lemonade::LemonadeSettings,
) -> Result<harness_core::lemonade::LemonadeSettings, String> {
    harness_core::lemonade::save_settings(settings).await
}

#[tauri::command]
pub(crate) async fn start_lemonade() -> Result<(), String> {
    harness_core::lemonade::start().await.map(|_| ())
}

#[tauri::command]
pub(crate) async fn stop_lemonade() -> Result<(), String> {
    harness_core::lemonade::stop().await
}

#[tauri::command]
pub(crate) async fn list_lemonade_models() -> Result<harness_core::lemonade::ModelList, String> {
    harness_core::lemonade::list_models().await
}

/// Starts `lemond` if needed and reads the current registry.
#[tauri::command]
pub(crate) async fn refresh_lemonade_models() -> Result<harness_core::lemonade::ModelList, String>
{
    harness_core::lemonade::list_models_started().await
}

#[tauri::command]
pub(crate) fn get_lemonade_progress() -> Option<harness_core::lemonade::PullProgress> {
    harness_core::lemonade::pull_progress()
}

#[tauri::command]
pub(crate) async fn pull_lemonade_model(model: String) -> Result<(), String> {
    harness_core::lemonade::pull_model(&model).await
}

#[tauri::command]
pub(crate) async fn delete_lemonade_model(model: String) -> Result<(), String> {
    harness_core::lemonade::delete_model(&model).await
}

#[tauri::command]
pub(crate) async fn install_lemonade_backend(recipe: String, backend: String) -> Result<(), String> {
    harness_core::lemonade::install_backend(&recipe, &backend).await
}

#[tauri::command]
pub(crate) fn get_lemonade_logs(lines: Option<usize>) -> Vec<String> {
    harness_core::lemonade::log_tail(lines.unwrap_or(100).min(200))
}
