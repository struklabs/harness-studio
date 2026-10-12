//! Turning what the user typed (alias, file name, path) into library entries, definitions and
//! running servers; waiting for a server to become ready.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::client::{seg, ApiClient};
use crate::error::{CliError, CliResult, Exit};
use crate::output::Out;

fn eq_ci(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn stem(name: &str) -> &str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".gguf") {
        &name[..name.len() - 5]
    } else {
        name
    }
}

/// Library entries (`GET /models` `entries[]`) matching `query`: id/path, file name, name or
/// stem (case-insensitive). An exact id wins.
pub fn match_entries<'a>(entries: &'a [Value], query: &str) -> Vec<&'a Value> {
    let q = query.trim();
    let exact: Vec<&Value> = entries
        .iter()
        .filter(|e| e["model"]["id"].as_str() == Some(q))
        .collect();
    if !exact.is_empty() {
        return exact;
    }
    let canon = Path::new(q)
        .canonicalize()
        .ok()
        .map(|p| p.to_string_lossy().trim_start_matches(r"\\?\").to_string());
    entries
        .iter()
        .filter(|e| {
            let m = &e["model"];
            let text = |k: &str| m[k].as_str().unwrap_or_default().to_string();
            let path = text("path");
            eq_ci(&text("filename"), q)
                || eq_ci(stem(&text("filename")), q)
                || eq_ci(&text("name"), q)
                || eq_ci(&path, q)
                || canon.as_deref().is_some_and(|c| eq_ci(&path, c))
        })
        .collect()
}

/// Definitions matching `query` by id or alias (case-insensitive).
pub fn match_definitions<'a>(defs: &'a [Value], query: &str) -> Option<&'a Value> {
    defs.iter()
        .find(|d| d["id"].as_str() == Some(query))
        .or_else(|| {
            defs.iter()
                .find(|d| d["alias"].as_str().is_some_and(|a| eq_ci(a, query)))
        })
}

fn array(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}

fn ambiguous(query: &str, candidates: &[&Value]) -> CliError {
    let names: Vec<String> = candidates
        .iter()
        .filter_map(|e| e["model"]["id"].as_str().map(str::to_string))
        .collect();
    CliError::usage(
        "cli.ambiguous",
        format!(
            "\"{}\" matches several models; use the full path:\n  {}",
            query,
            names.join("\n  ")
        ),
    )
    .with("candidates", json!(names))
}

/// A library entry by name (exit 3 when none, 2 when ambiguous).
pub async fn find_entry(client: &ApiClient, query: &str) -> CliResult<Value> {
    let models = client.get("/models").await?;
    let entries = array(&models["entries"]);
    let found = match_entries(&entries, query);
    match found.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(CliError::not_found("model", query)),
        many => Err(ambiguous(query, many)),
    }
}

/// Path of the model a query names: a library entry, a definition's local base or an
/// existing file.
pub async fn model_path(client: &ApiClient, query: &str) -> CliResult<String> {
    let models = client.get("/models").await?;
    let entries = array(&models["entries"]);
    match match_entries(&entries, query).as_slice() {
        [one] => {
            return Ok(one["model"]["path"]
                .as_str()
                .unwrap_or_default()
                .to_string())
        }
        [] => {}
        many => return Err(ambiguous(query, many)),
    }
    let defs = array(&models["definitions"]);
    if let Some(def) = match_definitions(&defs, query) {
        if let Some(p) = def["base"]["path"].as_str() {
            return Ok(p.to_string());
        }
    }
    let p = Path::new(query);
    if p.is_file() {
        return Ok(absolute(p));
    }
    Err(CliError::not_found("model", query))
}

