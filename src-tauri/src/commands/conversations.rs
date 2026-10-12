// Playground conversations (`state.db`); thin wrappers over `harness_core::state::StateDb`.

/// `state.db`, opened once at startup. An open error is kept so every command reports it.
pub(crate) struct ConversationStore(pub Result<Arc<StateDb>, String>);

async fn with_db<T: Send + 'static>(
    store: &ConversationStore,
    f: impl FnOnce(&StateDb) -> StateResult<T> + Send + 'static,
) -> Result<T, String> {
    let db = store.0.clone()?;
    blocking(move || f(&db).map_err(String::from)).await?
}

#[derive(serde::Serialize)]
pub(crate) struct ConversationDbStatus {
    path: String,
    read_only: bool,
    error: Option<String>,
}

#[tauri::command]
pub(crate) fn conversation_db_status(store: State<'_, ConversationStore>) -> ConversationDbStatus {
    match &store.0 {
        Ok(db) => ConversationDbStatus {
            path: db.path().to_string_lossy().to_string(),
            read_only: db.is_read_only(),
            error: None,
        },
        Err(e) => ConversationDbStatus {
            path: paths::state_db_file().to_string_lossy().to_string(),
            read_only: true,
            error: Some(e.clone()),
        },
    }
}

#[tauri::command]
pub(crate) async fn list_conversations(
    store: State<'_, ConversationStore>,
    query: Option<ListQuery>,
) -> Result<Vec<ConversationSummary>, String> {
    let q = query.unwrap_or_default();
    with_db(&store, move |db| db.list_conversations(&q)).await
}

#[tauri::command]
pub(crate) async fn create_conversation(
    store: State<'_, ConversationStore>,
    input: NewConversation,
) -> Result<Conversation, String> {
    with_db(&store, move |db| {
        let conv = db.create_conversation(&input)?;
        // The Playground shows the chat it just created: record it as last opened, so the next
        // start (or the window rebuilt after the tray released it) reopens this chat, not the
        // one before. Best effort: the chat exists either way.
        if let Err(e) = db.open_conversation(&conv.id) {
            eprintln!("[Harness Studio] New conversation not marked as opened: {}", String::from(e));
        }
        Ok(conv)
    })
    .await
}

/// Loads a conversation's current path and records it as last opened.
#[tauri::command]
pub(crate) async fn open_conversation(
    store: State<'_, ConversationStore>,
    id: String,
) -> Result<ConversationView, String> {
    with_db(&store, move |db| db.open_conversation(&id)).await
}

#[tauri::command]
pub(crate) async fn get_conversation(
    store: State<'_, ConversationStore>,
    id: String,
) -> Result<ConversationView, String> {
    with_db(&store, move |db| db.get_conversation(&id)).await
}

#[tauri::command]
pub(crate) async fn last_opened_conversation(
    store: State<'_, ConversationStore>,
) -> Result<Option<String>, String> {
    with_db(&store, |db| db.last_opened_conversation()).await
}

#[tauri::command]
pub(crate) async fn set_conversation_title(
    store: State<'_, ConversationStore>,
    id: String,
    title: String,
    source: String,
    only_if_source: Option<String>,
) -> Result<bool, String> {
    with_db(&store, move |db| {
        db.set_title(&id, &title, &source, only_if_source.as_deref())
    })
    .await
}

#[tauri::command]
pub(crate) async fn update_conversation(
    store: State<'_, ConversationStore>,
    id: String,
    patch: ConversationPatch,
) -> Result<Conversation, String> {
    with_db(&store, move |db| db.update_conversation(&id, &patch)).await
}

#[tauri::command]
pub(crate) async fn set_conversation_pinned(
    store: State<'_, ConversationStore>,
    id: String,
    pinned: bool,
) -> Result<(), String> {
    with_db(&store, move |db| db.set_pinned(&id, pinned)).await
}

