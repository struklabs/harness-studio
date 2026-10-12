//! `profile ...`, `benchmark ...`.

use std::path::Path;
use std::time::{Duration, Instant};

use harness_core::benchmark::{BenchResult, BenchmarkRequest};
use serde_json::{json, Value};

use super::resolve::{model_path, wait_running};
use super::servers::server_rows;
use crate::client::{seg, ApiClient};
use crate::error::{CliError, CliResult, Exit};
use crate::output::{status_text, str_of, table, Out};

// --- Profiles ---

pub const PROFILE_FORMAT: &str = "harness-profile";

/// `host:port` of a saved launch; `host:auto` when Harness picks the port at start.
fn address(config: &Value) -> String {
    let auto = config["port_auto"] == true || config["port"].as_u64().unwrap_or(0) == 0;
    let port = if auto {
        "auto".to_string()
    } else {
        str_of(&config["port"])
    };
    format!("{}:{}", str_of(&config["host"]), port)
}

async fn profiles(client: &ApiClient) -> CliResult<Vec<Value>> {
    Ok(client
        .get("/servers")
        .await?
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s["is_profile"] == true)
        .collect())
}

async fn profile_view(client: &ApiClient, id: &str) -> CliResult<Value> {
    let view = client.get(&format!("/servers/{}", seg(id))).await?;
    if view["is_profile"] != true {
        return Err(CliError::not_found("profile", id));
    }
    Ok(view)
}

pub async fn profile_ls(client: &ApiClient, out: &Out) -> CliResult<()> {
    let list = profiles(client).await?;
    let value = Value::Array(list.clone());
    out.data(&value, || {
        if list.is_empty() {
            return "No server profiles.".into();
        }
        let rows: Vec<Vec<String>> = list
            .iter()
            .map(|p| {
                let prof = &p["profile"];
                vec![
                    str_of(&p["id"]),
                    str_of(&p["name"]),
                    status_text(&p["status"]),
                    p["served_alias"].as_str().unwrap_or("-").to_string(),
                    address(&prof["config"]),
                    if prof["auto_start"] == true {
                        "yes".into()
                    } else {
                        "no".into()
                    },
                ]
            })
            .collect();
        table(
            &["ID", "NAME", "STATUS", "MODEL", "ADDRESS", "AUTOSTART"],
            &rows,
        )
    });
    Ok(())
}

pub async fn profile_show(client: &ApiClient, id: &str, out: &Out) -> CliResult<()> {
    let view = profile_view(client, id).await?;
    out.data(&view, || {
        let p = &view["profile"];
        let c = &p["config"];
        let mut lines = vec![
            format!("id:          {}", str_of(&view["id"])),
            format!("name:        {}", str_of(&view["name"])),
            format!("status:      {}", status_text(&view["status"])),
            format!("model:       {}", str_of(&c["model_path"])),
            format!("address:     {}", address(c)),
            format!("auto start:  {}", p["auto_start"] == true),
        ];
        if let Some(e) = view["endpoint"].as_str() {
            lines.push(format!("endpoint:    {}", e));
        }
        if let Some(d) = p["description"].as_str().filter(|d| !d.is_empty()) {
            lines.push(format!("description: {}", d));
        }
        lines.join("\n")
    });
    Ok(())
}

pub async fn profile_run(
    client: &ApiClient,
    id: &str,
    no_wait: bool,
    timeout: u64,
    out: &Out,
) -> CliResult<()> {
    profile_view(client, id).await?;
    out.info(format!("Starting {}...", id));
    let started = client
        .post(&format!("/servers/{}/start", seg(id)), &json!({}))
        .await?;
    let view = if no_wait {
        started
    } else {
        wait_running(client, id, Duration::from_secs(timeout), out).await?
    };
    out.data(&view, || server_rows(std::slice::from_ref(&view)));
    Ok(())
}

/// The export document: the profile (no key reference, no binary path) and its definition.
pub fn export_document(view: &Value, definition: Option<Value>) -> Value {
    let mut profile = view["profile"].clone();
    if let Some(config) = profile.get_mut("config").and_then(Value::as_object_mut) {
        for key in ["api_key", "api_key_ref", "binary_path", "mcp_config_path"] {
            config.remove(key);
        }
    }
    json!({
        "format": PROFILE_FORMAT,
        "version": 1,
        "exported_at": harness_core::capabilities::now_secs(),
        "profile": profile,
        "definition": definition,
    })
}

