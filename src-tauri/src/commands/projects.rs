// Projects (`state.db` rows + `harness_core::projects`): thin wrappers for the Projects page
// and the Playground.

/// An output as the Projects page shows it: is the file still there?
#[derive(serde::Serialize)]
pub(crate) struct ProjectOutputView {
    #[serde(flatten)]
    output: harness_core::state::ProjectOutput,
    exists: bool,
}

#[tauri::command]
pub(crate) async fn projects_list(
    store: State<'_, ConversationStore>,
    archived: Option<bool>,
) -> Result<Vec<harness_core::state::Project>, String> {
    let archived = archived.unwrap_or(false);
    with_db(&store, move |db| db.projects(archived)).await
}

#[tauri::command]
pub(crate) async fn project_create(
    store: State<'_, ConversationStore>,
    input: harness_core::state::ProjectInput,
) -> Result<harness_core::state::Project, String> {
    let input = blocking(move || harness_core::projects::check_input(input)).await??;
    with_db(&store, move |db| db.create_project(&input)).await
}

#[tauri::command]
pub(crate) async fn project_update(
    store: State<'_, ConversationStore>,
    id: String,
    input: harness_core::state::ProjectInput,
) -> Result<harness_core::state::Project, String> {
    let input = blocking(move || harness_core::projects::check_input(input)).await??;
    with_db(&store, move |db| db.update_project(&id, &input)).await
}

#[tauri::command]
pub(crate) async fn project_archive(
    store: State<'_, ConversationStore>,
    id: String,
    archived: bool,
) -> Result<harness_core::state::Project, String> {
    with_db(&store, move |db| db.set_project_archived(&id, archived)).await
}

#[tauri::command]
pub(crate) async fn project_delete(
    store: State<'_, ConversationStore>,
    id: String,
    delete_files: bool,
) -> Result<harness_core::projects::DeleteReport, String> {
    let db = store.0.clone()?;
    blocking(move || {
        let report = harness_core::projects::delete(&db, &id, delete_files)?;
        // Paperclip tasks linked to the project lose the link with it.
        let _ = harness_core::paperclip::forget_project(&id);
        Ok(report)
    })
    .await?
}

#[tauri::command]
pub(crate) async fn project_set_chat(
    store: State<'_, ConversationStore>,
    conversation_id: String,
    project_id: Option<String>,
) -> Result<(), String> {
    with_db(&store, move |db| {
        db.set_conversation_project(&conversation_id, project_id.as_deref())
    })
    .await
}

#[tauri::command]
pub(crate) async fn project_outputs(
    store: State<'_, ConversationStore>,
    id: String,
) -> Result<Vec<ProjectOutputView>, String> {
    let outputs = with_db(&store, move |db| db.project_outputs(&id)).await?;
    blocking(move || {
        outputs
            .into_iter()
            .map(|output| ProjectOutputView {
                exists: std::path::Path::new(&output.path).is_file(),
                output,
            })
            .collect()
    })
    .await
}

/// Forgets an output; the file stays on disk.
#[tauri::command]
pub(crate) async fn project_output_remove(
    store: State<'_, ConversationStore>,
    id: String,
) -> Result<(), String> {
    with_db(&store, move |db| db.remove_project_output(&id)).await
}

