//! `harness kb ...`: knowledge bases through the Control API (`/knowledge-bases`): create, list,
//! delete, add and remove sources, index (only what changed), rebuild, ask with citations.

use std::time::Duration;

use reqwest::Method;
use serde_json::{json, Value};

use crate::client::{seg, ApiClient};
use crate::error::{CliError, CliResult};
use crate::output::{str_of, table, Out};

/// The id of the base named or identified by `key`.
async fn resolve(client: &ApiClient, key: &str) -> CliResult<String> {
    let list = client.get("/knowledge-bases").await?;
    let rows = list.as_array().map(|a| a.as_slice()).unwrap_or_default();
    if let Some(r) = rows.iter().find(|r| r["id"] == key) {
        return Ok(str_of(&r["id"]));
    }
    let named: Vec<&Value> = rows.iter().filter(|r| r["name"] == key).collect();
    match named.as_slice() {
        [one] => Ok(str_of(&one["id"])),
        [] => Err(CliError::general(
            "rag.kb_not_found",
            format!("No knowledge base \"{key}\"."),
        )),
        _ => Err(CliError::general(
            "rag.kb_ambiguous",
            format!("Several knowledge bases are named \"{key}\"; use the id."),
        )),
    }
}

fn mib(bytes: &Value) -> String {
    format!("{:.1} MiB", bytes.as_f64().unwrap_or(0.0) / 1_048_576.0)
}

fn secs(ms: &Value) -> String {
    format!("{:.1} s", ms.as_f64().unwrap_or(0.0) / 1000.0)
}

fn summary(kb: &Value) -> String {
    let mut s = format!(
        "{} ({})\n  state {}{}; {} documents, {} chunks, {} embedded; model {}",
        str_of(&kb["name"]),
        str_of(&kb["id"]),
        str_of(&kb["state"]),
        kb["stale_reason"]
            .as_str()
            .map(|r| format!(" ({r})"))
            .unwrap_or_default(),
        str_of(&kb["documents"]),
        str_of(&kb["chunks"]),
        str_of(&kb["embedded"]),
        kb["embed_model"].as_str().unwrap_or("none (keywords)"),
    );
    s.push_str(&format!(
        "\n  folder {}; OCR {}; speech {}; rerank {}",
        kb["folder"].as_str().unwrap_or("-"),
        kb["ocr_model"].as_str().unwrap_or("-"),
        kb["asr_model"].as_str().unwrap_or("-"),
        kb["rerank_model"].as_str().unwrap_or("off"),
    ));
    let d = &kb["disk"];
    s.push_str(&format!(
        "\n  disk: text {}, vectors {} (sources {})",
        mib(&d["text_bytes"]),
        mib(&d["vector_bytes"]),
        mib(&d["source_bytes"]),
    ));
    if let Some(r) = kb["stats"]["last_run"].as_object() {
        let r = Value::Object(r.clone());
        s.push_str(&format!(
            "\n  last run: {} (read {}, embed {}); {} added, {} changed, {} unchanged, {} removed, {} failed\n  CPU avg {:.0}% peak {:.0}%, memory peak {}",
            secs(&r["elapsed_ms"]),
            secs(&r["extract_ms"]),
            secs(&r["embed_ms"]),
            str_of(&r["added"]),
            str_of(&r["changed"]),
            str_of(&r["unchanged"]),
            str_of(&r["removed"]),
            str_of(&r["failed"]),
            r["cpu_avg_pct"].as_f64().unwrap_or(0.0),
            r["cpu_peak_pct"].as_f64().unwrap_or(0.0),
            mib(&r["rss_peak_bytes"]),
        ));
    }
    let q = &kb["stats"];
    if q["query_count"].as_u64().unwrap_or(0) > 0 {
        let avg =
            q["query_ms_total"].as_f64().unwrap_or(0.0) / q["query_count"].as_f64().unwrap_or(1.0);
        s.push_str(&format!(
            "\n  questions: {} (last {} ms, average {:.0} ms)",
            str_of(&q["query_count"]),
            str_of(&q["query_ms_last"]),
            avg
        ));
    }
    s
}

pub async fn ls(client: &ApiClient, out: &Out) -> CliResult<()> {
    let list = client.get("/knowledge-bases").await?;
    out.data(&list, || {
        let rows: Vec<Vec<String>> = list
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|k| {
                vec![
                    str_of(&k["id"]),
                    str_of(&k["name"]),
                    str_of(&k["state"]),
                    str_of(&k["documents"]),
                    str_of(&k["chunks"]),
                    k["embed_model"].as_str().unwrap_or("-").to_string(),
                ]
            })
            .collect();
        if rows.is_empty() {
            return "No knowledge bases.".into();
        }
        table(&["ID", "NAME", "STATE", "DOCS", "CHUNKS", "MODEL"], &rows)
    });
    Ok(())
}

