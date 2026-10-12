//! Safetensors to GGUF: runs Hugging Face `safetensors` LLM repos on the existing
//! llama.cpp runtime by converting them locally.
//!
//! The pipeline of one job (`run.rs`):
//!
//! 1. **Preparing**: the managed conversion environment ([`env`]) for the llama.cpp tag of the
//!    managed runtime (a throwaway uv + Python + torch-CPU venv, built once, removable).
//! 2. **Downloading**: the repo's weights and tokenizer files into a work folder next to the
//!    model library (`<models>/.cache/convert/<owner>__<repo>`: same drive, so the last step
//!    is a rename), resumable, SHA-256 checked against Hugging Face's LFS digests.
//! 3. **Converting**: the official `convert_hf_to_gguf.py` writes a 16-bit GGUF.
//! 4. **Quantizing**: the runtime's own `llama-quantize` makes the chosen type (Q4_K_M...).
//! 5. **Finalizing**: the GGUF goes to `<models>/<owner>/<repo>-GGUF/`, where the library
//!    finds it like any downloaded model; the work folder and (unless asked to keep them) the
//!    downloaded safetensors are deleted.
//!
//! Python only ever runs in step 3 (and when [`env`] builds the venv). Which repos the
//! converter can take is read from the converter's own source ([`arch`]), so a repo is refused
//! with a clear reason before anything big is downloaded ([`plan`]).

pub mod arch;
pub mod env;
pub mod plan;
pub mod proc;
mod run;

#[cfg(test)]
mod tests;

pub use env::EnvStatus;
pub use plan::{build_plan, plan, ConvertPlan, PlanFile, QuantEstimate};

use crate::error_key::err;
use crate::events::{self, CoreEvent};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Internal sentinel: a cancelled job ends as `Cancelled`, never shown as an error.
pub(crate) const CANCELLED: &str = "convert.cancelled";

/// Quantization the dialog preselects.
pub const DEFAULT_QUANT: &str = "Q4_K_M";

/// A target type with the bits per weight it takes (for the size estimate).
#[derive(Debug, Clone, Copy)]
pub struct QuantChoice {
    pub name: &'static str,
    pub bits_per_weight: f64,
}

/// The types the Hub offers (any other type `llama-quantize` lists works from the CLI).
pub const CHOICES: &[QuantChoice] = &[
    QuantChoice {
        name: "Q4_K_M",
        bits_per_weight: 4.9,
    },
    QuantChoice {
        name: "Q5_K_M",
        bits_per_weight: 5.7,
    },
    QuantChoice {
        name: "Q6_K",
        bits_per_weight: 6.6,
    },
    QuantChoice {
        name: "Q8_0",
        bits_per_weight: 8.5,
    },
    QuantChoice {
        name: "F16",
        bits_per_weight: 16.0,
    },
    QuantChoice {
        name: "BF16",
        bits_per_weight: 16.0,
    },
];

/// `q4_k_m` -> `Q4_K_M`; `None` for text that cannot be a quantization name.
pub fn normalize_quant(raw: &str) -> Option<String> {
    let q = raw.trim().to_ascii_uppercase();
    (!q.is_empty() && q.len() <= 16 && q.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then_some(q)
}

/// Types that need no `llama-quantize` run: the converter writes them directly.
pub fn is_direct(quant: &str) -> bool {
    matches!(quant, "F16" | "BF16")
}

/// Bits per weight of a target type (a table value, else the Q8_0 one as a safe upper guess).
pub fn bits_per_weight(quant: &str) -> f64 {
    CHOICES
        .iter()
        .find(|c| c.name == quant)
        .map(|c| c.bits_per_weight)
        .unwrap_or(8.5)
}

// --- the managed llama.cpp runtime this conversion is matched to -------------------------

/// Folder of the managed llama.cpp runtime.
pub fn runtime_dir() -> PathBuf {
    crate::runtime::RuntimeManager::default().managed_dir()
}

/// Release tag of the managed llama.cpp runtime (`b11559`), `None` when it is not installed.
pub fn runtime_tag() -> Option<String> {
    runtime_tag_in(&runtime_dir())
}

pub fn runtime_tag_in(dir: &Path) -> Option<String> {
    let version = crate::runtime::read_manifest(dir).version?;
    env::valid_tag(&version).then_some(version)
}

/// `llama-quantize` of the managed runtime.
pub fn quantize_binary() -> Result<PathBuf, String> {
    let dir = runtime_dir();
    let server = dir.join(crate::runtime::binary_name());
    if !server.is_file() {
        return Err(err("convert.runtime_missing").into());
    }
    crate::runtimes::quant::quantize_binary_for(&server).ok_or_else(|| {
        String::from(
            err("convert.quantize_missing").with("path", dir.join("llama-quantize").display()),
        )
    })
}

// --- progress and cancellation handles -------------------------------------------------------

/// A cancellation flag shared by a job and the processes it runs.
#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    pub fn set(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub(crate) fn flag(&self) -> Arc<AtomicBool> {
        self.0.clone()
    }
}