#[tauri::command]
pub(crate) async fn set_conversation_favorite(
    store: State<'_, ConversationStore>,
    id: String,
    favorite: bool,
) -> Result<(), String> {
    with_db(&store, move |db| db.set_favorite(&id, favorite)).await
}

#[tauri::command]
pub(crate) async fn move_conversation(
    store: State<'_, ConversationStore>,
    id: String,
    folder_id: Option<String>,
) -> Result<(), String> {
    with_db(&store, move |db| db.move_to_folder(&id, folder_id.as_deref())).await
}

#[tauri::command]
pub(crate) async fn delete_conversation(
    store: State<'_, ConversationStore>,
    id: String,
) -> Result<(), String> {
    with_db(&store, move |db| db.delete_conversation(&id)).await
}

#[tauri::command]
pub(crate) async fn duplicate_conversation(
    store: State<'_, ConversationStore>,
    id: String,
    title: Option<String>,
) -> Result<Conversation, String> {
    with_db(&store, move |db| db.duplicate_conversation(&id, title.as_deref())).await
}

#[tauri::command]
pub(crate) async fn fork_conversation(
    store: State<'_, ConversationStore>,
    message_id: String,
    title: Option<String>,
) -> Result<Conversation, String> {
    with_db(&store, move |db| db.fork_conversation(&message_id, title.as_deref())).await
}

#[tauri::command]
pub(crate) async fn append_conversation_message(
    store: State<'_, ConversationStore>,
    input: AppendMessage,
) -> Result<PathMessage, String> {
    with_db(&store, move |db| db.append_message(input)).await
}

#[tauri::command]
pub(crate) async fn add_message_variant(
    store: State<'_, ConversationStore>,
    message_id: String,
    variant: VariantInput,
    variant_id: Option<String>,
) -> Result<PathMessage, String> {
    with_db(&store, move |db| db.add_variant(&message_id, &variant, variant_id)).await
}

#[tauri::command]
pub(crate) async fn update_message_variant(
    store: State<'_, ConversationStore>,
    variant_id: String,
    variant: VariantInput,
) -> Result<(), String> {
    with_db(&store, move |db| db.update_variant(&variant_id, &variant)).await
}

#[tauri::command]
pub(crate) async fn select_message_variant(
    store: State<'_, ConversationStore>,
    message_id: String,
    variant_id: String,
) -> Result<ConversationView, String> {
    with_db(&store, move |db| db.select_variant(&message_id, &variant_id)).await
}

/// Split view: continue the conversation with one side (`a` / `b`) of every split turn.
#[tauri::command]
pub(crate) async fn adopt_compare_side(
    store: State<'_, ConversationStore>,
    conversation_id: String,
    side: String,
) -> Result<ConversationView, String> {
    with_db(&store, move |db| db.adopt_compare_side(&conversation_id, &side)).await
}

#[tauri::command]
pub(crate) async fn select_conversation_branch(
    store: State<'_, ConversationStore>,
    message_id: String,
) -> Result<ConversationView, String> {
    with_db(&store, move |db| db.select_branch(&message_id)).await
}

#[tauri::command]
pub(crate) async fn edit_user_message(
    store: State<'_, ConversationStore>,
    message_id: String,
    content: String,
    new_message_id: Option<String>,
) -> Result<PathMessage, String> {
    with_db(&store, move |db| {
        db.edit_user_message(&message_id, &content, new_message_id)
    })
    .await
}

/// "Edit from here": rewinds the open conversation to just before a user message. The message
/// and everything after it leave the active path (kept in the tree as a branch).
#[tauri::command]
pub(crate) async fn rewind_conversation_before(
    store: State<'_, ConversationStore>,
    message_id: String,
) -> Result<ConversationView, String> {
    with_db(&store, move |db| db.rewind_before(&message_id)).await
}

