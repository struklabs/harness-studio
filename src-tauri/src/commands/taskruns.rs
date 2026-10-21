// Task runs (`harness_core::taskflow`): agent tasks and analysis sessions of a project that can
// be drafted, stepped, rewound, stopped, resumed after a restart and moved between devices.

#[derive(serde::Serialize)]
pub(crate) struct TaskRunDetail {
    run: harness_core::state::TaskRun,
    steps: Vec<harness_core::state::TaskStep>,
    checkpoints: Vec<harness_core::state::TaskCheckpoint>,
    events: Vec<harness_core::state::TaskEvent>,
    files: Vec<harness_core::taskflow::FileRefState>,
    /// The run is executing in this process.
    active: bool,
}

#[derive(serde::Serialize)]
pub(crate) struct TaskRunRow {
    #[serde(flatten)]
    run: harness_core::state::TaskRun,
    steps_total: usize,
    steps_done: usize,
    active: bool,
}

#[derive(serde::Deserialize)]
pub(crate) struct TaskRunDraft {
    #[serde(flatten)]
    base: harness_core::state::NewTaskRun,
    #[serde(default)]
    files: Vec<String>,
}

fn run_is_active(id: &str) -> bool {
    harness_core::taskflow::jobs().is_active(id)
}

#[tauri::command]
pub(crate) async fn taskrun_list(
    store: State<'_, ConversationStore>,
    project_id: Option<String>,
) -> Result<Vec<TaskRunRow>, String> {
    with_db(&store, move |db| {
        // A run whose process is gone becomes `interrupted` (never one running in this app).
        let _ = harness_core::taskflow::recover(db, None);
        let runs = db.task_runs(project_id.as_deref())?;
        let mut rows = Vec::new();
        for run in runs {
            let steps = db.task_steps(&run.id)?;
            rows.push(TaskRunRow {
                steps_total: steps.len(),
                steps_done: steps.iter().filter(|s| s.status == "done").count(),
                active: run_is_active(&run.id),
                run,
            });
        }
        Ok(rows)
    })
    .await
}

#[tauri::command]
pub(crate) async fn taskrun_get(
    store: State<'_, ConversationStore>,
    id: String,
) -> Result<TaskRunDetail, String> {
    with_db(&store, move |db| {
        let run = db.task_run(&id)?;
        let project = run.project_id.as_deref().and_then(|p| db.project(p).ok());
        Ok(TaskRunDetail {
            files: harness_core::taskflow::file_ref_states(project.as_ref(), &run.file_refs),
            steps: db.task_steps(&id)?,
            checkpoints: db.task_checkpoints(&id)?,
            events: db.task_events(&id)?,
            active: run_is_active(&id),
            run,
        })
    })
    .await
}

#[tauri::command]
pub(crate) async fn taskrun_create(
    store: State<'_, ConversationStore>,
    draft: TaskRunDraft,
) -> Result<harness_core::state::TaskRun, String> {
    let db = store.0.clone()?;
    blocking(move || harness_core::taskflow::create_draft(&db, draft.base, &draft.files)).await?
}

#[tauri::command]
pub(crate) async fn taskrun_update(
    store: State<'_, ConversationStore>,
    id: String,
    draft: TaskRunDraft,
) -> Result<harness_core::state::TaskRun, String> {
    let db = store.0.clone()?;
    blocking(move || harness_core::taskflow::update_draft(&db, &id, draft.base, &draft.files))
        .await?
}

#[tauri::command]
pub(crate) async fn taskrun_delete(
    store: State<'_, ConversationStore>,
    id: String,
) -> Result<(), String> {
    with_db(&store, move |db| db.delete_task_run(&id)).await
}

/// Starts or resumes a run in the background (`step` = just the next step). The page polls
/// `taskrun_get`; the model requirements are checked before every step.
#[tauri::command]
pub(crate) async fn taskrun_start(
    store: State<'_, ConversationStore>,
    id: String,
    endpoint: harness_core::compaction::Endpoint,
    step: bool,
) -> Result<(), String> {
    let db = store.0.clone()?;
    let jobs = harness_core::taskflow::jobs();
    let flag = jobs
        .register(&id)
        .ok_or_else(|| String::from(harness_core::error_key::err("taskrun.busy")))?;
    let mut opts = harness_core::taskflow::RunOptions::new(endpoint);
    if step {
        opts.action = harness_core::taskflow::RunAction::Step;
    }
    tauri::async_runtime::spawn(async move {
        if let Err(e) = harness_core::taskflow::run_task(db, &id, opts, flag).await {
            // Refused before it started (busy, too many runs): the page shows the run's state.
            eprintln!("task run {id}: {e}");
        }
        harness_core::taskflow::jobs().finish(&id);
    });
    Ok(())
}

