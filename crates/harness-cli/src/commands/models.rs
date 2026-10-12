//! `ls`, `show`, `search`, `pull`, `rm`.

use std::time::{Duration, Instant};

use harness_core::downloader::{
    DownloadManager, DownloadProgress, DownloadStatus, HfSearchOptions,
};
use serde_json::{json, Value};

use super::resolve::{find_entry, match_definitions};
use crate::client::{seg, ApiClient};
use crate::error::{CliError, CliResult, Exit};
use crate::output::{bytes, str_of, table, Out};

pub async fn ls(client: &ApiClient, out: &Out) -> CliResult<()> {
    let models = client.get("/models").await?;
    out.data(&models, || {
        let defs = models["definitions"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let rows: Vec<Vec<String>> = models["entries"]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|e| {
                let m = &e["model"];
                let id = m["id"].as_str().unwrap_or_default();
                let aliases: Vec<String> = defs
                    .iter()
                    .filter(|d| d["base"]["path"].as_str() == Some(id))
                    .filter_map(|d| d["alias"].as_str().map(str::to_string))
                    .collect();
                vec![
                    str_of(&m["filename"]),
                    str_of(&m["kind"]).to_lowercase(),
                    str_of(&m["quantization"]),
                    bytes(m["size_bytes"].as_u64().unwrap_or(0)),
                    if aliases.is_empty() {
                        "-".into()
                    } else {
                        aliases.join(", ")
                    },
                ]
            })
            .collect();
        if rows.is_empty() {
            return "No local models. Download one with `harness pull <owner/repo[:quant]>`."
                .into();
        }
        table(&["FILE", "KIND", "QUANT", "SIZE", "DEFINITIONS"], &rows)
    });
    Ok(())
}

