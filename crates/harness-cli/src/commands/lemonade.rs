//! `harness lemonade ...`: the optional Lemonade engine (Ryzen AI NPUs through FastFlowLM, and
//! Lemonade's own llama.cpp backends). Everything runs in this process: installing is a
//! download, and the commands that need the server (`models --refresh`, `pull`, `rm`,
//! `backend`) start a `lemond` of their own for the time of the command and stop it again.
//! The `lemond` of a running Harness window or `harness serve` is separate; the gateway
//! starts that one when a request names a Lemonade model.

use std::path::Path;
use std::time::Duration;

use harness_core::lemonade::{self, InstallRequest, LemonadeSettings, ModelKind};
use serde_json::{json, Value};

use crate::cli::{GlobalArgs, LemonadeCommand};
use crate::error::{CliError, CliResult};
use crate::output::{bytes, table, Out};
use crate::session::existing_client;

pub async fn command(
    command: &LemonadeCommand,
    g: &GlobalArgs,
    data_dir: &Path,
    out: &Out,
) -> CliResult<()> {
    match command {
        LemonadeCommand::Status => status(g, data_dir, out).await,
        LemonadeCommand::Install { tag } => install(tag.clone(), out).await,
        LemonadeCommand::Uninstall { id } => uninstall(id, out).await,
        LemonadeCommand::Models {
            refresh,
            downloaded,
        } => models(*refresh, *downloaded, out).await,
        LemonadeCommand::Pull { model } => pull(model, out).await,
        LemonadeCommand::Rm { model } => rm(model, out).await,
        LemonadeCommand::Backend { recipe, backend } => backend_install(recipe, backend, out).await,
        LemonadeCommand::Config { backend, runtime } => {
            config(backend.clone(), runtime.clone(), out).await
        }
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> CliResult<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| CliError::general("cli.io", e.to_string()))?
        .map_err(CliError::core)
}

/// The state of the engine. A running instance (the window, `harness serve`) owns the `lemond`
/// it started, so it is asked through its Control API; without one, this process looks locally.
async fn status(g: &GlobalArgs, data_dir: &Path, out: &Out) -> CliResult<()> {
    // A remote instance (`--host`) is the one asked about: its errors are reported, never
    // answered with this machine's state. A local instance whose API lacks the route (an older
    // build) falls back to the local look.
    let from_instance = match existing_client(g, data_dir)? {
        Some(client) => match client.get_as::<lemonade::LemonadeStatus>("/lemonade").await {
            Ok(s) => Some(s),
            Err(e) if g.host.is_some() => return Err(e),
            Err(_) => None,
        },
        None => None,
    };
    let s = match from_instance {
        Some(s) => s,
        None => blocking(|| Ok(lemonade::status())).await?,
    };
    let value = serde_json::to_value(&s).unwrap_or(Value::Null);
    out.data(&value, || status_lines(&s));
    Ok(())
}

fn status_lines(s: &lemonade::LemonadeStatus) -> String {
    {
        let mut lines = Vec::new();
        if !s.supported {
            lines.push("Lemonade has no build for this operating system or CPU.".to_string());
        }
        match s.installs.iter().find(|i| i.active) {
            Some(i) => lines.push(format!(
                "Lemonade {} installed ({} build{})",
                i.version.as_deref().unwrap_or("?"),
                s.installs.len(),
                if s.installs.len() == 1 { "" } else { "s" }
            )),
            None => lines.push("Lemonade is not installed. `harness lemonade install`.".into()),
        }
        lines.push(format!("llama.cpp backend: {}", s.settings.backend));
        lines.push(if s.npu.detected {
            format!(
                "NPU: detected ({}){}",
                s.npu.name.as_deref().unwrap_or("AMD XDNA"),
                if s.npu.driver_installed {
                    ""
                } else {
                    ", driver not installed"
                }
            )
        } else {
            "NPU: not detected".to_string()
        });
        lines.push(match &s.base_url {
            Some(url) => format!("lemond running at {url}"),
            None => "lemond is not running here (the gateway starts it on demand)".to_string(),
        });
        for b in &s.backends {
            lines.push(format!(
                "  {}:{}  {}{}",
                b.recipe,
                b.backend,
                b.state,
                b.version
                    .as_deref()
                    .map(|v| format!("  {v}"))
                    .unwrap_or_default()
            ));
        }
        lines.join("\n")
    }
}