fn absolute(p: &Path) -> String {
    p.canonicalize()
        .map(|c| c.to_string_lossy().trim_start_matches(r"\\?\").to_string())
        .unwrap_or_else(|_| p.to_string_lossy().to_string())
}

/// The model definition to load for `query`: a definition (alias/id), else the definition of a
/// library model or `.gguf` file, created when it has none. Returns the definition.
pub async fn definition_for(client: &ApiClient, query: &str, out: &Out) -> CliResult<Value> {
    let models = client.get("/models").await?;
    let defs = array(&models["definitions"]);
    if let Some(def) = match_definitions(&defs, query) {
        return Ok(def.clone());
    }
    let entries = array(&models["entries"]);
    let path = match match_entries(&entries, query).as_slice() {
        [one] => one["model"]["id"].as_str().unwrap_or_default().to_string(),
        [] => {
            let p = Path::new(query);
            let is_gguf = p
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("gguf"));
            if !(is_gguf && p.is_file()) {
                return Err(CliError::not_found("model", query));
            }
            absolute(p)
        }
        many => return Err(ambiguous(query, many)),
    };
    let same_path = |d: &&Value| {
        d["base"]["kind"] == "local" && d["base"]["path"].as_str().is_some_and(|p| eq_ci(p, &path))
    };
    let mut existing: Vec<&Value> = defs.iter().filter(same_path).collect();
    existing.sort_by_key(|d| d["created_at"].as_u64().unwrap_or(0));
    if let Some(def) = existing.first() {
        return Ok((*def).clone());
    }
    // No definition yet: create one named after the file (unique alias). A model folder
    // (mistral.rs safetensors) keeps its whole name: `Qwen3-0.6B` is not `Qwen3-0` + `.6B`.
    let name = if Path::new(&path).is_dir() {
        Path::new(&path).file_name()
    } else {
        Path::new(&path).file_stem()
    };
    let file_stem = name
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "model".into());
    // `model.onnx` says nothing (every ONNX repo calls its graph that): the model's own name.
    let file_stem = if Path::new(&path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("onnx"))
    {
        harness_core::process::model_alias(Path::new(&path)).unwrap_or(file_stem)
    } else {
        file_stem
    };
    let base_alias = harness_core::domain::sanitize_alias(&file_stem);
    let taken = |a: &str| {
        defs.iter()
            .any(|d| d["alias"].as_str().is_some_and(|x| eq_ci(x, a)))
    };
    let alias = if taken(&base_alias) {
        (2..)
            .map(|n| format!("{}-{}", base_alias, n))
            .find(|a| !taken(a))
            .unwrap_or(base_alias)
    } else {
        base_alias
    };
    let def = client
        .post(
            "/definitions",
            &json!({"alias": alias, "base": {"kind": "local", "path": path}}),
        )
        .await?;
    out.info(format!(
        "Created model definition \"{}\" for {}",
        def["alias"].as_str().unwrap_or_default(),
        path
    ));
    Ok(def)
}

/// Servers (`GET /servers`) running (or starting) a definition.
pub async fn servers_of(client: &ApiClient, def_id: &str) -> CliResult<Vec<Value>> {
    let servers = client.get("/servers").await?;
    Ok(array(&servers)
        .into_iter()
        .filter(|s| s["definition_id"].as_str() == Some(def_id))
        .filter(|s| matches!(s["status"].as_str(), Some("Running") | Some("Starting")))
        .collect())
}

/// A stopped (or failed) instance that last served this definition and is not a profile:
/// loading the definition again reuses it instead of creating `<id>-2`.
pub async fn idle_server_of(client: &ApiClient, def_id: &str) -> CliResult<Option<String>> {
    let servers = client.get("/servers").await?;
    Ok(array(&servers)
        .into_iter()
        .filter(|s| s["definition_id"].as_str() == Some(def_id) && s["is_profile"] != true)
        .find(|s| s["status"] == "Stopped" || s["status"].get("Error").is_some())
        .and_then(|s| s["id"].as_str().map(str::to_string)))
}