/// Progress of building the environment.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EnvProgress {
    /// `uv`, `source`, `python`, `packages`, `verify`.
    pub step: String,
    /// What is being fetched or the last line the tool printed.
    pub detail: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

/// Where environment progress goes.
#[derive(Clone)]
pub struct Progress {
    state: Arc<Mutex<EnvProgress>>,
    sink: Arc<dyn Fn(&EnvProgress) + Send + Sync>,
}

impl Progress {
    pub fn new(sink: impl Fn(&EnvProgress) + Send + Sync + 'static) -> Self {
        Self {
            state: Arc::default(),
            sink: Arc::new(sink),
        }
    }

    /// Progress that is also what [`env_progress`] reports (Settings polls it).
    pub fn global() -> Self {
        Self::new(|p| *ENV_PROGRESS.lock().unwrap() = Some(p.clone()))
    }

    fn emit(&self, f: impl FnOnce(&mut EnvProgress)) {
        let snapshot = {
            let mut s = self.state.lock().unwrap();
            f(&mut s);
            s.clone()
        };
        (self.sink)(&snapshot);
    }

    /// A new step starts.
    pub fn set(&self, step: &str, detail: String, total: u64) {
        self.emit(|s| {
            *s = EnvProgress {
                step: step.to_string(),
                detail,
                downloaded_bytes: 0,
                total_bytes: total,
            }
        });
    }

    /// Bytes of the current download so far.
    pub fn bytes(&self, n: u64) {
        self.emit(|s| s.downloaded_bytes = n);
    }

    /// A line the tool of the current step printed.
    pub fn detail(&self, step: &str, line: &str) {
        self.emit(|s| {
            s.step = step.to_string();
            s.detail = line.chars().take(200).collect();
        });
    }
}

static ENV_PROGRESS: Mutex<Option<EnvProgress>> = Mutex::new(None);
/// One environment build at a time (a conversion waits for the one Settings started).
pub(crate) static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static ENV_CANCEL: Mutex<Option<Cancel>> = Mutex::new(None);

/// Progress of the environment build running now (`None` when idle).
pub fn env_progress() -> Option<EnvProgress> {
    ENV_PROGRESS.lock().unwrap().clone()
}

/// Builds the environment for the managed runtime's tag (Settings > Runtime > Installed Runtime, `harness
/// convert --setup`). Returns when it is complete.
pub async fn install_env() -> Result<EnvStatus, String> {
    let tag = runtime_tag().ok_or_else(|| String::from(err("convert.runtime_missing")))?;
    let cancel = Cancel::default();
    *ENV_CANCEL.lock().unwrap() = Some(cancel.clone());
    *ENV_PROGRESS.lock().unwrap() = Some(EnvProgress::default());
    let result = env::ensure_in(&env::root(), &tag, &cancel, &Progress::global()).await;
    *ENV_PROGRESS.lock().unwrap() = None;
    *ENV_CANCEL.lock().unwrap() = None;
    match result {
        Ok(()) => Ok(env::status()),
        Err(e) if e == CANCELLED => Err(err("convert.cancelled").into()),
        Err(e) => Err(e),
    }
}

/// Stops the environment build Settings started.
pub fn cancel_env_install() {
    if let Some(c) = ENV_CANCEL.lock().unwrap().as_ref() {
        c.set();
    }
}

/// Deletes the environment (refused while it is being built or a conversion runs).
pub fn remove_env() -> Result<(), String> {
    if env_progress().is_some() || ENV_LOCK.try_lock().is_err() || active_jobs() > 0 {
        return Err(err("convert.env_in_use").into());
    }
    env::remove_in(&env::root())
}

// --- jobs ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ConvertStage {
    Queued,
    /// Building / checking the conversion environment.
    Preparing,
    Downloading,
    Converting,
    Quantizing,
    Finalizing,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ConvertStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl ConvertStatus {
    pub fn is_active(self) -> bool {
        matches!(self, ConvertStatus::Queued | ConvertStatus::Running)
    }
}

/// One conversion, as the queue shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConvertJob {
    pub id: u64,
    pub repo_id: String,
    /// Target type (`Q4_K_M`, `F16`...).
    pub quant: String,
    /// The downloaded safetensors are kept in the library after the conversion.
    pub keep_original: bool,
    pub status: ConvertStatus,
    pub stage: ConvertStage,
    /// Fraction of the current stage, `None` when it has no measure.
    pub progress: Option<f32>,
    /// The file or line the stage is on (technical text, not translated).
    pub detail: String,
    /// Bytes of the download stage.
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    /// Keyed error of a failed job.
    pub error: Option<String>,
    /// The finished GGUF.
    pub output: Option<String>,
    /// The finished vision projector, when the model has one.
    pub mmproj: Option<String>,
    /// Keyed note that does not fail the job (`convert.mmproj_failed`).
    pub warning: Option<String>,
    /// Size of the partial files a failed or cancelled job left for a retry.
    pub kept_bytes: u64,
    pub created_at: u64,
    pub updated_at: u64,
}

struct Entry {
    job: ConvertJob,
    cancel: Cancel,
    last_emit: Instant,
}

