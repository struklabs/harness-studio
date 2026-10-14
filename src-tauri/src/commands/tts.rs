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
    /// Text-to-speech models of an engine (mlx-audio) in the library: the gateway serves them
    /// by their definition's alias, which is made here when missing.
    engine_models: Vec<EngineTtsModel>,
}

#[derive(serde::Serialize)]
pub(crate) struct EngineTtsModel {
    /// The folder's path (the library id).
    id: String,
    /// Alias of its definition: what a request's `model` names.
    name: String,
    family: &'static str,
    size_bytes: u64,
    /// Named voices (empty: the model's one default voice).
    voices: Vec<String>,
}

/// The library's mlx-audio folders, each with a definition (made once, like the library's
/// default definitions).
fn engine_tts_models() -> Vec<EngineTtsModel> {
    use harness_core::domain::ModelKind;
    let folders: Vec<_> = harness_core::models::ModelManager
        .scan_models()
        .into_iter()
        .filter(|m| m.is_ready && harness_core::engine::mlx::is_tts_dir(&m.path))
        .map(|mut m| {
            m.kind = ModelKind::Pipeline;
            m
        })
        .collect();
    if folders.is_empty() {
        return Vec::new();
    }
    let _ = harness_core::library_definitions::ensure(&folders, &[ModelKind::Pipeline]);
    let defs = harness_core::domain::store::Domain::global()
        .definitions()
        .unwrap_or_default();
    folders
        .into_iter()
        .filter_map(|m| {
            let def = defs
                .iter()
                .find(|d| d.base.local_path() == Some(m.path.as_path()))?;
            Some(EngineTtsModel {
                id: m.id.clone(),
                name: def.alias.clone(),
                family: harness_core::engine::MLX_AUDIO,
                size_bytes: m.size_bytes,
                voices: harness_core::engine::mlx::tts_voices(&m.path),
            })
        })
        .collect()
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
            engine_models: engine_tts_models(),
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