/// Polls `GET /servers/{id}` until it runs. A failed start becomes an error with the
/// classified cause (see [`start_failure`]).
pub async fn wait_running(
    client: &ApiClient,
    id: &str,
    timeout: Duration,
    out: &Out,
) -> CliResult<Value> {
    // 0 = no own limit: the server's load timeout (at most 2 h) ends a start that hangs.
    let timeout = if timeout.is_zero() {
        Duration::from_secs(7300)
    } else {
        timeout
    };
    let started = Instant::now();
    let path = format!("/servers/{}", seg(id));
    let mut last_note = Instant::now();
    // In place on a terminal; a line every 10 s in logs.
    let every = if std::io::IsTerminal::is_terminal(&std::io::stderr()) {
        Duration::from_millis(900)
    } else {
        Duration::from_secs(10)
    };
    loop {
        let view = client.get(&path).await?;
        match &view["status"] {
            Value::String(s) if s == "Running" => {
                out.progress_done();
                return Ok(view);
            }
            Value::String(s) if s == "Starting" => {}
            status => {
                out.progress_done();
                return Err(start_failure(client, id, status, &view, out).await);
            }
        }
        if started.elapsed() > timeout {
            out.progress_done();
            return Err(CliError::new(
                Exit::General,
                "cli.timeout",
                format!("Server {} was not ready after {} s", id, timeout.as_secs()),
            )
            .with("server_id", id));
        }
        if last_note.elapsed() > every {
            last_note = Instant::now();
            out.progress(&format!(
                "Loading {}... {:.0}s",
                id,
                started.elapsed().as_secs_f32()
            ));
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

/// The error of a failed start: the server's classified failure (`servers.out_of_memory` →
/// exit 5, `servers.port_in_use` → 4, `servers.model_not_found` → 3, others 1) with the log
/// tail, and after an out-of-memory failure the recovery options (`params.recovery`; human
/// output prints them as `harness retry` commands). Servers without a classified failure
/// (older instance) fall back to `cli.out_of_memory` / `cli.server_failed`.
async fn start_failure(
    client: &ApiClient,
    id: &str,
    status: &Value,
    view: &Value,
    out: &Out,
) -> CliError {
    let message = match status {
        Value::Object(m) => m
            .get("Error")
            .and_then(Value::as_str)
            .map_or_else(|| "stopped".to_string(), crate::messages::core_text),
        other => crate::output::str_of(other),
    };
    let logs = client
        .get(&format!("/servers/{}/logs?limit=60", seg(id)))
        .await
        .ok()
        .map(|l| array(&l["lines"]))
        .unwrap_or_default();
    let tail: Vec<String> = logs
        .iter()
        .filter_map(|l| l.as_str().map(str::to_string))
        .collect();
    let failure = &view["failure"];
    let mut err = match failure["code"].as_str() {
        Some(code) => {
            let params = failure["params"].as_object().cloned().unwrap_or_default();
            let exit = match failure["kind"].as_str() {
                Some("out_of_memory") => Exit::OutOfMemory,
                Some("port_in_use") => Exit::Conflict,
                Some("model_not_found") => Exit::NotFound,
                _ => Exit::General,
            };
            CliError::from_code(exit, code, params).with("failure", failure.clone())
        }
        None => {
            let oom = crate::messages::mentions_out_of_memory(&message)
                || tail
                    .iter()
                    .any(|l| crate::messages::mentions_out_of_memory(l));
            let (exit, code) = if oom {
                (Exit::OutOfMemory, "cli.out_of_memory")
            } else {
                (Exit::General, "cli.server_failed")
            };
            let mut e = CliError::from_code(exit, code, Default::default());
            e.message = format!("{} {}", e.message, message);
            e
        }
    };
    err = err
        .with("server_id", id)
        .with("status_message", message)
        .with(
            "log_tail",
            json!(tail.iter().rev().take(15).rev().collect::<Vec<_>>()),
        );
    let mut hints = Vec::new();
    if err.exit == Exit::OutOfMemory {
        if let Ok(plan) = client.get(&format!("/servers/{}/recovery", seg(id))).await {
            for o in array(&plan["options"]) {
                let label = crate::messages::template(o["label_key"].as_str().unwrap_or_default())
                    .map(|t| {
                        crate::messages::fill(
                            t,
                            o["params"].as_object().unwrap_or(&Default::default()),
                        )
                    })
                    .unwrap_or_default();
                let warning = o["warning_key"]
                    .as_str()
                    .and_then(crate::messages::template)
                    .map(|t| {
                        format!(
                            " ({})",
                            crate::messages::fill(
                                t,
                                o["params"].as_object().unwrap_or(&Default::default()),
                            )
                        )
                    })
                    .unwrap_or_default();
                hints.push(format!(
                    "  harness retry {} --option {}{}   # {}{}",
                    id,
                    crate::output::str_of(&o["id"]),
                    if o["recommended"] == true {
                        " (recommended)"
                    } else {
                        ""
                    },
                    label,
                    warning
                ));
            }
            err = err.with("recovery", plan["options"].clone());
        }
    }
    if !out.json {
        if !tail.is_empty() {
            eprintln!("--- last log lines of {} ---", id);
            for l in tail.iter().rev().take(15).rev() {
                eprintln!("{}", l);
            }
        }
        if !hints.is_empty() {
            eprintln!("--- ways to retry ---");
            for h in &hints {
                eprintln!("{}", h);
            }
        }
    }
    err
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_match_by_name_file_and_path() {
        let entries = vec![
            json!({"model": {"id": "/m/a/Qwen3-4B-Q4_K_M.gguf", "path": "/m/a/Qwen3-4B-Q4_K_M.gguf",
                "filename": "Qwen3-4B-Q4_K_M.gguf", "name": "Qwen3 4B"}}),
            json!({"model": {"id": "/m/b/Qwen3-4B-Q4_K_M.gguf", "path": "/m/b/Qwen3-4B-Q4_K_M.gguf",
                "filename": "Qwen3-4B-Q4_K_M.gguf", "name": "Qwen3 4B b"}}),
            json!({"model": {"id": "/m/c/tiny.gguf", "path": "/m/c/tiny.gguf",
                "filename": "tiny.gguf", "name": "tiny"}}),
        ];
        assert_eq!(match_entries(&entries, "tiny").len(), 1);
        assert_eq!(match_entries(&entries, "TINY.gguf").len(), 1);
        assert_eq!(match_entries(&entries, "qwen3-4b-q4_k_m").len(), 2);
        assert_eq!(
            match_entries(&entries, "/m/b/Qwen3-4B-Q4_K_M.gguf").len(),
            1
        );
        assert!(match_entries(&entries, "nope").is_empty());
        let defs = vec![
            json!({"id": "d1", "alias": "Qwen"}),
            json!({"id": "d2", "alias": "x"}),
        ];
        assert_eq!(match_definitions(&defs, "qwen").unwrap()["id"], "d1");
        assert_eq!(match_definitions(&defs, "d2").unwrap()["alias"], "x");
        assert!(match_definitions(&defs, "y").is_none());
    }
}
