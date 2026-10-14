// Speech recognition with sherpa-onnx models (Parakeet, SenseVoice, Omnilingual...): the models
// on disk and the recommended downloads, plus the library's mlx-audio recognition folders. The
// text itself is made by the gateway (`/v1/audio/transcriptions`), which the Playground and the
// chat dictation call like they call it for a whisper model; nothing here recognises speech.

/// What the Speech tab needs to draw its sherpa-onnx recognition section.
#[derive(serde::Serialize)]
pub(crate) struct AsrOverview {
    /// Installed recognition models (folders under the model directories).
    models: Vec<harness_core::asr::AsrModel>,
    /// Recommended downloads, each with whether it is installed.
    catalog: Vec<AsrCatalogRow>,
    /// A sherpa-onnx runtime is registered.
    runtime_installed: bool,
    /// Recognition models of an engine (mlx-audio: Whisper, Parakeet) in the library: the
    /// gateway serves them by their definition's alias, which is made here when missing.
    engine_models: Vec<EngineAsrModel>,
}

#[derive(serde::Serialize)]
pub(crate) struct EngineAsrModel {
    /// The folder's path (the library id).
    id: String,
    /// Alias of its definition: what a request's `model` names.
    name: String,
    family: &'static str,
    size_bytes: u64,
    /// It translates to English too (Whisper); Parakeet only transcribes.
    translates: bool,
}

/// The library's mlx-audio speech recognition folders.
fn engine_asr_models() -> Vec<EngineAsrModel> {
    engine_folders(harness_core::engine::mlx::is_stt_dir)
        .into_iter()
        .map(|(m, name)| EngineAsrModel {
            id: m.id.clone(),
            name,
            family: harness_core::engine::MLX_AUDIO,
            size_bytes: m.size_bytes,
            translates: m.architecture.as_deref() == Some("whisper"),
        })
        .collect()
}

#[derive(serde::Serialize)]
pub(crate) struct AsrCatalogRow {
    #[serde(flatten)]
    entry: harness_core::asr::catalog::CatalogEntry,
    installed: bool,
}

#[tauri::command]
pub(crate) async fn get_asr_overview() -> Result<AsrOverview, String> {
    blocking(|| {
        let models = harness_core::asr::installed_models();
        let catalog = harness_core::asr::catalog::catalog()
            .into_iter()
            .map(|entry| AsrCatalogRow {
                installed: models.iter().any(|m| m.name == entry.id),
                entry,
            })
            .collect();
        AsrOverview {
            models,
            catalog,
            runtime_installed: harness_core::asr::run::resolve_binary(None).is_ok(),
            engine_models: engine_asr_models(),
        }
    })
    .await
}

/// Downloads a recommended recognition model (checked against its SHA-256); progress is read
/// with `get_asr_install_progress`.
#[tauri::command]
pub(crate) async fn install_asr_model(id: String) -> Result<harness_core::asr::AsrModel, String> {
    harness_core::asr::catalog::install(&id).await
}

#[tauri::command]
pub(crate) fn get_asr_install_progress() -> Option<RuntimeDownloadProgress> {
    harness_core::asr::catalog::install_progress()
}

/// CrispASR's helper models (language detection, punctuation): pinned files, with whether each
/// is installed. The worker never fetches them itself.
#[tauri::command]
pub(crate) async fn get_crispasr_helpers() -> Result<Vec<harness_core::engine::crispasr_helpers::HelperStatus>, String> {
    blocking(harness_core::engine::crispasr_helpers::status).await
}

/// Queues the download of one helper model; it shows in the download queue.
#[tauri::command]
pub(crate) fn download_crispasr_helper(
    download_mgr: State<'_, Arc<DownloadManager>>,
    id: String,
) -> Result<u64, String> {
    harness_core::engine::crispasr_helpers::start_download(&download_mgr, &id)
}

#[tauri::command]
pub(crate) async fn remove_crispasr_helper(id: String) -> Result<(), String> {
    blocking(move || harness_core::engine::crispasr_helpers::remove(&id)).await?
}
