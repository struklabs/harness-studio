//! Training jobs: LoRA / QLoRA supervised fine-tuning of a library model on a local JSONL
//! dataset, run by `python/training_worker.py` in the managed `python-hf` environment
//! (transformers + peft + trl).
//!
//! Lifecycle of a job: `draft` (checked and estimated, nothing runs) -> `queued` (waiting for
//! the one training slot and the GPU scheduler of [`crate::jobs`]) -> `running` -> `done`,
//! `failed` or `cancelled`. A job can be cancelled at any point (a running worker saves a
//! checkpoint first), and a failed or cancelled one resumes from its last complete checkpoint.
//!
//! Everything a job owns lives in `<data dir>/training/<id>/`: `job.json` (this struct, also
//! the worker's config), `dataset.jsonl` (the snapshot that was checked), `metrics.jsonl`
//! (per-step loss and eval loss), `train.log`, `checkpoint-*` and the final `adapter/`.
//! [`export`] merges the adapter into the base model, writes safetensors into the model library
//! and optionally converts them to GGUF.

pub mod dataset;
pub mod estimate;
mod export;
mod run;

pub use dataset::{DatasetFormat, DatasetReport};
pub use estimate::{Compatibility, Estimate, MethodSupport, Probe, Support};
pub use export::{ExportInfo, ExportQuant, ExportRequest, ExportStatus};

use crate::error_key::err;
use crate::events::{self, CoreEvent};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = TrainingMethod))]
pub enum Method {
    Lora,
    /// LoRA on a 4-bit base model (bitsandbytes, CUDA only).
    Qlora,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = TrainingDevice))]
pub enum Device {
    Cpu,
    Cuda,
    Mps,
}

impl Device {
    pub fn as_str(self) -> &'static str {
        match self {
            Device::Cpu => "cpu",
            Device::Cuda => "cuda",
            Device::Mps => "mps",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum TrainingStatus {
    Draft,
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl TrainingStatus {
    pub fn is_active(self) -> bool {
        matches!(self, TrainingStatus::Queued | TrainingStatus::Running)
    }
}

/// What the worker trains with (the `config` of `job.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TrainingConfig {
    pub name: String,
    /// The base model folder (inside a models directory).
    pub model: String,
    /// The dataset file the job was created from (the job trains on its snapshot).
    pub dataset: String,
    pub method: Method,
    pub device: Device,
    pub steps: u32,
    pub learning_rate: f64,
    pub rank: u32,
    pub sequence_length: u32,
    pub validation_fraction: f64,
    pub checkpoint_steps: u32,
    pub eval_steps: u32,
}

/// Body of a new job; unset values take the defaults below.
#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateRequest {
    pub name: Option<String>,
    pub model: String,
    pub dataset: String,
    pub method: Option<Method>,
    pub device: Option<Device>,
    pub steps: Option<u32>,
    pub learning_rate: Option<f64>,
    pub rank: Option<u32>,
    pub sequence_length: Option<u32>,
    pub validation_fraction: Option<f64>,
    pub checkpoint_steps: Option<u32>,
    pub eval_steps: Option<u32>,
}

/// One job as the page shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TrainingJob {
    pub id: u64,
    pub config: TrainingConfig,
    pub status: TrainingStatus,
    /// Last step the worker reported.
    pub step: u32,
    pub total_steps: u32,
    pub loss: Option<f64>,
    pub eval_loss: Option<f64>,
    /// Step of the last complete checkpoint a resume would start from.
    pub checkpoint_step: Option<u32>,
    /// The worker is training (not loading the model).
    pub training: bool,
    /// Keyed error of a failed job.
    pub error: Option<String>,
    pub dataset_report: DatasetReport,
    pub estimate: Estimate,
    pub export: Option<ExportInfo>,
    /// The job was resumed at least once.
    pub resumed: bool,
    pub created_at: u64,
    pub updated_at: u64,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
}

