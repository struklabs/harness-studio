// Ollama Modelfile import. The dialog only reads a file; saving goes through
// `import_model_definition` with the previewed `import`.

/// An installed Ollama model offered for "import from Ollama".
#[derive(serde::Serialize)]
pub(crate) struct OllamaInstalledModel {
    name: String,
    size_bytes: u64,
    has_projector: bool,
    quantization: Option<String>,
}

/// Installed Ollama models (GGUF only), read from the Ollama models directory without
/// changing it. `dir` is `None` when no Ollama directory was found.
#[derive(serde::Serialize)]
pub(crate) struct OllamaInstalledModels {
    dir: Option<std::path::PathBuf>,
    models: Vec<OllamaInstalledModel>,
}

#[tauri::command]
pub(crate) async fn list_ollama_models() -> Result<OllamaInstalledModels, String> {
    blocking(|| {
        let (dir, list) = harness_core::importer::scan_ollama_installed();
        OllamaInstalledModels {
            dir,
            models: list
                .into_iter()
                .map(|c| OllamaInstalledModel {
                    name: c.name,
                    size_bytes: c.size_bytes,
                    has_projector: c.mmproj_path.is_some(),
                    quantization: c.quantization,
                })
                .collect(),
        }
    })
    .await
}

/// What importing an installed Ollama model would create (`FROM <name>`, as the CLI's
/// `harness import modelfile --ollama <name>`); nothing is saved.
#[tauri::command]
pub(crate) async fn preview_ollama_import(
    name: String,
) -> Result<harness_core::modelfile::ModelfileImport, String> {
    blocking(move || {
        let name = name.trim().to_string();
        if name.is_empty() || name.chars().any(char::is_whitespace) {
            return Err(harness_core::error_key::err("modelfile.invalid_ollama_name")
                .with("name", &name)
                .into());
        }
        let taken = harness_core::domain::store::Domain::global()
            .definitions()?
            .into_iter()
            .map(|d| d.alias)
            .collect();
        harness_core::modelfile::import_text(
            &format!("FROM {name}\n"),
            &harness_core::modelfile::ImportOptions {
                taken_aliases: taken,
                ..Default::default()
            },
        )
    })
    .await?
}

/// Reads a Modelfile chosen in an open dialog and describes what importing it would create;
/// nothing is saved (`None` if cancelled). Relative paths in it start at its directory.
#[tauri::command]
pub(crate) async fn preview_modelfile_import(
    app: tauri::AppHandle,
) -> Result<Option<harness_core::modelfile::ModelfileImport>, String> {
    use tauri_plugin_dialog::DialogExt;
    const MAX_BYTES: u64 = 4 * 1024 * 1024;
    let picked = blocking(move || app.dialog().file().blocking_pick_file()).await?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    blocking(move || {
        use harness_core::error_key::err;
        let size = std::fs::metadata(&path)
            .map_err(|e| {
                String::from(
                    err("fs.read_failed")
                        .with("path", path.display())
                        .with("error", e),
                )
            })?
            .len();
        if size > MAX_BYTES {
            return Err(err("modelfile.file_too_large")
                .with("path", path.display())
                .num("max_mb", MAX_BYTES / (1024 * 1024))
                .into());
        }
        let taken = harness_core::domain::store::Domain::global()
            .definitions()?
            .into_iter()
            .map(|d| d.alias)
            .collect();
        harness_core::modelfile::import_file(
            &path,
            &harness_core::modelfile::ImportOptions {
                taken_aliases: taken,
                ..Default::default()
            },
        )
        .map(Some)
    })
    .await?
}
