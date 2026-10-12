// Text to speech (sherpa-onnx): the voice models on disk and the recommended downloads. The
// audio itself is made by the gateway (`/v1/audio/speech`), which the Playground calls like it
// calls `/v1/audio/transcriptions`; nothing here synthesizes.

/// What the Speech tab needs to draw its TTS section.
#[derive(serde::Serialize)]
pub(crate) struct TtsOverview {
    /// Installed voice models (folders under the model directories).
    models: Vec<harness_core::tts::TtsModel>,
    /// Recommended downloads, each with whether it is installed.
    catalog: Vec<TtsCatalogRow>,
    /// A sherpa-onnx runtime is registered.
    runtime_installed: bool,
}

#[derive(serde::Serialize)]
pub(crate) struct TtsCatalogRow {
    #[serde(flatten)]
    entry: harness_core::tts::catalog::CatalogEntry,
    installed: bool,
}

#[tauri::command]
pub(crate) async fn get_tts_overview() -> Result<TtsOverview, String> {
    blocking(|| {
        let models = harness_core::tts::installed_models();
        let catalog = harness_core::tts::catalog::catalog()
            .into_iter()
            .map(|entry| TtsCatalogRow {
                installed: models.iter().any(|m| m.name == entry.id),
                entry,
            })
            .collect();
        TtsOverview {
            models,
            catalog,
            runtime_installed: harness_core::tts::run::resolve_binary(None).is_ok(),
        }
    })
    .await
}

/// Downloads a recommended voice model (checked against its SHA-256); progress is read with
/// `get_tts_install_progress`.
#[tauri::command]
pub(crate) async fn install_tts_model(id: String) -> Result<harness_core::tts::TtsModel, String> {
    harness_core::tts::catalog::install(&id).await
}

#[tauri::command]
pub(crate) fn get_tts_install_progress() -> Option<RuntimeDownloadProgress> {
    harness_core::tts::catalog::install_progress()
}
