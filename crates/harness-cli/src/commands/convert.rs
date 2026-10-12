//! `harness convert <owner/repo> [--quant Q4_K_M] [--keep-original] [--plan]` and
//! `harness convert env status|install|rm`: safetensors to GGUF. Everything runs
//! in this process (no instance is needed): the pipeline is `harness_core::convert`, the
//! result lands in the model library like a download.

use std::time::Duration;

use harness_core::convert::{self, ConvertJob, ConvertPlan, ConvertStage, ConvertStatus};
use serde_json::{json, Value};

use crate::cli::{ConvertArgs, ConvertCommand, ConvertEnvCommand};
use crate::error::{CliError, CliResult};
use crate::output::{bytes, Out};

pub async fn run(args: &ConvertArgs, out: &Out) -> CliResult<()> {
    if let Some(ConvertCommand::Env { command }) = &args.command {
        return env(command, out).await;
    }
    let Some(repo) = args.repo.as_deref() else {
        return Err(CliError::usage(
            "cli.convert_repo_required",
            "name the Hugging Face repo to convert (`harness convert owner/repo`)",
        ));
    };
    let repo = harness_core::downloader::repo_from_hf_url(repo).unwrap_or_else(|| repo.to_string());
    let quant = convert::normalize_quant(&args.quant).ok_or_else(|| {
        CliError::core(
            harness_core::error_key::err("convert.invalid_quant")
                .with("quant", &args.quant)
                .to_string(),
        )
    })?;

    let plan = convert::plan(&repo, Some(&quant))
        .await
        .map_err(CliError::core)?;
    if args.plan {
        out.data(&plan_json(&plan, &quant), || plan_text(&plan, &quant));
        return Ok(());
    }
    plan.check(&quant).map_err(CliError::core)?;
    out.info(plan_text(&plan, &quant));
    out.confirm(&format!(
        "Download {} of {} and convert it to {}?",
        bytes(plan.download_bytes),
        repo,
        quant
    ))?;

    let job = convert::start(&repo, Some(&quant), args.keep_original)
        .await
        .map_err(CliError::core)?;
    let done = follow(job.id, out).await?;
    let value = job_json(&done);
    out.data(&value, || {
        let mut lines = vec![format!(
            "Converted {} to {}: {}",
            done.repo_id,
            done.quant,
            done.output.clone().unwrap_or_default()
        )];
        if let Some(m) = &done.mmproj {
            lines.push(format!("Vision projector: {m}"));
        }
        lines.push("It is in the library now (`harness ls`).".to_string());
        lines.join("\n")
    });
    if let Some(w) = &done.warning {
        out.warn(CliError::core(w.clone()).message);
    }
    Ok(())
}

/// Follows a job to its end, printing the stage and its progress; Ctrl+C cancels it.
async fn follow(id: u64, out: &Out) -> CliResult<ConvertJob> {
    let mut last_stage = None;
    let mut last_line = String::new();
    let mut interrupted = false;
    loop {
        let Some(job) = convert::job(id) else {
            return Err(CliError::general("cli.io", "the conversion job vanished"));
        };
        if last_stage != Some(job.stage) {
            if last_stage.is_some() {
                out.progress_done();
            }
            last_stage = Some(job.stage);
        }
        if !job.status.is_active() {
            out.progress_done();
            return match job.status {
                ConvertStatus::Completed => Ok(job),
                ConvertStatus::Cancelled => Err(CliError::general(
                    "convert.cancelled",
                    "the conversion was cancelled (downloaded files are kept for a retry; `harness convert` resumes)",
                )),
                _ => Err(CliError::core(
                    job.error
                        .clone()
                        .unwrap_or_else(|| "convert.conversion_failed".to_string()),
                )),
            };
        }
        // Not a terminal: one line per change, not one per poll.
        let line = progress_line(&job);
        if line != last_line {
            out.progress(&line);
            last_line = line;
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(700)) => {}
            _ = tokio::signal::ctrl_c(), if !interrupted => {
                interrupted = true;
                out.progress_done();
                out.info("Cancelling...");
                let _ = convert::cancel(id);
            }
        }
    }
}

fn stage_name(stage: ConvertStage) -> &'static str {
    match stage {
        ConvertStage::Queued => "queued",
        ConvertStage::Preparing => "preparing the conversion environment",
        ConvertStage::Downloading => "downloading",
        ConvertStage::Converting => "converting to GGUF",
        ConvertStage::Quantizing => "quantizing",
        ConvertStage::Finalizing => "finishing",
        ConvertStage::Done => "done",
    }
}

fn progress_line(job: &ConvertJob) -> String {
    let mut line = stage_name(job.stage).to_string();
    if job.stage == ConvertStage::Downloading {
        line.push_str(&format!(
            " {} / {}",
            bytes(job.downloaded_bytes),
            bytes(job.total_bytes)
        ));
    } else if let Some(p) = job.progress {
        line.push_str(&format!(" {:.0}%", p * 100.0));
    }
    if !job.detail.is_empty() {
        line.push_str(&format!(" ({})", job.detail));
    }
    line
}

