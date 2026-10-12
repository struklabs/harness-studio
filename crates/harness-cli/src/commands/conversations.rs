//! `harness conversations ...`: stored chats through the Control API (`/conversations`):
//! list, show, export, documents, compaction.

use std::io::Write;
use std::path::PathBuf;

use reqwest::Method;
use serde_json::{json, Value};

use crate::client::{seg, ApiClient};
use crate::error::{CliError, CliResult};
use crate::output::{str_of, table, Out};

fn when(ms: &Value) -> String {
    let Some(ms) = ms.as_i64() else {
        return "-".into();
    };
    let secs = (harness_core::analytics::now_ms() as i64 - ms).max(0) / 1000;
    match secs {
        0..=59 => format!("{secs}s ago"),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

pub struct ListArgs {
    pub query: Option<String>,
    pub folder: Option<String>,
    pub pinned: bool,
    pub favorites: bool,
    pub limit: u32,
}

pub async fn ls(client: &ApiClient, a: ListArgs, out: &Out) -> CliResult<()> {
    let mut q = vec![format!("limit={}", a.limit)];
    if let Some(s) = a.query.as_deref().filter(|s| !s.is_empty()) {
        q.push(format!("query={}", seg(s)));
    }
    if let Some(f) = &a.folder {
        q.push(format!("folder_id={}", seg(f)));
    }
    if a.pinned {
        q.push("pinned_only=true".into());
    }
    if a.favorites {
        q.push("favorites_only=true".into());
    }
    let list = client
        .get(&format!("/conversations?{}", q.join("&")))
        .await?;
    out.data(&list, || {
        let rows: Vec<Vec<String>> = list
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|c| {
                vec![
                    str_of(&c["id"]),
                    str_of(&c["title"]),
                    c["model_alias"].as_str().unwrap_or("-").to_string(),
                    when(&c["updated_at"]),
                    match (c["pinned"] == true, c["favorite"] == true) {
                        (true, true) => "pinned, favorite".into(),
                        (true, false) => "pinned".into(),
                        (false, true) => "favorite".into(),
                        _ => String::new(),
                    },
                ]
            })
            .collect();
        if rows.is_empty() {
            return "No conversations.".into();
        }
        table(&["ID", "TITLE", "MODEL", "UPDATED", ""], &rows)
    });
    Ok(())
}

/// One message as text: heading line and its content.
fn message_text(m: &Value) -> String {
    let v = &m["variant"];
    let role = m["role"].as_str().unwrap_or("?");
    let role = if role == "system" && v["meta"]["compaction"].is_object() {
        "summary of earlier messages"
    } else {
        role
    };
    let mut text = format!("[{role}]");
    if let Some(model) = v["model_alias"].as_str().filter(|_| role == "assistant") {
        text.push_str(&format!(" ({model})"));
    }
    text.push('\n');
    if let Some(r) = v["reasoning"].as_str().filter(|r| !r.trim().is_empty()) {
        text.push_str(&format!("<reasoning>\n{}\n</reasoning>\n", r.trim()));
    }
    text.push_str(v["content"].as_str().unwrap_or("").trim_end());
    for call in v["tool_calls"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or_default()
    {
        text.push_str(&format!(
            "\n[tool call] {} {}",
            call["name"].as_str().unwrap_or("?"),
            call["arguments"].as_str().unwrap_or("")
        ));
    }
    if let Some(e) = v["error"].as_str().filter(|e| !e.is_empty()) {
        text.push_str(&format!("\n[error] {e}"));
    }
    text
}

pub async fn show(client: &ApiClient, id: &str, out: &Out) -> CliResult<()> {
    let view = client.get(&format!("/conversations/{}", seg(id))).await?;
    out.data(&view, || {
        let c = &view["conversation"];
        let mut text = format!(
            "{}\n{} · {} · updated {}\n",
            str_of(&c["title"]),
            str_of(&c["id"]),
            c["model_alias"].as_str().unwrap_or("no model"),
            when(&c["updated_at"])
        );
        if let Some(s) = c["system_prompt"].as_str().filter(|s| !s.trim().is_empty()) {
            text.push_str(&format!("\n[system prompt]\n{}\n", s.trim()));
        }
        for m in view["messages"]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or_default()
        {
            text.push('\n');
            text.push_str(&message_text(m));
            text.push('\n');
        }
        text
    });
    Ok(())
}

pub async fn export(
    client: &ApiClient,
    id: &str,
    format: &str,
    output: Option<PathBuf>,
    out: &Out,
) -> CliResult<()> {
    let value = client
        .get(&format!(
            "/conversations/{}/export?format={}",
            seg(id),
            seg(format)
        ))
        .await?;
    let text = match &value {
        Value::String(s) => s.clone(),
        v => serde_json::to_string_pretty(v).unwrap_or_default(),
    };
    match output {
        Some(path) => {
            std::fs::write(&path, &text).map_err(|e| CliError::general("cli.io", e.to_string()))?;
            out.info(format!("Wrote {}", path.display()));
        }
        None => {
            // A closed pipe (`| head`) is not an error.
            let _ = writeln!(std::io::stdout(), "{}", text.trim_end());
        }
    }
    Ok(())
}

pub async fn docs(client: &ApiClient, id: &str, out: &Out) -> CliResult<()> {
    let list = client
        .get(&format!("/conversations/{}/documents", seg(id)))
        .await?;
    out.data(&list, || {
        let rows: Vec<Vec<String>> = list
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|d| {
                vec![
                    str_of(&d["id"]),
                    str_of(&d["name"]),
                    str_of(&d["kind"]),
                    crate::output::bytes(d["size"].as_u64().unwrap_or(0)),
                    d["pages"]
                        .as_u64()
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "-".into()),
                    format!(
                        "{}/{}",
                        d["embedded"].as_u64().unwrap_or(0),
                        d["chunks"].as_u64().unwrap_or(0)
                    ),
                    str_of(&d["status"]),
                    str_of(&d["mode"]),
                ]
            })
            .collect();
        if rows.is_empty() {
            return "No documents.".into();
        }
        table(
            &[
                "ID", "NAME", "KIND", "SIZE", "PAGES", "INDEXED", "STATUS", "MODE",
            ],
            &rows,
        )
    });
    Ok(())
}