#[tauri::command]
pub(crate) async fn list_conversation_folders(
    store: State<'_, ConversationStore>,
) -> Result<Vec<Folder>, String> {
    with_db(&store, |db| db.list_folders()).await
}

#[tauri::command]
pub(crate) async fn create_conversation_folder(
    store: State<'_, ConversationStore>,
    name: String,
    parent_id: Option<String>,
) -> Result<Folder, String> {
    with_db(&store, move |db| db.create_folder(&name, parent_id.as_deref())).await
}

#[tauri::command]
pub(crate) async fn rename_conversation_folder(
    store: State<'_, ConversationStore>,
    id: String,
    name: String,
) -> Result<(), String> {
    with_db(&store, move |db| db.rename_folder(&id, &name)).await
}

#[tauri::command]
pub(crate) async fn delete_conversation_folder(
    store: State<'_, ConversationStore>,
    id: String,
) -> Result<(), String> {
    with_db(&store, move |db| db.delete_folder(&id)).await
}

/// One-time move of the old single-chat Playground (localStorage) into `state.db`.
#[tauri::command]
pub(crate) async fn import_legacy_playground(
    store: State<'_, ConversationStore>,
    import: LinearImport,
) -> Result<Option<String>, String> {
    with_db(&store, move |db| db.import_legacy_playground(&import)).await
}

/// Moves the complete browser-persisted Playground state in one transaction. The returned
/// verification result tells the webview exactly which source keys are safe to remove.
#[tauri::command]
pub(crate) async fn import_playground_state(
    store: State<'_, ConversationStore>,
    import: PlaygroundStateImport,
) -> Result<PlaygroundStateImportResult, String> {
    with_db(&store, move |db| db.import_playground_state(&import)).await
}

#[tauri::command]
pub(crate) async fn get_playground_state(
    store: State<'_, ConversationStore>,
    key: String,
) -> Result<Option<String>, String> {
    with_db(&store, move |db| db.get_flag(&key)).await
}

#[tauri::command]
pub(crate) async fn set_playground_state(
    store: State<'_, ConversationStore>,
    key: String,
    value: String,
) -> Result<(), String> {
    with_db(&store, move |db| db.set_flag(&key, &value)).await
}