/// One point of the loss chart.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Metric {
    pub step: u32,
    pub loss: Option<f64>,
    pub eval_loss: Option<f64>,
}

// --- paths -------------------------------------------------------------------------------------

pub fn root() -> PathBuf {
    crate::paths::data_root().join("training")
}

pub fn job_dir(id: u64) -> PathBuf {
    root().join(id.to_string())
}

// --- the store ---------------------------------------------------------------------------------

pub(crate) struct Entry {
    pub job: TrainingJob,
    /// Cancel asked: a running worker saves a checkpoint and stops.
    pub soft: Arc<AtomicBool>,
    /// The worker must die now.
    pub hard: Arc<AtomicBool>,
    last_emit: Instant,
    last_save: Instant,
}

static JOBS: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
// Serialize admission so resume, removal and export cannot pass each other's checks.
static LIFECYCLE: Mutex<()> = Mutex::new(());
/// One training worker at a time (each takes the whole device).
pub(crate) static SLOT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const EMIT_INTERVAL: Duration = Duration::from_millis(400);

fn store() -> &'static Mutex<Vec<Entry>> {
    JOBS.get_or_init(|| Mutex::new(load_all(&root())))
}

fn load_all(root: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    let Ok(read) = std::fs::read_dir(root) else {
        return out;
    };
    for e in read.flatten() {
        let Ok(text) = std::fs::read_to_string(e.path().join("job.json")) else {
            continue;
        };
        let Ok(mut job) = serde_json::from_str::<TrainingJob>(&text) else {
            continue;
        };
        // A job that was running when Harness stopped did not finish: it can be resumed.
        if job.status.is_active() {
            job.status = TrainingStatus::Failed;
            job.training = false;
            job.error = Some(err("training.interrupted").into());
        }
        if let Some(export) = job.export.as_mut() {
            if export.status == ExportStatus::Running {
                export.status = ExportStatus::Failed;
                export.error = Some(err("training.interrupted").into());
            }
        }
        job.checkpoint_step = run::latest_checkpoint(&e.path());
        NEXT_ID.fetch_max(job.id + 1, Ordering::SeqCst);
        out.push(Entry {
            job,
            soft: Arc::default(),
            hard: Arc::default(),
            last_emit: Instant::now(),
            last_save: Instant::now(),
        });
    }
    out.sort_by_key(|e| e.job.id);
    out
}

pub fn jobs() -> Vec<TrainingJob> {
    store()
        .lock()
        .unwrap()
        .iter()
        .map(|e| e.job.clone())
        .collect()
}

pub fn job(id: u64) -> Option<TrainingJob> {
    store()
        .lock()
        .unwrap()
        .iter()
        .find(|e| e.job.id == id)
        .map(|e| e.job.clone())
}

pub(crate) fn flags(id: u64) -> Option<(Arc<AtomicBool>, Arc<AtomicBool>)> {
    store()
        .lock()
        .unwrap()
        .iter()
        .find(|e| e.job.id == id)
        .map(|e| (e.soft.clone(), e.hard.clone()))
}

fn save(job: &TrainingJob) {
    let dir = job_dir(job.id);
    let Ok(text) = serde_json::to_string_pretty(job) else {
        return;
    };
    let tmp = dir.join("job.json.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = crate::paths::rename_retrying(&tmp, &dir.join("job.json"));
    }
}

/// Changes a job, writes it to disk (status changes at once, progress at most every 2 s) and
/// publishes it (progress at most every 400 ms).
pub(crate) fn update(id: u64, f: impl FnOnce(&mut TrainingJob)) {
    let published = {
        let mut jobs = store().lock().unwrap();
        let Some(e) = jobs.iter_mut().find(|e| e.job.id == id) else {
            return;
        };
        let status = e.job.status;
        let export = e.job.export.as_ref().map(|x| x.status);
        f(&mut e.job);
        e.job.updated_at = crate::capabilities::now_secs();
        let important = e.job.status != status || e.job.export.as_ref().map(|x| x.status) != export;
        if important || e.last_save.elapsed() >= Duration::from_secs(2) {
            e.last_save = Instant::now();
            save(&e.job);
        }
        if important || e.last_emit.elapsed() >= EMIT_INTERVAL {
            e.last_emit = Instant::now();
            Some(e.job.clone())
        } else {
            None
        }
    };
    if let Some(job) = published {
        events::publish(CoreEvent::Training { job: Box::new(job) });
    }
}

