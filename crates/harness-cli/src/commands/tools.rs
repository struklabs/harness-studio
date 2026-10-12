//! `harness tools list` and `harness tools run`: the tool engine (`harness_core::tools`) through
//! the Control API (`/tools`). The policy is applied by the core: a tool it asks about needs the
//! user's approval, which this command collects (a y/N question on a terminal, `--yes`
//! otherwise) and passes as `approved`; a policy of "off" refuses whatever is answered.

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::client::{seg, ApiClient};
use crate::error::{CliError, CliResult};
use crate::output::{table, Out};

fn short(text: &str, max: usize) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max {
        one_line
    } else {
        let head: String = one_line.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

pub async fn list(client: &ApiClient, out: &Out) -> CliResult<()> {
    let v = client.get("/tools").await?;
    out.data(&v, || {
        let tools = v["tools"].as_array().cloned().unwrap_or_default();
        if tools.is_empty() {
            return "No tools.".into();
        }
        let rows: Vec<Vec<String>> = tools
            .iter()
            .map(|t| {
                vec![
                    t["name"].as_str().unwrap_or("").to_string(),
                    t["class"].as_str().unwrap_or("").to_string(),
                    t["source"].as_str().unwrap_or("").to_string(),
                    t["decision"].as_str().unwrap_or("").to_string(),
                    short(t["description"].as_str().unwrap_or(""), 60),
                ]
            })
            .collect();
        format!(
            "Tool policy: {}{}\n\n{}",
            v["policy"].as_str().unwrap_or("?"),
            match v["trusted"].as_array().filter(|t| !t.is_empty()) {
                Some(t) => format!(
                    " (trusted: {})",
                    t.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                None => String::new(),
            },
            table(&["NAME", "CLASS", "SOURCE", "POLICY", "DESCRIPTION"], &rows)
        )
    });
    Ok(())
}

/// `harness tools mcp`: the state of Harness's own MCP client.
pub async fn mcp_status(client: &ApiClient, retry: bool, out: &Out) -> CliResult<()> {
    let v = client
        .get(if retry {
            "/tools/mcp?retry=true"
        } else {
            "/tools/mcp"
        })
        .await?;
    out.data(&v, || {
        let servers = v.as_array().cloned().unwrap_or_default();
        if servers.is_empty() {
            return "No MCP servers configured (Connections > MCP servers).".into();
        }
        let rows: Vec<Vec<String>> = servers
            .iter()
            .map(|s| {
                vec![
                    s["name"].as_str().unwrap_or("").to_string(),
                    s["transport"].as_str().unwrap_or("").to_string(),
                    s["state"].as_str().unwrap_or("").to_string(),
                    s["auth"].as_str().unwrap_or("-").to_string(),
                    s["tools"].to_string(),
                    s["protocol"].as_str().unwrap_or("-").to_string(),
                    s["server"].as_str().unwrap_or("-").to_string(),
                    short(s["error"].as_str().unwrap_or(""), 60),
                ]
            })
            .collect();
        table(
            &[
                "NAME",
                "TRANSPORT",
                "STATE",
                "AUTH",
                "TOOLS",
                "MCP",
                "SERVER",
                "ERROR",
            ],
            &rows,
        )
    });
    Ok(())
}

/// `harness tools mcp-resources <server>`: the resources and templates of one MCP server, or
/// (`--read <uri>`) the contents of one resource.
pub async fn mcp_resources(
    client: &ApiClient,
    server: &str,
    read: Option<&str>,
    out: &Out,
) -> CliResult<()> {
    if let Some(uri) = read {
        let v = client
            .get(&format!(
                "/tools/mcp/{}/resource?uri={}",
                seg(server),
                seg(uri)
            ))
            .await?;
        out.data(&v, || {
            v["contents"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|c| match c["text"].as_str() {
                    Some(t) => t.to_string(),
                    None => format!(
                        "[binary resource {} ({}), {} base64 characters]",
                        c["uri"].as_str().unwrap_or(""),
                        c["mime_type"].as_str().unwrap_or("?"),
                        c["blob"].as_str().map_or(0, str::len)
                    ),
                })
                .collect::<Vec<_>>()
                .join("\n")
        });
        return Ok(());
    }
    let v = client
        .get(&format!("/tools/mcp/{}/resources", seg(server)))
        .await?;
    out.data(&v, || {
        let resources = v["resources"].as_array().cloned().unwrap_or_default();
        let templates = v["templates"].as_array().cloned().unwrap_or_default();
        if resources.is_empty() && templates.is_empty() {
            return "This server offers no resources.".into();
        }
        let mut text = String::new();
        if !resources.is_empty() {
            let rows: Vec<Vec<String>> = resources
                .iter()
                .map(|r| {
                    vec![
                        r["uri"].as_str().unwrap_or("").to_string(),
                        r["name"].as_str().unwrap_or("").to_string(),
                        r["mime_type"].as_str().unwrap_or("-").to_string(),
                        short(r["description"].as_str().unwrap_or(""), 50),
                    ]
                })
                .collect();
            text.push_str(&table(&["URI", "NAME", "TYPE", "DESCRIPTION"], &rows));
        }
        if !templates.is_empty() {
            let rows: Vec<Vec<String>> = templates
                .iter()
                .map(|r| {
                    vec![
                        r["uri_template"].as_str().unwrap_or("").to_string(),
                        r["name"].as_str().unwrap_or("").to_string(),
                        r["mime_type"].as_str().unwrap_or("-").to_string(),
                        short(r["description"].as_str().unwrap_or(""), 50),
                    ]
                })
                .collect();
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&table(
                &["URI TEMPLATE", "NAME", "TYPE", "DESCRIPTION"],
                &rows,
            ));
        }
        text
    });
    Ok(())
}

/// `harness tools mcp-authorize <server>`: the OAuth sign-in of a remote MCP server. The browser
/// opens on the machine Harness runs on; this waits until the user approves.
pub async fn mcp_authorize(client: &ApiClient, server: &str, out: &Out) -> CliResult<()> {
    if !out.json {
        eprintln!("Opening your browser to authorize \"{server}\"; waiting up to five minutes...");
    }
    let v = client
        .post(&format!("/tools/mcp/{}/authorize", seg(server)), &json!({}))
        .await?;
    out.data(&v, || {
        format!(
            "\"{server}\" is {} ({} tools).",
            match v["state"].as_str() {
                Some("connected") => "authorized and connected".to_string(),
                Some(other) => format!("authorized, state: {other}"),
                None => "authorized".to_string(),
            },
            v["tools"]
        )
    });
    Ok(())
}

/// `harness tools mcp-logout <server>`.
pub async fn mcp_logout(client: &ApiClient, server: &str, out: &Out) -> CliResult<()> {
    let v = client
        .post(&format!("/tools/mcp/{}/logout", seg(server)), &json!({}))
        .await?;
    out.data(&v, || format!("Signed \"{server}\" out."));
    Ok(())
}

pub async fn run(
    client: &ApiClient,
    name: &str,
    args: &str,
    workdir: Option<PathBuf>,
    timeout: Option<u64>,
    out: &Out,
) -> CliResult<()> {
    let args: Value = serde_json::from_str(args).map_err(|e| {
        CliError::usage(
            "cli.invalid_argument",
            format!("--args is not valid JSON: {e}"),
        )
    })?;
    if !args.is_object() {
        return Err(CliError::usage(
            "cli.invalid_argument",
            "--args must be a JSON object, e.g. '{\"path\": \"src/main.rs\"}'",
        ));
    }
    let workdir = match workdir {
        Some(d) => d,
        None => std::env::current_dir()
            .map_err(|e| CliError::general("cli.io", format!("no current directory: {e}")))?,
    };
    let workdir = std::path::absolute(&workdir).unwrap_or(workdir);

    // What would the policy do? Ask the user here when it says "ask".
    let listing = client.get("/tools").await?;
    let entry = listing["tools"]
        .as_array()
        .and_then(|t| t.iter().find(|t| t["name"] == name))
        .ok_or_else(|| CliError::not_found("tool", name))?;
    let mut approved = false;
    if entry["decision"] == "ask" {
        out.confirm(&format!(
            "Run the {} tool \"{}\" with {} in {}?",
            entry["class"].as_str().unwrap_or("?"),
            name,
            short(&args.to_string(), 200),
            workdir.display()
        ))?;
        approved = true;
    }

    let mut body = json!({
        "args": args,
        "workdir": workdir.to_string_lossy(),
        "approved": approved,
        "client": "cli",
    });
    if let Some(t) = timeout {
        body["timeout_secs"] = json!(t);
    }
    let result = client
        .post(&format!("/tools/{}/run", seg(name)), &body)
        .await?;
    let content = result["content"].as_str().unwrap_or("").to_string();
    match result["error"].as_str() {
        None => {
            out.data(&result, || content.clone());
            Ok(())
        }
        Some(code) => {
            if out.json {
                out.data(&result, String::new);
            }
            Err(CliError::general("cli.tool_failed", content).with("tool_error", code))
        }
    }
}

pub async fn search_provider(
    client: &ApiClient,
    provider: Option<&str>,
    url: Option<&str>,
    api_key: Option<&str>,
    out: &Out,
) -> CliResult<()> {
    let current = client.get("/tools/search-provider").await?;
    let value = match provider {
        Some(p @ ("none" | "duckduckgo" | "searxng" | "brave" | "tavily")) => p,
        Some(_) => {
            return Err(CliError::usage(
                "cli.invalid_argument",
                "--provider must be none, duckduckgo, searxng, brave, or tavily",
            ))
        }
        None => current["provider"].as_str().unwrap_or("none"),
    };
    let result = if provider.is_some() || url.is_some() || api_key.is_some() {
        let mut body = json!({"provider":value,"searxng_url":url.unwrap_or_else(||current["searxng_url"].as_str().unwrap_or(""))});
        if let Some(key) = api_key {
            body["api_key"] = json!(key);
        }
        client.put("/tools/search-provider", &body).await?
    } else {
        current
    };
    out.data(&result, || {
        format!(
            "Web search provider: {}\nSearXNG URL: {}\nAPI key: {}",
            result["provider"].as_str().unwrap_or("none"),
            result["searxng_url"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("-"),
            if result["api_key_set"].as_bool().unwrap_or(false) {
                "saved in OS keychain"
            } else {
                "not saved"
            }
        )
    });
    Ok(())
}

pub async fn inventory(client: &ApiClient, out: &Out) -> CliResult<()> {
    let v = client.get("/tools/inventory").await?;
    out.data(&v, || serde_json::to_string_pretty(&v).unwrap_or_default());
    Ok(())
}
pub async fn automatic(
    client: &ApiClient,
    command: crate::cli::AutoToolsCommand,
    out: &Out,
) -> CliResult<()> {
    use crate::cli::AutoToolsCommand;
    let v = match command {
        AutoToolsCommand::Status => client.get("/tools/automatic").await?,
        AutoToolsCommand::Action { action, id, order } => {
            client
                .post(
                    "/tools/automatic/action",
                    &json!({"action":action,"id":id,"order":order}),
                )
                .await?
        }
        AutoToolsCommand::Start {
            endpoint,
            model,
            count,
            resume,
        } => {
            client.post("/tools/automatic/start", &json!({"endpoint":{"base_url":endpoint,"model":model},"count":count,"resume":resume})).await?;
            loop {
                let current = client.get("/tools/automatic").await?;
                if current["status"] != "running" {
                    break current;
                }
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {},
                    _ = tokio::signal::ctrl_c() => { break client.post("/tools/automatic/action", &json!({"action":"stop"})).await?; }
                }
            }
        }
    };
    out.data(&v, || serde_json::to_string_pretty(&v).unwrap_or_default());
    Ok(())
}