/// Exports a conversation through a save dialog. `format` = "json" | "markdown".
/// Returns the written path, or `None` if the dialog was cancelled.
#[tauri::command]
pub(crate) async fn export_conversation(
    app: tauri::AppHandle,
    store: State<'_, ConversationStore>,
    id: String,
    format: String,
    file_name: Option<String>,
    labels: Option<MarkdownLabels>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let markdown = match format.as_str() {
        "json" => false,
        "markdown" | "md" => true,
        other => {
            return Err(harness_core::error_key::err("export.unknown_format")
                .with("format", other)
                .into())
        }
    };
    let text = with_db(&store, move |db| {
        if markdown {
            db.export_markdown(&id, &labels.unwrap_or_default())
        } else {
            let e = db.export_json(&id)?;
            serde_json::to_string_pretty(&e).map_err(|e| {
                StateError::Invalid(harness_core::error_key::err("json.serialize_failed").with("error", e).into())
            })
        }
    })
    .await?;
    let ext = if markdown { "md" } else { "json" };
    let base = file_name
        .as_deref()
        .and_then(paths::sanitize_file_name)
        .unwrap_or_else(|| "sohbet".to_string());
    let default_name = format!("{}.{}", base, ext);
    let picked = blocking(move || {
        let dialog = app.dialog().file().set_file_name(&default_name);
        let dialog = if markdown {
            dialog.add_filter("Markdown", &["md"])
        } else {
            dialog.add_filter("JSON", &["json"])
        };
        dialog.blocking_save_file()
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

/// Saves a diagram rendered in the Playground (Mermaid SVG, already sanitized in the webview)
/// through a save dialog. Returns the written path, or `None` if the dialog was cancelled.
#[tauri::command]
pub(crate) async fn export_diagram_svg(
    app: tauri::AppHandle,
    svg: String,
    file_name: Option<String>,
) -> Result<Option<String>, String> {
    const MAX_BYTES: usize = 8 * 1024 * 1024;
    if svg.len() > MAX_BYTES || !is_svg_document(&svg) {
        return Err(harness_core::error_key::err("export.invalid_svg").into());
    }
    let base = file_name
        .as_deref()
        .and_then(paths::sanitize_file_name)
        .unwrap_or_else(|| "diyagram".to_string());
    save_text_dialog(&app, format!("{}.svg", base), ("SVG", &["svg"]), svg).await
}

/// An SVG document: `<svg …>`, optionally preceded by the XML declaration the webview adds.
fn is_svg_document(text: &str) -> bool {
    let text = text.trim_start();
    let body = match text.strip_prefix("<?xml") {
        Some(rest) => match rest.find("?>") {
            Some(end) => rest[end + 2..].trim_start(),
            None => return false,
        },
        None => text,
    };
    body.starts_with("<svg")
}

/// Saves a diagram rendered to PNG in the webview (Mermaid SVG drawn on a canvas) through a
/// save dialog. Returns the written path, or `None` if the dialog was cancelled.
#[tauri::command]
pub(crate) async fn export_diagram_png(
    app: tauri::AppHandle,
    png: Vec<u8>,
    file_name: Option<String>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    const MAX_BYTES: usize = 32 * 1024 * 1024;
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if png.len() > MAX_BYTES || !png.starts_with(&SIGNATURE) {
        return Err(harness_core::error_key::err("export.invalid_png").into());
    }
    let base = file_name
        .as_deref()
        .and_then(paths::sanitize_file_name)
        .unwrap_or_else(|| "diyagram".to_string());
    let default_name = format!("{base}.png");
    let picked = blocking(move || {
        app.dialog()
            .file()
            .set_file_name(&default_name)
            .add_filter("PNG", &["png"])
            .blocking_save_file()
    })
    .await?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let out = path.clone();
    blocking(move || std::fs::write(&out, png))
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

/// Imports a `harness.conversation.v1` JSON file chosen in an open dialog.
/// Returns `None` if the dialog was cancelled.
#[tauri::command]
pub(crate) async fn import_conversation_dialog(
    app: tauri::AppHandle,
    store: State<'_, ConversationStore>,
) -> Result<Option<Conversation>, String> {
    use tauri_plugin_dialog::DialogExt;
    const MAX_IMPORT_BYTES: u64 = 256 * 1024 * 1024;
    let picked = blocking(move || {
        app.dialog()
            .file()
            .add_filter("JSON", &["json"])
            .blocking_pick_file()
    })
    .await?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let text = blocking(move || -> Result<String, String> {
        let read_failed = |e: std::io::Error| {
            String::from(
                harness_core::error_key::err("fs.read_failed")
                    .with("path", path.display())
                    .with("error", e),
            )
        };
        let size = std::fs::metadata(&path).map_err(read_failed)?.len();
        if size > MAX_IMPORT_BYTES {
            return Err(harness_core::error_key::err("portable.file_too_large")
                .with("path", path.display())
                .num("max_mb", MAX_IMPORT_BYTES / (1024 * 1024))
                .into());
        }
        std::fs::read_to_string(&path).map_err(read_failed)
    })
    .await??;
    with_db(&store, move |db| db.import_json_str(&text))
        .await
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::is_svg_document;

    #[test]
    fn svg_export_accepts_the_xml_declaration_the_webview_adds() {
        assert!(is_svg_document("<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>"));
        assert!(is_svg_document(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>"
        ));
        assert!(!is_svg_document("<?xml version=\"1.0\"?>\n<html></html>"));
        assert!(!is_svg_document("<?xml version=\"1.0\" <svg>"));
        assert!(!is_svg_document("<html><svg></svg></html>"));
    }
}
