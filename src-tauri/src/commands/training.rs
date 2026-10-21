// Training jobs: the Training page (wizard, job list, loss chart, export). The work itself is
// `harness_core::training`; job changes reach the window as `training-changed` (push.rs).

use harness_core::training::{self, CreateRequest, ExportRequest};

#[tauri::command]
pub(crate) async fn training_check_dataset(
    path: String,
    validation_fraction: Option<f64>,
) -> Result<training::DatasetReport, String> {
    blocking(move || {
        training::dataset::inspect(std::path::Path::new(path.trim()), validation_fraction.unwrap_or(0.1))
    })
    .await
}

/// Config with defaults filled in, dataset report and estimate: the wizard's review step.
#[tauri::command]
pub(crate) async fn training_preview(
    request: CreateRequest,
) -> Result<(training::TrainingConfig, training::DatasetReport, training::Estimate), String> {
    blocking(move || training::preview(&request)).await?
}

#[tauri::command]
pub(crate) async fn training_compatibility() -> training::Compatibility {
    training::compatibility().await
}

#[tauri::command]
pub(crate) async fn training_env() -> Result<harness_core::pyenv::envs::EnvStatus, String> {
    blocking(training::env_status).await
}

#[tauri::command]
pub(crate) async fn training_install_env() -> Result<harness_core::pyenv::envs::EnvStatus, String> {
    training::install_env().await
}

#[tauri::command]
pub(crate) fn training_list() -> Vec<training::TrainingJob> {
    training::jobs()
}

#[tauri::command]
pub(crate) async fn training_create(request: CreateRequest) -> Result<training::TrainingJob, String> {
    blocking(move || training::create(&request)).await?
}

#[tauri::command]
pub(crate) fn training_start(job_id: u64) -> Result<training::TrainingJob, String> {
    training::start(job_id)
}

#[tauri::command]
pub(crate) fn training_resume(job_id: u64) -> Result<training::TrainingJob, String> {
    training::resume(job_id)
}

#[tauri::command]
pub(crate) fn training_cancel(job_id: u64) -> Result<(), String> {
    training::cancel(job_id)
}

#[tauri::command]
pub(crate) async fn training_remove(job_id: u64) -> Result<(), String> {
    blocking(move || training::remove(job_id)).await?
}

#[tauri::command]
pub(crate) async fn training_metrics(job_id: u64) -> Result<Vec<training::Metric>, String> {
    blocking(move || training::metrics(job_id)).await?
}

#[tauri::command]
pub(crate) async fn training_log(job_id: u64) -> Result<String, String> {
    blocking(move || training::log_tail(job_id)).await?
}

#[tauri::command]
pub(crate) fn training_export(job_id: u64, request: ExportRequest) -> Result<training::TrainingJob, String> {
    training::export(job_id, &request)
}

/// Opens a file dialog for a training dataset (`.jsonl` / `.json`); `None` when cancelled.
#[tauri::command]
pub(crate) async fn pick_training_dataset_file(
    app: tauri::AppHandle,
    label: String,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    blocking(move || {
        app.dialog()
            .file()
            .add_filter(label, &["jsonl", "json"])
            .blocking_pick_file()
            .and_then(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
    })
    .await
}
