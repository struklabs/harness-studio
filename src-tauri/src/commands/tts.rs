// Text to speech (sherpa-onnx and resident audio.cpp): the voice models on disk and the recommended downloads. The
// audio itself is made by the gateway (`/v1/audio/speech`), which the Playground calls like it
// calls `/v1/audio/transcriptions`; nothing here synthesizes.

/// What the Speech tab needs to draw its TTS section.
#[derive(serde::Serialize)]
pub(crate) struct TtsOverview {
    /// Installed voice models (folders under the model directories).
    models: Vec<TtsPickerModel>,
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
    /// It speaks at a chosen speed (Qwen3-TTS, Spark-TTS); the others at one.
    supports_speed: bool,
}

/// The library's ready folders `keep` accepts, each with the alias of its definition (made once,
/// like the library's default definitions, as a pipeline model).
pub(crate) fn engine_folders(
    keep: fn(&std::path::Path) -> bool,
) -> Vec<(harness_core::models::LocalModel, String)> {
    use harness_core::domain::ModelKind;
    let folders: Vec<_> = harness_core::models::ModelManager
        .scan_models()
        .into_iter()
        .filter(|m| m.is_ready && keep(&m.path))
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
            let alias = defs
                .iter()
                .find(|d| d.base.local_path() == Some(m.path.as_path()))?
                .alias
                .clone();
            Some((m, alias))
        })
        .collect()
}

/// The library's mlx-audio text-to-speech folders.
fn engine_tts_models() -> Vec<EngineTtsModel> {
    engine_folders(harness_core::engine::mlx::is_tts_dir)
        .into_iter()
        .map(|(m, name)| EngineTtsModel {
            id: m.id.clone(),
            name,
            family: harness_core::engine::MLX_AUDIO,
            size_bytes: m.size_bytes,
            voices: harness_core::engine::mlx::tts_voices(&m.path),
            supports_speed: harness_core::engine::mlx::tts_supports_speed(&m.path),
        })
        .collect()
}

#[derive(serde::Serialize)]
pub(crate) struct TtsCatalogRow {
    #[serde(flatten)]
    entry: harness_core::tts::catalog::CatalogEntry,
    installed: bool,
}

/// Fields shared by native TTS models in the Playground picker.
#[derive(serde::Serialize)]
pub(crate) struct TtsPickerModel {
    id: String,
    name: String,
    family: String,
    engine: String,
    supports_speed: bool,
    runtime_installed: bool,
    languages: Vec<String>,
    quantization: String,
    sample_rate: Option<u32>,
    size_bytes: u64,
    voices: Vec<harness_core::tts::Voice>,
}

