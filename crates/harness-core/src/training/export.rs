//! Exporting a trained job: merge the LoRA into the base model, write safetensors into the
//! model library, and (when the llama.cpp converter environment is ready) convert them to GGUF.
//!
//! The merged model lands in `<models>/training/<name>/`, where the library finds it like any
//! other `python-hf` model; the GGUF in `<models>/training/<name>-GGUF/`.

use super::{run, TrainingStatus};
use crate::error_key::err;
use crate::pyenv::proc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ExportStatus {
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ExportInfo {
    pub status: ExportStatus,
    /// The merged safetensors model folder in the library.
    pub dir: Option<String>,
    /// The GGUF file, when one was made.
    pub gguf: Option<String>,
    /// Keyed note that does not fail the export (`training.gguf_unavailable`).
    pub warning: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ExportRequest {
    /// Library name of the new model (default: the job's name).
    pub name: Option<String>,
    /// Also convert to GGUF (needs the conversion environment of Settings).
    #[serde(default)]
    pub gguf: bool,
}

fn models_dir() -> PathBuf {
    crate::config::AppSettings::load()
        .primary_models_dir()
        .join("training")
}

fn set(id: u64, info: ExportInfo) {
    super::update(id, |j| j.export = Some(info));
}

/// Starts the export in the background; progress and result are `TrainingJob::export`.
pub fn export(id: u64, req: &ExportRequest) -> Result<super::TrainingJob, String> {
    let job =
        super::job(id).ok_or_else(|| String::from(err("training.job_not_found").with("id", id)))?;
    if job.status.is_active() {
        return Err(err("training.job_active").with("id", id).into());
    }
    if job.status == TrainingStatus::Draft {
        return Err(err("training.nothing_to_export").with("id", id).into());
    }
    let dir = super::job_dir(id);
    if !dir.join("adapter").is_dir() && run::latest_checkpoint(&dir).is_none() {
        return Err(err("training.nothing_to_export").with("id", id).into());
    }
    if job
        .export
        .as_ref()
        .is_some_and(|x| x.status == ExportStatus::Running)
    {
        return Err(err("training.export_active").with("id", id).into());
    }
    let label = match req.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => n.to_string(),
        None => job.config.name.clone(),
    };
    let mut name = super::slug(&label);
    if name.is_empty() {
        return Err(err("training.invalid_param")
            .with("name", "name")
            .with("value", label)
            .into());
    }
    // Two jobs with the same name must not overwrite each other's model.
    if models_dir().join(&name).exists()
        && !matches!(&job.export, Some(x) if x.dir.as_deref() == models_dir().join(&name).to_str())
    {
        name = format!("{name}-{id}");
    }
    set(
        id,
        ExportInfo {
            status: ExportStatus::Running,
            dir: None,
            gguf: None,
            warning: None,
            error: None,
        },
    );
    let (gguf, label) = (req.gguf, label.clone());
    tokio::spawn(async move {
        let info = match do_export(id, &name, &label, gguf).await {
            Ok(info) => info,
            Err(e) => ExportInfo {
                status: ExportStatus::Failed,
                dir: None,
                gguf: None,
                warning: None,
                error: Some(e),
            },
        };
        set(id, info);
    });
    super::job(id).ok_or_else(|| String::from(err("training.job_not_found").with("id", id)))
}

async fn do_export(id: u64, name: &str, label: &str, gguf: bool) -> Result<ExportInfo, String> {
    let job =
        super::job(id).ok_or_else(|| String::from(err("training.job_not_found").with("id", id)))?;
    let job_dir = super::job_dir(id);
    let base = models_dir();
    let target = base.join(name);
    let partial = base.join(format!(".{name}.partial"));
    for d in [&partial, &target] {
        let _ = std::fs::remove_dir_all(d);
    }
    std::fs::create_dir_all(&base).map_err(|e| {
        String::from(
            err("fs.create_dir_failed")
                .with("path", base.display())
                .with("error", e),
        )
    })?;
    let cancel = Arc::new(AtomicBool::new(false));
    let cmd = run::worker_command(
        &[
            "--job".as_ref(),
            job_dir.as_os_str(),
            "--export".as_ref(),
            partial.as_os_str(),
        ],
        &job_dir,
    )?;
    let outcome = proc::run(cmd, &cancel, |_| {})
        .await
        .map_err(|e| String::from(err("training.spawn_failed").with("error", e)))?;
    if !outcome.success || !partial.join("config.json").is_file() {
        let _ = std::fs::remove_dir_all(&partial);
        return Err(err("training.worker_failed")
            .with("log", outcome.log())
            .into());
    }
    // What the library and the user can learn about where the model came from.
    let meta = serde_json::json!({
        "name": label,
        "base_model": job.config.model,
        "method": job.config.method,
        "steps": job.step,
        "learning_rate": job.config.learning_rate,
        "rank": job.config.rank,
        "dataset_license": job.dataset_report.license,
        "dataset_rows": job.dataset_report.rows,
        "training_job": id,
        "created_at": crate::capabilities::now_secs(),
    });
    let _ = std::fs::write(
        partial.join("harness-training.json"),
        serde_json::to_string_pretty(&meta).unwrap_or_default(),
    );
    crate::paths::rename_retrying(&partial, &target).map_err(|e| {
        String::from(
            err("fs.write_failed")
                .with("path", target.display())
                .with("error", e),
        )
    })?;

    let mut info = ExportInfo {
        status: ExportStatus::Done,
        dir: Some(target.display().to_string()),
        gguf: None,
        warning: None,
        error: None,
    };
    if gguf {
        match convert_gguf(&target, name).await {
            Ok(path) => info.gguf = Some(path.display().to_string()),
            Err(e) => info.warning = Some(e),
        }
    }
    Ok(info)
}

/// The llama.cpp converter (the conversion environment of Settings) turns the merged folder
/// into a 16-bit GGUF. Without that environment the safetensors export still stands.
async fn convert_gguf(model: &Path, name: &str) -> Result<PathBuf, String> {
    let status = crate::convert::env::status();
    let tag = status
        .runtime_tag
        .clone()
        .filter(|_| status.ready)
        .ok_or_else(|| String::from(err("training.gguf_unavailable")))?;
    let env_root = crate::convert::env::root();
    let out_dir = models_dir().join(format!("{name}-GGUF"));
    std::fs::create_dir_all(&out_dir).map_err(|e| {
        String::from(
            err("fs.create_dir_failed")
                .with("path", out_dir.display())
                .with("error", e),
        )
    })?;
    let out = out_dir.join(format!("{name}-F16.gguf"));
    let mut cmd = tokio::process::Command::new(crate::convert::env::python_bin_in(&env_root));
    cmd.current_dir(crate::convert::env::source_dir_in(&env_root, &tag))
        .arg("convert_hf_to_gguf.py")
        .arg(model)
        .args(["--outtype", "f16", "--outfile"])
        .arg(&out)
        .envs(crate::convert::env::python_env(&env_root));
    let cancel = Arc::new(AtomicBool::new(false));
    let outcome = proc::run(cmd, &cancel, |_| {})
        .await
        .map_err(|e| String::from(err("training.spawn_failed").with("error", e)))?;
    if outcome.success && out.is_file() {
        Ok(out)
    } else {
        let _ = std::fs::remove_dir_all(&out_dir);
        Err(err("training.gguf_failed")
            .with("log", outcome.log())
            .into())
    }
}
