// Managed Python environments of the optional Python engines (Settings > Runtime > Installed Runtime >
// Other Runtimes): status, install with progress, cancel and remove. The work itself is
// `harness_core::pyenv::envs`.

/// Every environment this version knows, with the progress of a build running now.
#[tauri::command]
pub(crate) async fn list_python_envs() -> Result<Vec<harness_core::pyenv::envs::EnvStatus>, String> {
    blocking(harness_core::pyenv::envs::status_all).await
}

/// Builds the environment `name`; progress is read with `list_python_envs` (`progress`).
#[tauri::command]
pub(crate) async fn install_python_env(name: String) -> Result<harness_core::pyenv::envs::EnvStatus, String> {
    harness_core::pyenv::envs::install(&name).await
}

#[tauri::command]
pub(crate) fn cancel_python_env_install(name: String) {
    harness_core::pyenv::envs::cancel_install(&name);
}

#[tauri::command]
pub(crate) async fn remove_python_env(name: String) -> Result<(), String> {
    blocking(move || harness_core::pyenv::envs::remove(&name)).await?
}