#[tauri::command]
pub(crate) async fn get_tts_overview() -> Result<TtsOverview, String> {
    blocking(|| -> Result<TtsOverview, String> {
        use harness_core::engine::{audio_cpp, AUDIO_CPP, SHERPA_ONNX};
        let sherpa_ready = harness_core::tts::run::resolve_binary(None).is_ok();
        let mut models: Vec<TtsPickerModel> = harness_core::tts::installed_models()
            .into_iter()
            .map(|m| TtsPickerModel {
                id: m.id,
                name: m.name,
                family: m.family.as_str().into(),
                engine: SHERPA_ONNX.into(),
                supports_speed: true,
                runtime_installed: sherpa_ready,
                languages: m.languages,
                quantization: m.quantization,
                sample_rate: m.sample_rate,
                size_bytes: m.size_bytes,
                voices: m.voices,
            })
            .collect();
        let library = harness_core::models::ModelManager.scan_models();
        harness_core::library_definitions::ensure(&library, &[harness_core::domain::ModelKind::Tts])?;
        let audio_ready = harness_core::runtimes::resolve_family_binary_in(
            &harness_core::runtimes::RuntimeRegistry::global(), AUDIO_CPP, None,
        ).is_ok();
        for def in harness_core::domain::store::Domain::global().definitions()? {
            let Some(path) = def.base.local_path() else { continue };
            if let Some(family) = harness_core::engine::crispasr_tts::backend(path) {
                let voices = harness_core::engine::crispasr_tts::voices(path).into_iter().enumerate()
                    .map(|(sid, id)| harness_core::tts::Voice { id, sid: sid as u32, language: "en".into(), gender: String::new() }).collect();
                models.push(TtsPickerModel {
                    id: path.to_string_lossy().into_owned(), name: def.alias.clone(), family: family.into(),
                    engine: harness_core::engine::CRISPASR.into(), supports_speed: family == "kokoro",
                    runtime_installed: harness_core::runtimes::resolve_family_binary_in(
                        &harness_core::runtimes::RuntimeRegistry::global(), harness_core::engine::CRISPASR, None).is_ok(),
                    languages: vec!["en".into()], quantization: harness_core::gguf::read_gguf_metadata(path).map(|m| m.quantization).unwrap_or_default(),
                    sample_rate: Some(24000), size_bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0), voices,
                });
                continue;
            }
            let Some(meta) = audio_cpp::metadata(path) else { continue };
            let Some(family) = meta.audiocpp_family.filter(|f| audio_cpp::task(f) == Some("tts")) else { continue };
            let voices = audio_cpp::voices(path).into_iter().enumerate().map(|(sid, id)| harness_core::tts::Voice {
                id, sid: sid as u32, language: String::new(), gender: String::new(),
            }).collect();
            models.push(TtsPickerModel {
                id: path.to_string_lossy().into_owned(),
                name: def.alias.clone(),
                supports_speed: audio_cpp::supports_speed(&family),
                family,
                engine: AUDIO_CPP.into(),
                runtime_installed: audio_ready,
                languages: audio_cpp::language(path).into_iter().collect(),
                quantization: meta.quantization,
                sample_rate: None,
                size_bytes: audio_cpp::weights_bytes(path),
                voices,
            });
        }
        let catalog = harness_core::tts::catalog::catalog()
            .into_iter()
            .map(|entry| TtsCatalogRow {
                installed: models.iter().any(|m| m.name == entry.id),
                entry,
            })
            .collect();
        Ok(TtsOverview {
            models,
            catalog,
            runtime_installed: sherpa_ready,
            engine_models: engine_tts_models(),
        })
    })
    .await?
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

#[derive(serde::Serialize)]
pub(crate) struct AudioProcessModel {
    name: String,
    family: String,
    options: Vec<harness_core::engine::audio_process::OptionSpec>,
}

#[derive(serde::Serialize)]
pub(crate) struct AudioProcessOverview {
    models: Vec<AudioProcessModel>,
    runtime_installed: bool,
}

#[tauri::command]
pub(crate) async fn get_audio_process_overview() -> Result<AudioProcessOverview, String> {
    blocking(|| -> Result<AudioProcessOverview, String> {
        use harness_core::engine::{audio_cpp, audio_process, AUDIO_CPP};
        let library = harness_core::models::ModelManager.scan_models();
        harness_core::library_definitions::ensure(&library, &[harness_core::domain::ModelKind::Pipeline])?;
        let models = harness_core::domain::store::Domain::global().definitions()?.into_iter().filter_map(|def| {
            let family = audio_cpp::metadata(def.base.local_path()?)?.audiocpp_family?;
            audio_cpp::processing_task(&family)?;
            Some(AudioProcessModel { name: def.alias, options: audio_process::options(&family), family })
        }).collect();
        Ok(AudioProcessOverview { models, runtime_installed: harness_core::runtimes::resolve_family_binary_in(&harness_core::runtimes::RuntimeRegistry::global(), AUDIO_CPP, None).is_ok() })
    }).await?
}

/// Uses the same native save dialog and filename sanitation as generated images.
#[tauri::command]
pub(crate) async fn save_processed_audio(app: tauri::AppHandle, data: String, file_name: Option<String>) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let bytes = harness_core::engine::audio_process::decode_output(&data)
        .map_err(|_| String::from(harness_core::error_key::err("audio_process.invalid_audio")))?;
    let base = file_name.as_deref().and_then(paths::sanitize_file_name).unwrap_or_else(|| "audio".into());
    let default_name = format!("{}.wav", base.trim_end_matches(".wav"));
    let picked = blocking(move || app.dialog().file().set_file_name(&default_name).add_filter("WAV", &["wav"]).blocking_save_file()).await?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else { return Ok(None); };
    let out = path.clone();
    blocking(move || std::fs::write(&out, bytes)).await?.map_err(|e| String::from(harness_core::error_key::err("fs.write_failed").with("path", path.display()).with("error", e)))?;
    Ok(Some(path.to_string_lossy().into_owned()))
}
