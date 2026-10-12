#[tauri::command]
pub(crate) fn list_server_log_files(
    server_id: Option<String>,
) -> Result<Vec<server_logs::LogFileInfo>, String> {
    Ok(server_logs::list(&ServerRegistry::resolve_id(server_id)?))
}

#[tauri::command]
pub(crate) async fn read_server_log_file(server_id: Option<String>, name: String) -> Result<String, String> {
    let id = ServerRegistry::resolve_id(server_id)?;
    blocking(move || server_logs::read(&id, &name)).await?
}

/// Searches the live log (`name` = None) or a saved run log.
#[tauri::command]
pub(crate) async fn search_server_logs(
    registry: State<'_, Arc<ServerRegistry>>,
    server_id: Option<String>,
    name: Option<String>,
    query: String,
    regex: Option<bool>,
    case_sensitive: Option<bool>,
    max: Option<usize>,
) -> Result<Vec<server_logs::LogMatch>, String> {
    let id = ServerRegistry::resolve_id(server_id)?;
    let text = match name {
        Some(name) => blocking(move || server_logs::read(&id, &name)).await??,
        None => server(&registry, Some(id))?.get_logs(None).join("\n"),
    };
    server_logs::search(
        text.lines(),
        &query,
        regex.unwrap_or(false),
        case_sensitive.unwrap_or(false),
        max.unwrap_or(2000).min(10_000),
    )
}

/// Saves the live log or a run log to a file chosen by the user.
/// `sanitize` redacts user paths, tokens and e-mails (same rules as diagnostics).
#[tauri::command]
pub(crate) async fn export_server_logs(
    app: tauri::AppHandle,
    registry: State<'_, Arc<ServerRegistry>>,
    server_id: Option<String>,
    name: Option<String>,
    sanitize: Option<bool>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let id = ServerRegistry::resolve_id(server_id)?;
    let (text, default_name) = match &name {
        Some(n) => {
            let (id2, n2) = (id.clone(), n.clone());
            (
                blocking(move || server_logs::read(&id2, &n2)).await??,
                format!("harness-{}-{}", id, n),
            )
        }
        None => (
            server(&registry, Some(id.clone()))?
                .get_logs(None)
                .join("\n")
                + "\n",
            format!("harness-{}-live.log", id),
        ),
    };
    let text = if sanitize.unwrap_or(true) {
        diagnostics::sanitize_text(&text)
    } else {
        text
    };
    let picked = blocking(move || {
        app.dialog()
            .file()
            .set_file_name(&default_name)
            .add_filter("Log", &["log", "txt"])
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



