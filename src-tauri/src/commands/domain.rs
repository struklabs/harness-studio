// Domain model commands: model definitions, inference presets, effective config.
// The pre-v2 commands (profiles, model presets) keep their shapes in servers.rs / models.rs.

use harness_core::domain::launch::LaunchDefaults;
use harness_core::domain::store::{DataFileStatus, Domain};
use harness_core::domain::{InferencePreset, ModelDefinition};

#[tauri::command]
pub(crate) async fn list_model_definitions() -> Result<Vec<ModelDefinition>, String> {
    blocking(|| Domain::global().definitions()).await?
}

#[tauri::command]
pub(crate) async fn get_model_definition(id: String) -> Result<ModelDefinition, String> {
    blocking(move || Domain::global().definition(&id)).await?
}

/// Creates (empty `id`) or replaces a definition; returns it as stored.
#[tauri::command]
pub(crate) async fn save_model_definition(
    definition: ModelDefinition,
) -> Result<ModelDefinition, String> {
    blocking(move || Domain::global().save_definition(definition)).await?
}

#[tauri::command]
pub(crate) async fn delete_model_definition(id: String) -> Result<(), String> {
    blocking(move || Domain::global().delete_definition(&id)).await?
}

/// Bytes of the companion files (VAE, text encoders) a definition loads with its model; the
/// studios add them to the model file for their memory estimate.
#[tauri::command]
pub(crate) async fn definition_companion_bytes(id: String) -> Result<u64, String> {
    blocking(move || {
        let def = Domain::global().definition(&id)?;
        Ok(harness_core::engine::sd_component_files(&def.load)
            .iter()
            .filter_map(|(_, p)| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum())
    })
    .await?
}

#[tauri::command]
pub(crate) async fn duplicate_model_definition(
    id: String,
    alias: Option<String>,
) -> Result<ModelDefinition, String> {
    blocking(move || Domain::global().duplicate_definition(&id, alias)).await?
}

/// Built-in presets (read-only) first, then the user's.
#[tauri::command]
pub(crate) async fn list_inference_presets() -> Result<Vec<InferencePreset>, String> {
    blocking(|| Domain::global().inference_presets()).await?
}

#[tauri::command]
pub(crate) async fn get_inference_preset(id: String) -> Result<InferencePreset, String> {
    blocking(move || Domain::global().inference_preset(&id)).await?
}

#[tauri::command]
pub(crate) async fn save_inference_preset(
    preset: InferencePreset,
) -> Result<InferencePreset, String> {
    blocking(move || Domain::global().save_inference_preset(preset)).await?
}

#[tauri::command]
pub(crate) async fn delete_inference_preset(id: String) -> Result<(), String> {
    blocking(move || Domain::global().delete_inference_preset(&id)).await?
}

/// Effective launch config of a server profile, or of a definition served with defaults
/// (defaults <- definition.load <- profile.serve_overrides <- `overrides`).
/// The API key itself is never part of the result.
#[tauri::command]
pub(crate) async fn resolve_effective_config(
    profile_id: Option<String>,
    definition_id: Option<String>,
    overrides: Option<params::ParamValues>,
) -> Result<LaunchConfig, String> {
    blocking(move || {
        let domain = Domain::global();
        let defaults = LaunchDefaults::load();
        let overrides = overrides.unwrap_or_default();
        let mut config = match (profile_id, definition_id) {
            (Some(p), _) => domain.resolve_profile(&defaults, &p, &overrides),
            (None, Some(d)) => domain.resolve_definition(&defaults, &d, &overrides),
            (None, None) => Err(String::from(harness_core::error_key::err(
                "domain.profile_or_definition_required",
            ))),
        }?;
        config.api_key = None;
        Ok(config)
    })
    .await?
}

// --- definitions from the Dashboard form, run states, import/export ---

/// Profiles, model files and the Dashboard session that use a definition.
#[tauri::command]
pub(crate) async fn get_definition_refs(
    id: String,
) -> Result<harness_core::domain::store::DefinitionRefs, String> {
    blocking(move || Domain::global().definition_refs(&id)).await?
}

/// Dashboard form of a definition (its load settings + the file's Dashboard serve settings).
#[tauri::command]
pub(crate) async fn get_definition_form(definition_id: String) -> Result<LaunchConfig, String> {
    blocking(move || {
        let mut c = Domain::global().definition_form(&LaunchDefaults::load(), &definition_id)?;
        c.api_key = None;
        Ok(c)
    })
    .await?
}

/// The definition the Dashboard shows for a model file (None = the file has none).
#[tauri::command]
pub(crate) async fn get_dashboard_definition(model_id: String) -> Result<Option<String>, String> {
    blocking(move || Domain::global().dashboard_definition(&model_id)).await?
}

/// "Varsayılan olarak kaydet" (`definition_id`) / "Yeni tanım olarak kaydet" (`alias`).
#[tauri::command]
pub(crate) async fn save_definition_form(
    definition_id: Option<String>,
    alias: Option<String>,
    config: LaunchConfig,
) -> Result<ModelDefinition, String> {
    blocking(move || {
        ModelManager::save_definition_form(definition_id.as_deref(), alias.as_deref(), config)
    })
    .await?
}

/// "Load and Save to Definition": persists load settings into the definition's `load`
/// (an empty value = automatic again).
#[tauri::command]
pub(crate) async fn save_definition_load(
    definition_id: String,
    overrides: harness_core::params::ParamValues,
) -> Result<ModelDefinition, String> {
    blocking(move || Domain::global().save_definition_load(&definition_id, &overrides)).await?
}

/// Run state of every definition across the server instances.
#[tauri::command]
pub(crate) fn list_definition_states(
    registry: State<'_, Arc<ServerRegistry>>,
) -> Result<Vec<harness_core::domain::status::DefinitionState>, String> {
    crate::orchestrator::definition_states(&registry)
}

/// Loads a definition on an existing server, a profile or a new server; returns the
/// instance id.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri injects the states; the command arguments are few
pub(crate) async fn load_definition(
    app: tauri::AppHandle,
    registry: State<'_, Arc<ServerRegistry>>,
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    gate: State<'_, Arc<OperationGate>>,
    pending: State<'_, PendingExposure>,
    definition_id: String,
    target: crate::orchestrator::LoadTarget,
    overrides: Option<harness_core::params::ParamValues>,
) -> Result<String, String> {
    use crate::orchestrator::LoadPlan;
    // `overrides`: load settings for this load only (Playground "Load Settings"); the
    // definition is not changed.
    let plan = crate::orchestrator::plan_load_definition_with(
        &registry,
        &definition_id,
        target,
        &overrides.unwrap_or_default(),
    )
    .await?;
    let id = plan.server_id().to_string();
    // A start refused by the network rules can be replayed after "open anyway" (network.rs).
    let (host, retry) = match &plan {
        LoadPlan::Profile(pid) => {
            let host = servers::load_profiles()
                .into_iter()
                .find(|p| &p.id == pid)
                .map(|p| p.config.host)
                .unwrap_or_default();
            (host, PendingStart::Profile)
        }
        LoadPlan::Config { config, .. } => {
            (config.host.clone(), PendingStart::Config(config.clone()))
        }
    };
    let result = crate::orchestrator::run_load_plan(&registry, &runtime_mgr, &gate, plan)
        .await
        .map(|_| ());
    note_exposure_block(&app, &pending, &id, &host, retry, result)?;
    Ok(id)
}

fn models_dirs() -> Vec<PathBuf> {
    AppSettings::load().all_models_dirs()
}

async fn save_text_dialog(
    app: &tauri::AppHandle,
    default_name: String,
    filter: (&'static str, &'static [&'static str]),
    text: String,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let app = app.clone();
    let picked = blocking(move || {
        app.dialog()
            .file()
            .set_file_name(&default_name)
            .add_filter(filter.0, filter.1)
            .blocking_save_file()
    })
    .await?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let out = path.clone();
    blocking(move || std::fs::write(&out, text))
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

async fn open_text_dialog(
    app: &tauri::AppHandle,
    filter: (&'static str, &'static [&'static str]),
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    const MAX_BYTES: u64 = 4 * 1024 * 1024;
    let app = app.clone();
    let picked = blocking(move || {
        app.dialog()
            .file()
            .add_filter(filter.0, filter.1)
            .blocking_pick_file()
    })
    .await?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    blocking(move || -> Result<Option<String>, String> {
        let read_failed = |e: std::io::Error| {
            String::from(
                harness_core::error_key::err("fs.read_failed")
                    .with("path", path.display())
                    .with("error", e),
            )
        };
        let size = std::fs::metadata(&path).map_err(read_failed)?.len();
        if size > MAX_BYTES {
            return Err(harness_core::error_key::err("portable.file_too_large")
                .with("path", path.display())
                .num("max_mb", MAX_BYTES / (1024 * 1024))
                .into());
        }
        std::fs::read_to_string(&path)
            .map(Some)
            .map_err(read_failed)
    })
    .await?
}

/// Writes presets (`ids`, or every user preset) as `harness.preset.v1` JSON chosen in a save
/// dialog. Returns the path, `None` if cancelled.
#[tauri::command]
pub(crate) async fn export_inference_presets(
    app: tauri::AppHandle,
    ids: Option<Vec<String>>,
    file_name: Option<String>,
) -> Result<Option<String>, String> {
    let text =
        blocking(move || Domain::global().export_inference_presets(ids.as_deref())).await??;
    let base = file_name
        .as_deref()
        .and_then(paths::sanitize_file_name)
        .unwrap_or_else(|| "presets".to_string());
    save_text_dialog(
        &app,
        format!("{}.preset.json", base),
        ("JSON", &["json"]),
        text,
    )
    .await
}

/// Imports presets from a JSON file chosen in an open dialog (`None` if cancelled).
#[tauri::command]
pub(crate) async fn import_inference_presets_dialog(
    app: tauri::AppHandle,
) -> Result<Option<Vec<InferencePreset>>, String> {
    let Some(text) = open_text_dialog(&app, ("JSON", &["json"])).await? else {
        return Ok(None);
    };
    blocking(move || Domain::global().import_inference_presets(&text).map(Some)).await?
}

/// Writes a definition as `<alias>.harness.toml` (optionally with its default preset).
#[tauri::command]
pub(crate) async fn export_model_definition(
    app: tauri::AppHandle,
    id: String,
    embed_preset: bool,
) -> Result<Option<String>, String> {
    let (text, name) = blocking(move || {
        let domain = Domain::global();
        let def = domain.definition(&id)?;
        let text = domain.export_definition_toml(&id, embed_preset, &models_dirs())?;
        Ok::<_, String>((text, harness_core::domain::portable::file_name(&def.alias)))
    })
    .await??;
    save_text_dialog(&app, name, ("Harness TOML", &["toml"]), text).await
}

/// Reads a `*.harness.toml` chosen in an open dialog; nothing is saved (`None` if cancelled).
#[tauri::command]
pub(crate) async fn preview_model_definition_import(
    app: tauri::AppHandle,
) -> Result<Option<harness_core::domain::portable::DefinitionImport>, String> {
    let Some(text) = open_text_dialog(&app, ("Harness TOML", &["toml"])).await? else {
        return Ok(None);
    };
    blocking(move || {
        Domain::global()
            .preview_definition_import(&text, &models_dirs())
            .map(Some)
    })
    .await?
}

/// Saves a previewed import under `alias`.
#[tauri::command]
pub(crate) async fn import_model_definition(
    import: harness_core::domain::portable::DefinitionImport,
    alias: String,
) -> Result<ModelDefinition, String> {
    blocking(move || Domain::global().import_definition(import, &alias)).await?
}

/// Schema version of every data file (a newer file is read-only for this build).
#[tauri::command]
pub(crate) fn get_data_files_status() -> Vec<DataFileStatus> {
    harness_core::domain::store::data_files_status(&harness_core::paths::config_dir())
}

// --- chat template editor ---

/// Custom chat templates saved by the editor (`<config>/chat-templates/*.jinja`).
#[tauri::command]
pub(crate) async fn list_chat_templates() -> Result<Vec<harness_core::chat_template::SavedTemplate>, String>
{
    blocking(|| {
        harness_core::chat_template::list_templates(&Domain::global().chat_templates_dir())
    })
    .await
}

/// Text of a template file.
#[tauri::command]
pub(crate) async fn read_chat_template(path: String) -> Result<String, String> {
    blocking(move || harness_core::chat_template::read_template(std::path::Path::new(&path)))
        .await?
}

/// Saves `text` as the template `name` (replacing a file of that name); returns its path.
#[tauri::command]
pub(crate) async fn save_chat_template(
    name: String,
    text: String,
) -> Result<harness_core::chat_template::SavedTemplate, String> {
    blocking(move || {
        harness_core::chat_template::save_template(
            &Domain::global().chat_templates_dir(),
            &name,
            &text,
        )
    })
    .await?
}

/// The chat template embedded in a GGUF (`tokenizer.chat_template`), `None` if it has none.
#[tauri::command]
pub(crate) async fn read_model_chat_template(path: String) -> Result<Option<String>, String> {
    blocking(move || harness_core::gguf::read_gguf_chat_template(std::path::Path::new(&path)))
        .await?
}

/// Opens a file dialog for a LoRA adapter (`.safetensors`) of the Image tab; `None` when cancelled.
#[tauri::command]
pub(crate) async fn pick_lora_file(
    app: tauri::AppHandle,
    label: String,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    blocking(move || {
        app.dialog()
            .file()
            .add_filter(label, &["safetensors"])
            .blocking_pick_file()
            .and_then(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
    })
    .await
}

/// Opens a file dialog for a Jinja chat template; `None` when cancelled.
#[tauri::command]
pub(crate) async fn pick_chat_template_file(
    app: tauri::AppHandle,
    label: String,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    blocking(move || {
        app.dialog()
            .file()
            .add_filter(label, &["jinja", "j2", "tpl", "txt"])
            .blocking_pick_file()
            .and_then(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
    })
    .await
}
