// Speech recognition with sherpa-onnx models (Parakeet, SenseVoice, Omnilingual...): the models
// on disk and the recommended downloads. The text itself is made by the gateway
// (`/v1/audio/transcriptions`), which the Playground and the chat dictation call like they call
// it for a whisper model; nothing here recognises speech.

/// What the Speech tab needs to draw its sherpa-onnx recognition section.
#[derive(serde::Serialize)]
pub(crate) struct AsrOverview {
    /// Installed recognition models (folders under the model directories).
    models: Vec<harness_core::asr::AsrModel>,
    /// Recommended downloads, each with whether it is installed.
    catalog: Vec<AsrCatalogRow>,
    /// A sherpa-onnx runtime is registered.
    runtime_installed: bool,
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