async fn install(tag: Option<String>, out: &Out) -> CliResult<()> {
    let pick = lemonade::preview_release(tag.clone())
        .await
        .map_err(CliError::core)?;
    let size: u64 = pick.assets.iter().map(|a| a.size).sum();
    out.confirm(&format!(
        "Download Lemonade {} ({}, {}) from github.com/{}?",
        pick.tag,
        pick.assets
            .first()
            .map(|a| a.name.as_str())
            .unwrap_or("archive"),
        bytes(size),
        pick.repo
    ))?;
    let task = tokio::spawn(async move { lemonade::install(InstallRequest { tag }).await });
    tokio::pin!(task);
    let entry = loop {
        tokio::select! {
            res = &mut task => {
                break res
                    .map_err(|e| CliError::general("cli.io", e.to_string()))?
                    .map_err(CliError::core)?;
            }
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                if let Some(p) = harness_core::runtimes::github::progress() {
                    out.progress(&format!(
                        "{} {} / {}",
                        p.current_asset,
                        bytes(p.downloaded_bytes),
                        bytes(p.total_bytes)
                    ));
                }
            }
        }
    };
    out.progress_done();
    let value = json!({"id": entry.id, "name": entry.name, "version": entry.version});
    out.data(&value, || {
        format!("Installed {} ({}).", entry.name, entry.id)
    });
    Ok(())
}

async fn uninstall(id: &str, out: &Out) -> CliResult<()> {
    out.confirm(&format!("Remove the Lemonade build {id}?"))?;
    let id = id.to_string();
    let shown = id.clone();
    blocking(move || lemonade::remove_install(&id)).await?;
    out.data(&json!({"removed": shown}), || format!("Removed {shown}"));
    Ok(())
}

/// Runs `f` with `lemond` up and stops it after, whatever `f` returns.
async fn with_server<T>(
    out: &Out,
    f: impl std::future::Future<Output = Result<T, String>>,
) -> CliResult<T> {
    out.info("Starting lemond (it scans the devices first, this takes a few seconds)...");
    let result = f.await;
    let _ = lemonade::stop().await;
    result.map_err(CliError::core)
}

async fn models(refresh: bool, downloaded_only: bool, out: &Out) -> CliResult<()> {
    let list = if refresh {
        with_server(out, lemonade::list_models_started()).await?
    } else {
        lemonade::list_models().await.map_err(CliError::core)?
    };
    let shown: Vec<_> = list
        .catalog
        .models
        .iter()
        .filter(|m| !downloaded_only || m.downloaded)
        .collect();
    let value = json!({
        "live": list.live,
        "updated_at": list.catalog.updated_at,
        "models": shown,
    });
    out.data(&value, || {
        if shown.is_empty() {
            return if list.catalog.updated_at == 0 {
                "No registry yet. `harness lemonade models --refresh` starts lemond and reads it."
                    .to_string()
            } else {
                "No model matches.".to_string()
            };
        }
        let rows: Vec<Vec<String>> = shown
            .iter()
            .map(|m| {
                vec![
                    m.id.clone(),
                    m.recipe.clone(),
                    kind_text(m.kind).to_string(),
                    format!("{:.2} GB", m.size_gb),
                    if m.downloaded { "yes" } else { "-" }.to_string(),
                ]
            })
            .collect();
        table(&["MODEL", "RECIPE", "KIND", "SIZE", "DOWNLOADED"], &rows)
    });
    Ok(())
}

fn kind_text(k: ModelKind) -> &'static str {
    match k {
        ModelKind::Chat => "chat",
        ModelKind::Embedding => "embeddings",
        ModelKind::Rerank => "rerank",
        ModelKind::Transcription => "transcription",
        ModelKind::Speech => "speech",
        ModelKind::Image => "image",
        ModelKind::Other => "other",
    }
}

/// Prints the pull progress while `task` runs.
async fn watch<T>(
    out: &Out,
    task: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    tokio::pin!(task);
    let result = loop {
        tokio::select! {
            res = &mut task => break res,
            _ = tokio::time::sleep(Duration::from_millis(700)) => {
                if let Some(p) = lemonade::pull_progress() {
                    let total = if p.total_download_size > 0 { p.total_download_size } else { p.bytes_total };
                    out.progress(&format!(
                        "{} {} {} / {}",
                        p.target,
                        p.file,
                        bytes(p.bytes_downloaded),
                        bytes(total)
                    ));
                }
            }
        }
    };
    out.progress_done();
    result
}

async fn pull(model: &str, out: &Out) -> CliResult<()> {
    lemonade::validate_model_ref(model).map_err(CliError::core)?;
    out.confirm(&format!(
        "Download {model} with Lemonade (from Hugging Face; the llama.cpp backend is fetched on first use)?"
    ))?;
    let m = model.to_string();
    with_server(out, async { watch(out, lemonade::pull_model(&m)).await }).await?;
    out.data(&json!({"pulled": model}), || format!("Downloaded {model}"));
    Ok(())
}