pub async fn show(client: &ApiClient, query: &str, out: &Out) -> CliResult<()> {
    let models = client.get("/models").await?;
    let defs = models["definitions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let (entry, definitions) = match find_entry(client, query).await {
        Ok(entry) => {
            let id = entry["model"]["id"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let d: Vec<Value> = defs
                .iter()
                .filter(|d| d["base"]["path"].as_str() == Some(id.as_str()))
                .cloned()
                .collect();
            (entry, d)
        }
        Err(e) if e.exit == Exit::NotFound => {
            let def = match_definitions(&defs, query)
                .cloned()
                .ok_or_else(|| CliError::not_found("model", query))?;
            let path = def["base"]["path"].as_str().unwrap_or_default().to_string();
            let entry = models["entries"]
                .as_array()
                .and_then(|a| a.iter().find(|e| e["model"]["id"].as_str() == Some(&path)))
                .cloned()
                .unwrap_or(Value::Null);
            (entry, vec![def])
        }
        Err(e) => return Err(e),
    };
    // What a default load starts with (the launch's own rule); older APIs lack it.
    let automatic = match entry["model"]["id"].as_str() {
        Some(id) => client
            .get(&format!("/models/{}/automatic", seg(id)))
            .await
            .unwrap_or(Value::Null),
        None => Value::Null,
    };
    // Memory estimate of this machine for that same load, so the two lines agree (without it:
    // default context, full offload); older APIs lack it.
    let fit = match entry["model"]["id"].as_str() {
        Some(id) => client
            .get(&format!("/models/{}/fit{}", seg(id), fit_query(&automatic)))
            .await
            .unwrap_or(Value::Null),
        None => Value::Null,
    };
    let result =
        json!({"model": entry, "definitions": definitions, "fit": fit, "automatic": automatic});
    out.data(&result, || {
        let m = &entry["model"];
        let mut lines = Vec::new();
        if !m.is_null() {
            lines.push(format!("file:          {}", str_of(&m["path"])));
            lines.push(format!(
                "kind:          {}",
                str_of(&m["kind"]).to_lowercase()
            ));
            lines.push(format!("architecture:  {}", str_of(&m["architecture"])));
            lines.push(format!("quantization:  {}", str_of(&m["quantization"])));
            lines.push(format!(
                "size:          {}",
                bytes(m["size_bytes"].as_u64().unwrap_or(0))
            ));
            if let Some(ctx) = m["context_length"].as_u64() {
                lines.push(format!("context:       {}", ctx));
            }
            if let Some(p) = entry["params"]["label"].as_str() {
                lines.push(format!("parameters:    {}", p));
            } else if let Some(n) = entry["params"]["count"].as_u64() {
                lines.push(format!("parameters:    {}", n));
            }
            if m["is_split"] == true {
                lines.push(format!(
                    "shards:        {}/{}",
                    str_of(&m["present_shards"]),
                    str_of(&m["total_shards"])
                ));
            }
        } else {
            lines.push("file:          (not in the library)".into());
        }
        if let Some(vram) = fit["estimate"]["vram_bytes"].as_u64() {
            let budget = fit["budget"]["vram_bytes"].as_u64().unwrap_or(0);
            lines.push(format!(
                "memory:        {} VRAM / {} ({}%), {} RAM, {} [{}], -ngl {}",
                bytes(vram),
                bytes(budget),
                str_of(&fit["vram_percentage"]),
                bytes(fit["estimate"]["ram_bytes"].as_u64().unwrap_or(0)),
                str_of(&fit["category"]),
                str_of(&fit["source"]),
                str_of(&fit["recommended_gpu_layers"]),
            ));
        }
        if let Some(line) = automatic_line(&automatic) {
            lines.push(line);
        }
        for d in &definitions {
            lines.push(format!(
                "definition:    {} ({}) -> {}",
                str_of(&d["alias"]),
                str_of(&d["id"]),
                str_of(&d["base"]["path"])
            ));
        }
        lines.join("\n")
    });
    Ok(())
}

/// The `/fit` query of the default load (`?context_size=..&gpu_layers=..`), empty without one.
pub fn fit_query(a: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(n) = a["context"].as_u64() {
        parts.push(format!("context_size={n}"));
    }
    if let Some(n) = a["layers"].as_u64() {
        parts.push(format!("gpu_layers={n}"));
    }
    if let Some(n) = a["slots"].as_u64() {
        parts.push(format!("parallel_slots={n}"));
    }
    if let (Some(k), Some(v)) = (a["cache_type_k"].as_str(), a["cache_type_v"].as_str()) {
        parts.push(format!("cache_type_k={}&cache_type_v={}", seg(k), seg(v)));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    }
}

/// The default load as the launch runs it: `-c 24576 -ngl 37 -np 2, KV q8_0/q8_0, flash attention`.
pub fn automatic_line(a: &Value) -> Option<String> {
    let context = a["context"].as_u64()?;
    let mut parts = vec![format!("-c {context}")];
    if let Some(n) = a["layers"].as_u64() {
        parts.push(format!("-ngl {n}"));
    }
    if let Some(n) = a["slots"].as_u64() {
        parts.push(format!("-np {n}"));
    }
    if let (Some(k), Some(v)) = (a["cache_type_k"].as_str(), a["cache_type_v"].as_str()) {
        parts.push(format!("KV {k}/{v}"));
    }
    parts.push(format!(
        "flash attention {}",
        if a["flash_attn"] == true {
            "on"
        } else {
            "auto"
        }
    ));
    Some(format!("default load:  {}", parts.join(", ")))
}

/// Hugging Face search (no instance needed: it only reads the HF token).
pub async fn search(query: &str, limit: u32, sort: Option<String>, out: &Out) -> CliResult<()> {
    let token = harness_core::hf_auth::current_token().await;
    let options = HfSearchOptions {
        query: query.to_string(),
        sort,
        limit: Some(limit),
        ..Default::default()
    };
    let results = DownloadManager::search_models(&options, token.as_deref())
        .await
        .map_err(|e| {
            let mut err = CliError::core(e);
            if err.code == "core.error" || err.code == "hf.search_failed" {
                err.exit = Exit::Connection;
            }
            err
        })?;
    let value = serde_json::to_value(&results).unwrap_or(Value::Null);
    out.data(&value, || {
        if results.is_empty() {
            return "No GGUF models found.".into();
        }
        let rows: Vec<Vec<String>> = results
            .iter()
            .map(|r| {
                vec![
                    r.id.clone(),
                    r.downloads.to_string(),
                    r.likes.to_string(),
                    r.quants.join(","),
                ]
            })
            .collect();
        table(&["REPO", "DOWNLOADS", "LIKES", "QUANTS"], &rows)
    });
    Ok(())
}

/// `owner/repo[:quant]`, `hf.co/owner/repo[:quant]` or a huggingface.co URL.
pub fn parse_pull_ref(raw: &str) -> CliResult<(String, Option<String>)> {
    let mut s = raw.trim();
    for prefix in [
        "https://huggingface.co/",
        "http://huggingface.co/",
        "huggingface.co/",
        "https://hf.co/",
        "hf.co/",
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest;
            break;
        }
    }
    let (repo, quant) = match s.split_once(':') {
        Some((r, q)) if !q.is_empty() => (r, Some(q.to_string())),
        Some((r, _)) => (r, None),
        None => (s, None),
    };
    // URLs may carry `/tree/main` or `/blob/...`: keep owner/repo.
    let parts: Vec<&str> = repo.trim_matches('/').split('/').collect();
    if parts.len() < 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(CliError::usage(
            "cli.invalid_argument",
            format!("Expected owner/repo[:quant], got \"{}\"", raw),
        ));
    }
    Ok((format!("{}/{}", parts[0], parts[1]), quant))
}