pub async fn profile_export(
    client: &ApiClient,
    id: &str,
    output: Option<&Path>,
    out: &Out,
) -> CliResult<()> {
    let view = profile_view(client, id).await?;
    let definition = match view["profile"]["config"]["definition_id"].as_str() {
        Some(def_id) => match client.get(&format!("/definitions/{}", seg(def_id))).await {
            Ok(d) => Some(d),
            Err(e) if e.exit == Exit::NotFound => None,
            Err(e) => return Err(e),
        },
        None => None,
    };
    let doc = export_document(&view, definition);
    match output {
        Some(path) => {
            let text = serde_json::to_string_pretty(&doc).unwrap_or_default();
            std::fs::write(path, text + "\n")
                .map_err(|e| CliError::general("cli.io", format!("{}: {}", path.display(), e)))?;
            let result = json!({"id": id, "file": path});
            out.data(&result, || format!("Exported {} to {}", id, path.display()));
        }
        // The document itself is the data (with or without --json).
        None => out.json_line_pretty(&doc),
    }
    Ok(())
}

pub async fn profile_import(client: &ApiClient, file: &Path, out: &Out) -> CliResult<()> {
    let text = if file.as_os_str() == "-" {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)
            .map_err(|e| CliError::general("cli.io", e.to_string()))?;
        s
    } else {
        std::fs::read_to_string(file)
            .map_err(|e| CliError::general("cli.io", format!("{}: {}", file.display(), e)))?
    };
    let doc: Value = serde_json::from_str(&text).map_err(|e| {
        CliError::usage(
            "cli.invalid_argument",
            format!("Not a JSON document: {}", e),
        )
    })?;
    if doc["format"] != PROFILE_FORMAT {
        return Err(CliError::usage(
            "cli.invalid_argument",
            format!(
                "Not a {} document (see `harness profile export`)",
                PROFILE_FORMAT
            ),
        ));
    }
    let mut profile = doc["profile"].clone();
    if !profile.is_object() {
        return Err(CliError::usage(
            "cli.invalid_argument",
            "The document has no profile",
        ));
    }
    // The definition: reuse a local one with the same alias, else create it.
    let mut definition_id = Value::Null;
    let mut created_definition = None;
    if let Some(def) = doc["definition"].as_object() {
        let alias = def.get("alias").and_then(Value::as_str).unwrap_or_default();
        let local = client.get("/definitions").await?;
        let existing = local.as_array().and_then(|l| {
            l.iter()
                .find(|d| {
                    d["alias"]
                        .as_str()
                        .is_some_and(|a| a.eq_ignore_ascii_case(alias))
                })
                .cloned()
        });
        definition_id = match existing {
            Some(d) => {
                if d["base"] != def["base"] {
                    out.warn(format!(
                        "definition \"{}\" exists here with another model file; the profile uses the local one",
                        alias
                    ));
                }
                d["id"].clone()
            }
            None => {
                let mut body = Value::Object(def.clone());
                for key in ["id", "created_at", "updated_at", "auto_created"] {
                    body.as_object_mut().map(|o| o.remove(key));
                }
                let created = client.post("/definitions", &body).await?;
                created_definition = created["id"].as_str().map(str::to_string);
                created["id"].clone()
            }
        };
    }
    if let Some(config) = profile.get_mut("config").and_then(Value::as_object_mut) {
        if !definition_id.is_null() {
            config.insert("definition_id".into(), definition_id);
        }
        config.remove("api_key_ref");
    }
    let created = match client.post("/servers", &profile).await {
        Ok(c) => c,
        Err(e) => {
            // Leave nothing half-imported.
            if let Some(id) = &created_definition {
                let _ = client.delete(&format!("/definitions/{}", seg(id))).await;
            }
            return Err(e);
        }
    };
    if created_definition.is_some() {
        out.info(format!(
            "Created model definition \"{}\"",
            doc["definition"]["alias"].as_str().unwrap_or_default()
        ));
    }
    out.data(&created, || {
        format!(
            "Imported profile {} ({})",
            str_of(&created["id"]),
            str_of(&created["name"])
        )
    });
    Ok(())
}

// --- Modelfile import ---

