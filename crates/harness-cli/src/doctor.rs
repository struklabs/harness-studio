//! `status` and `doctor`: what runs where, and whether the installation works.
//!
//! Both only read. Without a running instance they never take the lock and never write state
//! files: the capability probe of the runtime is not cached to disk here, the settings are
//! only read.

use std::path::Path;

use harness_core::doctor::CheckStatus;
use harness_core::instance::{self, OwnerState};
use serde_json::{json, Value};

use crate::cli::GlobalArgs;
use crate::client::{normalize_base, ApiClient};
use crate::error::{CliError, CliResult, Exit};
use crate::output::{bytes, str_of, Out};
use crate::session::owner_client;

use harness_core::doctor::{owner_json, runtime_json};

pub async fn status(g: &GlobalArgs, data_dir: &Path, out: &Out) -> CliResult<()> {
    if let Some(host) = &g.host {
        let token = g.token.clone().unwrap_or_default();
        let client = ApiClient::new(normalize_base(host)?, token);
        let api = client.get("/status").await?;
        let value = json!({"via": "remote", "api_url": client.base, "api": api});
        out.data(&value, || human_status(&value));
        return Ok(());
    }
    let state = instance::probe(data_dir);
    let mut value = json!({
        "data_dir": data_dir,
        "owner": owner_json(&state),
        "via": Value::Null,
        "api": Value::Null,
    });
    match &state {
        OwnerState::Live(Some(info)) if info.api.is_some() => {
            let client = owner_client(info)?.expect("api");
            value["api"] = client.get("/status").await?;
            value["api_url"] = json!(client.base);
            value["via"] = json!("instance");
        }
        _ => {
            value["runtime"] = tokio::task::spawn_blocking(runtime_json)
                .await
                .unwrap_or(Value::Null);
        }
    }
    out.data(&value, || human_status(&value));
    Ok(())
}

fn human_status(v: &Value) -> String {
    let mut lines = Vec::new();
    if let Some(d) = v["data_dir"].as_str() {
        lines.push(format!("data dir:   {}", d));
    }
    match v["owner"]["state"].as_str() {
        Some("running") => lines.push(format!(
            "instance:   {} (pid {}, {})",
            str_of(&v["owner"]["info"]["kind"]),
            str_of(&v["owner"]["info"]["pid"]),
            str_of(&v["api_url"])
        )),
        Some("running_without_api") => lines.push(format!(
            "instance:   {} (pid {}), Control API off",
            str_of(&v["owner"]["info"]["kind"]),
            str_of(&v["owner"]["info"]["pid"])
        )),
        Some("locked") => lines.push("instance:   locked by another process".into()),
        Some(_) => lines.push("instance:   none running".into()),
        None => lines.push(format!("instance:   remote ({})", str_of(&v["api_url"]))),
    }
    let api = &v["api"];
    if api.is_object() {
        let rt = &api["runtime"];
        lines.push(format!(
            "runtime:    {} ({}){}",
            rt["binary_path"].as_str().unwrap_or("not installed"),
            rt["version"]["raw"].as_str().unwrap_or("-"),
            source_note(rt)
        ));
        let hw = &api["hardware"];
        lines.push(format!(
            "hardware:   {} ({} cores), {} RAM",
            str_of(&hw["cpu_name"]).trim(),
            str_of(&hw["cpu_cores"]),
            bytes(hw["total_ram_bytes"].as_u64().unwrap_or(0))
        ));
        for gpu in hw["gpus"]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or_default()
        {
            lines.push(format!(
                "gpu:        {} ({})",
                str_of(&gpu["name"]),
                gpu["vram_bytes"].as_u64().map(bytes).unwrap_or("-".into())
            ));
        }
        lines.push(format!(
            "servers:    {} running / {} known",
            str_of(&api["servers"]["running"]),
            str_of(&api["servers"]["total"])
        ));
        let gw = &api["gateway"];
        lines.push(match gw["endpoint"]["base_url"].as_str() {
            Some(url) if gw["running"] == true => format!(
                "gateway:    {} (managed endpoint{})",
                url,
                if gw["require_token"] == true {
                    ", token required"
                } else {
                    ""
                }
            ),
            _ => format!(
                "gateway:    off{}",
                gw["error"]
                    .as_str()
                    .map(|e| format!(" ({})", crate::messages::core_text(e)))
                    .unwrap_or_default()
            ),
        });
    } else if v["runtime"].is_object() {
        let rt = &v["runtime"];
        lines.push(format!(
            "runtime:    {}{}",
            rt["binary_path"]
                .as_str()
                .map(str::to_string)
                .or_else(|| rt["error"]
                    .as_str()
                    .map(|e| format!("not available ({})", crate::messages::core_text(e))))
                .unwrap_or_default(),
            source_note(rt)
        ));
    }
    lines.join("\n")
}

/// ` [system llama-server; configured: managed]` when the configured runtime isn't the one used.
fn source_note(rt: &Value) -> String {
    match (rt["source"].as_str(), rt["fallback"] == true) {
        (Some(src), true) => format!(
            " [{} llama-server; configured: {}]",
            src,
            rt["mode"].as_str().unwrap_or("-")
        ),
        (Some(src), false) => format!(" [{}]", src),
        _ => String::new(),
    }
}

// --- doctor ---

pub async fn doctor(data_dir: &Path, out: &Out) -> CliResult<()> {
    let dir = data_dir.to_path_buf();
    let report = tokio::task::spawn_blocking(move || {
        harness_core::doctor::run(&dir, crate::session::VERSION, &[])
    })
    .await
    .map_err(|e| CliError::general("cli.io", e.to_string()))?;
    let value = serde_json::to_value(&report).unwrap_or(Value::Null);
    out.data(&value, || {
        report
            .checks
            .iter()
            .map(|c| {
                format!(
                    "[{}] {:<14} {}",
                    match c.status {
                        CheckStatus::Ok => " ok ",
                        CheckStatus::Warn => "warn",
                        CheckStatus::Fail => "FAIL",
                    },
                    c.id,
                    c.summary
                )
            })
            .collect::<Vec<_>>()
            .join(
                "
",
            )
    });
    if !report.ok {
        // The report is the output; the exit code says a check failed.
        return Err(CliError {
            exit: Exit::General,
            code: "cli.doctor_failed".into(),
            message: "Some checks failed.".into(),
            params: Default::default(),
        });
    }
    Ok(())
}