fn progress_line(p: &DownloadProgress) -> String {
    let pct = if p.total_bytes > 0 {
        p.downloaded_bytes as f64 * 100.0 / p.total_bytes as f64
    } else {
        0.0
    };
    let status = match &p.status {
        DownloadStatus::Queued => "queued".to_string(),
        DownloadStatus::Verifying => "verifying".to_string(),
        _ => format!(
            "{:5.1}%  {} / {}  {}/s",
            pct,
            bytes(p.downloaded_bytes),
            bytes(p.total_bytes),
            bytes(p.speed_bytes_per_sec)
        ),
    };
    let shard = if p.total_shards > 1 {
        format!(" [{}/{}]", p.current_shard_index, p.total_shards)
    } else {
        String::new()
    };
    format!("pulling {}{}  {}", p.variant_name, shard, status)
}

pub async fn pull(
    client: &ApiClient,
    model: &str,
    file: Option<String>,
    detach: bool,
    out: &Out,
) -> CliResult<()> {
    let (repo, quant) = parse_pull_ref(model)?;
    out.info(format!("Resolving {}...", repo));
    let started = client
        .post(
            "/models/pull",
            &json!({"repo": repo, "quant": quant, "file": file}),
        )
        .await?;
    let job = started["job_id"].as_u64().unwrap_or(0);
    if detach {
        out.data(&started, || {
            format!(
                "Queued {} ({}), download job {}",
                str_of(&started["variant"]["name"]),
                bytes(started["variant"]["total_size_bytes"].as_u64().unwrap_or(0)),
                job
            ) + &started["companions"]
                .as_array()
                .filter(|a| !a.is_empty())
                .map(|a| format!(" and {} companion file(s) (VAE, text encoders)", a.len()))
                .unwrap_or_default()
        });
        return Ok(());
    }
    let path = format!("/downloads/{}", job);
    let mut last = Instant::now() - Duration::from_secs(10);
    let final_progress = loop {
        let wait = tokio::time::sleep(Duration::from_millis(500));
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                out.progress_done();
                let _ = client.post(&format!("{}/pause", path), &json!({})).await;
                out.info(format!("Paused download job {} (run pull again to resume).", job));
                return Err(CliError::from_code(Exit::General, "cli.aborted", Default::default()));
            }
            _ = wait => {}
        }
        let p: DownloadProgress = client.get_as(&path).await?;
        match &p.status {
            DownloadStatus::Completed => break p,
            DownloadStatus::Error(msg) => {
                out.progress_done();
                return Err(CliError::new(
                    Exit::General,
                    "cli.download_failed",
                    format!("Download failed: {}", crate::messages::core_text(msg)),
                )
                .with("job_id", job));
            }
            DownloadStatus::Paused | DownloadStatus::Idle => {
                out.progress_done();
                return Err(CliError::new(
                    Exit::General,
                    "cli.download_failed",
                    "The download was paused by someone else",
                )
                .with("job_id", job));
            }
            _ => {
                let interval = if out.json || !std::io::IsTerminal::is_terminal(&std::io::stderr())
                {
                    Duration::from_secs(5)
                } else {
                    Duration::from_millis(400)
                };
                if last.elapsed() >= interval {
                    last = Instant::now();
                    out.progress(&progress_line(&p));
                }
            }
        }
    };
    // The VAE / text encoders a known image model needs come with it.
    let companions: Vec<u64> = started["companions"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_default();
    for id in &companions {
        let path = format!("/downloads/{}", id);
        loop {
            tokio::time::sleep(Duration::from_millis(700)).await;
            let p: DownloadProgress = client.get_as(&path).await?;
            match &p.status {
                DownloadStatus::Completed => break,
                DownloadStatus::Error(msg) => {
                    out.progress_done();
                    return Err(CliError::new(
                        Exit::General,
                        "cli.download_failed",
                        format!("Download failed: {}", crate::messages::core_text(msg)),
                    )
                    .with("job_id", *id));
                }
                DownloadStatus::Paused | DownloadStatus::Idle => {
                    out.progress_done();
                    return Err(CliError::new(
                        Exit::General,
                        "cli.download_failed",
                        "The download was paused by someone else",
                    )
                    .with("job_id", *id));
                }
                _ => out.progress(&progress_line(&p)),
            }
        }
    }
    out.progress_done();
    let result = json!({
        "job_id": job,
        "variant": started["variant"],
        "companions": companions,
        "download": final_progress,
    });
    out.data(&result, || {
        format!(
            "Downloaded {} ({})",
            final_progress.variant_name,
            bytes(final_progress.total_bytes)
        )
    });
    Ok(())
}