/// Opens a recorded output with the system's default app (only files inside the project).
#[tauri::command]
pub(crate) async fn project_output_open(
    app: tauri::AppHandle,
    store: State<'_, ConversationStore>,
    id: String,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let db = store.0.clone()?;
    let path = blocking(move || harness_core::projects::openable(&db, &id)).await??;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn project_output_reveal(
    store: State<'_, ConversationStore>,
    id: String,
) -> Result<(), String> {
    let db = store.0.clone()?;
    blocking(move || {
        let path = harness_core::projects::openable(&db, &id)?;
        harness_core::library::ops::reveal_in_folder(&path)
    })
    .await?
}

/// Shows the project folder in the file manager.
#[tauri::command]
pub(crate) async fn project_reveal_folder(
    app: tauri::AppHandle,
    store: State<'_, ConversationStore>,
    id: String,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let project = with_db(&store, move |db| db.project(&id)).await?;
    let folder = project
        .folder
        .filter(|f| std::path::Path::new(f).is_dir())
        .ok_or_else(|| String::from(harness_core::error_key::err("projects.no_folder")))?;
    app.opener()
        .open_path(folder, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Saves a generated file (base64) into `<project folder>/outputs` and records it.
#[tauri::command]
pub(crate) async fn project_save_output(
    store: State<'_, ConversationStore>,
    project_id: String,
    name: String,
    data: String,
    from: harness_core::projects::OutputSource,
) -> Result<harness_core::state::ProjectOutput, String> {
    use base64::Engine as _;
    const MAX_BYTES: usize = 256 * 1024 * 1024;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|_| String::from(harness_core::error_key::err("projects.invalid_data")))?;
    if bytes.len() > MAX_BYTES {
        return Err(harness_core::error_key::err("projects.invalid_data").into());
    }
    let db = store.0.clone()?;
    blocking(move || harness_core::projects::save_output(&db, &project_id, &name, &bytes, &from))
        .await?
}

/// Exports the project as a zip through a save dialog. Returns the path, or `None` when the
/// dialog was cancelled.
#[tauri::command]
pub(crate) async fn project_export(
    app: tauri::AppHandle,
    store: State<'_, ConversationStore>,
    id: String,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let project = with_db(&store, {
        let id = id.clone();
        move |db| db.project(&id)
    })
    .await?;
    let base = paths::sanitize_file_name(&project.name).unwrap_or_else(|| "project".into());
    let picked = blocking(move || {
        app.dialog()
            .file()
            .set_file_name(format!("{base}.zip"))
            .add_filter("Zip", &["zip"])
            .blocking_save_file()
    })
    .await?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let db = store.0.clone()?;
    let dest = path.clone();
    blocking(move || harness_core::projects::export_zip(&db, &id, &dest)).await??;
    Ok(Some(path.to_string_lossy().to_string()))
}

#[tauri::command]
pub(crate) async fn project_agent_start(
    store: State<'_, ConversationStore>,
    id: String,
    prompt: String,
) -> Result<(), String> {
    let db = store.0.clone()?;
    blocking(move || harness_core::projects::start_agent_task(&db, &id, prompt)).await?
}

#[tauri::command]
pub(crate) async fn project_agent_tasks(
    id: String,
) -> Result<Vec<harness_core::modules::agents::TaskStatus>, String> {
    blocking(move || harness_core::projects::agent_tasks(&id)).await?
}

/// Saves a finished agent task's answer as a Markdown output of its project.
#[tauri::command]
pub(crate) async fn project_agent_save_output(
    store: State<'_, ConversationStore>,
    id: String,
    task_id: String,
) -> Result<harness_core::state::ProjectOutput, String> {
    let db = store.0.clone()?;
    blocking(move || {
        let task = harness_core::projects::agent_tasks(&id)?
            .into_iter()
            .find(|t| t.id == task_id && !t.output.trim().is_empty())
            .ok_or_else(|| String::from(harness_core::error_key::err("projects.task_not_found")))?;
        let name = format!("agent-{}.md", task.id.chars().take(8).collect::<String>());
        let text = format!("# {}\n\n{}\n", task.prompt.trim(), task.output.trim());
        harness_core::projects::save_output(
            &db,
            &id,
            &name,
            text.as_bytes(),
            &harness_core::projects::OutputSource {
                source: "agent".into(),
                model: Some(task.model.clone()).filter(|m| !m.is_empty()),
                conversation_id: None,
                agent_task_id: Some(task.id.clone()),
            },
        )
    })
    .await?
}
