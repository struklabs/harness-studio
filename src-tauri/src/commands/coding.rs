// Coding area (`harness_core::coding`): project-scoped code work with a model through the
// Gateway. Thin wrappers: the run loop, workspace rules, checkpoints and patches live in core.

/// The running coding tasks, by run id, so a Stop button can cancel one.
#[derive(Default)]
pub(crate) struct CodingRuns(std::sync::Mutex<std::collections::HashMap<String, crate::tools::CancelToken>>);

/// A project that can take a coding run: not archived.
async fn coding_project(
    store: &ConversationStore,
    project_id: &str,
) -> Result<harness_core::state::Project, String> {
    let id = project_id.to_string();
    let project = with_db(store, move |db| db.project(&id)).await?;
    if project.archived {
        return Err(harness_core::error_key::err("projects.archived")
            .with("id", project_id)
            .into());
    }
    Ok(project)
}

#[tauri::command]
pub(crate) async fn coding_permissions(
    store: State<'_, ConversationStore>,
    project_id: String,
) -> Result<harness_core::coding::WorkspacePermissions, String> {
    let project = coding_project(&store, &project_id).await?;
    blocking(move || {
        let coding_store = harness_core::coding::Store::shared();
        let backend = match &project.folder {
            Some(folder) if std::path::Path::new(folder).is_dir() => {
                coding_store.detect_backend(std::path::Path::new(folder))
            }
            _ => harness_core::coding::Backend::Snapshot,
        };
        let classes: Vec<_> = crate::tools::ToolService::shared()
            .registry()
            .list()
            .into_iter()
            .map(|d| (d.name, d.class))
            .collect();
        harness_core::coding::guard::permissions(
            &project,
            &AppSettings::load().tools,
            backend,
            &classes,
        )
    })
    .await
}

#[derive(serde::Deserialize)]
pub(crate) struct CodingRunInput {
    project_id: String,
    #[serde(flatten)]
    request: harness_core::coding::RunRequest,
}

/// Starts a coding run and returns its id; progress arrives as `coding-event` events.
#[tauri::command]
pub(crate) async fn coding_run(
    app: tauri::AppHandle,
    store: State<'_, ConversationStore>,
    runs: State<'_, CodingRuns>,
    input: CodingRunInput,
) -> Result<String, String> {
    if input.request.prompt.trim().is_empty() {
        return Err(harness_core::error_key::err("coding.empty_prompt").into());
    }
    let project = coding_project(&store, &input.project_id).await?;
    static NEXT_RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let run_id = format!(
        "run-{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        NEXT_RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let cancel = crate::tools::CancelToken::new();
    runs.0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(run_id.clone(), cancel.clone());
    let id = run_id.clone();
    tauri::async_runtime::spawn(async move {
        let emitter = app.clone();
        let sink: harness_core::coding::run::Sink = Arc::new(move |event| {
            let _ = emitter.emit("coding-event", event);
        });
        let handle = harness_core::coding::RunHandle {
            run_id: id.clone(),
            sink: sink.clone(),
            cancel,
        };
        let settings = AppSettings::load().tools;
        let result = harness_core::coding::run_task(
            crate::tools::ToolService::shared(),
            &harness_core::coding::Store::shared(),
            &project,
            input.request,
            &settings,
            handle,
        )
        .await;
        if let Err(error) = result {
            sink(harness_core::coding::Event::Finished {
                run_id: id.clone(),
                stop: "error".into(),
                text: String::new(),
                error: Some(error),
                changes: 0,
            });
        }
        use tauri::Manager as _;
        let runs = app.state::<CodingRuns>();
        runs.0.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    });
    Ok(run_id)
}

#[tauri::command]
pub(crate) fn coding_cancel(run_id: String, runs: State<'_, CodingRuns>) -> bool {
    let token = runs
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&run_id)
        .cloned();
    token.map(|t| t.cancel()).is_some()
}

/// The user's answer to a command the model wants to run (`coding-event` `approval`).
#[tauri::command]
pub(crate) fn coding_approve(approval_id: String, allow: bool) -> bool {
    harness_core::coding::answer_approval(&approval_id, allow)
}

#[tauri::command]
pub(crate) async fn coding_checkpoints(
    project_id: String,
) -> Result<Vec<harness_core::coding::Checkpoint>, String> {
    blocking(move || harness_core::coding::Store::shared().list(&project_id)).await?
}

/// The files changed since a checkpoint, with their diffs.
#[tauri::command]
pub(crate) async fn coding_changes(
    project_id: String,
    checkpoint_id: String,
) -> Result<Vec<harness_core::coding::FileChange>, String> {
    blocking(move || {
        let store = harness_core::coding::Store::shared();
        store.changes(&store.get(&project_id, &checkpoint_id)?)
    })
    .await?
}

/// Undoes a checkpoint: all changed files, or only `files`.
#[tauri::command]
pub(crate) async fn coding_undo(
    project_id: String,
    checkpoint_id: String,
    files: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    blocking(move || {
        let store = harness_core::coding::Store::shared();
        store.undo(&store.get(&project_id, &checkpoint_id)?, files.as_deref())
    })
    .await?
}

#[tauri::command]
pub(crate) async fn coding_checkpoint_create(
    store: State<'_, ConversationStore>,
    project_id: String,
    label: String,
) -> Result<harness_core::coding::Checkpoint, String> {
    let project = coding_project(&store, &project_id).await?;
    blocking(move || {
        let folder = harness_core::coding::guard::workspace_folder(&project)?;
        harness_core::coding::Store::shared().create(&project.id, &folder, &label)
    })
    .await?
}

#[tauri::command]
pub(crate) async fn coding_checkpoint_delete(
    project_id: String,
    checkpoint_id: String,
) -> Result<(), String> {
    blocking(move || {
        let store = harness_core::coding::Store::shared();
        store.delete(&store.get(&project_id, &checkpoint_id)?)
    })
    .await?
}

/// Applies a unified diff to the project folder as one undoable step.
#[tauri::command]
pub(crate) async fn coding_apply_patch(
    store: State<'_, ConversationStore>,
    project_id: String,
    patch: String,
) -> Result<harness_core::coding::patch::Applied, String> {
    let project = coding_project(&store, &project_id).await?;
    blocking(move || {
        let folder = harness_core::coding::guard::workspace_folder(&project)?;
        harness_core::coding::patch::apply(
            &harness_core::coding::Store::shared(),
            &project.id,
            &folder,
            &patch,
            "Patch",
        )
    })
    .await?
}
