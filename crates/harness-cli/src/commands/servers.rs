//! `ps`, `load`, `unload`, `stop`, `logs`, `shutdown`.

use std::time::Duration;

use serde_json::{json, Value};

use super::resolve::{definition_for, idle_server_of, match_definitions, wait_running};
use crate::client::{seg, ApiClient};
use crate::error::{CliError, CliResult, Exit};
use crate::output::{status_text, str_of, table, Out};
use crate::session::Session;

fn is_active(s: &Value) -> bool {
    matches!(
        s["status"].as_str(),
        Some("Running") | Some("Starting") | Some("Stopping")
    )
}

pub fn server_rows(servers: &[Value]) -> String {
    let rows: Vec<Vec<String>> = servers
        .iter()
        .map(|s| {
            vec![
                str_of(&s["id"]),
                status_text(&s["status"]),
                s["served_alias"].as_str().unwrap_or("-").to_string(),
                s["endpoint"].as_str().unwrap_or("-").to_string(),
                if s["is_profile"] == true {
                    "profile".into()
                } else {
                    "-".into()
                },
            ]
        })
        .collect();
    table(&["ID", "STATUS", "MODEL", "ENDPOINT", "PROFILE"], &rows)
}

pub async fn ps(client: &ApiClient, all: bool, out: &Out) -> CliResult<()> {
    let servers = client.get("/servers").await?;
    let list: Vec<Value> = servers
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| all || is_active(s))
        .collect();
    let value = Value::Array(list.clone());
    out.data(&value, || {
        if list.is_empty() {
            "No running servers.".into()
        } else {
            server_rows(&list)
        }
    });
    Ok(())
}

pub async fn load(
    session: &Session,
    model: &str,
    server: Option<String>,
    no_wait: bool,
    timeout: u64,
    out: &Out,
) -> CliResult<()> {
    let client = &session.client;
    let def = definition_for(client, model, out).await?;
    let alias = str_of(&def["alias"]);
    let server = match server {
        Some(s) => Some(s),
        None => idle_server_of(client, &str_of(&def["id"])).await?,
    };
    out.info(format!("Loading {}...", alias));
    let loaded = client
        .post(
            "/load",
            &json!({"definition_id": def["id"], "server_id": server}),
        )
        .await?;
    let id = str_of(&loaded["server_id"]);
    let view = if no_wait {
        loaded["server"].clone()
    } else {
        wait_running(client, &id, Duration::from_secs(timeout), out).await?
    };
    if session.via == crate::session::Via::Temporary && !no_wait {
        out.warn("temporary instance: the server stops when this command exits");
    }
    let result = json!({"server_id": id, "definition": def, "server": view});
    out.data(&result, || {
        format!(
            "{} {} on {} ({})",
            alias,
            if no_wait { "is starting" } else { "is ready" },
            id,
            view["endpoint"].as_str().unwrap_or("-")
        )
    });
    Ok(())
}

pub async fn unload(
    client: &ApiClient,
    model: Option<String>,
    server: Option<String>,
    out: &Out,
) -> CliResult<()> {
    let body = match (model, server) {
        (_, Some(server)) => json!({"server_id": server}),
        (Some(model), None) => {
            let defs = client.get("/definitions").await?;
            let defs = defs.as_array().cloned().unwrap_or_default();
            let def = match_definitions(&defs, &model)
                .ok_or_else(|| CliError::not_found("model definition", &model))?;
            json!({"alias": def["id"]})
        }
        (None, None) => {
            return Err(CliError::usage(
                "cli.invalid_argument",
                "Name a model or pass --server",
            ))
        }
    };
    let result = client.post("/unload", &body).await?;
    out.data(&result, || {
        let stopped: Vec<String> = result["stopped"]
            .as_array()
            .map(|a| a.iter().map(str_of).collect())
            .unwrap_or_default();
        if stopped.is_empty() {
            "Nothing was running.".into()
        } else {
            format!("Stopped {}", stopped.join(", "))
        }
    });
    Ok(())
}

