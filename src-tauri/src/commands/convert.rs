// Safetensors to GGUF: Model Hub "Convert to GGUF" dialog, the conversion jobs of
// the download queue and the Settings card of the conversion environment. The work itself is
// `harness_core::convert`; job changes reach the window as `converts-changed` (push.rs).

/// What converting a Hub repo would take (the dialog's content). Fetches the converter's
/// source on the first call after a runtime update (~40 MB).
#[tauri::command]
pub(crate) async fn convert_plan(repo_id: String) -> Result<harness_core::convert::ConvertPlan, String> {
    harness_core::convert::plan(&repo_id, None).await
}

/// Queues a conversion. `quant` is a type name (`Q4_K_M`, `F16`...), empty = the default.
#[tauri::command]
pub(crate) async fn convert_start(
    repo_id: String,
    quant: Option<String>,
    keep_original: bool,
) -> Result<harness_core::convert::ConvertJob, String> {
    harness_core::convert::start(&repo_id, quant.as_deref().filter(|q| !q.is_empty()), keep_original)
        .await
}

#[tauri::command]
pub(crate) fn list_convert_jobs() -> Vec<harness_core::convert::ConvertJob> {
    harness_core::convert::jobs()
}

#[tauri::command]
pub(crate) fn cancel_convert_job(job_id: u64) -> Result<(), String> {
    harness_core::convert::cancel(job_id)
}

/// Removes a finished job and deletes the partial files it kept.
#[tauri::command]
pub(crate) async fn remove_convert_job(job_id: u64) -> Result<(), String> {
    blocking(move || harness_core::convert::remove(job_id)).await?
}

#[tauri::command]
pub(crate) async fn clear_finished_conversions() -> Result<(), String> {
    blocking(harness_core::convert::clear_finished).await
}

#[tauri::command]
pub(crate) async fn get_convert_env() -> Result<harness_core::convert::EnvStatus, String> {
    blocking(harness_core::convert::env::status).await
}

/// Builds the conversion environment (Settings); progress is read with
/// `get_convert_env` (`progress`).
#[tauri::command]
pub(crate) async fn install_convert_env() -> Result<harness_core::convert::EnvStatus, String> {
    harness_core::convert::install_env().await
}

#[tauri::command]
pub(crate) fn cancel_convert_env_install() {
    harness_core::convert::cancel_env_install();
}

#[tauri::command]
pub(crate) async fn remove_convert_env() -> Result<(), String> {
    blocking(harness_core::convert::remove_env).await?
}
