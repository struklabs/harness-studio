//! Running one training job: wait for the training slot and the GPU scheduler, start the
//! worker, turn its `HS_METRIC` lines into the job's loss history, and end the job as done,
//! failed or cancelled.

use super::{update, Device, Metric, Probe, TrainingStatus};
use crate::error_key::err;
use crate::jobs::{self, Priority};
use crate::pyenv::proc;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const WORKER: &str = include_str!("../../python/training_worker.py");
const CANCELLED: &str = "training.cancelled";
/// A cancelled worker gets this long to save its checkpoint before it is killed.
const GRACE: Duration = Duration::from_secs(90);

/// The worker script, written next to the environment (rewritten when this version's differs).
pub(super) fn worker_script() -> Result<PathBuf, String> {
    let spec = crate::engine::python_hf::env();
    let dir = crate::pyenv::envs::env_dir_in(&crate::pyenv::envs::root(), spec.name);
    let path = dir.join("training_worker.py");
    if std::fs::read_to_string(&path).ok().as_deref() == Some(WORKER) {
        return Ok(path);
    }
    std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(&path, WORKER))
        .map_err(|e| {
            String::from(
                err("fs.write_failed")
                    .with("path", path.display())
                    .with("error", e),
            )
        })?;
    Ok(path)
}

/// Environment of every worker run: nothing from site-packages, offline Hub, tensors-only
/// `torch.load`.
pub(super) fn worker_env() -> Vec<(&'static str, std::ffi::OsString)> {
    let mut env = crate::pyenv::python_env(&crate::pyenv::envs::root());
    env.push(("TORCH_FORCE_WEIGHTS_ONLY_LOAD", "1".into()));
    env.push(("HF_HUB_OFFLINE", "1".into()));
    env.push(("WANDB_DISABLED", "true".into()));
    env
}

pub(super) fn worker_command(
    args: &[&std::ffi::OsStr],
    cwd: &Path,
) -> Result<tokio::process::Command, String> {
    let python = crate::engine::python_hf::binary()?;
    let script = worker_script()?;
    let mut cmd = tokio::process::Command::new(python);
    cmd.arg(script)
        .args(args)
        .current_dir(cwd)
        .envs(worker_env());
    Ok(cmd)
}

/// `devices` and QLoRA support of the environment; `None` when it is not installed or fails.
pub(super) async fn run_probe() -> Option<Probe> {
    let cwd = std::env::temp_dir();
    let cmd = worker_command(&["--probe".as_ref()], &cwd).ok()?;
    let mut last = None;
    let cancel = Arc::new(AtomicBool::new(false));
    let outcome = tokio::time::timeout(
        Duration::from_secs(120),
        proc::run(cmd, &cancel, |line| {
            if line.starts_with('{') {
                last = serde_json::from_str::<Probe>(line).ok();
            }
        }),
    )
    .await;
    cancel.store(true, Ordering::SeqCst);
    match outcome {
        Ok(Ok(o)) if o.success => last,
        _ => None,
    }
}

// --- checkpoints and metrics -------------------------------------------------------------------

/// Step of the newest complete checkpoint of a job folder: `trainer_state.json` is written
/// last, so a checkpoint that was cut off by a kill has none and is never resumed from.
pub(super) fn latest_checkpoint(dir: &Path) -> Option<u32> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let step: u32 = name.strip_prefix("checkpoint-")?.parse().ok()?;
            let p = e.path();
            (p.join("trainer_state.json").is_file()
                && p.join("adapter_model.safetensors").is_file())
            .then_some(step)
        })
        .max()
}

pub(super) fn read_metrics(path: &Path) -> Vec<Metric> {
    let mut by_step: std::collections::BTreeMap<u32, Metric> = Default::default();
    for line in std::fs::read_to_string(path).unwrap_or_default().lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(step) = v.get("step").and_then(Value::as_u64) else {
            continue;
        };
        let m = by_step.entry(step as u32).or_insert(Metric {
            step: step as u32,
            loss: None,
            eval_loss: None,
        });
        if let Some(x) = v.get("loss").and_then(Value::as_f64) {
            m.loss = Some(x);
        }
        if let Some(x) = v.get("eval_loss").and_then(Value::as_f64) {
            m.eval_loss = Some(x);
        }
    }
    by_step.into_values().collect()
}

// --- the run -----------------------------------------------------------------------------------