static JOBS: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// Conversions run one at a time (each one takes the CPU and the disk).
static QUEUE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
/// Minimum gap between two progress-only events of one job.
const EMIT_INTERVAL: Duration = Duration::from_millis(400);

fn active_jobs() -> usize {
    JOBS.lock()
        .unwrap()
        .iter()
        .filter(|e| e.job.status.is_active())
        .count()
}

/// All jobs, oldest first.
pub fn jobs() -> Vec<ConvertJob> {
    JOBS.lock().unwrap().iter().map(|e| e.job.clone()).collect()
}

pub fn job(id: u64) -> Option<ConvertJob> {
    JOBS.lock()
        .unwrap()
        .iter()
        .find(|e| e.job.id == id)
        .map(|e| e.job.clone())
}

/// Changes a job and publishes it (progress-only changes at most every 400 ms; stage and
/// status changes at once).
pub(crate) fn update(id: u64, f: impl FnOnce(&mut ConvertJob)) {
    let published = {
        let mut jobs = JOBS.lock().unwrap();
        let Some(e) = jobs.iter_mut().find(|e| e.job.id == id) else {
            return;
        };
        let (stage, status) = (e.job.stage, e.job.status);
        f(&mut e.job);
        e.job.updated_at = crate::capabilities::now_secs();
        let important = e.job.stage != stage || e.job.status != status;
        if important || e.last_emit.elapsed() >= EMIT_INTERVAL {
            e.last_emit = Instant::now();
            Some(e.job.clone())
        } else {
            None
        }
    };
    if let Some(job) = published {
        events::publish(CoreEvent::Convert { job: Box::new(job) });
    }
}

/// Queues a conversion of `repo_id` to `quant` (default [`DEFAULT_QUANT`]). The plan is checked
/// first, so a repo the converter cannot take, a full disk or a missing token is an error here,
/// not a failed job. Must run inside a tokio runtime.
pub async fn start(
    repo_id: &str,
    quant: Option<&str>,
    keep_original: bool,
) -> Result<ConvertJob, String> {
    crate::downloader::DownloadManager::validate_repo_id(repo_id)?;
    let quant = match quant {
        Some(q) => normalize_quant(q)
            .ok_or_else(|| String::from(err("convert.invalid_quant").with("quant", q)))?,
        None => DEFAULT_QUANT.to_string(),
    };
    if JOBS
        .lock()
        .unwrap()
        .iter()
        .any(|e| e.job.status.is_active() && e.job.repo_id.eq_ignore_ascii_case(repo_id))
    {
        return Err(err("convert.already_running").with("repo", repo_id).into());
    }
    let p = plan(repo_id, Some(&quant)).await?;
    p.check(&quant)?;

    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let now = crate::capabilities::now_secs();
    let job = ConvertJob {
        id,
        repo_id: repo_id.to_string(),
        quant: quant.clone(),
        keep_original,
        status: ConvertStatus::Queued,
        stage: ConvertStage::Queued,
        progress: None,
        detail: String::new(),
        downloaded_bytes: 0,
        total_bytes: p.download_bytes,
        error: None,
        output: None,
        mmproj: None,
        warning: None,
        kept_bytes: 0,
        created_at: now,
        updated_at: now,
    };
    let cancel = Cancel::default();
    JOBS.lock().unwrap().push(Entry {
        job: job.clone(),
        cancel: cancel.clone(),
        last_emit: Instant::now(),
    });
    events::publish(CoreEvent::Convert {
        job: Box::new(job.clone()),
    });
    let plan_for_run = p;
    tokio::spawn(async move {
        let _turn = QUEUE.lock().await;
        run::drive(id, plan_for_run, cancel).await;
    });
    Ok(job)
}

/// Stops a job (a running one is killed; a queued one is cancelled before it starts).
pub fn cancel(id: u64) -> Result<(), String> {
    let jobs = JOBS.lock().unwrap();
    let e = jobs
        .iter()
        .find(|e| e.job.id == id)
        .ok_or_else(|| String::from(err("convert.job_not_found").with("id", id)))?;
    e.cancel.set();
    Ok(())
}

/// Removes a finished job from the list and deletes the partial files it kept. A running
/// job cannot be removed (cancel it first).
pub fn remove(id: u64) -> Result<(), String> {
    let entry = {
        let mut jobs = JOBS.lock().unwrap();
        let pos = jobs
            .iter()
            .position(|e| e.job.id == id)
            .ok_or_else(|| String::from(err("convert.job_not_found").with("id", id)))?;
        if jobs[pos].job.status.is_active() {
            return Err(err("convert.job_active").with("id", id).into());
        }
        jobs.remove(pos)
    };
    if entry.job.status != ConvertStatus::Completed {
        let _ = std::fs::remove_dir_all(run::work_dir(&entry.job.repo_id));
    }
    events::publish(CoreEvent::ConvertRemoved { id });
    Ok(())
}

/// Removes every finished job (the ones that kept partial files lose them).
pub fn clear_finished() {
    let ids: Vec<u64> = jobs()
        .iter()
        .filter(|j| !j.status.is_active())
        .map(|j| j.id)
        .collect();
    for id in ids {
        let _ = remove(id);
    }
}