pub struct CreateArgs {
    pub name: String,
    pub folder: Option<String>,
    pub ocr: Option<String>,
    pub asr: Option<String>,
    pub rerank: Option<String>,
}

pub async fn create(client: &ApiClient, a: CreateArgs, out: &Out) -> CliResult<()> {
    let body = json!({
        "name": a.name, "folder": a.folder, "ocr_model": a.ocr,
        "asr_model": a.asr, "rerank_model": a.rerank,
    });
    let kb = client.post("/knowledge-bases", &body).await?;
    out.data(&kb, || format!("Created {}", summary(&kb)));
    Ok(())
}

pub async fn show(
    client: &ApiClient,
    key: &str,
    embed_model: Option<&str>,
    out: &Out,
) -> CliResult<()> {
    let id = resolve(client, key).await?;
    let mut path = format!("/knowledge-bases/{}", seg(&id));
    if let Some(m) = embed_model {
        path.push_str(&format!("?embed_model={}", seg(m)));
    }
    let kb = client.get(&path).await?;
    out.data(&kb, || summary(&kb));
    Ok(())
}

pub async fn docs(client: &ApiClient, key: &str, out: &Out) -> CliResult<()> {
    let id = resolve(client, key).await?;
    let list = client
        .get(&format!("/knowledge-bases/{}/documents", seg(&id)))
        .await?;
    out.data(&list, || {
        let rows: Vec<Vec<String>> = list
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|d| {
                vec![
                    str_of(&d["name"]),
                    str_of(&d["kind"]),
                    str_of(&d["status"]),
                    str_of(&d["chunks"]),
                    d["meta"]["duration_ms"]
                        .as_u64()
                        .map(|ms| secs(&json!(ms)))
                        .unwrap_or_else(|| "-".into()),
                ]
            })
            .collect();
        if rows.is_empty() {
            return "No documents.".into();
        }
        table(&["NAME", "KIND", "STATUS", "CHUNKS", "LENGTH"], &rows)
    });
    Ok(())
}

pub struct SetArgs {
    pub name: Option<String>,
    pub folder: Option<String>,
    pub ocr: Option<String>,
    pub asr: Option<String>,
    pub rerank: Option<String>,
}

pub async fn set(client: &ApiClient, key: &str, a: SetArgs, out: &Out) -> CliResult<()> {
    let id = resolve(client, key).await?;
    let mut body = json!({});
    for (k, v) in [
        ("name", a.name),
        ("folder", a.folder),
        ("ocr_model", a.ocr),
        ("asr_model", a.asr),
        ("rerank_model", a.rerank),
    ] {
        if let Some(v) = v {
            body[k] = Value::from(v);
        }
    }
    let kb = client
        .request(
            Method::PATCH,
            &format!("/knowledge-bases/{}", seg(&id)),
            Some(&body),
        )
        .await?;
    out.data(&kb, || summary(&kb));
    Ok(())
}

pub async fn rm(client: &ApiClient, key: &str, out: &Out) -> CliResult<()> {
    let id = resolve(client, key).await?;
    let r = client
        .delete(&format!("/knowledge-bases/{}", seg(&id)))
        .await?;
    out.data(&r, || format!("Deleted {id}."));
    Ok(())
}

pub struct AddArgs {
    pub key: String,
    pub folder: Option<String>,
    pub paths: Vec<String>,
    pub no_index: bool,
    pub embed_model: Option<String>,
    pub wait: bool,
}

