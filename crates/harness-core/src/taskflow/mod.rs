//! Task runs: multi-step agent tasks and analysis sessions of a project that can be drafted,
//! run step by step, rewound, paused and resumed later, on this or another device.
//!
//! Everything that makes a run resumable is stored as data in `state.db` (goal, steps, outputs,
//! file references, checkpoints, tally, event log), never as live RAM state (no KV cache, no
//! open connections): a run survives a crash or a restart, and a bundle carries it to another
//! machine ([`bundle`]). A run talks only to the Harness gateway (OpenAI-compatible), so local
//! models and BYOK cloud providers (`cloud:<provider>/<model>`) are used the same way.
//!
//! * [`exec`]: the runner. Requirement checks before every step ([`check_model`]), model
//!   fallback with the reason kept per step, retries with exponential backoff, per-request
//!   timeouts, a cap of concurrent runs, a token and cost tally, approval gates, a heartbeat
//!   that lets [`recover`] tell a crashed run from a live one.
//! * [`bundle`]: export and import.
//! * [`compatibility`]: what changes when a run resumes on another model.
//!
//! Ownership: the **core** (this module and `state::taskruns`) owns run state, policy and the
//! runner; the **modules/adapters** (Hermes, OpenClaw in `modules::agents`) stay opaque,
//! one-shot upstream agents that cannot be stepped, so they are not task runs; the **gateway**
//! owns routing, provider keys and local model loading; front ends only call these functions.

pub mod bundle;
pub(crate) mod exec;

pub use exec::{run_task, RunAction, RunOptions};

use crate::{
    compaction::Endpoint,
    error_key::err,
    state::{FileRef, Project, StateDb, TaskRun, TaskStep, HEARTBEAT_STALE_MS},
};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// Most file references of one run.
pub const MAX_FILE_REFS: usize = 16;
/// Text of one referenced file given to the model (bytes); more is cut with a marker.
pub const MAX_FILE_EXCERPT: usize = 8 * 1024;

pub(crate) fn http() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .no_proxy()
                .connect_timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default()
        })
        .clone()
}

/// What the gateway says about a model, and what a probe proved.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct ModelInfo {
    pub model: String,
    /// Context length in tokens, when the gateway knows it.
    pub context: Option<u64>,
    /// Tool calling: declared by the provider, or proven by a probe; `None` = untested.
    pub tools: Option<bool>,
    pub input_price: Option<f64>,
    pub output_price: Option<f64>,
    /// `local | cloud | link`.
    pub provider: String,
}

/// One candidate of the model chain as the requirement check saw it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ModelCheck {
    pub model: String,
    pub ok: bool,
    /// Keyed error string (translated by the UI) when `ok` is false.
    pub reason: Option<String>,
    pub info: Option<ModelInfo>,
}

/// `GET {base}/models`, the model objects; `None` when the endpoint does not list models.
pub(crate) async fn fetch_models(ep: &Endpoint) -> Option<Vec<Value>> {
    let url = format!("{}/models", ep.base_url.trim_end_matches('/'));
    let mut req = http().get(&url).timeout(Duration::from_secs(15));
    for (k, v) in &ep.headers {
        req = req.header(k, v);
    }
    let v: Value = req.send().await.ok()?.json().await.ok()?;
    v["data"].as_array().cloned()
}

fn info_of(model: &str, entry: &Value) -> ModelInfo {
    let h = &entry["harness"];
    let cloud = &h["cloud"];
    let provider = if cloud.is_object() {
        "cloud"
    } else if h["remote"].is_object() {
        "link"
    } else {
        "local"
    };
    ModelInfo {
        model: model.to_string(),
        context: cloud["context_length"]
            .as_u64()
            .or_else(|| h["context_size"].as_u64())
            .filter(|c| *c > 0),
        tools: cloud["tools"].as_bool(),
        input_price: cloud["input_price"].as_f64(),
        output_price: cloud["output_price"].as_f64(),
        provider: provider.into(),
    }
}

fn probe_flag(model: &str) -> String {
    format!("taskrun.tools.{model}")
}

