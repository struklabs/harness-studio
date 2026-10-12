//! Turning what an agent names (alias, file name, path, server id) into library entries,
//! definitions and servers, and waiting for long operations. Same rules as the `harness` CLI
//! (`crates/harness-cli/src/commands/resolve.rs`): an exact id wins, then file name, file
//! stem, display name or path (case-insensitive); a model without a definition gets one named
//! after its file.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::{Backend, BackendError, BackendResult, Method};

/// Percent-encodes one path segment (model ids are file paths).
pub fn seg(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

pub async fn get(b: &dyn Backend, path: &str) -> BackendResult<Value> {
    b.request(Method::Get, path, None).await
}

pub async fn post(b: &dyn Backend, path: &str, body: Value) -> BackendResult<Value> {
    b.request(Method::Post, path, Some(body)).await
}

pub fn array(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}

fn eq_ci(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn stem(name: &str) -> &str {
    if name.to_ascii_lowercase().ends_with(".gguf") {
        &name[..name.len() - 5]
    } else {
        name
    }
}

fn normalize_path(p: &str) -> String {
    p.replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

/// Library entries (`GET /models` `entries[]`) matching `query`.
pub fn match_entries<'a>(entries: &'a [Value], query: &str) -> Vec<&'a Value> {
    let q = query.trim();
    let exact: Vec<&Value> = entries
        .iter()
        .filter(|e| e["model"]["id"].as_str() == Some(q))
        .collect();
    if !exact.is_empty() {
        return exact;
    }
    let qpath = normalize_path(q);
    entries
        .iter()
        .filter(|e| {
            let m = &e["model"];
            let text = |k: &str| m[k].as_str().unwrap_or_default().to_string();
            eq_ci(&text("filename"), q)
                || eq_ci(stem(&text("filename")), q)
                || eq_ci(&text("name"), q)
                || normalize_path(&text("path")) == qpath
        })
        .collect()
}

/// A definition by id or alias (case-insensitive).
pub fn match_definition<'a>(defs: &'a [Value], query: &str) -> Option<&'a Value> {
    let q = query.trim();
    defs.iter()
        .find(|d| d["id"].as_str() == Some(q))
        .or_else(|| {
            defs.iter()
                .find(|d| d["alias"].as_str().is_some_and(|a| eq_ci(a, q)))
        })
}

fn not_found(what: &str, query: &str) -> BackendError {
    BackendError::new(
        "api.not_found",
        format!(
            "No {} matches \"{}\". Call list_models to see what is available.",
            what, query
        ),
    )
    .with("kind", what)
    .with("id", query)
}

fn ambiguous(query: &str, candidates: &[&Value]) -> BackendError {
    let ids: Vec<String> = candidates
        .iter()
        .filter_map(|e| e["model"]["id"].as_str().map(str::to_string))
        .collect();
    BackendError::new(
        "mcp.ambiguous_model",
        format!(
            "\"{}\" matches several models; pass one of these ids instead.",
            query
        ),
    )
    .with("candidates", json!(ids))
}

/// One library entry for `query` (an alias resolves to its definition's local file).
pub async fn find_entry(b: &dyn Backend, query: &str) -> BackendResult<Value> {
    let models = get(b, "/models").await?;
    let entries = array(&models["entries"]);
    match match_entries(&entries, query).as_slice() {
        [one] => return Ok((*one).clone()),
        [] => {}
        many => return Err(ambiguous(query, many)),
    }
    let defs = array(&models["definitions"]);
    if let Some(path) = match_definition(&defs, query).and_then(|d| d["base"]["path"].as_str()) {
        if let [one] = match_entries(&entries, path).as_slice() {
            return Ok((*one).clone());
        }
    }
    Err(not_found("model", query))
}

/// The model file path for `query`: a library entry, a definition's local file or an existing
/// `.gguf` path.
pub async fn model_path(b: &dyn Backend, query: &str) -> BackendResult<String> {
    match find_entry(b, query).await {
        Ok(entry) => Ok(entry["model"]["path"]
            .as_str()
            .unwrap_or_default()
            .to_string()),
        Err(e) if e.code == "api.not_found" && is_gguf_file(query) => Ok(absolute(query)),
        Err(e) => Err(e),
    }
}

fn is_gguf_file(query: &str) -> bool {
    let p = Path::new(query);
    p.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
        && p.is_file()
}