pub async fn stop(client: &ApiClient, ids: Vec<String>, all: bool, out: &Out) -> CliResult<()> {
    let ids = if all {
        client
            .get("/servers")
            .await?
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(is_active)
            .map(|s| str_of(&s["id"]))
            .collect()
    } else if ids.is_empty() {
        return Err(CliError::usage(
            "cli.invalid_argument",
            "Name servers to stop or pass --all",
        ));
    } else {
        ids
    };
    let mut stopped = Vec::new();
    for id in &ids {
        client
            .post(&format!("/servers/{}/stop", seg(id)), &json!({}))
            .await?;
        stopped.push(id.clone());
    }
    let result = json!({"stopped": stopped});
    out.data(&result, || {
        if stopped.is_empty() {
            "Nothing was running.".into()
        } else {
            format!("Stopped {}", stopped.join(", "))
        }
    });
    Ok(())
}

pub async fn logs(
    client: &ApiClient,
    server: &str,
    follow: bool,
    lines: usize,
    out: &Out,
) -> CliResult<()> {
    let base = format!("/servers/{}/logs", seg(server));
    let first = client.get(&format!("{}?limit={}", base, lines)).await?;
    let print = |batch: &Value| {
        if out.json {
            if follow {
                out.json_line(batch);
            } else {
                out.data(batch, String::new);
            }
        } else {
            for l in batch["lines"]
                .as_array()
                .map(|a| a.as_slice())
                .unwrap_or_default()
            {
                println!("{}", l.as_str().unwrap_or_default());
            }
        }
    };
    print(&first);
    if !follow {
        return Ok(());
    }
    let mut next = first["next"].as_u64().unwrap_or(0);
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(()),
            _ = tokio::time::sleep(Duration::from_millis(500)) => {}
        }
        let batch = match client.get(&format!("{}?since={}", base, next)).await {
            Ok(b) => b,
            // The server was removed (or the instance went away): the log ends.
            Err(e) if e.exit == Exit::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        next = batch["next"].as_u64().unwrap_or(next);
        if batch["lines"].as_array().is_some_and(|a| !a.is_empty()) {
            print(&batch);
        }
    }
}

/// Table of `GET /servers/{id}/lora` (`adapters`: id, path, scale).
pub fn lora_rows(body: &Value) -> String {
    let adapters = body["adapters"].as_array().cloned().unwrap_or_default();
    if adapters.is_empty() {
        return "This server loaded no LoRA adapters.".into();
    }
    let rows: Vec<Vec<String>> = adapters
        .iter()
        .map(|a| {
            vec![
                str_of(&a["id"]),
                format!("{}", a["scale"].as_f64().unwrap_or(0.0)),
                str_of(&a["path"]),
            ]
        })
        .collect();
    table(&["ID", "SCALE", "PATH"], &rows)
}

pub async fn lora_list(client: &ApiClient, server: &str, out: &Out) -> CliResult<()> {
    let body = client
        .get(&format!("/servers/{}/lora", seg(server)))
        .await?;
    out.data(&body, || lora_rows(&body));
    Ok(())
}

pub async fn lora_set(
    client: &ApiClient,
    server: &str,
    id: u32,
    scale: f64,
    out: &Out,
) -> CliResult<()> {
    if !scale.is_finite() {
        return Err(CliError::usage(
            "cli.invalid_argument",
            "The scale must be a number",
        ));
    }
    let body = client
        .post(
            &format!("/servers/{}/lora", seg(server)),
            &json!({"adapters": [{"id": id, "scale": scale}]}),
        )
        .await?;
    out.data(&body, || lora_rows(&body));
    Ok(())
}

pub async fn shutdown(session: &Session, out: &Out) -> CliResult<()> {
    session.client.post("/shutdown", &json!({})).await?;
    let result = json!({"shutdown": true});
    out.data(&result, || "Shutdown requested.".into());
    Ok(())
}

