//! Deterministic before/after generations, using a snapshot of the run's adapter.
use super::{run, TrainingStatus};
use crate::error_key::err;
use serde::{Deserialize, Serialize};
use std::sync::{atomic::AtomicBool, Arc};

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TrainingCompareRequest {
    pub prompts: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TrainingComparisonRow {
    pub prompt: String,
    pub base: String,
    pub trained: String,
}

pub async fn compare(
    id: u64,
    request: &TrainingCompareRequest,
) -> Result<Vec<TrainingComparisonRow>, String> {
    super::validate_prompts(&request.prompts)?;
    if request.prompts.is_empty() {
        return Err(err("training.prompts_invalid").into());
    }
    let device = super::job(id)
        .ok_or_else(|| super::not_found(id))?
        .config
        .device;
    let snapshot = tempfile::tempdir().map_err(|e| e.to_string())?;
    {
        let _admission = super::LIFECYCLE.lock().unwrap();
        let job = super::job(id).ok_or_else(|| super::not_found(id))?;
        if job.status.is_active() || job.status == TrainingStatus::Draft {
            return Err(err("training.job_active").with("id", id).into());
        }
        let dir = super::job_dir(id);
        let source = if run::has_adapter(&dir.join("adapter")) {
            dir.join("adapter")
        } else {
            dir.join(format!(
                "checkpoint-{}",
                run::latest_checkpoint(&dir).ok_or_else(|| String::from(
                    err("training.nothing_to_export").with("id", id)
                ))?
            ))
        };
        std::fs::create_dir(snapshot.path().join("adapter")).map_err(|e| e.to_string())?;
        // PEFT's weights file (PyTorch) or mlx-lm's (MLX), with its config.
        let weights = run::ADAPTER_FILES
            .into_iter()
            .find(|f| source.join(f).is_file())
            .unwrap_or(run::ADAPTER_FILES[0]);
        for name in [weights, "adapter_config.json"] {
            std::fs::copy(
                source.join(name),
                snapshot.path().join("adapter").join(name),
            )
            .map_err(|e| e.to_string())?;
        }
        std::fs::write(
            snapshot.path().join("job.json"),
            serde_json::to_vec(&job).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    std::fs::write(
        snapshot.path().join("prompts.json"),
        serde_json::to_vec(&request.prompts).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let _slot = super::SLOT.lock().await;
    let cmd = run::worker_command(
        device,
        &[
            "--job".as_ref(),
            snapshot.path().as_os_str(),
            "--compare".as_ref(),
        ],
        snapshot.path(),
    )?;
    let cancel = Arc::new(AtomicBool::new(false));
    let mut rows = None;
    let outcome = crate::pyenv::proc::run(cmd, &cancel, |line| {
        if let Some(json) = line.strip_prefix("HS_COMPARE ") {
            rows = serde_json::from_str::<Vec<TrainingComparisonRow>>(json).ok();
        }
    })
    .await
    .map_err(|e| String::from(err("training.spawn_failed").with("error", e)))?;
    if !outcome.success {
        return Err(err("training.worker_failed")
            .with("log", outcome.log())
            .into());
    }
    rows.ok_or_else(|| {
        String::from(err("training.worker_failed").with("log", "missing comparison"))
    })
}