fn absolute(p: &str) -> String {
    Path::new(p)
        .canonicalize()
        .map(|c| c.to_string_lossy().trim_start_matches(r"\\?\").to_string())
        .unwrap_or_else(|_| p.to_string())
}

/// The definition to load for `query`: an existing definition (alias/id), else the definition
/// of a library model or `.gguf` file, created (alias = file stem, made unique) when it has
/// none. Returns `(definition, created)`.
pub async fn definition_for(b: &dyn Backend, query: &str) -> BackendResult<(Value, bool)> {
    let models = get(b, "/models").await?;
    let defs = array(&models["definitions"]);
    if let Some(def) = match_definition(&defs, query) {
        return Ok((def.clone(), false));
    }
    let entries = array(&models["entries"]);
    let path = match match_entries(&entries, query).as_slice() {
        [one] => one["model"]["id"].as_str().unwrap_or_default().to_string(),
        [] if is_gguf_file(query) => absolute(query),
        [] => return Err(not_found("model or definition", query)),
        many => return Err(ambiguous(query, many)),
    };
    let same_path = |d: &&Value| {
        d["base"]["kind"] == "local"
            && d["base"]["path"]
                .as_str()
                .is_some_and(|p| normalize_path(p) == normalize_path(&path))
    };
    let mut existing: Vec<&Value> = defs.iter().filter(same_path).collect();
    existing.sort_by_key(|d| d["created_at"].as_u64().unwrap_or(0));
    if let Some(def) = existing.first() {
        return Ok(((*def).clone(), false));
    }
    let file_stem = Path::new(&path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "model".into());
    // `model.onnx` names nothing: the repo folder does.
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
    let def = post(
        b,
        "/definitions",
        json!({"alias": alias, "base": {"kind": "local", "path": path}}),
    )
    .await?;
    Ok((def, true))
}

pub fn is_active(s: &Value) -> bool {
    matches!(
        s["status"].as_str(),
        Some("Running") | Some("Starting") | Some("Stopping")
    )
}

/// A stopped (or failed) non-profile instance that last served this definition: loading it
/// again reuses it instead of creating `<id>-2`.
pub async fn idle_server_of(b: &dyn Backend, def_id: &str) -> BackendResult<Option<String>> {
    let servers = get(b, "/servers").await?;
    Ok(array(&servers)
        .into_iter()
        .filter(|s| s["definition_id"].as_str() == Some(def_id) && s["is_profile"] != true)
        .find(|s| s["status"] == "Stopped" || s["status"].get("Error").is_some())
        .and_then(|s| s["id"].as_str().map(str::to_string)))
}

/// Running/starting servers that serve `query` (definition alias/id, library model or server
/// id).
pub async fn servers_for(b: &dyn Backend, query: &str) -> BackendResult<Vec<Value>> {
    let servers = array(&get(b, "/servers").await?);
    if let Some(s) = servers.iter().find(|s| s["id"].as_str() == Some(query)) {
        return Ok(vec![s.clone()]);
    }
    let models = get(b, "/models").await?;
    let defs = array(&models["definitions"]);
    let def_ids: Vec<String> = match match_definition(&defs, query) {
        Some(d) => vec![d["id"].as_str().unwrap_or_default().to_string()],
        None => {
            let entries = array(&models["entries"]);
            let mut paths: Vec<String> = match_entries(&entries, query)
                .iter()
                .filter_map(|e| e["model"]["path"].as_str().map(normalize_path))
                .collect();
            // A .gguf path outside the library: its definitions point at it directly.
            paths.push(normalize_path(query));
            defs.iter()
                .filter(|d| {
                    d["base"]["path"]
                        .as_str()
                        .is_some_and(|p| paths.contains(&normalize_path(p)))
                })
                .filter_map(|d| d["id"].as_str().map(str::to_string))
                .collect()
        }
    };
    Ok(servers
        .into_iter()
        .filter(is_active)
        .filter(|s| {
            s["definition_id"]
                .as_str()
                .is_some_and(|d| def_ids.iter().any(|x| x == d))
                || s["served_alias"]
                    .as_str()
                    .is_some_and(|a| a.eq_ignore_ascii_case(query))
        })
        .collect())
}

/// Polls `GET /servers/{id}` until it runs. A failed start is an error with the classified
/// failure, the log tail and (after running out of memory) the recovery options.
pub async fn wait_running(b: &dyn Backend, id: &str, timeout: Duration) -> BackendResult<Value> {
    let started = Instant::now();
    let path = format!("/servers/{}", seg(id));
    loop {
        let view = get(b, &path).await?;
        match &view["status"] {
            Value::String(s) if s == "Running" => return Ok(view),
            Value::String(s) if s == "Starting" => {}
            status => return Err(start_failure(b, id, status, &view).await),
        }
        if started.elapsed() > timeout {
            return Err(BackendError::new(
                "mcp.timeout",
                format!(
                    "Server {} was not ready after {} s; it keeps loading. Check list_servers or get_logs.",
                    id,
                    timeout.as_secs()
                ),
            )
            .with("server_id", id));
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
}

async fn start_failure(b: &dyn Backend, id: &str, status: &Value, view: &Value) -> BackendError {
    let message = match status {
        Value::Object(m) => m
            .get("Error")
            .and_then(Value::as_str)
            .unwrap_or("stopped")
            .to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let tail: Vec<Value> = get(b, &format!("/servers/{}/logs?limit=20", seg(id)))
        .await
        .map(|l| array(&l["lines"]))
        .unwrap_or_default();
    let failure = &view["failure"];
    let code = failure["code"]
        .as_str()
        .unwrap_or("mcp.server_failed")
        .to_string();
    let mut err = BackendError::new(code, format!("Server {} failed to start: {}", id, message))
        .with("server_id", id)
        .with("failure", failure.clone())
        .with("log_tail", Value::Array(tail));
    if failure["kind"] == "out_of_memory" {
        if let Ok(plan) = get(b, &format!("/servers/{}/recovery", seg(id))).await {
            err = err.with("recovery_options", plan["options"].clone());
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
            json!({"model": {"id": "C:\\m\\a\\Qwen3-4B-Q4_K_M.gguf", "path": "C:\\m\\a\\Qwen3-4B-Q4_K_M.gguf",
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
            match_entries(&entries, "c:/m/a/qwen3-4b-q4_k_m.gguf").len(),
            1
        );
        assert!(match_entries(&entries, "nope").is_empty());
        let defs = vec![
            json!({"id": "d1", "alias": "Qwen"}),
            json!({"id": "d2", "alias": "x"}),
        ];
        assert_eq!(match_definition(&defs, "qwen").unwrap()["id"], "d1");
        assert_eq!(match_definition(&defs, "d2").unwrap()["alias"], "x");
        assert!(match_definition(&defs, "y").is_none());
        assert_eq!(seg("C:\\m\\a b.gguf"), "C%3A%5Cm%5Ca%20b.gguf");
    }
}