// --- creating a job ----------------------------------------------------------------------------

fn bad(name: &str, value: impl std::fmt::Display) -> String {
    err("training.invalid_param")
        .with("name", name)
        .with("value", value)
        .into()
}

fn slug(text: &str) -> String {
    let s: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    s.chars().take(48).collect()
}

/// Folder name of a model, for default job names.
fn model_label(model: &Path) -> String {
    model
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "model".into())
}

/// Checks a request and makes the full config (defaults filled in).
pub fn build_config(req: &CreateRequest) -> Result<TrainingConfig, String> {
    let model = crate::paths::normalize(Path::new(req.model.trim()));
    let in_library = crate::config::AppSettings::load()
        .all_models_dirs()
        .iter()
        .any(|d| crate::paths::is_within(&model, d));
    if !in_library {
        return Err(err("training.model_outside_library")
            .with("path", model.display())
            .into());
    }
    let devices = estimate::hardware_devices();
    let device = req.device.unwrap_or_else(|| {
        if devices.iter().any(|d| d == "cuda") {
            Device::Cuda
        } else if devices.iter().any(|d| d == "mps") {
            Device::Mps
        } else {
            Device::Cpu
        }
    });
    if !devices.iter().any(|d| d == device.as_str()) {
        return Err(err("training.device_unavailable")
            .with("device", device.as_str())
            .into());
    }
    let method = req.method.unwrap_or(Method::Lora);
    if method == Method::Qlora && device != Device::Cuda {
        return Err(err("training.qlora_needs_cuda").into());
    }
    let steps = req.steps.unwrap_or(100);
    if !(1..=100_000).contains(&steps) {
        return Err(bad("steps", steps));
    }
    let learning_rate = req.learning_rate.unwrap_or(2e-4);
    if !(learning_rate > 0.0 && learning_rate < 1.0) {
        return Err(bad("learning_rate", learning_rate));
    }
    let rank = req.rank.unwrap_or(16);
    if !(1..=256).contains(&rank) {
        return Err(bad("rank", rank));
    }
    let sequence_length = req.sequence_length.unwrap_or(512);
    if !(32..=8192).contains(&sequence_length) {
        return Err(bad("sequence_length", sequence_length));
    }
    let validation_fraction = req.validation_fraction.unwrap_or(0.1);
    if !(0.01..=0.5).contains(&validation_fraction) {
        return Err(bad("validation_fraction", validation_fraction));
    }
    let checkpoint_steps = req.checkpoint_steps.unwrap_or((steps / 4).clamp(1, 200));
    let eval_steps = req.eval_steps.unwrap_or((steps / 10).clamp(1, 100));
    if checkpoint_steps == 0 {
        return Err(bad("checkpoint_steps", 0));
    }
    if eval_steps == 0 {
        return Err(bad("eval_steps", 0));
    }
    let name = match req.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) if n.chars().count() <= 80 && !n.chars().any(char::is_control) => n.to_string(),
        Some(n) => return Err(bad("name", n)),
        None => format!("{} LoRA", model_label(&model)),
    };
    Ok(TrainingConfig {
        name,
        model: model.display().to_string(),
        dataset: req.dataset.trim().to_string(),
        method,
        device,
        steps,
        learning_rate,
        rank,
        sequence_length,
        validation_fraction,
        checkpoint_steps,
        eval_steps,
    })
}