/// English text of a warning code (the GUI translates the same codes).
fn modelfile_warning(w: &Value) -> String {
    let detail = w["detail"].as_str().unwrap_or_default();
    let text = match w["code"].as_str().unwrap_or_default() {
        "unknown_parameter" => format!("unknown PARAMETER ignored: {}", detail),
        "ignored_parameter" => format!("PARAMETER has no llama.cpp equivalent, ignored: {}", detail),
        "ignored_value" => format!("value ignored: {}", detail),
        "invalid_value" => format!("invalid value ignored: {}", detail),
        "unknown_instruction" => format!("unknown instruction ignored: {}", detail),
        "ignored_instruction" => format!("ignored: {}", detail),
        "template_ignored" => {
            "TEMPLATE (Go template) is not used; the GGUF's own Jinja chat template applies".into()
        }
        "template_ignored_no_jinja" => "TEMPLATE (Go template) is not used and this GGUF has no chat template; pass one with --chat-template".into(),
        "messages_as_examples" => format!(
            "{} MESSAGE turn(s) were appended to the system prompt as an example conversation (the preset has no turn list)",
            detail
        ),
        "adapter_unsupported" => format!("ADAPTER is not a GGUF LoRA file, skipped: {}", detail),
        "model_file_missing" => format!("FROM file not found: {}", detail),
        other => format!("{}: {}", other, detail),
    };
    match w["line"].as_u64() {
        Some(l) => format!("line {}: {}", l, text),
        None => text,
    }
}