/// Asks the model to call a trivial tool; true when it answers with a tool call.
async fn probe_tools(ep: &Endpoint, model: &str) -> Result<bool, String> {
    let body = json!({
        "model": model,
        "messages": [{"role": "user", "content": "What time is it? Use the tool."}],
        "tools": [{"type": "function", "function": {
            "name": "get_time", "description": "Returns the current time.",
            "parameters": {"type": "object", "properties": {}}}}],
        "tool_choice": "auto",
        "max_tokens": 64,
        "temperature": 0,
        "stream": false,
    });
    let url = format!("{}/chat/completions", ep.base_url.trim_end_matches('/'));
    let mut req = http()
        .post(&url)
        .timeout(Duration::from_secs(120))
        .json(&body);
    for (k, v) in &ep.headers {
        req = req.header(k, v);
    }
    let res = req
        .send()
        .await
        .map_err(|e| crate::http_error::transport(&e, &url))?;
    if !res.status().is_success() {
        return Ok(false);
    }
    let v: Value = res.json().await.unwrap_or_default();
    Ok(v["choices"][0]["message"]["tool_calls"]
        .as_array()
        .is_some_and(|c| !c.is_empty()))
}

/// Checks one model against the run's requirements: it exists and is online, its context is
/// at least `min_context`, and (when required) it calls tools. Tool support that the gateway
/// does not declare is probed once and remembered (`app_state`).
pub(crate) async fn check_model(
    db: &StateDb,
    ep: &Endpoint,
    listing: Option<&[Value]>,
    model: &str,
    min_context: Option<u64>,
    require_tools: bool,
) -> ModelCheck {
    let fail = |reason: crate::error_key::ErrorKey, info: Option<ModelInfo>| ModelCheck {
        model: model.to_string(),
        ok: false,
        reason: Some(reason.with("model", model).into()),
        info,
    };
    let mut info = ModelInfo {
        model: model.into(),
        provider: if model.starts_with("cloud:") {
            "cloud"
        } else {
            "local"
        }
        .into(),
        ..Default::default()
    };
    if let Some(list) = listing {
        let Some(entry) = list.iter().find(|e| e["id"].as_str() == Some(model)) else {
            return fail(err("taskrun.reason.unavailable"), None);
        };
        info = info_of(model, entry);
        if entry["harness"]["state"].as_str() == Some("offline") {
            return fail(err("taskrun.reason.offline"), Some(info));
        }
    }
    if let (Some(need), Some(have)) = (min_context, info.context) {
        if have < need {
            return fail(
                err("taskrun.reason.context_small")
                    .num("have", have)
                    .num("need", need),
                Some(info),
            );
        }
    }
    if require_tools {
        if info.tools.is_none() {
            info.tools = match db.get_flag(&probe_flag(model)).ok().flatten().as_deref() {
                Some("yes") => Some(true),
                Some("no") => Some(false),
                _ => match probe_tools(ep, model).await {
                    Ok(ok) => {
                        let _ = db.set_flag(&probe_flag(model), if ok { "yes" } else { "no" });
                        Some(ok)
                    }
                    // Not proven either way (gateway unreachable now): the request will say.
                    Err(_) => None,
                },
            };
        }
        if info.tools == Some(false) {
            return fail(err("taskrun.reason.no_tools"), Some(info));
        }
    }
    ModelCheck {
        model: model.into(),
        ok: true,
        reason: None,
        info: Some(info),
    }
}

/// Checks the whole model chain of a run (the pre-run requirement check).
pub async fn preflight(
    db: &StateDb,
    run_id: &str,
    ep: &Endpoint,
) -> Result<Vec<ModelCheck>, String> {
    let run = db.task_run(run_id).map_err(String::from)?;
    if run.policy.models.is_empty() {
        return Err(err("taskrun.no_models").into());
    }
    let listing = fetch_models(ep).await;
    let mut out = Vec::new();
    for m in &run.policy.models {
        out.push(
            check_model(
                db,
                ep,
                listing.as_deref(),
                m,
                run.policy.min_context,
                run.policy.require_tools,
            )
            .await,
        );
    }
    Ok(out)
}

/// Marks runs whose process vanished as `interrupted` (their in-flight step starts over).
/// Call at start-up (the UI also calls it when it lists runs).
pub fn recover(db: &StateDb, stale_ms: Option<i64>) -> Result<Vec<String>, String> {
    db.recover_task_runs(stale_ms.unwrap_or(HEARTBEAT_STALE_MS))
        .map_err(String::from)
}

/// One note about resuming a run on another model. `level` is `info` or `warn`; `message` is a
/// keyed error string the UI translates.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CompatNote {
    pub level: &'static str,
    pub message: String,
}

fn note(level: &'static str, k: crate::error_key::ErrorKey) -> CompatNote {
    CompatNote {
        level,
        message: k.into(),
    }
}