async fn rm(model: &str, out: &Out) -> CliResult<()> {
    lemonade::validate_model_ref(model).map_err(CliError::core)?;
    out.confirm(&format!("Delete {model} from Lemonade's model folder?"))?;
    let m = model.to_string();
    with_server(out, async { lemonade::delete_model(&m).await }).await?;
    out.data(&json!({"removed": model}), || format!("Deleted {model}"));
    Ok(())
}

async fn backend_install(recipe: &str, backend: &str, out: &Out) -> CliResult<()> {
    out.confirm(&format!(
        "Install the {recipe}:{backend} backend with Lemonade (a download from the project's release page)?"
    ))?;
    let (r, b) = (recipe.to_string(), backend.to_string());
    with_server(out, async {
        watch(out, lemonade::install_backend(&r, &b)).await
    })
    .await?;
    out.data(&json!({"installed": format!("{recipe}:{backend}")}), || {
        format!("Installed {recipe}:{backend}")
    });
    Ok(())
}

async fn config(backend: Option<String>, runtime: Option<String>, out: &Out) -> CliResult<()> {
    let current = blocking(|| Ok(lemonade::settings())).await?;
    let new = LemonadeSettings {
        backend: backend.unwrap_or(current.backend),
        runtime: match runtime.as_deref() {
            None => current.runtime,
            Some("default") | Some("") => None,
            Some(id) => Some(id.to_string()),
        },
    };
    let saved = lemonade::save_settings(new).await.map_err(CliError::core)?;
    let value = serde_json::to_value(&saved).unwrap_or(Value::Null);
    out.data(&value, || {
        format!(
            "backend: {}\nbuild: {}",
            saved.backend,
            saved.runtime.as_deref().unwrap_or("newest installed")
        )
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Command, LemonadeCommand};
    use clap::Parser;

    fn parse(args: &[&str]) -> Command {
        let mut v = vec!["harness"];
        v.extend_from_slice(args);
        crate::cli::Cli::try_parse_from(v).unwrap().command
    }

    fn global(args: &[&str]) -> GlobalArgs {
        let mut v = vec!["harness", "ps"];
        v.extend_from_slice(args);
        crate::cli::Cli::try_parse_from(v).unwrap().global
    }

    #[test]
    fn status_asks_a_running_instance_and_falls_back_to_local_detection() {
        let dir = std::env::temp_dir().join(format!("hs-lemonade-status-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Nobody owns the data directory: local detection.
        assert!(existing_client(&global(&[]), &dir).unwrap().is_none());
        // --host with a token: that instance's Control API.
        let remote = existing_client(&global(&["--host", "127.0.0.1:1", "--token", "t"]), &dir)
            .unwrap()
            .unwrap();
        assert!(remote.base.ends_with("/api/harness/v1"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_text_names_the_lemond_the_instance_started() {
        let mut s: lemonade::LemonadeStatus =
            serde_json::from_value(serde_json::to_value(lemonade::status()).unwrap()).unwrap();
        assert!(status_lines(&s).contains("lemond is not running here"));
        s.running = true;
        s.base_url = Some("http://127.0.0.1:13305".into());
        assert!(status_lines(&s).contains("lemond running at http://127.0.0.1:13305"));
    }

    #[test]
    fn lemonade_commands_parse_and_need_no_instance() {
        assert!(matches!(
            parse(&["lemonade", "status"]),
            Command::Lemonade {
                command: LemonadeCommand::Status
            }
        ));
        let Command::Lemonade {
            command: LemonadeCommand::Pull { model },
        } = parse(&["lemonade", "pull", "Qwen3-0.6B-GGUF"])
        else {
            panic!()
        };
        assert_eq!(model, "Qwen3-0.6B-GGUF");
        let Command::Lemonade {
            command: LemonadeCommand::Backend { recipe, backend },
        } = parse(&["lemonade", "backend", "llamacpp", "rocm"])
        else {
            panic!()
        };
        assert_eq!((recipe.as_str(), backend.as_str()), ("llamacpp", "rocm"));
        assert!(super::super::need(&parse(&["lemonade", "install"])).is_none());
        assert!(crate::cli::Cli::try_parse_from(["harness", "lemonade", "pull"]).is_err());
    }

    #[test]
    fn lemonade_keys_have_english_messages() {
        let e = CliError::core(harness_core::error_key::err("lemonade.not_installed").to_string());
        assert_eq!(e.code, "lemonade.not_installed");
        assert!(
            e.message.contains("harness lemonade install"),
            "{}",
            e.message
        );
    }
}
