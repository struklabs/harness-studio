/// Starts a background download; progress is polled via `get_download_progress`.
#[tauri::command]
pub(crate) fn download_hf_variant(
    download_mgr: State<'_, Arc<DownloadManager>>,
    repo_id: String,
    variant: HfModelVariant,
) -> Result<u64, String> {
    download_mgr.start_variant_download(repo_id, variant)
}

/// Queues the companion files a model needs and does not have yet: the VAE / text encoders of
/// a known image model (FLUX, SD3.5) by its main file's name `file`, or the codec of an
/// mlx-audio text-to-speech repo `repo` (OuteTTS, Orpheus, Dia). Returns the download job ids.
#[tauri::command]
pub(crate) async fn download_image_companions(
    download_mgr: State<'_, Arc<DownloadManager>>,
    file: String,
    repo: Option<String>,
) -> Result<Vec<u64>, String> {
    let token = harness_core::hf_auth::current_token().await;
    download_mgr
        .pull_companions(&file, repo.as_deref(), token.as_deref())
        .await
}

/// What a model needs besides itself (the curated bundle of an image model's main file, or an
/// mlx-audio repo's codec: companions, where they come from, which are on disk) or `None` when
/// Harness has no recipe for it.
#[tauri::command]
pub(crate) async fn get_image_bundle_plan(
    file: String,
    repo: Option<String>,
) -> Result<Option<harness_core::image_bundles::BundlePlan>, String> {
    blocking(move || {
        let libraries = AppSettings::load().all_models_dirs();
        harness_core::downloader::companion_plan(&file, repo.as_deref(), &libraries)
    })
    .await
}

/// Makes a model definition for every ready image model that has none (companions included);
/// returns the aliases it created.
#[tauri::command]
pub(crate) async fn sync_image_definitions() -> Result<Vec<String>, String> {
    blocking(harness_core::image_bundles::sync_library_definitions).await?
}

/// Makes the default definition of every ready library file of `kinds` (image, speech,
/// embedding, reranker) that was found by a scan and has none, so the Playground pickers and
/// the gateway list it. Returns the aliases it created.
#[tauri::command]
pub(crate) async fn ensure_library_definitions(
    kinds: Vec<harness_core::domain::ModelKind>,
) -> Result<Vec<String>, String> {
    blocking(move || {
        let models = harness_core::models::ModelManager.scan_models();
        harness_core::library_definitions::ensure(&models, &kinds)
    })
    .await?
}

/// Saves a generated image (base64 of a PNG, JPEG or WebP, as the gateway answered it) to a file
/// the user picks. Returns the path, or `None` when the dialog was cancelled.
#[tauri::command]
pub(crate) async fn save_generated_image(
    app: tauri::AppHandle,
    data: String,
    file_name: Option<String>,
) -> Result<Option<String>, String> {
    use base64::Engine as _;
    use tauri_plugin_dialog::DialogExt;
    const MAX_BYTES: usize = 64 * 1024 * 1024;
    let invalid = || String::from(harness_core::error_key::err("export.invalid_image"));
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|_| invalid())?;
    let ext = match bytes.as_slice() {
        b if b.len() > MAX_BYTES => return Err(invalid()),
        [0x89, b'P', b'N', b'G', ..] => "png",
        [0xff, 0xd8, 0xff, ..] => "jpg",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "webp",
        _ => return Err(invalid()),
    };
    let base = file_name
        .as_deref()
        .and_then(paths::sanitize_file_name)
        .unwrap_or_else(|| "image".to_string());
    let default_name = format!("{}.{}", base.trim_end_matches(&format!(".{ext}")), ext);
    let picked = blocking(move || {
        app.dialog()
            .file()
            .set_file_name(&default_name)
            .add_filter("Image", &[ext])
            .blocking_save_file()
    })
    .await?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let out = path.clone();
    blocking(move || std::fs::write(&out, bytes))
        .await?
        .map_err(|e| {
            String::from(
                harness_core::error_key::err("fs.write_failed")
                    .with("path", path.display())
                    .with("error", e),
            )
        })?;
    Ok(Some(path.to_string_lossy().to_string()))
}

/// Starts a background download from a custom URL.
#[tauri::command]
pub(crate) async fn download_custom_url(
    download_mgr: State<'_, Arc<DownloadManager>>,
    url: String,
    custom_filename: Option<String>,
) -> Result<u64, String> {
    let token = harness_core::hf_auth::current_token().await;
    download_mgr
        .start_custom_url_download(url, custom_filename, token)
        .await
}

#[tauri::command]
pub(crate) async fn check_model_artifact_update(
    repo_id: String,
    filename: String,
) -> Result<ModelArtifactUpdate, String> {
    let token = harness_core::hf_auth::current_token().await;
    DownloadManager::check_model_artifact_update(&repo_id, &filename, token.as_deref()).await
}

#[tauri::command]
pub(crate) fn get_download_progress(
    download_mgr: State<'_, Arc<DownloadManager>>,
) -> Option<DownloadProgress> {
    download_mgr.get_progress()
}

#[tauri::command]
pub(crate) fn list_downloads(download_mgr: State<'_, Arc<DownloadManager>>) -> Vec<DownloadProgress> {
    download_mgr.list()
}

#[tauri::command]
pub(crate) fn resume_download(
    download_mgr: State<'_, Arc<DownloadManager>>,
    job_id: u64,
) -> Result<(), String> {
    download_mgr.resume_download(job_id)
}

#[tauri::command]
pub(crate) fn remove_download(
    download_mgr: State<'_, Arc<DownloadManager>>,
    job_id: u64,
    delete_partial: Option<bool>,
) -> Result<(), String> {
    download_mgr.remove_download(job_id, delete_partial.unwrap_or(false))
}

#[tauri::command]
pub(crate) fn clear_finished_downloads(download_mgr: State<'_, Arc<DownloadManager>>) {
    download_mgr.clear_finished();
}

#[tauri::command]
pub(crate) fn prioritize_download(download_mgr: State<'_, Arc<DownloadManager>>, job_id: u64) {
    download_mgr.prioritize(job_id);
}

#[tauri::command]
pub(crate) fn cancel_active_download(download_mgr: State<'_, Arc<DownloadManager>>, job_id: Option<u64>) {
    download_mgr.cancel_download(job_id);
}