/// Report and estimate for a config (the wizard's last step, also the draft's content).
pub fn plan(config: &TrainingConfig) -> Result<(DatasetReport, Estimate), String> {
    let report = dataset::inspect(Path::new(&config.dataset), config.validation_fraction);
    let shape = estimate::model_shape(Path::new(&config.model))?;
    let avg = report
        .estimated_tokens
        .checked_div(report.rows)
        .unwrap_or(0);
    let est = estimate::estimate(
        &shape,
        config.method,
        config.device,
        config.rank,
        config.sequence_length,
        config.steps,
        avg,
    );
    Ok((report, est))
}

/// Checks a request and returns what the wizard shows before anything is created.
pub fn preview(req: &CreateRequest) -> Result<(TrainingConfig, DatasetReport, Estimate), String> {
    let config = build_config(req)?;
    let (report, est) = plan(&config)?;
    Ok((config, report, est))
}

/// Creates a draft: the dataset is checked and copied into the job folder. Nothing runs.
pub fn create(req: &CreateRequest) -> Result<TrainingJob, String> {
    let (config, report, est) = preview(req)?;
    if !report.ok {
        return Err(err("training.dataset_invalid").into());
    }
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let dir = job_dir(id);
    std::fs::create_dir_all(&dir).map_err(|e| {
        String::from(
            err("fs.create_dir_failed")
                .with("path", dir.display())
                .with("error", e),
        )
    })?;
    std::fs::copy(&config.dataset, dir.join("dataset.jsonl")).map_err(|e| {
        String::from(
            err("fs.read_failed")
                .with("path", &config.dataset)
                .with("error", e),
        )
    })?;
    let now = crate::capabilities::now_secs();
    let job = TrainingJob {
        id,
        total_steps: config.steps,
        config,
        status: TrainingStatus::Draft,
        step: 0,
        loss: None,
        eval_loss: None,
        checkpoint_step: None,
        training: false,
        error: None,
        dataset_report: report,
        estimate: est,
        export: None,
        resumed: false,
        created_at: now,
        updated_at: now,
        started_at: None,
        finished_at: None,
    };
    save(&job);
    store().lock().unwrap().push(Entry {
        job: job.clone(),
        soft: Arc::default(),
        hard: Arc::default(),
        last_emit: Instant::now(),
        last_save: Instant::now(),
    });
    events::publish(CoreEvent::Training {
        job: Box::new(job.clone()),
    });
    Ok(job)
}

// --- the environment ---------------------------------------------------------------------------

/// The managed Python environment training runs in (the `python-hf` one).
pub fn env_status() -> crate::pyenv::envs::EnvStatus {
    let root = crate::pyenv::envs::root();
    crate::pyenv::envs::status_in(&root, crate::engine::python_hf::env_in(&root))
}

/// Builds the environment (about 1.5 GB on CPU, 3 GB with CUDA) and returns when it is ready.
pub async fn install_env() -> Result<crate::pyenv::envs::EnvStatus, String> {
    let name = crate::engine::python_hf::env().name;
    crate::pyenv::envs::install(name).await
}

fn ensure_env() -> Result<(), String> {
    crate::engine::python_hf::binary().map(|_| ())
}

// --- lifecycle ---------------------------------------------------------------------------------

fn not_found(id: u64) -> String {
    err("training.job_not_found").with("id", id).into()
}