pub async fn add(client: &ApiClient, a: AddArgs, out: &Out) -> CliResult<()> {
    let id = resolve(client, &a.key).await?;
    let paths: Vec<String> = a
        .paths
        .iter()
        .map(|p| {
            std::fs::canonicalize(p)
                .map(|c| c.to_string_lossy().trim_start_matches(r"\\?\").to_string())
                .unwrap_or_else(|_| p.clone())
        })
        .collect();
    let folder = a.folder.as_ref().map(|f| {
        std::fs::canonicalize(f)
            .map(|c| c.to_string_lossy().trim_start_matches(r"\\?\").to_string())
            .unwrap_or_else(|_| f.clone())
    });
    let body = json!({
        "folder": folder, "paths": paths, "index": !a.no_index, "embed_model": a.embed_model,
    });
    let r = client
        .post(&format!("/knowledge-bases/{}/sources", seg(&id)), &body)
        .await?;
    out.data(&r, || format!("Added {} file(s).", str_of(&r["files"])));
    if a.wait && !a.no_index {
        wait(client, &id, out).await?;
    }
    Ok(())
}

pub async fn remove(
    client: &ApiClient,
    key: &str,
    sources: Vec<String>,
    out: &Out,
) -> CliResult<()> {
    let id = resolve(client, key).await?;
    let r = client
        .request(
            Method::DELETE,
            &format!("/knowledge-bases/{}/sources", seg(&id)),
            Some(&json!({ "sources": sources })),
        )
        .await?;
    out.data(&r, || {
        format!("Removed {} document(s).", str_of(&r["removed"]))
    });
    Ok(())
}

pub async fn index(
    client: &ApiClient,
    key: &str,
    rebuild: bool,
    embed_model: Option<String>,
    wait_for_it: bool,
    out: &Out,
) -> CliResult<()> {
    let id = resolve(client, key).await?;
    let r = client
        .post(
            &format!("/knowledge-bases/{}/index", seg(&id)),
            &json!({ "rebuild": rebuild, "embed_model": embed_model }),
        )
        .await?;
    out.data(&r, || {
        format!(
            "Indexing started with {}.",
            r["embed_model"].as_str().unwrap_or("keyword search only")
        )
    });
    if wait_for_it {
        wait(client, &id, out).await?;
    }
    Ok(())
}

/// Polls the run until it ends and prints the base.
async fn wait(client: &ApiClient, id: &str, out: &Out) -> CliResult<()> {
    out.progress("Indexing...");
    loop {
        tokio::time::sleep(Duration::from_millis(700)).await;
        let p = client
            .get(&format!("/knowledge-bases/{}/progress", seg(id)))
            .await?;
        if p["running"] != true {
            break;
        }
    }
    out.progress_done();
    let kb = client.get(&format!("/knowledge-bases/{}", seg(id))).await?;
    out.data(&kb, || summary(&kb));
    Ok(())
}

pub async fn cancel(client: &ApiClient, key: &str, out: &Out) -> CliResult<()> {
    let id = resolve(client, key).await?;
    let r = client
        .post(
            &format!("/knowledge-bases/{}/index/cancel", seg(&id)),
            &json!({}),
        )
        .await?;
    out.data(&r, || {
        if r["cancelled"] == true {
            "Stopping the run.".into()
        } else {
            "No run is active.".into()
        }
    });
    Ok(())
}

pub struct QueryArgs {
    pub key: String,
    pub question: String,
    pub top_k: usize,
    pub embed_model: Option<String>,
    pub rerank: Option<String>,
    pub no_rerank: bool,
}

/// `p. 3`, `pp. 3-4` or `1:05-1:32` for a source.
fn place(s: &Value) -> String {
    if let Some(t) = s["t_start_ms"].as_u64() {
        return harness_core::rag::media::format_span(t, s["t_end_ms"].as_u64());
    }
    match (s["page_start"].as_i64(), s["page_end"].as_i64()) {
        (Some(a), Some(b)) if a != b => format!("pp. {a}-{b}"),
        (Some(a), _) => format!("p. {a}"),
        _ => String::new(),
    }
}

pub async fn query(client: &ApiClient, a: QueryArgs, out: &Out) -> CliResult<()> {
    let id = resolve(client, &a.key).await?;
    let mut body = json!({
        "query": a.question, "top_k": a.top_k, "embed_model": a.embed_model,
    });
    if a.no_rerank {
        body["rerank"] = json!(false);
    } else if let Some(m) = a.rerank {
        body["rerank"] = json!(m);
    }
    let r = client
        .post(&format!("/knowledge-bases/{}/query", seg(&id)), &body)
        .await?;
    out.data(&r, || {
        let mut s = String::new();
        for src in r["sources"]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or_default()
        {
            let place = place(src);
            s.push_str(&format!(
                "[{}] {}{}{}\n    {}\n",
                str_of(&src["n"]),
                str_of(&src["document_name"]),
                if place.is_empty() {
                    String::new()
                } else {
                    format!(", {place}")
                },
                src["similarity"]
                    .as_f64()
                    .map(|v| format!("  (similarity {v:.2})"))
                    .unwrap_or_default(),
                str_of(&src["text"])
                    .replace('\n', " ")
                    .chars()
                    .take(220)
                    .collect::<String>(),
            ));
        }
        if s.is_empty() {
            s.push_str("No excerpts found.\n");
        }
        s.push_str(&format!(
            "{} search{}, {} ms",
            r["retrieval"].as_str().unwrap_or("-"),
            r["reranked"]
                .as_str()
                .map(|m| format!(" + rerank {m}"))
                .unwrap_or_default(),
            str_of(&r["latency_ms"]),
        ));
        s
    });
    Ok(())
}