pub async fn import_modelfile(
    client: &ApiClient,
    file: Option<std::path::PathBuf>,
    ollama: Option<String>,
    alias: Option<String>,
    dry_run: bool,
    out: &Out,
) -> CliResult<()> {
    let io = |p: &str, e: std::io::Error| CliError::general("cli.io", format!("{}: {}", p, e));
    let (text, base_dir) = match (&file, &ollama) {
        (_, Some(name)) => (
            format!(
                "FROM {}
",
                name
            ),
            None,
        ),
        (Some(f), None) if f.as_os_str() == "-" => {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)
                .map_err(|e| io("stdin", e))?;
            (s, std::env::current_dir().ok())
        }
        (Some(f), None) => {
            let bytes = std::fs::read(f).map_err(|e| io(&f.display().to_string(), e))?;
            let dir = f
                .canonicalize()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()));
            (String::from_utf8_lossy(&bytes).into_owned(), dir)
        }
        (None, None) => unreachable!("clap requires FILE or --ollama"),
    };
    let mut body = json!({"text": text});
    if let Some(d) = base_dir {
        // `canonicalize` on Windows yields verbatim (`\\?\`) paths; plain ones are nicer.
        let d = d.display().to_string();
        body["base_dir"] = json!(d.strip_prefix(r"\\?\").unwrap_or(&d));
    }
    if let Some(a) = &alias {
        body["alias"] = json!(a);
    }
    let preview = client.post("/modelfile/preview", &body).await?;
    let imp = &preview["import"];
    let warnings: Vec<String> = preview["warnings"]
        .as_array()
        .map(|w| w.iter().map(modelfile_warning).collect())
        .unwrap_or_default();
    if dry_run {
        out.data(&preview, || {
            let rows: Vec<Vec<String>> = preview["mapped"]
                .as_array()
                .map(|m| {
                    m.iter()
                        .map(|r| {
                            vec![
                                str_of(&r["source"]),
                                str_of(&r["target"]),
                                str_of(&r["value"]),
                            ]
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut s = format!(
                "Would create \"{}\"{}
{}",
                str_of(&imp["suggested_alias"]),
                if imp["model_found"] == true {
                    ""
                } else {
                    " (model file not found)"
                },
                table(&["MODELFILE", "SETS", "VALUE"], &rows)
            );
            for w in &warnings {
                s.push_str(&format!(
                    "
warning: {}",
                    w
                ));
            }
            s
        });
        return Ok(());
    }
    let created = client.post("/modelfile/import", &body).await?;
    for w in &warnings {
        out.warn(w);
    }
    out.data(&created, || {
        let d = &created["definition"];
        format!(
            "Imported model definition \"{}\"{}",
            str_of(&d["alias"]),
            if d["default_preset_id"].is_null() {
                String::new()
            } else {
                format!(" with inference preset {}", str_of(&d["default_preset_id"]))
            }
        )
    });
    Ok(())
}

// --- Benchmarks ---

fn result_rows(results: &[BenchResult]) -> String {
    let rows: Vec<Vec<String>> = results
        .iter()
        .map(|r| {
            let test = match (r.n_prompt, r.n_gen) {
                (p, 0) => format!("pp{}", p),
                (0, g) => format!("tg{}", g),
                (p, g) => format!("pp{}+tg{}", p, g),
            };
            let test = if r.n_depth > 0 {
                format!("{} @ d{}", test, r.n_depth)
            } else {
                test
            };
            vec![
                test,
                r.n_gpu_layers.to_string(),
                r.backends.clone(),
                format!("{:.2} ± {:.2}", r.avg_ts, r.stddev_ts),
            ]
        })
        .collect();
    table(&["TEST", "NGL", "BACKEND", "TOKENS/S"], &rows)
}

fn run_results(view: &Value) -> Vec<BenchResult> {
    let results = if view["run"]["results"].is_array() {
        &view["run"]["results"]
    } else {
        &view["status"]["results"]
    };
    serde_json::from_value(results.clone()).unwrap_or_default()
}

pub struct BenchArgs {
    pub model: String,
    pub n_prompt: Vec<u32>,
    pub n_gen: Vec<u32>,
    pub ngl: Vec<i32>,
    pub repetitions: Option<u32>,
    pub threads: Option<u32>,
}

pub async fn benchmark_run(client: &ApiClient, args: BenchArgs, out: &Out) -> CliResult<()> {
    let path = model_path(client, &args.model).await?;
    let request = BenchmarkRequest {
        model_path: path.into(),
        n_prompt: args.n_prompt,
        n_gen: args.n_gen,
        gpu_layers: args.ngl,
        repetitions: args.repetitions,
        threads: args.threads,
        ..Default::default()
    };
    let started = client
        .post(
            "/benchmarks",
            &serde_json::to_value(&request).unwrap_or(Value::Null),
        )
        .await?;
    let id = str_of(&started["run_id"]);
    out.info(format!("Benchmark {} started", id));
    let path = format!("/benchmarks/{}", seg(&id));
    let t0 = Instant::now();
    let view = loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                out.progress_done();
                let _ = client.post(&format!("{}/cancel", path), &json!({})).await;
                return Err(CliError::from_code(Exit::General, "cli.aborted", Default::default()));
            }
            _ = tokio::time::sleep(Duration::from_millis(1000)) => {}
        }
        let view = client.get(&path).await?;
        if view["running"] != true && !view["run"].is_null() {
            break view;
        }
        let s = &view["status"];
        out.progress(&format!(
            "{} {}/{} {:.0}s",
            s["phase"].as_str().unwrap_or("running"),
            str_of(&s["current"]),
            str_of(&s["total"]),
            t0.elapsed().as_secs_f32()
        ));
    };
    out.progress_done();
    let status = view["run"]["status"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    if status != "completed" {
        let msg = view["run"]["error"]
            .as_str()
            .map(crate::messages::core_text)
            .unwrap_or_else(|| status.clone());
        return Err(CliError::new(
            if crate::messages::mentions_out_of_memory(&msg) {
                Exit::OutOfMemory
            } else {
                Exit::General
            },
            "cli.benchmark_failed",
            format!("Benchmark {}: {}", status, msg),
        )
        .with("run_id", id)
        .with("run", view["run"].clone()));
    }
    let results = run_results(&view);
    out.data(&view, || result_rows(&results));
    Ok(())
}

pub async fn benchmark_ls(client: &ApiClient, out: &Out) -> CliResult<()> {
    let list = client.get("/benchmarks").await?;
    out.data(&list, || {
        let runs = list["runs"].as_array().cloned().unwrap_or_default();
        if runs.is_empty() {
            return "No benchmark runs.".into();
        }
        let rows: Vec<Vec<String>> = runs
            .iter()
            .map(|r| {
                vec![
                    str_of(&r["id"]),
                    str_of(&r["model_name"]),
                    str_of(&r["status"]),
                    r["results"]
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0)
                        .to_string(),
                ]
            })
            .collect();
        table(&["ID", "MODEL", "STATUS", "RESULTS"], &rows)
    });
    Ok(())
}

pub async fn benchmark_show(client: &ApiClient, id: &str, out: &Out) -> CliResult<()> {
    let view = client.get(&format!("/benchmarks/{}", seg(id))).await?;
    let results = run_results(&view);
    out.data(&view, || {
        format!(
            "{} ({}{})\n{}",
            id,
            str_of(&view["run"]["model_name"]),
            if view["running"] == true {
                ", running"
            } else {
                ""
            },
            result_rows(&results)
        )
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_drops_local_secrets_and_paths() {
        let view = json!({"profile": {"id": "p", "name": "P", "config": {"model_path": "/m.gguf",
            "api_key_ref": "profile:p", "binary_path": "/bin/llama-server", "port": 18500}}});
        let doc = export_document(&view, None);
        assert_eq!(doc["format"], PROFILE_FORMAT);
        let c = &doc["profile"]["config"];
        assert!(c.get("api_key_ref").is_none() && c.get("binary_path").is_none());
        assert_eq!(c["port"], 18500);
    }
}