pub async fn rm(client: &ApiClient, query: &str, definitions: bool, out: &Out) -> CliResult<()> {
    let entry = find_entry(client, query).await?;
    let id = entry["model"]["id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    out.confirm(&format!(
        "Delete {} ({})?",
        id,
        bytes(entry["model"]["size_bytes"].as_u64().unwrap_or(0))
    ))?;
    let report = client
        .delete(&format!(
            "/models/{}?delete_definitions={}",
            seg(&id),
            definitions
        ))
        .await?;
    out.data(&report, || format!("Deleted {}", id));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_refs() {
        assert_eq!(
            parse_pull_ref("unsloth/Qwen3-4B-GGUF:Q4_K_M").unwrap(),
            ("unsloth/Qwen3-4B-GGUF".into(), Some("Q4_K_M".into()))
        );
        assert_eq!(parse_pull_ref("hf.co/a/b").unwrap(), ("a/b".into(), None));
        assert_eq!(
            parse_pull_ref("https://huggingface.co/a/b/tree/main").unwrap(),
            ("a/b".into(), None)
        );
        assert_eq!(parse_pull_ref("nope").unwrap_err().exit, Exit::Usage);
    }

    #[test]
    fn show_prints_the_automatic_launch_not_a_fixed_default() {
        let line = automatic_line(&json!({
            "context": 24576, "layers": 37, "slots": 2,
            "cache_type_k": "q8_0", "cache_type_v": "q8_0", "flash_attn": true
        }))
        .unwrap();
        assert_eq!(
            line,
            "default load:  -c 24576, -ngl 37, -np 2, KV q8_0/q8_0, flash attention on"
        );
        assert_eq!(automatic_line(&Value::Null), None);
    }

    #[test]
    fn show_estimates_the_memory_of_the_default_load() {
        let query = fit_query(&json!({
            "context": 12288, "layers": 37, "slots": 1,
            "cache_type_k": "q8_0", "cache_type_v": "q8_0", "flash_attn": true
        }));
        assert_eq!(
            query,
            "?context_size=12288&gpu_layers=37&parallel_slots=1&cache_type_k=q8_0&cache_type_v=q8_0"
        );
        assert_eq!(fit_query(&Value::Null), "");
    }
}
