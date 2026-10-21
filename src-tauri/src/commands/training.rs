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

/// Hardware-aware presets for a model and dataset.
#[tauri::command]
pub(crate) async fn training_presets(
    request: training::PresetRequest,
) -> Result<training::PresetSet, String> {
    training::presets::presets(&request).await
}

/// Whether a model name is free, and a suggested one.
#[tauri::command]
pub(crate) async fn training_check_name(
    name: Option<String>,
    model: Option<String>,
    dataset: Option<String>,
    exclude_job: Option<u64>,
) -> Result<training::NameCheck, String> {
    blocking(move || {
        training::check_name(
            name.as_deref(),
            model.as_deref(),
            dataset.as_deref(),
            exclude_job,
        )
    })
    .await
}

#[tauri::command]
pub(crate) async fn training_rename(job_id: u64, name: String) -> Result<training::TrainingJob, String> {
    blocking(move || training::rename(job_id, &name)).await?
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

// Async so the job and export tasks are spawned on the Tauri runtime: a sync command runs on
// the main thread, where `tokio::spawn` panics and aborts the app.
#[tauri::command]
pub(crate) async fn training_start(job_id: u64) -> Result<training::TrainingJob, String> {
    training::start(job_id)
}

#[tauri::command]
pub(crate) async fn training_resume(job_id: u64) -> Result<training::TrainingJob, String> {
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

// Async for the same reason as `training_start`.
#[tauri::command]
pub(crate) async fn training_export(job_id: u64, request: ExportRequest) -> Result<training::TrainingJob, String> {
    training::export(job_id, &request)
}

/// Opens a folder dialog for a training dataset folder (JSONL, JSON and CSV files inside).
#[tauri::command]
pub(crate) async fn pick_training_dataset_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_folder()
            .and_then(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
    })
    .await
}

/// Opens a file dialog for a training dataset (`.jsonl`, `.json`, `.csv`); `None` when cancelled.
#[tauri::command]
pub(crate) async fn pick_training_dataset_file(
    app: tauri::AppHandle,
    label: String,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    blocking(move || {
        app.dialog()
            .file()
            .add_filter(label, &["jsonl", "ndjson", "json", "csv"])
            .blocking_pick_file()
            .and_then(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
    })
    .await
}

/// Downloads a safetensors Hub repo (weights, config, tokenizer) into the library as a training
/// base model. Returns the download job id.
#[tauri::command]
pub(crate) async fn training_download_base(
    download_mgr: State<'_, Arc<harness_core::downloader::DownloadManager>>,
    repo_id: String,
) -> Result<u64, String> {
    let token = harness_core::hf_auth::current_token().await;
    download_mgr
        .pull_training_base(repo_id.trim(), token.as_deref())
        .await
}

/// Starts a failed or cancelled job without a checkpoint over from the beginning.
#[tauri::command]
pub(crate) async fn training_restart(job_id: u64) -> Result<training::TrainingJob, String> {
    training::restart(job_id)
}