fn job_json(job: &ConvertJob) -> Value {
    serde_json::to_value(job).unwrap_or(Value::Null)
}

fn plan_json(plan: &ConvertPlan, quant: &str) -> Value {
    let mut v = serde_json::to_value(plan).unwrap_or(Value::Null);
    v["quant"] = json!(quant);
    v["estimate"] = serde_json::to_value(plan.estimate(quant)).unwrap_or(Value::Null);
    v
}

fn plan_text(plan: &ConvertPlan, quant: &str) -> String {
    let est = plan.estimate(quant);
    let mut lines = vec![format!(
        "{} ({}, {} parameters)",
        plan.repo_id,
        plan.architecture
            .as_deref()
            .unwrap_or("unknown architecture"),
        compact(plan.parameters)
    )];
    match (&plan.convertible, &plan.reason) {
        (true, _) => lines.push(format!(
            "convertible to GGUF{}",
            if plan.vision {
                " (with a vision projector)"
            } else {
                ""
            }
        )),
        (false, Some(reason)) => lines.push(format!(
            "not convertible: {}",
            CliError::core(reason.clone()).message
        )),
        (false, None) => lines.push("not convertible".to_string()),
    }
    lines.push(format!(
        "download {} in {} files; result {} ({}); disk needed about {}, free {}",
        bytes(plan.download_bytes),
        plan.files.len(),
        bytes(est.size_bytes),
        quant,
        bytes(est.disk_bytes),
        bytes(plan.free_bytes)
    ));
    if !plan.env_ready {
        lines.push(format!(
            "the conversion environment is built first (about {}, once)",
            bytes(plan.env_estimated_bytes)
        ));
    }
    if plan.gated {
        lines.push("gated repo: needs `harness hf login` and accepted terms".to_string());
    }
    lines.join("\n")
}

/// `134515008` -> `134.5M`.
fn compact(n: u64) -> String {
    match n {
        0..=999_999 => n.to_string(),
        1_000_000..=999_999_999 => format!("{:.1}M", n as f64 / 1e6),
        _ => format!("{:.1}B", n as f64 / 1e9),
    }
}

async fn env(command: &ConvertEnvCommand, out: &Out) -> CliResult<()> {
    match command {
        ConvertEnvCommand::Status => {
            let status = tokio::task::spawn_blocking(convert::env::status)
                .await
                .map_err(|e| CliError::general("cli.io", e.to_string()))?;
            out.data(&serde_json::to_value(&status).unwrap_or(Value::Null), || {
                if !status.supported {
                    return "The conversion environment is not available on this platform.".into();
                }
                match (&status.manifest, status.ready) {
                    (None, _) => format!(
                        "Not built yet (about {} when the first conversion needs it).",
                        bytes(status.estimated_size_bytes)
                    ),
                    (Some(m), true) => format!(
                        "Ready: llama.cpp {}, {}, uv {}, {} in {}",
                        m.tag,
                        m.python,
                        m.uv_version,
                        bytes(status.size_bytes),
                        status.dir.display()
                    ),
                    (Some(m), false) => format!(
                        "Built for llama.cpp {} but the runtime is {}: it is rebuilt at the next conversion ({} in {}).",
                        m.tag,
                        status.runtime_tag.as_deref().unwrap_or("not installed"),
                        bytes(status.size_bytes),
                        status.dir.display()
                    ),
                }
            });
            Ok(())
        }
        ConvertEnvCommand::Install => {
            out.confirm(&format!(
                "Build the conversion environment (about {} of downloads: uv, Python, torch CPU, transformers)?",
                bytes(convert::env::ESTIMATED_SIZE_BYTES)
            ))?;
            let task = tokio::spawn(convert::install_env());
            tokio::pin!(task);
            let status = loop {
                tokio::select! {
                    res = &mut task => {
                        break res
                            .map_err(|e| CliError::general("cli.io", e.to_string()))?
                            .map_err(CliError::core)?;
                    }
                    _ = tokio::time::sleep(Duration::from_millis(700)) => {
                        if let Some(p) = convert::env_progress() {
                            let mut line = format!("{} {}", p.step, p.detail);
                            if p.total_bytes > 0 {
                                line = format!("{} {} / {}", p.step, bytes(p.downloaded_bytes), bytes(p.total_bytes));
                            }
                            out.progress(&line);
                        }
                    }
                }
            };
            out.progress_done();
            out.data(
                &serde_json::to_value(&status).unwrap_or(Value::Null),
                || {
                    format!(
                        "Conversion environment ready ({}).",
                        bytes(status.size_bytes)
                    )
                },
            );
            Ok(())
        }
        ConvertEnvCommand::Rm => {
            let before = convert::env::status();
            out.confirm(&format!(
                "Delete the conversion environment ({}) in {}?",
                bytes(before.size_bytes),
                before.dir.display()
            ))?;
            tokio::task::spawn_blocking(convert::remove_env)
                .await
                .map_err(|e| CliError::general("cli.io", e.to_string()))?
                .map_err(CliError::core)?;
            out.data(&json!({"removed": before.dir}), || {
                format!("Removed {}.", before.dir.display())
            });
            Ok(())
        }
    }
}