/// Rough tokens of the saved history (4 characters per token).
pub fn history_tokens(run: &TaskRun, steps: &[TaskStep]) -> u64 {
    let chars: usize = run.goal.len()
        + steps
            .iter()
            .filter(|s| s.status == "done")
            .map(|s| s.output.len() + s.prompt.len())
            .sum::<usize>();
    (chars / 4) as u64
}

/// What changes when `run` continues on `target` instead of the model it ran on. Pure; the
/// facts come from [`ModelInfo`] of the target and the run's last model.
pub fn compat_notes(
    run: &TaskRun,
    history_tokens: u64,
    target: &ModelInfo,
    needs_tools: bool,
) -> Vec<CompatNote> {
    let mut out = Vec::new();
    let from = run.last_model.as_deref().unwrap_or_default();
    if !from.is_empty() && from != target.model {
        out.push(note(
            "info",
            err("taskrun.compat.model_changed")
                .with("from", from)
                .with("to", &target.model),
        ));
        let was_cloud = from.starts_with("cloud:");
        if was_cloud != (target.provider == "cloud") {
            out.push(note(
                "info",
                err("taskrun.compat.provider_changed")
                    .with("from", if was_cloud { "cloud" } else { "local" })
                    .with("to", &target.provider),
            ));
        }
    }
    if target.provider == "cloud" {
        out.push(note(
            "warn",
            err("taskrun.compat.cloud_data").with("model", &target.model),
        ));
    }
    match target.context {
        Some(ctx) if history_tokens * 4 > ctx * 3 => out.push(note(
            "warn",
            err("taskrun.compat.context_tight")
                .num("need", history_tokens)
                .num("have", ctx),
        )),
        None => out.push(note(
            "info",
            err("taskrun.compat.context_unknown").with("model", &target.model),
        )),
        _ => {}
    }
    if needs_tools && target.tools != Some(true) {
        out.push(note(
            if target.tools == Some(false) {
                "warn"
            } else {
                "info"
            },
            err("taskrun.compat.tools_unverified").with("model", &target.model),
        ));
    }
    out
}

/// Compatibility of resuming `run_id` on `model` (the first model of its chain when `None`).
pub async fn compatibility(
    db: &StateDb,
    run_id: &str,
    ep: &Endpoint,
    model: Option<&str>,
) -> Result<Vec<CompatNote>, String> {
    let run = db.task_run(run_id).map_err(String::from)?;
    let steps = db.task_steps(run_id).map_err(String::from)?;
    let model = model
        .map(str::to_string)
        .or_else(|| run.policy.models.first().cloned())
        .ok_or_else(|| String::from(err("taskrun.no_models")))?;
    let listing = fetch_models(ep).await;
    let check = check_model(
        db,
        ep,
        listing.as_deref(),
        &model,
        run.policy.min_context,
        run.policy.require_tools,
    )
    .await;
    let mut notes = Vec::new();
    if let Some(reason) = &check.reason {
        notes.push(CompatNote {
            level: "warn",
            message: reason.clone(),
        });
    }
    let info = check.info.unwrap_or(ModelInfo {
        model: model.clone(),
        provider: if model.starts_with("cloud:") {
            "cloud"
        } else {
            "local"
        }
        .into(),
        ..Default::default()
    });
    notes.extend(compat_notes(
        &run,
        history_tokens(&run, &steps),
        &info,
        run.policy.require_tools,
    ));
    Ok(notes)
}

// ---- drafts and running jobs ----

/// Creates a draft; `files` (relative or absolute paths inside the project folder) become
/// file references.
pub fn create_draft(
    db: &StateDb,
    mut input: crate::state::NewTaskRun,
    files: &[String],
) -> Result<TaskRun, String> {
    input.file_refs = refs_for(db, input.project_id.as_deref(), files)?;
    db.create_task_run(&input).map_err(String::from)
}

/// Edits a draft or a paused run (see [`StateDb::update_task_run`]).
pub fn update_draft(
    db: &StateDb,
    id: &str,
    mut input: crate::state::NewTaskRun,
    files: &[String],
) -> Result<TaskRun, String> {
    let project_id = db.task_run(id).map_err(String::from)?.project_id;
    input.file_refs = refs_for(db, project_id.as_deref(), files)?;
    db.update_task_run(id, &input).map_err(String::from)
}

fn refs_for(
    db: &StateDb,
    project_id: Option<&str>,
    files: &[String],
) -> Result<Vec<FileRef>, String> {
    if files.iter().all(|f| f.trim().is_empty()) {
        return Ok(Vec::new());
    }
    let project = project_id
        .ok_or_else(|| String::from(err("projects.no_folder")))
        .and_then(|p| db.project(p).map_err(String::from))?;
    resolve_file_refs(&project, files)
}