pub struct CompactArgs {
    pub id: String,
    pub model: Option<String>,
    pub n_ctx: Option<u64>,
    pub keep_turns: Option<usize>,
    pub keep_tool_results: bool,
    pub summary_tokens: Option<u64>,
    pub instruction: Option<String>,
}

pub async fn compact(client: &ApiClient, a: CompactArgs, out: &Out) -> CliResult<()> {
    let mut body = json!({"clear_tool_results": !a.keep_tool_results});
    for (key, value) in [
        ("model", a.model.map(Value::from)),
        ("n_ctx", a.n_ctx.map(Value::from)),
        ("keep_turns", a.keep_turns.map(Value::from)),
        ("summary_tokens", a.summary_tokens.map(Value::from)),
        ("instruction", a.instruction.map(Value::from)),
    ] {
        if let Some(v) = value {
            body[key] = v;
        }
    }
    out.progress("Summarizing the older turns...");
    let result = client
        .post(&format!("/conversations/{}/compact", seg(&a.id)), &body)
        .await;
    out.progress_done();
    let result = result?;
    out.data(&result, || {
        let m = &result["marker"];
        format!(
            "Compacted: {} turns summarized, {} kept as they are.\nPrompt tokens {} -> {} (summary {} tokens, {} request(s)).\nMarker message {} (undo with `harness conversations undo {} {}`).",
            str_of(&m["turns"]),
            str_of(&m["kept_turns"]),
            str_of(&m["tokens_before"]),
            str_of(&m["tokens_after"]),
            str_of(&m["summary_tokens"]),
            str_of(&result["requests"]),
            str_of(&result["marker_id"]),
            a.id,
            str_of(&result["marker_id"]),
        )
    });
    Ok(())
}

pub async fn undo(
    client: &ApiClient,
    id: &str,
    marker: &str,
    redo: bool,
    out: &Out,
) -> CliResult<()> {
    let result = client
        .request(
            Method::PUT,
            &format!("/conversations/{}/compactions/{}", seg(id), seg(marker)),
            Some(&json!({"undone": !redo})),
        )
        .await?;
    out.data(&result, || {
        if redo {
            "Compaction applied again.".to_string()
        } else {
            "Compaction undone: requests send the full history again.".to_string()
        }
    });
    Ok(())
}