#[tauri::command]
pub(crate) fn taskrun_stop(id: String) -> bool {
    harness_core::taskflow::jobs().stop(&id)
}

#[tauri::command]
pub(crate) async fn taskrun_rewind(
    store: State<'_, ConversationStore>,
    id: String,
    idx: i64,
) -> Result<harness_core::state::TaskRun, String> {
    with_db(&store, move |db| db.rewind_task_run(&id, idx)).await
}

/// Grants or denies the approval gate of a step (a denied step is skipped).
#[tauri::command]
pub(crate) async fn taskrun_approve(
    store: State<'_, ConversationStore>,
    id: String,
    idx: i64,
    grant: bool,
) -> Result<harness_core::state::TaskRun, String> {
    with_db(&store, move |db| {
        db.set_task_step_approval(&id, idx, if grant { "granted" } else { "denied" })?;
        db.set_task_run_status(&id, "paused", None)
    })
    .await
}

/// Skips a failed or pending step.
#[tauri::command]
pub(crate) async fn taskrun_skip(
    store: State<'_, ConversationStore>,
    id: String,
    idx: i64,
) -> Result<(), String> {
    with_db(&store, move |db| db.skip_task_step(&id, idx)).await
}

#[tauri::command]
pub(crate) async fn taskrun_preflight(
    store: State<'_, ConversationStore>,
    id: String,
    endpoint: harness_core::compaction::Endpoint,
) -> Result<Vec<harness_core::taskflow::ModelCheck>, String> {
    let db = store.0.clone()?;
    harness_core::taskflow::preflight(&db, &id, &endpoint).await
}

#[tauri::command]
pub(crate) async fn taskrun_compat(
    store: State<'_, ConversationStore>,
    id: String,
    endpoint: harness_core::compaction::Endpoint,
    model: Option<String>,
) -> Result<Vec<harness_core::taskflow::CompatNote>, String> {
    let db = store.0.clone()?;
    harness_core::taskflow::compatibility(&db, &id, &endpoint, model.as_deref()).await
}

/// Exports the run as a bundle through a save dialog; `None` when it was cancelled.
#[tauri::command]
pub(crate) async fn taskrun_export(
    app: tauri::AppHandle,
    store: State<'_, ConversationStore>,
    id: String,
    embed_files: bool,
) -> Result<Option<harness_core::taskflow::bundle::ExportReport>, String> {
    use tauri_plugin_dialog::DialogExt;
    let run = with_db(&store, {
        let id = id.clone();
        move |db| db.task_run(&id)
    })
    .await?;
    let base = paths::sanitize_file_name(&run.title).unwrap_or_else(|| "task".into());
    let picked = blocking(move || {
        app.dialog()
            .file()
            .set_file_name(format!("{base}.taskrun.json"))
            .add_filter("Task bundle", &["json"])
            .blocking_save_file()
    })
    .await?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let db = store.0.clone()?;
    blocking(move || harness_core::taskflow::bundle::export(&db, &id, &path, embed_files))
        .await?
        .map(Some)
}

/// Imports a bundle chosen in an open dialog as a paused run; `None` when cancelled.
#[tauri::command]
pub(crate) async fn taskrun_import(
    app: tauri::AppHandle,
    store: State<'_, ConversationStore>,
    project_id: Option<String>,
) -> Result<Option<harness_core::taskflow::bundle::ImportReport>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = blocking(move || {
        app.dialog()
            .file()
            .add_filter("Task bundle", &["json"])
            .blocking_pick_file()
    })
    .await?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let db = store.0.clone()?;
    blocking(move || harness_core::taskflow::bundle::import(&db, &path, project_id.as_deref()))
        .await?
        .map(Some)
}