type StopFlag = std::sync::Arc<std::sync::atomic::AtomicBool>;

/// Stop flags of the runs executing in this process.
#[derive(Default)]
pub struct Jobs {
    map: std::sync::Mutex<std::collections::HashMap<String, StopFlag>>,
}

pub fn jobs() -> &'static Jobs {
    static JOBS: std::sync::OnceLock<Jobs> = std::sync::OnceLock::new();
    JOBS.get_or_init(Jobs::default)
}

impl Jobs {
    fn lock(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<String, StopFlag>> {
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Registers a run; `None` while it already runs in this process.
    pub fn register(&self, id: &str) -> Option<StopFlag> {
        let mut map = self.lock();
        if map.contains_key(id) {
            return None;
        }
        let flag = StopFlag::default();
        map.insert(id.to_string(), flag.clone());
        Some(flag)
    }

    /// Asks the run to stop after its current request; false when it does not run here.
    pub fn stop(&self, id: &str) -> bool {
        match self.lock().get(id) {
            Some(f) => {
                f.store(true, std::sync::atomic::Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    pub fn is_active(&self, id: &str) -> bool {
        self.lock().contains_key(id)
    }

    pub fn finish(&self, id: &str) {
        self.lock().remove(id);
    }
}

// ---- file references ----

fn sha256_file(path: &Path) -> std::io::Result<(u64, String)> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        h.update(&buf[..n]);
    }
    Ok((size, crate::hex::lower(&h.finalize())))
}

/// A relative `/`-separated path with only plain components (no `..`, drive or root).
pub(crate) fn clean_relative(raw: &str) -> Option<String> {
    let parts: Vec<&str> = raw.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    if parts.is_empty()
        || parts
            .iter()
            .any(|p| *p == "." || *p == ".." || p.contains(':') || p.chars().any(char::is_control))
    {
        return None;
    }
    Some(parts.join("/"))
}

/// Turns the paths a user typed (relative to the project folder, or absolute inside it) into
/// references with size and hash. Anything outside the project's folders is refused.
pub fn resolve_file_refs(project: &Project, raw: &[String]) -> Result<Vec<FileRef>, String> {
    let root = project
        .folder
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| String::from(err("projects.no_folder")))?;
    if raw.len() > MAX_FILE_REFS {
        return Err(err("taskrun.too_many_files")
            .num("max", MAX_FILE_REFS as u64)
            .into());
    }
    let canonical_root = std::fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
    let mut out: Vec<FileRef> = Vec::new();
    for item in raw.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let candidate = if Path::new(item).is_absolute() {
            PathBuf::from(item)
        } else {
            root.join(item)
        };
        let invalid = || String::from(err("taskrun.file_invalid").with("path", item));
        let canonical = std::fs::canonicalize(&candidate)
            .ok()
            .filter(|p| p.is_file())
            .ok_or_else(invalid)?;
        let rel = canonical
            .strip_prefix(&canonical_root)
            .map_err(|_| String::from(err("projects.outside_folders").with("path", item)))?;
        let rel = rel.to_string_lossy().replace('\\', "/");
        if out.iter().any(|f| f.path == rel) {
            continue;
        }
        let (size, sha256) = sha256_file(&canonical).map_err(|_| invalid())?;
        out.push(FileRef {
            path: rel,
            size,
            sha256,
        });
    }
    Ok(out)
}

/// State of a file reference on this device.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FileRefState {
    pub path: String,
    /// `present | changed | missing`.
    pub state: &'static str,
}

/// Whether each referenced file still exists with the content it had.
pub fn file_ref_states(project: Option<&Project>, refs: &[FileRef]) -> Vec<FileRefState> {
    let root = project.and_then(|p| p.folder.as_ref()).map(PathBuf::from);
    refs.iter()
        .map(|r| {
            let state = root
                .as_ref()
                .and_then(|root| {
                    let rel = clean_relative(&r.path)?;
                    let p = root.join(rel);
                    paths_ok(&p, root).then_some(p)
                })
                .and_then(|p| sha256_file(&p).ok())
                .map_or("missing", |(_, hash)| {
                    if hash == r.sha256 {
                        "present"
                    } else {
                        "changed"
                    }
                });
            FileRefState {
                path: r.path.clone(),
                state,
            }
        })
        .collect()
}

fn paths_ok(path: &Path, root: &Path) -> bool {
    path.is_file() && crate::paths::is_within(path, root)
}

#[cfg(test)]
mod tests;