async fn until_set(flag: &AtomicBool) {
    while !flag.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

pub(super) async fn drive(id: u64, resume: bool) {
    let Some((soft, hard)) = super::flags(id) else {
        return;
    };
    let result = run(id, resume, &soft, &hard).await;
    let now = crate::capabilities::now_secs();
    let dir = super::job_dir(id);
    let _ = std::fs::remove_file(dir.join("cancel"));
    let checkpoint = latest_checkpoint(&dir);
    update(id, |j| {
        j.training = false;
        j.finished_at = Some(now);
        j.checkpoint_step = checkpoint;
        match &result {
            Ok(()) => j.status = TrainingStatus::Done,
            Err(e) if e == CANCELLED => j.status = TrainingStatus::Cancelled,
            Err(e) => {
                j.status = TrainingStatus::Failed;
                j.error = Some(e.clone());
            }
        }
    });
}

async fn run(
    id: u64,
    resume: bool,
    soft: &Arc<AtomicBool>,
    hard: &Arc<AtomicBool>,
) -> Result<(), String> {
    let job = super::job(id).ok_or_else(|| String::from("training.job_not_found"))?;
    let dir = super::job_dir(id);
    let cancelled = || -> Result<(), String> {
        if soft.load(Ordering::SeqCst) {
            Err(CANCELLED.into())
        } else {
            Ok(())
        }
    };
    // One worker at a time; a cancel ends the wait.
    let _slot = tokio::select! {
        slot = super::SLOT.lock() => slot,
        _ = until_set(soft) => return Err(CANCELLED.into()),
    };
    cancelled()?;
    let gpu_bytes = if job.config.device == Device::Cuda {
        job.estimate.memory_bytes
    } else {
        0
    };
    let ticket = jobs::global().submit(
        format!("training: {}", job.config.name),
        Priority::Background,
        gpu_bytes,
    );
    tokio::select! {
        r = ticket.wait() => r.map_err(|_| String::from(CANCELLED))?,
        _ = until_set(soft) => return Err(CANCELLED.into()),
    }
    cancelled()?;

    let _ = std::fs::remove_file(dir.join("cancel"));
    let mut args: Vec<&std::ffi::OsStr> = vec!["--job".as_ref(), dir.as_os_str()];
    if resume {
        args.push("--resume".as_ref());
    }
    let cmd = worker_command(&args, &dir)?;
    update(id, |j| {
        j.status = TrainingStatus::Running;
        j.started_at.get_or_insert(crate::capabilities::now_secs());
        j.error = None;
    });

    let ready = Arc::new(AtomicBool::new(false));
    let exited = Arc::new(AtomicBool::new(false));
    // A cancel kills a worker that is still loading; a training one saves a checkpoint first.
    let watcher = {
        let (soft, hard, ready, exited, dir) = (
            soft.clone(),
            hard.clone(),
            ready.clone(),
            exited.clone(),
            dir.clone(),
        );
        tokio::spawn(async move {
            until_set(&soft).await;
            if !ready.load(Ordering::SeqCst) {
                hard.store(true, Ordering::SeqCst);
                return;
            }
            let _ = std::fs::write(dir.join("cancel"), b"1");
            let deadline = tokio::time::Instant::now() + GRACE;
            while !exited.load(Ordering::SeqCst) && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            hard.store(true, Ordering::SeqCst);
        })
    };

    use std::io::Write;
    let open = |name: &str| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(name))
            .ok()
    };
    let (mut metrics_file, mut log_file) = (open("metrics.jsonl"), open("train.log"));
    let outcome = {
        // Both files are appended as lines arrive, so the page can read the loss live.
        let on_line = |line: &str| {
            if line == "HS_READY" {
                ready.store(true, Ordering::SeqCst);
                ticket.loaded();
                update(id, |j| j.training = true);
            } else if let Some(json) = line.strip_prefix("HS_METRIC ") {
                if let Ok(v) = serde_json::from_str::<Value>(json) {
                    let step = v.get("step").and_then(Value::as_u64).unwrap_or(0) as u32;
                    let loss = v.get("loss").and_then(Value::as_f64);
                    let eval = v.get("eval_loss").and_then(Value::as_f64);
                    if let Some(f) = metrics_file.as_mut() {
                        let _ = writeln!(f, "{json}");
                    }
                    update(id, |j| {
                        j.step = j.step.max(step);
                        if loss.is_some() {
                            j.loss = loss;
                        }
                        if eval.is_some() {
                            j.eval_loss = eval;
                        }
                        j.checkpoint_step = latest_checkpoint(&dir);
                    });
                }
            } else if !proc::is_bar_line(line) && !line.is_empty() {
                if let Some(f) = log_file.as_mut() {
                    let _ = writeln!(f, "{line}");
                }
            }
        };
        proc::run(cmd, hard, on_line).await
    };
    exited.store(true, Ordering::SeqCst);
    watcher.abort();
    let outcome =
        outcome.map_err(|e| String::from(err("training.spawn_failed").with("error", e)))?;
    if outcome.cancelled || soft.load(Ordering::SeqCst) {
        return Err(CANCELLED.into());
    }
    if !outcome.success {
        return Err(err("training.worker_failed")
            .with("log", outcome.log())
            .into());
    }
    if !dir.join("adapter").is_dir() {
        return Err(err("training.worker_failed")
            .with("log", "the worker finished without an adapter")
            .into());
    }
    Ok(())
}
