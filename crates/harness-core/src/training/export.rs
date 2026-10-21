//! Training exports use the converter matched to the managed llama.cpp runtime.
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = TrainingQuant))]
pub enum ExportQuant {
    F16,
    Q8_0,
    #[default]
    Q4_K_M,
    Q5_K_M,
}
impl ExportQuant {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::F16 => "F16",
            Self::Q8_0 => "Q8_0",
            Self::Q4_K_M => "Q4_K_M",
            Self::Q5_K_M => "Q5_K_M",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ExportInfo {
    pub status: ExportStatus,
    /// Merged model or adapter safetensors directory.
    pub dir: Option<String>,
    pub gguf: Option<String>,
    pub warning: Option<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub adapter_only: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ExportRequest {
    pub name: Option<String>,
    /// Also convert to GGUF; missing converter dependencies are installed automatically.
    #[serde(default)]
    pub gguf: bool,
    /// Merged GGUF quantization. Adapter GGUFs always use F16.
    #[serde(default)]
    pub quant: ExportQuant,
    /// Export only the adapter, without merging or copying the base weights.
    #[serde(default)]
    pub adapter_only: bool,
}

fn models_dir() -> PathBuf {
    crate::config::AppSettings::load()
        .primary_models_dir()
        .join("training")
}
static QUEUE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
fn set(id: u64, info: ExportInfo) {
    super::update(id, |j| j.export = Some(info));
}

/// Starts a background export; the result is persisted in `TrainingJob::export`.
pub fn export(id: u64, req: &ExportRequest) -> Result<super::TrainingJob, String> {
    let _admission = super::LIFECYCLE.lock().unwrap();
    let job =
        super::job(id).ok_or_else(|| String::from(err("training.job_not_found").with("id", id)))?;
    if job.status.is_active() {
        return Err(err("training.job_active").with("id", id).into());
    }
    let dir = super::job_dir(id);
    if job.status == TrainingStatus::Draft
        || (!dir.join("adapter/adapter_model.safetensors").is_file()
            && run::latest_checkpoint(&dir).is_none())
    {
        return Err(err("training.nothing_to_export").with("id", id).into());
    }
    if job
        .export
        .as_ref()
        .is_some_and(|x| x.status == ExportStatus::Running)
    {
        return Err(err("training.export_active").with("id", id).into());
    }
    let label = req
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(&job.config.name)
        .to_string();
    let name = super::slug(&label);
    if name.is_empty() {
        return Err(err("training.invalid_param")
            .with("name", "name")
            .with("value", label)
            .into());
    }
    let (gguf, quant, adapter_only) = (req.gguf, req.quant, req.adapter_only);
    set(
        id,
        ExportInfo {
            status: ExportStatus::Running,
            dir: None,
            gguf: None,
            warning: None,
            error: None,
            adapter_only,
        },
    );
    tokio::spawn(async move {
        let _turn = QUEUE.lock().await;
        let info = match do_export(id, &name, &label, gguf, quant, adapter_only).await {
            Ok(info) => info,
            Err(e) => ExportInfo {
                status: ExportStatus::Failed,
                dir: None,
                gguf: None,
                warning: None,
                error: Some(e),
                adapter_only,
            },
        };
        set(id, info);
    });
    super::job(id).ok_or_else(|| String::from(err("training.job_not_found").with("id", id)))
}

/// Never replace an existing export, including a different job's model or a loaded GGUF.
fn unused_name(base: &Path, name: &str) -> String {
    let mut candidate = name.to_string();
    let mut n = 1;
    while base.join(&candidate).exists() || base.join(format!("{candidate}-GGUF")).exists() {
        candidate = format!("{name}-{n}");
        n += 1;
    }
    candidate
}

async fn do_export(
    id: u64,
    name: &str,
    label: &str,
    gguf: bool,
    quant: ExportQuant,
    adapter_only: bool,
) -> Result<ExportInfo, String> {
    let _mutation = crate::models::LibraryMutation::begin();
    let job =
        super::job(id).ok_or_else(|| String::from(err("training.job_not_found").with("id", id)))?;
    let job_dir = super::job_dir(id);
    let base = models_dir();
    let name = unused_name(&base, name);
    let target = base.join(&name);
    // Staging stays outside the Library scan. TempDir removes partial output on every failure.
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let cache = base.join(".cache");
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let staging = tempfile::Builder::new()
        .prefix("export-")
        .tempdir_in(&cache)
        .map_err(|e| e.to_string())?;
    let partial = staging.path().join("model");
    if adapter_only {
        let source = if job_dir.join("adapter/adapter_model.safetensors").is_file() {
            job_dir.join("adapter")
        } else {
            job_dir.join(format!(
                "checkpoint-{}",
                run::latest_checkpoint(&job_dir).ok_or_else(|| String::from(
                    err("training.nothing_to_export").with("id", id)
                ))?
            ))
        };
        copy_adapter(&source, &partial)?;
    } else {
        let cmd = run::worker_command(
            &[
                "--job".as_ref(),
                job_dir.as_os_str(),
                "--export".as_ref(),
                partial.as_os_str(),
            ],
            &job_dir,
        )?;
        run_export_command(cmd, &partial.join("config.json"), "training.worker_failed").await?;
    }
    let meta = serde_json::json!({ "name": label, "base_model": job.config.model, "method": job.config.method,
        "steps": job.step, "learning_rate": job.config.learning_rate, "rank": job.config.rank,
        "dataset_license": job.dataset_report.license, "dataset_rows": job.dataset_report.rows,
        "training_job": id, "adapter_only": adapter_only, "created_at": crate::capabilities::now_secs() });
    std::fs::write(
        partial.join("harness-training.json"),
        serde_json::to_vec_pretty(&meta).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    // Keep safetensors even if the requested GGUF conversion fails. Report failure explicitly.
    crate::paths::rename_retrying(&partial, &target).map_err(|e| e.to_string())?;
    let mut info = ExportInfo {
        status: ExportStatus::Done,
        dir: Some(target.display().to_string()),
        gguf: None,
        warning: None,
        error: None,
        adapter_only,
    };
    if gguf {
        match convert_gguf(
            &target,
            Path::new(&job.config.model),
            &name,
            quant,
            adapter_only,
            staging.path(),
        )
        .await
        {
            Ok(path) => info.gguf = Some(path.display().to_string()),
            Err(e) if adapter_only => info.warning = Some(e),
            Err(e) => {
                info.status = ExportStatus::Failed;
                info.error = Some(e);
            }
        }
    }
    Ok(info)
}

fn copy_adapter(source: &Path, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target).map_err(|e| e.to_string())?;
    // Checkpoint optimizer, RNG, pickle and trainer state are never exported.
    for file in ["adapter_config.json", "adapter_model.safetensors"] {
        std::fs::copy(source.join(file), target.join(file)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn run_export_command(
    cmd: tokio::process::Command,
    expected: &Path,
    error_key: &'static str,
) -> Result<(), String> {
    let cancel = Arc::new(AtomicBool::new(false));
    let outcome = proc::run(cmd, &cancel, |_| {})
        .await
        .map_err(|e| String::from(err("training.spawn_failed").with("error", e)))?;
    if !outcome.success || !expected.is_file() {
        return Err(err(error_key).with("log", outcome.log()).into());
    }
    Ok(())
}

async fn convert_gguf(
    model: &Path,
    base_model: &Path,
    name: &str,
    quant: ExportQuant,
    adapter: bool,
    staging: &Path,
) -> Result<PathBuf, String> {
    let tag = crate::convert::runtime_tag()
        .ok_or_else(|| String::from(err("convert.runtime_missing")))?;
    let root = crate::convert::env::root();
    crate::convert::env::ensure_in(
        &root,
        &tag,
        &crate::convert::Cancel::default(),
        &crate::convert::Progress::new(|_| {}),
    )
    .await?;
    // Prevent Settings from removing the environment while a converter is using it.
    let _env = crate::convert::ENV_LOCK.lock().await;
    let out_dir = staging.join("gguf");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let f16 = out_dir.join(format!("{name}-F16.gguf"));
    let mut cmd = tokio::process::Command::new(crate::convert::env::python_bin_in(&root));
    cmd.current_dir(crate::convert::env::source_dir_in(&root, &tag))
        .arg(if adapter {
            "convert_lora_to_gguf.py"
        } else {
            "convert_hf_to_gguf.py"
        })
        .arg(model)
        .args(["--outtype", "f16", "--outfile"])
        .arg(&f16)
        .envs(crate::convert::env::python_env(&root))
        .env("HF_HUB_OFFLINE", "1");
    if adapter {
        cmd.arg("--base").arg(base_model);
    }
    run_export_command(cmd, &f16, "training.gguf_failed").await?;
    let produced = if adapter || quant == ExportQuant::F16 {
        f16
    } else {
        let output = out_dir.join(format!("{name}-{}.gguf", quant.as_str()));
        let bin = crate::convert::quantize_binary()?;
        let mut cmd = tokio::process::Command::new(&bin);
        cmd.current_dir(bin.parent().unwrap())
            .arg(&f16)
            .arg(&output)
            .arg(quant.as_str());
        run_export_command(cmd, &output, "training.gguf_failed").await?;
        std::fs::remove_file(&f16).map_err(|e| e.to_string())?;
        output
    };
    // Publish only a complete GGUF, with provenance alongside it.
    std::fs::copy(
        model.join("harness-training.json"),
        out_dir.join("harness-training.json"),
    )
    .map_err(|e| e.to_string())?;
    let dest = if adapter {
        model.join("gguf")
    } else {
        models_dir().join(format!("{name}-GGUF"))
    };
    let file = produced.file_name().unwrap().to_owned();
    crate::paths::rename_retrying(&out_dir, &dest).map_err(|e| e.to_string())?;
    Ok(dest.join(file))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_defaults_and_quant_validation() {
        let req: ExportRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(req.quant, ExportQuant::Q4_K_M);
        assert!(!req.gguf && !req.adapter_only);
        assert!(serde_json::from_str::<ExportRequest>(r#"{"quant":"Q1_BAD"}"#).is_err());
    }
    #[test]
    fn exports_never_overwrite_and_adapter_copy_excludes_checkpoint_state() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("test-GGUF")).unwrap();
        assert_eq!(unused_name(root.path(), "test"), "test-1");
        let source = root.path().join("source");
        std::fs::create_dir(&source).unwrap();
        for file in [
            "adapter_config.json",
            "adapter_model.safetensors",
            "optimizer.pt",
        ] {
            std::fs::write(source.join(file), b"x").unwrap();
        }
        let target = root.path().join("target");
        copy_adapter(&source, &target).unwrap();
        assert!(target.join("adapter_model.safetensors").is_file());
        assert!(!target.join("optimizer.pt").exists());
    }
}