fn queue(id: u64, resume: bool) -> Result<TrainingJob, String> {
    let _admission = LIFECYCLE.lock().unwrap();
    ensure_env()?;
    {
        let mut jobs = store().lock().unwrap();
        let e = jobs
            .iter_mut()
            .find(|e| e.job.id == id)
            .ok_or_else(|| not_found(id))?;
        let ok = if resume {
            matches!(
                e.job.status,
                TrainingStatus::Failed | TrainingStatus::Cancelled
            ) && e.job.checkpoint_step.is_some()
        } else {
            e.job.status == TrainingStatus::Draft
        };
        if e.job
            .export
            .as_ref()
            .is_some_and(|x| x.status == ExportStatus::Running)
        {
            return Err(err("training.export_active").with("id", id).into());
        }
        if !ok {
            return Err(if resume {
                err("training.not_resumable").with("id", id).into()
            } else {
                err("training.not_draft").with("id", id).into()
            });
        }
        e.soft.store(false, Ordering::SeqCst);
        e.hard.store(false, Ordering::SeqCst);
    }
    update(id, |j| {
        j.status = TrainingStatus::Queued;
        j.error = None;
        j.training = false;
        j.finished_at = None;
        j.resumed |= resume;
    });
    tokio::spawn(run::drive(id, resume));
    job(id).ok_or_else(|| not_found(id))
}

/// Queues a draft. Must run inside a tokio runtime.
pub fn start(id: u64) -> Result<TrainingJob, String> {
    queue(id, false)
}

/// Queues a failed or cancelled job to continue from its last complete checkpoint.
pub fn resume(id: u64) -> Result<TrainingJob, String> {
    queue(id, true)
}

/// Cancels a job at any point: a draft is closed, a queued one never starts, a running worker
/// saves a checkpoint and stops (killed after a minute if it does not).
pub fn cancel(id: u64) -> Result<(), String> {
    let status = {
        let jobs = store().lock().unwrap();
        let e = jobs
            .iter()
            .find(|e| e.job.id == id)
            .ok_or_else(|| not_found(id))?;
        e.soft.store(true, Ordering::SeqCst);
        e.job.status
    };
    if status == TrainingStatus::Draft {
        update(id, |j| {
            j.status = TrainingStatus::Cancelled;
            j.finished_at = Some(crate::capabilities::now_secs());
        });
    }
    Ok(())
}

/// Deletes a job that is not running, with its checkpoints (an exported model stays).
pub fn remove(id: u64) -> Result<(), String> {
    let _admission = LIFECYCLE.lock().unwrap();
    {
        let mut jobs = store().lock().unwrap();
        let pos = jobs
            .iter()
            .position(|e| e.job.id == id)
            .ok_or_else(|| not_found(id))?;
        let busy = jobs[pos].job.status.is_active()
            || jobs[pos]
                .job
                .export
                .as_ref()
                .is_some_and(|x| x.status == ExportStatus::Running);
        if busy {
            return Err(err("training.job_active").with("id", id).into());
        }
        jobs.remove(pos);
    }
    let _ = std::fs::remove_dir_all(job_dir(id));
    events::publish(CoreEvent::TrainingRemoved { id });
    Ok(())
}

/// Loss and eval loss of every step, in step order (a resumed run's repeated steps keep the
/// later value).
pub fn metrics(id: u64) -> Result<Vec<Metric>, String> {
    if job(id).is_none() {
        return Err(not_found(id));
    }
    Ok(run::read_metrics(&job_dir(id).join("metrics.jsonl")))
}

/// The tail of the worker's output.
pub fn log_tail(id: u64) -> Result<String, String> {
    if job(id).is_none() {
        return Err(not_found(id));
    }
    let text = std::fs::read_to_string(job_dir(id).join("train.log")).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    Ok(lines[lines.len().saturating_sub(60)..].join("\n"))
}

// --- what this machine can run ----------------------------------------------------------------

static PROBE: Mutex<Option<Probe>> = Mutex::new(None);

/// Asks the environment which devices and methods work (cached; `None` while it is not
/// installed).
pub async fn probe() -> Option<Probe> {
    if let Some(p) = PROBE.lock().unwrap().clone() {
        return Some(p);
    }
    let p = run::run_probe().await?;
    *PROBE.lock().unwrap() = Some(p.clone());
    Some(p)
}

/// Methods this machine can run, from the environment's probe when it is installed.
pub async fn compatibility() -> Compatibility {
    estimate::compatibility(probe().await.as_ref())
}

pub use export::export;

#[cfg(test)]
mod tests;
