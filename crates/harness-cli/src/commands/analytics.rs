//! `harness analytics` and `harness requests`: the gateway request log from the
//! Control API (`/analytics/*`). Managed-endpoint traffic only; direct llama-server ports are
//! not observed.

use serde_json::Value;

use crate::client::{seg, ApiClient};
use crate::error::CliResult;
use crate::output::{bytes, table, Out};

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
}

fn ms(v: &Value) -> String {
    match num(v) {
        Some(x) if x >= 10_000.0 => format!("{:.1} s", x / 1000.0),
        Some(x) => format!("{:.0} ms", x),
        None => "-".into(),
    }
}

fn tps(v: &Value) -> String {
    num(v)
        .map(|x| format!("{:.1}", x))
        .unwrap_or_else(|| "-".into())
}

fn pct(v: &Value) -> String {
    num(v)
        .map(|x| format!("{:.1}%", x * 100.0))
        .unwrap_or_else(|| "-".into())
}

fn count(v: &Value) -> String {
    v.as_u64()
        .map(|n| n.to_string())
        .unwrap_or_else(|| "-".into())
}

fn when(v: &Value) -> String {
    let Some(ms) = v.as_u64() else {
        return "-".into();
    };
    let secs = (harness_core::analytics::now_ms().saturating_sub(ms)) / 1000;
    match secs {
        0..=59 => format!("{}s ago", secs),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

fn str_or<'a>(v: &'a Value, dflt: &'a str) -> &'a str {
    v.as_str().filter(|s| !s.is_empty()).unwrap_or(dflt)
}

const DIRECT_NOTE: &str = "Only requests through the managed endpoint (gateway) are recorded one by one; direct llama-server ports show only as an aggregate.";

/// One line for the tokens that bypassed the gateway (aggregate from the servers' /metrics).
fn direct_line(s: &Value) -> Option<String> {
    let d = &s["direct"];
    let (p, c) = (
        d["prompt_tokens"].as_u64()?,
        d["completion_tokens"].as_u64()?,
    );
    (p + c > 0).then(|| {
        format!(
            "Direct (outside the gateway, aggregate only): {p} prompt / {c} generated tokens, from server /metrics minus gateway-recorded tokens."
        )
    })
}

pub async fn summary(client: &ApiClient, range: &str, out: &Out) -> CliResult<()> {
    let s = client
        .get(&format!("/analytics/summary?range={}", seg(range)))
        .await?;
    out.data(&s, || {
        let models = s["models"].as_array().cloned().unwrap_or_default();
        if models.is_empty() {
            let mut text = format!("No gateway requests recorded in this range ({range}).");
            if let Some(line) = direct_line(&s) {
                text.push_str(&format!("\n{line}"));
            }
            return format!("{text}\n{DIRECT_NOTE}");
        }
        let rows: Vec<Vec<String>> = models
            .iter()
            .chain(std::iter::once(&s["totals"]))
            .map(|m| {
                vec![
                    match str_or(&m["alias"], "(unresolved)") {
                        "*" => "TOTAL".to_string(),
                        a => a.to_string(),
                    },
                    count(&m["requests"]),
                    format!(
                        "{}/{}",
                        count(&m["prompt_tokens"]),
                        count(&m["completion_tokens"])
                    ),
                    tps(&m["avg_gen_tps"]),
                    format!("{} / {}", ms(&m["ttft_p50_ms"]), ms(&m["ttft_p95_ms"])),
                    format!("{} / {}", ms(&m["latency_p50_ms"]), ms(&m["latency_p95_ms"])),
                    m["vram_peak_bytes"]
                        .as_u64()
                        .map(bytes)
                        .unwrap_or_else(|| "-".into()),
                    pct(&m["error_rate"]),
                    pct(&m["oom_rate"]),
                    when(&m["last_used_ms"]),
                ]
            })
            .collect();
        let mut text = table(
            &[
                "MODEL",
                "RUNS",
                "TOKENS IN/OUT",
                "GEN T/S",
                "TTFT P50/P95",
                "LATENCY P50/P95",
                "VRAM PEAK",
                "ERRORS",
                "OOM",
                "LAST USED",
            ],
            &rows,
        );
        let clients: Vec<String> = s["clients"]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|c| {
                format!(
                    "{} {:.0}% ({})",
                    str_or(&c["client"], "unknown"),
                    c["share"].as_f64().unwrap_or(0.0) * 100.0,
                    count(&c["requests"])
                )
            })
            .collect();
        if !clients.is_empty() {
            text.push_str(&format!("\n\nClients: {}", clients.join(", ")));
        }
        let benches: Vec<String> = models
            .iter()
            .filter(|m| m["benchmark"].is_object())
            .map(|m| {
                let b = &m["benchmark"];
                format!(
                    "{}: real gen {} t/s vs llama-bench tg {} t/s; real prompt {} t/s vs pp {} t/s",
                    str_or(&m["alias"], "?"),
                    tps(&m["avg_gen_tps"]),
                    tps(&b["gen_tps"]),
                    tps(&m["avg_prompt_tps"]),
                    tps(&b["prompt_tps"])
                )
            })
            .collect();
        if !benches.is_empty() {
            text.push_str(&format!("\nBenchmark vs real use:\n  {}", benches.join("\n  ")));
        }
        if s["rollup_days"].as_u64().unwrap_or(0) > 0 {
            text.push_str(&format!(
                "\n{} day(s) older than the raw retention come from daily rollups (no percentiles).",
                count(&s["rollup_days"])
            ));
        }
        if let Some(line) = direct_line(&s) {
            text.push_str(&format!("\n{line}"));
        }
        text.push_str(&format!("\n{DIRECT_NOTE}"));
        text
    });
    Ok(())
}

pub struct RequestsArgs {
    pub id: Option<String>,
    pub model: Option<String>,
    pub client: Option<String>,
    pub status: Option<String>,
    /// Only requests with a tool-call finding.
    pub tool_issue: bool,
    /// Only findings of this category (`runtime`, `context`, `model`, `client`).
    pub tool_category: Option<String>,
    pub limit: u32,
}

pub async fn requests(client: &ApiClient, args: RequestsArgs, out: &Out) -> CliResult<()> {
    if let Some(id) = &args.id {
        return request(client, id, out).await;
    }
    let mut query = vec![format!("limit={}", args.limit.clamp(1, 1000))];
    for (k, v) in [
        ("model", &args.model),
        ("client", &args.client),
        ("status", &args.status),
    ] {
        if let Some(v) = v {
            query.push(format!("{}={}", k, seg(v)));
        }
    }
    if args.tool_issue {
        query.push("tool_issue=true".into());
    }
    if let Some(c) = &args.tool_category {
        query.push(format!("tool_category={}", seg(c)));
    }
    let page = client
        .get(&format!("/analytics/requests?{}", query.join("&")))
        .await?;
    out.data(&page, || {
        let list = page["requests"].as_array().cloned().unwrap_or_default();
        if list.is_empty() {
            return format!("No gateway requests recorded.\n{DIRECT_NOTE}");
        }
        let rows: Vec<Vec<String>> = list
            .iter()
            .map(|r| {
                vec![
                    str_or(&r["id"], "-").to_string(),
                    when(&r["started_at_ms"]),
                    str_or(&r["alias"], str_or(&r["model"], "-")).to_string(),
                    str_or(&r["client"], "-").to_string(),
                    str_or(&r["endpoint"], "-").to_string(),
                    {
                        let status = match r["error_code"].as_str() {
                            Some(e) => format!("{} {}", count(&r["status"]), e),
                            None if r["cancelled"].as_bool() == Some(true) => "cancelled".into(),
                            None => count(&r["status"]),
                        };
                        // A tool-call finding rides along: who is at fault and what.
                        match (
                            r["tool_diag_category"].as_str(),
                            r["tool_diag_code"].as_str(),
                        ) {
                            (Some(cat), Some(code)) => format!("{status} tool:{cat}/{code}"),
                            _ => status,
                        }
                    },
                    format!(
                        "{}/{}",
                        count(&r["prompt_tokens"]),
                        count(&r["completion_tokens"])
                    ),
                    ms(&r["ttft_ms"]),
                    ms(&r["total_ms"]),
                    tps(&r["predicted_per_second"]),
                ]
            })
            .collect();
        let mut text = table(
            &[
                "ID", "WHEN", "MODEL", "CLIENT", "ENDPOINT", "STATUS", "TOKENS", "TTFT", "TOTAL",
                "GEN T/S",
            ],
            &rows,
        );
        text.push_str(&format!(
            "\n{} of {} request(s). `harness requests <ID>` shows one in detail.",
            list.len(),
            count(&page["total"])
        ));
        text
    });
    Ok(())
}

async fn request(client: &ApiClient, id: &str, out: &Out) -> CliResult<()> {
    let d = client
        .get(&format!("/analytics/requests/{}", seg(id)))
        .await?;
    out.data(&d, || {
        let r = &d["request"];
        let mut lines = vec![
            format!("Request   {}", str_or(&r["id"], "-")),
            format!(
                "Model     {} (server {}, backend {})",
                str_or(&r["alias"], str_or(&r["model"], "-")),
                str_or(&r["server_id"], "-"),
                str_or(&r["backend"], "-")
            ),
            format!(
                "Client    {} (User-Agent {}, token {})",
                str_or(&r["client"], "-"),
                str_or(&r["user_agent"], "-"),
                str_or(&r["token_name"], "-")
            ),
            format!(
                "Endpoint  {}{}",
                str_or(&r["endpoint"], "-"),
                if r["stream"].as_bool() == Some(true) {
                    " (stream)"
                } else {
                    ""
                }
            ),
            format!(
                "Status    {}{}{}{}",
                count(&r["status"]),
                r["error_code"]
                    .as_str()
                    .map(|e| format!(" {}", e))
                    .unwrap_or_default(),
                if r["oom"].as_bool() == Some(true) {
                    " OOM"
                } else {
                    ""
                },
                if r["cancelled"].as_bool() == Some(true) {
                    " cancelled"
                } else {
                    ""
                }
            ),
            format!(
                "Tokens    prompt {} (cached {}), completion {}; finish {}",
                count(&r["prompt_tokens"]),
                count(&r["cached_tokens"]),
                count(&r["completion_tokens"]),
                str_or(&r["finish_reason"], "-")
            ),
            format!(
                "Speed     prompt {} t/s, generation {} t/s; context {} of {}",
                tps(&r["prompt_per_second"]),
                tps(&r["predicted_per_second"]),
                pct(&r["context_fill"]),
                count(&r["ctx_size"])
            ),
            format!(
                "Times     TTFT {}, total {}, queue {}, load {}",
                ms(&r["ttft_ms"]),
                ms(&r["total_ms"]),
                ms(&r["queue_wait_ms"]),
                ms(&r["load_ms"])
            ),
            format!(
                "Memory    VRAM peak {}, RAM peak {}",
                r["vram_peak_bytes"]
                    .as_u64()
                    .map(bytes)
                    .unwrap_or_else(|| "-".into()),
                r["ram_peak_bytes"]
                    .as_u64()
                    .map(bytes)
                    .unwrap_or_else(|| "-".into())
            ),
        ];
        if let Some(rate) = d["spec_accept_rate"].as_f64() {
            lines.push(format!("Draft     {:.1}% accepted", rate * 100.0));
        }
        lines.push("Timeline".into());
        for p in d["timeline"]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or_default()
        {
            lines.push(format!(
                "  {:<11} +{:>8}  {}",
                str_or(&p["kind"], "?"),
                ms(&p["start_ms"]),
                ms(&p["duration_ms"])
            ));
        }
        if let (Some(cat), Some(code)) = (
            r["tool_diag_category"].as_str(),
            r["tool_diag_code"].as_str(),
        ) {
            lines.push(format!(
                "Tool diag {cat}: {code} ({} tool(s) offered, {} call(s))",
                count(&r["tools_offered"]),
                count(&r["tool_calls"])
            ));
            if let Some(detail) = r["tool_diag_detail"].as_str() {
                lines.push(format!("          evidence {detail}"));
            }
        } else if r["tool_involved"].as_bool() == Some(true) {
            lines.push(format!(
                "Tool diag none ({} tool(s) offered, {} call(s))",
                count(&r["tools_offered"]),
                count(&r["tool_calls"])
            ));
        }
        if d["content"].is_object() {
            lines.push("Content   captured (use --json to see it)".into());
        }
        lines.join("\n")
    });
    Ok(())
}
