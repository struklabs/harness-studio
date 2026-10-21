//! Task bundle: one JSON file that carries a run to another device.
//!
//! It holds the run (goal, policy, tally), every step with its output, the checkpoints, the
//! event log and the file references (path relative to the project folder, size, SHA-256).
//! Small text files can be embedded (`embed_files`); on import they are restored only where no
//! file exists yet. It never contains endpoints, tokens or provider keys, and an imported run
//! always arrives paused: nothing runs until the user resumes it, on whatever model they pick.

use super::{clean_relative, file_ref_states, FileRefState};
use crate::{
    error_key::err,
    state::{FileRef, StateDb, TaskCheckpoint, TaskEvent, TaskRun, TaskStep, MAX_STEPS},
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const BUNDLE_SCHEMA: &str = "harness.taskrun.v1";
/// Largest bundle file accepted on import.
const MAX_BUNDLE_BYTES: u64 = 16 * 1024 * 1024;
/// Embedded files: each at most, and all together at most (bytes).
const MAX_EMBED_FILE: u64 = 256 * 1024;
const MAX_EMBED_TOTAL: u64 = 2 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
struct EmbeddedFile {
    #[serde(flatten)]
    file: FileRef,
    /// Base64 content, only for small files exported with `embed_files`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content_b64: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Bundle {
    schema: String,
    exported_at: i64,
    /// Where it was exported (for the user's eyes only).
    app_version: String,
    run: BundleRun,
    steps: Vec<BundleStep>,
    checkpoints: Vec<TaskCheckpoint>,
    events: Vec<TaskEvent>,
    files: Vec<EmbeddedFile>,
}

// The stored types serialize for the UI (output only); the bundle needs both directions, so it
// mirrors them.
#[derive(Debug, Serialize, Deserialize)]
struct BundleRun {
    kind: String,
    title: String,
    goal: String,
    mode: String,
    policy: crate::state::RunPolicy,
    tokens_in: i64,
    tokens_out: i64,
    cost_micro: i64,
    last_model: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BundleStep {
    idx: i64,
    title: String,
    prompt: String,
    gate: bool,
    approval: String,
    status: String,
    output: String,
    model: Option<String>,
    fallback_reason: Option<String>,
    attempts: i64,
    tokens_in: i64,
    tokens_out: i64,
    cost_micro: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportReport {
    pub path: String,
    pub steps: usize,
    pub embedded_files: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub run: TaskRun,
    pub files: Vec<FileRefState>,
    /// Files written from the bundle because they were missing here.
    pub restored_files: usize,
}

fn project_folder(db: &StateDb, run: &TaskRun) -> Option<PathBuf> {
    run.project_id
        .as_deref()
        .and_then(|p| db.project(p).ok())
        .and_then(|p| p.folder)
        .map(PathBuf::from)
}

/// Writes the run to `dest` (atomically). With `embed_files`, small referenced files are
/// carried inside the bundle.
pub fn export(
    db: &StateDb,
    run_id: &str,
    dest: &Path,
    embed_files: bool,
) -> Result<ExportReport, String> {
    let run = db.task_run(run_id).map_err(String::from)?;
    if run.status == "running" {
        return Err(err("taskrun.busy").into());
    }
    let steps = db.task_steps(run_id).map_err(String::from)?;
    let fail = |e: &dyn std::fmt::Display| -> String {
        err("taskrun.export_failed")
            .with("path", dest.display())
            .with("error", e)
            .into()
    };
    let root = project_folder(db, &run);
    let mut embedded = 0usize;
    let mut budget = MAX_EMBED_TOTAL;
    let files: Vec<EmbeddedFile> = run
        .file_refs
        .iter()
        .map(|f| {
            let content_b64 = (|| {
                if !embed_files || f.size > MAX_EMBED_FILE || f.size > budget {
                    return None;
                }
                let root = root.as_ref()?;
                let path = root.join(clean_relative(&f.path)?);
                if !path.is_file() || !crate::paths::is_within(&path, root) {
                    return None;
                }
                let bytes = std::fs::read(&path).ok()?;
                // Only the version the reference describes is worth carrying.
                let hash = crate::hex::lower(&sha2::Sha256::digest(&bytes));
                if hash != f.sha256 {
                    return None;
                }
                budget -= f.size;
                embedded += 1;
                Some(base64::engine::general_purpose::STANDARD.encode(bytes))
            })();
            EmbeddedFile {
                file: f.clone(),
                content_b64,
            }
        })
        .collect();
    let bundle = Bundle {
        schema: BUNDLE_SCHEMA.into(),
        exported_at: crate::state::now_ms(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        run: BundleRun {
            kind: run.kind.clone(),
            title: run.title.clone(),
            goal: run.goal.clone(),
            mode: run.mode.clone(),
            policy: run.policy.clone(),
            tokens_in: run.tokens_in,
            tokens_out: run.tokens_out,
            cost_micro: run.cost_micro,
            last_model: run.last_model.clone(),
        },
        steps: steps
            .iter()
            .map(|s| BundleStep {
                idx: s.idx,
                title: s.title.clone(),
                prompt: s.prompt.clone(),
                gate: s.gate,
                approval: s.approval.clone(),
                status: s.status.clone(),
                output: s.output.clone(),
                model: s.model.clone(),
                fallback_reason: s.fallback_reason.clone(),
                attempts: s.attempts,
                tokens_in: s.tokens_in,
                tokens_out: s.tokens_out,
                cost_micro: s.cost_micro,
            })
            .collect(),
        checkpoints: db.task_checkpoints(run_id).map_err(String::from)?,
        events: db.task_events(run_id).map_err(String::from)?,
        files,
    };
    let tmp = dest.with_extension("part");
    let json = serde_json::to_vec_pretty(&bundle).map_err(|e| fail(&e))?;
    std::fs::write(&tmp, json).map_err(|e| fail(&e))?;
    std::fs::rename(&tmp, dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        fail(&e)
    })?;
    Ok(ExportReport {
        path: crate::paths::display_path(dest),
        steps: steps.len(),
        embedded_files: embedded,
    })
}

use sha2::Digest;

/// Reads a bundle, validates it and files it as a new paused run of `project_id` (or of no
/// project). Embedded files are restored into the project folder where none exists.
pub fn import(db: &StateDb, src: &Path, project_id: Option<&str>) -> Result<ImportReport, String> {
    let bad = |why: &str| -> String {
        err("taskrun.bundle_invalid")
            .with("path", src.display())
            .with("why", why)
            .into()
    };
    let meta = std::fs::metadata(src).map_err(|e| bad(&e.to_string()))?;
    if !meta.is_file() || meta.len() > MAX_BUNDLE_BYTES {
        return Err(bad("size"));
    }
    let bytes = std::fs::read(src).map_err(|e| bad(&e.to_string()))?;
    let bundle: Bundle = serde_json::from_slice(&bytes).map_err(|e| bad(&e.to_string()))?;
    if bundle.schema != BUNDLE_SCHEMA {
        return Err(bad("schema"));
    }
    if bundle.steps.is_empty() || bundle.steps.len() > MAX_STEPS {
        return Err(bad("steps"));
    }
    let mut steps = Vec::new();
    for (i, s) in bundle.steps.iter().enumerate() {
        if s.idx != i as i64
            || s.prompt.trim().is_empty()
            || s.prompt.len() > crate::state::MAX_PROMPT * 4
            || s.output.len() > 1024 * 1024
        {
            return Err(bad("step"));
        }
        steps.push(TaskStep {
            id: String::new(),
            run_id: String::new(),
            idx: s.idx,
            title: s.title.chars().take(120).collect(),
            prompt: s.prompt.clone(),
            gate: s.gate,
            approval: match s.approval.as_str() {
                "granted" | "denied" | "pending" => s.approval.clone(),
                _ => "none".into(),
            },
            status: match s.status.as_str() {
                "done" | "skipped" => s.status.clone(),
                _ => "pending".into(),
            },
            output: s.output.clone(),
            model: s.model.clone(),
            fallback_reason: s.fallback_reason.clone(),
            attempts: s.attempts,
            tokens_in: s.tokens_in,
            tokens_out: s.tokens_out,
            cost_micro: s.cost_micro,
            error: None,
            started_at: None,
            finished_at: None,
        });
    }
    let refs: Vec<FileRef> = bundle
        .files
        .iter()
        .take(super::MAX_FILE_REFS)
        .filter_map(|f| {
            Some(FileRef {
                path: clean_relative(&f.file.path)?,
                size: f.file.size,
                sha256: f.file.sha256.chars().take(64).collect(),
            })
        })
        .collect();
    let run = TaskRun {
        id: String::new(),
        project_id: None,
        kind: bundle.run.kind.clone(),
        title: bundle.run.title.chars().take(120).collect(),
        goal: bundle.run.goal.chars().take(4000).collect(),
        status: String::new(),
        mode: bundle.run.mode.clone(),
        policy: bundle.run.policy.clone(),
        file_refs: refs.clone(),
        tokens_in: bundle.run.tokens_in.max(0),
        tokens_out: bundle.run.tokens_out.max(0),
        cost_micro: bundle.run.cost_micro.max(0),
        last_model: bundle.run.last_model.clone(),
        error: None,
        imported_from: None,
        heartbeat_at: 0,
        created_at: 0,
        updated_at: 0,
        finished_at: None,
    };
    let origin = format!(
        "{} ({})",
        bundle.app_version,
        src.file_name().and_then(|n| n.to_str()).unwrap_or("bundle")
    );
    let imported = db
        .import_task_run(
            &run,
            &steps,
            &bundle.checkpoints,
            &bundle.events,
            project_id,
            &origin,
        )
        .map_err(String::from)?;
    // Restore embedded files that are missing here (never overwrite).
    let project = project_id.and_then(|p| db.project(p).ok());
    let root = project
        .as_ref()
        .and_then(|p| p.folder.as_ref())
        .map(PathBuf::from);
    let mut restored = 0usize;
    if let Some(root) = &root {
        for f in &bundle.files {
            let (Some(b64), Some(rel)) = (&f.content_b64, clean_relative(&f.file.path)) else {
                continue;
            };
            let path = root.join(&rel);
            if path.exists() {
                continue;
            }
            let Ok(data) = base64::engine::general_purpose::STANDARD.decode(b64) else {
                continue;
            };
            if data.len() as u64 > MAX_EMBED_FILE
                || crate::hex::lower(&sha2::Sha256::digest(&data)) != f.file.sha256
            {
                continue;
            }
            if let Some(parent) = path.parent() {
                if std::fs::create_dir_all(parent).is_err()
                    || !crate::paths::is_within(parent, root)
                {
                    continue;
                }
            }
            if std::fs::File::create_new(&path)
                .and_then(|mut file| std::io::Write::write_all(&mut file, &data))
                .is_ok()
            {
                restored += 1;
            }
        }
    }
    Ok(ImportReport {
        files: file_ref_states(project.as_ref(), &refs),
        run: imported,
        restored_files: restored,
    })
}