fn router_rows(models: &[Value]) -> String {
    let rows: Vec<Vec<String>> = models
        .iter()
        .map(|m| {
            vec![
                str_of(&m["id"]),
                str_of(&m["state"]),
                m["context_length"]
                    .as_u64()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "-".into()),
                m["source"].as_str().unwrap_or("-").to_string(),
            ]
        })
        .collect();
    table(&["MODEL", "STATE", "CONTEXT", "SOURCE"], &rows)
}

/// `harness router models <server>`.
pub async fn router_models(client: &ApiClient, server: &str, out: &Out) -> CliResult<()> {
    let result = client
        .get(&format!("/servers/{}/router/models", seg(server)))
        .await?;
    let models = result["models"].as_array().cloned().unwrap_or_default();
    out.data(&result, || {
        if models.is_empty() {
            "The router lists no models.".into()
        } else {
            router_rows(&models)
        }
    });
    Ok(())
}

/// `harness router load <server> <model>`: asks the router, then waits until the model reports
/// loaded (or failed).
pub async fn router_load(
    client: &ApiClient,
    server: &str,
    model: &str,
    no_wait: bool,
    timeout: u64,
    out: &Out,
) -> CliResult<()> {
    let base = format!("/servers/{}/router/models", seg(server));
    out.info(format!("Loading {}...", model));
    let mut state = client
        .post(&format!("{base}/load"), &json!({"model": model}))
        .await?;
    if !no_wait {
        let deadline = std::time::Instant::now() + Duration::from_secs(timeout.max(1));
        while state["state"] == "loading" || state["state"] == "unloaded" {
            if std::time::Instant::now() > deadline {
                return Err(CliError::usage(
                    "cli.invalid_argument",
                    format!("{} did not load within {} s", model, timeout),
                ));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            let list = client.get(&base).await?;
            if let Some(m) = list["models"]
                .as_array()
                .and_then(|a| a.iter().find(|m| m["id"] == model))
            {
                state = m.clone();
            }
        }
        if state["state"] == "failed" {
            return Err(CliError::usage(
                "cli.invalid_argument",
                format!(
                    "{} failed to load (exit code {}); see `harness logs {}`",
                    model, state["exit_code"], server
                ),
            ));
        }
    }
    out.data(&state, || {
        format!("{} is {}", model, state["state"].as_str().unwrap_or("-"))
    });
    Ok(())
}

/// `harness router unload <server> <model>`.
pub async fn router_unload(
    client: &ApiClient,
    server: &str,
    model: &str,
    out: &Out,
) -> CliResult<()> {
    let state = client
        .post(
            &format!("/servers/{}/router/models/unload", seg(server)),
            &json!({"model": model}),
        )
        .await?;
    out.data(&state, || {
        format!("{} is {}", model, state["state"].as_str().unwrap_or("-"))
    });
    Ok(())
}

pub struct RetryArgs {
    pub server: String,
    pub option: Option<String>,
    pub set: Vec<String>,
    pub unload: Vec<String>,
    pub save: bool,
    pub no_wait: bool,
    pub timeout: u64,
}

/// `id=value` → a catalog value (integer, float, bool, else text).
pub fn parse_set(item: &str) -> CliResult<(String, Value)> {
    let (k, v) = item.split_once('=').ok_or_else(|| {
        CliError::usage(
            "cli.invalid_argument",
            format!("--set expects id=value: {}", item),
        )
    })?;
    let v = v.trim();
    let value = if let Ok(n) = v.parse::<i64>() {
        json!(n)
    } else if let Ok(f) = v.parse::<f64>() {
        json!(f)
    } else if v == "true" || v == "false" {
        json!(v == "true")
    } else {
        json!(v)
    };
    Ok((k.trim().to_string(), value))
}

fn option_label(o: &Value) -> String {
    crate::messages::template(o["label_key"].as_str().unwrap_or_default())
        .map(|t| crate::messages::fill(t, o["params"].as_object().unwrap_or(&Default::default())))
        .unwrap_or_default()
}

fn recovery_rows(plan: &Value) -> String {
    let failure = &plan["failure"];
    let mut lines = vec![crate::messages::render(
        failure["code"].as_str().unwrap_or("servers.start_failed"),
        failure["params"].as_object().unwrap_or(&Default::default()),
    )];
    if let Some(stage) = failure["params"]["stage"].as_str() {
        lines.push(format!("did not fit: {}", stage.replace('_', " ")));
    }
    if let (Some(free), Some(need)) = (
        plan["free_vram_bytes"].as_u64(),
        plan["estimated_need_bytes"].as_u64(),
    ) {
        lines.push(format!(
            "free VRAM {}, estimated need {}",
            crate::output::bytes(free),
            crate::output::bytes(need)
        ));
    }
    let options = plan["options"].as_array().cloned().unwrap_or_default();
    if options.is_empty() {
        lines.push("No automatic option for this failure; see `harness logs`.".into());
        return lines.join("\n");
    }
    let rows: Vec<Vec<String>> = options
        .iter()
        .map(|o| {
            vec![
                str_of(&o["id"]),
                if o["recommended"] == true {
                    "yes".into()
                } else {
                    String::new()
                },
                option_label(o),
            ]
        })
        .collect();
    lines.push(table(&["OPTION", "RECOMMENDED", "WHAT"], &rows));
    lines.push(format!(
        "Retry: harness retry {} --option <OPTION> [--save]",
        str_of(&plan["server_id"])
    ));
    lines.join("\n")
}

/// `harness retry`: the recovery plan, or a retry with an option / explicit values.
pub async fn retry(client: &ApiClient, args: RetryArgs, out: &Out) -> CliResult<()> {
    let id = args.server;
    if args.option.is_none() && args.set.is_empty() && args.unload.is_empty() {
        let plan = client
            .get(&format!("/servers/{}/recovery", seg(&id)))
            .await?;
        out.data(&plan, || recovery_rows(&plan));
        return Ok(());
    }
    let mut overrides = serde_json::Map::new();
    for item in &args.set {
        let (k, v) = parse_set(item)?;
        overrides.insert(k, v);
    }
    out.info(format!("Starting {} again...", id));
    let started = client
        .post(
            &format!("/servers/{}/retry", seg(&id)),
            &json!({
                "option": args.option,
                "overrides": overrides,
                "unload": args.unload,
                "save": args.save,
            }),
        )
        .await?;
    let view = if args.no_wait {
        started
    } else {
        wait_running(client, &id, Duration::from_secs(args.timeout), out).await?
    };
    out.data(&view, || {
        format!(
            "{} is {} ({})",
            id,
            if args.no_wait { "starting" } else { "ready" },
            view["endpoint"].as_str().unwrap_or("-")
        )
    });
    Ok(())
}

#[cfg(test)]
mod retry_tests {
    use super::*;

    #[test]
    fn lora_table_lists_ids_scales_and_paths() {
        let body = json!({"adapters": [
            {"id": 0, "path": "/lora/a.gguf", "scale": 0.5},
            {"id": 1, "path": "/lora/b.gguf", "scale": 0.0}
        ]});
        let text = lora_rows(&body);
        assert!(text.contains("SCALE"));
        assert!(text.contains("0.5"));
        assert!(text.contains("b.gguf"));
        assert_eq!(
            lora_rows(&json!({"adapters": []})),
            "This server loaded no LoRA adapters."
        );
    }

    #[test]
    fn set_values_are_typed() {
        assert_eq!(parse_set("context_size=8192").unwrap().1, json!(8192));
        assert_eq!(parse_set("flash_attn=true").unwrap().1, json!(true));
        assert_eq!(parse_set("fit=on").unwrap().1, json!("on"));
        assert_eq!(parse_set("rope_freq_scale=0.5").unwrap().1, json!(0.5));
        assert!(parse_set("nothing").is_err());
    }
}
