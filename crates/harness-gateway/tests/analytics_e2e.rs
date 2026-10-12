//! Request analytics end to end with a real llama-server and a real model (no mocks):
//!
//! ```text
//! HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<chat gguf>
//! cargo test -p harness-gateway --test analytics_e2e -- --ignored --nocapture
//! ```
//!
//! Temporary data root, in-memory secrets, gateway on 19000, the model on the next free
//! port, the real 1 Hz telemetry sampler. Everything started here is stopped here.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use harness_core::analytics::{self, RequestQuery, RequestRow};
use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::params::ParamValue;
use harness_core::servers::ServerRegistry;
use harness_core::telemetry::TelemetryPoller;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

const ALIAS: &str = "an-e2e-qwen";

fn env_path(name: &str) -> Option<PathBuf> {
    let p = std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_file());
    if p.is_none() {
        eprintln!("{name} not set, skipping");
    }
    p
}

fn data_root(bin: &Path) -> PathBuf {
    harness_core::secrets::use_memory_store();
    let dir = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("an-e2e"))
        .tempdir()
        .unwrap()
        .keep();
    harness_core::paths::init(dir.clone());
    let mut s = harness_core::config::AppSettings::load();
    s.runtime_mode = harness_core::config::RuntimeMode::Custom;
    s.custom_binary.binary_path = Some(bin.to_path_buf());
    s.custom_binary.is_valid = true;
    s.server.port = 19040;
    s.control_api.port = 19099;
    s.save().unwrap();
    dir
}

fn save(def: ModelDefinition) -> ModelDefinition {
    harness_core::domain::store::Domain::global()
        .save_definition(def)
        .unwrap()
}

/// Bytes of `analytics.db` and its WAL / shared-memory files.
fn db_bytes() -> Vec<u8> {
    let path = analytics::db_file();
    let mut out = Vec::new();
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut p = path.clone().into_os_string();
        p.push(suffix);
        if let Ok(b) = std::fs::read(PathBuf::from(p)) {
            out.extend(b);
        }
    }
    out
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

/// Percentile computed independently of `analytics::stats` (type 7, by hand).
fn p_by_hand(values: &[u64], p: f64) -> f64 {
    let mut v: Vec<f64> = values.iter().map(|x| *x as f64).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let rank = (v.len() - 1) as f64 * p / 100.0;
    let (lo, hi) = (rank.floor() as usize, rank.ceil() as usize);
    v[lo] + (v[hi] - v[lo]) * (rank - lo as f64)
}

async fn chat(
    http: &reqwest::Client,
    base: &str,
    headers: &[(&str, &str)],
    body: Value,
) -> (u16, String) {
    let mut req = http
        .post(format!("{}/v1/chat/completions", base))
        .json(&body);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = req.send().await.unwrap();
    let status = resp.status().as_u16();
    let mut text = String::new();
    let mut s = resp.bytes_stream();
    while let Some(Ok(c)) = s.next().await {
        text.push_str(&String::from_utf8_lossy(&c));
    }
    (status, text)
}

/// Rows, rollups, P50/P95 against the raw values, client distribution, VRAM peak from real
/// telemetry, privacy with capture off, content capture on, retention to rollups, `/metrics`.
#[tokio::test]
#[ignore]
async fn e2e_analytics_rows_rollups_privacy_retention() {
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_MODEL")) else {
        return;
    };
    let dir = data_root(&bin);
    let mut def = ModelDefinition::new(ALIAS, ModelRef::Local { path: model });
    def.load.insert("gpu_layers".into(), ParamValue::Int(99));
    def.load
        .insert("context_size".into(), ParamValue::Int(4096));
    let def = save(def);
    assert!(!def.capture_content, "content capture is off by default");

    let registry = Arc::new(ServerRegistry::default());
    let poller = Arc::new(tokio::sync::Mutex::new(TelemetryPoller::new(
        harness_core::gpu::list_adapters().into_iter().next(),
    )));
    analytics::set_telemetry(poller.clone());
    let sampler = tokio::spawn(harness_core::orchestrator::run_telemetry_sampler(
        poller.clone(),
        registry.clone(),
        |_| {},
    ));
    let server = GatewayServer::new();
    let status = server
        .start(
            GatewayContext {
                registry: registry.clone(),
                runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
                gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
                data_dir: dir.clone(),
                downloads: None,
            },
            &GatewaySettings {
                enabled: true,
                port: 19000,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();

    // --- Privacy: capture off. The prompt carries a marker the model repeats. ---
    let marker = format!("HSPRIV{:016x}", rand_u64());
    let (st, body) = chat(
        &http,
        &base,
        &[("user-agent", "curl/8.4.0")],
        json!({"model": ALIAS, "max_tokens": 40, "temperature": 0,
               "messages": [{"role": "user", "content": format!("Repeat exactly, nothing else: {marker}")}]}),
    )
    .await;
    assert_eq!(st, 200, "{body}");
    assert!(
        body.contains(&marker),
        "the reply repeats the marker: {body}"
    );

    // Different clients, streaming and not, two endpoints, one error.
    let prompts = [
        "Count from 1 to 20.",
        "Name three colors.",
        "Write one sentence about the sea.",
        "What is 12 times 12?",
        "List four fruits, comma separated.",
        "Say hello.",
    ];
    let clients: [&[(&str, &str)]; 6] = [
        &[("x-harness-client", "Harness Playground")],
        &[("x-harness-client", "Harness Playground")],
        &[("user-agent", "opencode/1.2.3 ai-sdk/5")],
        &[("user-agent", "opencode/1.2.3 ai-sdk/5")],
        &[("user-agent", "claude-cli/2.1.3 (external, cli)")],
        &[("x-harness-client", "Harness Playground")],
    ];
    for (i, (p, h)) in prompts.iter().zip(clients).enumerate() {
        let (st, body) = chat(
            &http,
            &base,
            h,
            json!({"model": ALIAS, "max_tokens": 24 + 8 * i, "temperature": 0,
                   "stream": i % 2 == 0, "messages": [{"role": "user", "content": p}]}),
        )
        .await;
        assert_eq!(st, 200, "{body}");
    }
    let resp = http
        .post(format!("{}/v1/completions", base))
        .header("user-agent", "curl/8.4.0")
        .json(&json!({"model": ALIAS, "prompt": "1, 2, 3,", "max_tokens": 8}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let _ = resp.bytes().await;
    let (st, _) = chat(
        &http,
        &base,
        &[("user-agent", "curl/8.4.0")],
        json!({"model": "no-such-model", "messages": [{"role": "user", "content": "x"}]}),
    )
    .await;
    assert_eq!(st, 404);

    // Let the telemetry sample after the last request land, then write.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(analytics::flush(Duration::from_secs(10)));

    let page = analytics::requests(&RequestQuery {
        limit: Some(100),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(page.total, 9, "{:#?}", page.requests);
    let rows: Vec<&RequestRow> = page
        .requests
        .iter()
        .filter(|r| r.alias.as_deref() == Some(ALIAS))
        .collect();
    assert_eq!(rows.len(), 8);
    let first = rows.last().unwrap(); // newest first
    assert!(
        first.load_ms.is_some(),
        "the first request loaded the model"
    );
    assert!(first.queue_wait_ms >= first.load_ms.unwrap());
    for r in &rows {
        eprintln!(
            "{} {} {} client={:?} status={} ttft={:?} total={} p={:?} c={:?} pps={:?} gps={:?} ctx={:?} fill={:?} vram={:?} ram={:?} backend={:?} finish={:?}",
            r.id, r.endpoint, r.stream, r.client, r.status, r.ttft_ms, r.total_ms,
            r.prompt_tokens, r.completion_tokens, r.prompt_per_second, r.predicted_per_second,
            r.ctx_size, r.context_fill, r.vram_peak_bytes, r.ram_peak_bytes, r.backend, r.finish_reason
        );
        assert_eq!(r.status, 200);
        assert!(r.error_code.is_none());
        assert!(r.ttft_ms.is_some());
        assert!(r.prompt_tokens.unwrap_or(0) > 0);
        assert!(r.completion_tokens.unwrap_or(0) > 0);
        assert!(r.predicted_per_second.unwrap_or(0.0) > 0.0);
        assert!(r.finish_reason.is_some());
        assert_eq!(r.ctx_size, Some(4096));
        let used = r.prompt_tokens.unwrap() + r.completion_tokens.unwrap();
        assert!((r.context_fill.unwrap() - used as f64 / 4096.0).abs() < 1e-9);
        assert!(r.backend.is_some());
        assert_eq!(r.server_id.as_deref(), Some("gw-an-e2e-qwen"));
        assert!(r.vram_peak_bytes.is_some(), "VRAM peak from telemetry");
        assert!(!r.content_captured);
    }
    let err = page
        .requests
        .iter()
        .find(|r| r.alias.is_none())
        .expect("error row");
    assert_eq!(err.status, 404);
    assert_eq!(err.error_code.as_deref(), Some("model_not_found"));
    assert_eq!(err.model.as_deref(), Some("no-such-model"));

    // Filters.
    let pg = analytics::requests(&RequestQuery {
        client: Some("Harness Playground".into()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(pg.total, 3);
    let errs = analytics::requests(&RequestQuery {
        status: Some("error".into()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(errs.total, 1);
    let comp = analytics::requests(&RequestQuery {
        endpoint: Some("/v1/completions".into()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(comp.total, 1);

    // Summary against the raw rows.
    let s = analytics::summary(None, None).unwrap();
    assert_eq!(s.totals.requests, 9);
    assert_eq!(s.totals.errors, 1);
    let m = s.models.iter().find(|m| m.alias == ALIAS).unwrap();
    assert_eq!(m.requests, 8);
    assert_eq!(m.errors, 0);
    assert_eq!(m.loads, 1);
    let totals: Vec<u64> = rows.iter().map(|r| r.total_ms).collect();
    let ttfts: Vec<u64> = rows.iter().map(|r| r.ttft_ms.unwrap()).collect();
    let near = |a: Option<f64>, b: f64| (a.unwrap() - b).abs() < 1e-6;
    assert!(near(m.latency_p50_ms, p_by_hand(&totals, 50.0)));
    assert!(near(m.latency_p95_ms, p_by_hand(&totals, 95.0)));
    assert!(near(m.ttft_p50_ms, p_by_hand(&ttfts, 50.0)));
    assert!(near(m.ttft_p95_ms, p_by_hand(&ttfts, 95.0)));
    eprintln!(
        "latency P50 {:?} P95 {:?} (raw {:?}); TTFT P50 {:?} P95 {:?} (raw {:?})",
        m.latency_p50_ms, m.latency_p95_ms, totals, m.ttft_p50_ms, m.ttft_p95_ms, ttfts
    );
    let sum = |f: fn(&RequestRow) -> Option<u64>| -> u64 { rows.iter().filter_map(|r| f(r)).sum() };
    assert_eq!(m.prompt_tokens, sum(|r| r.prompt_tokens));
    assert_eq!(m.completion_tokens, sum(|r| r.completion_tokens));
    assert_eq!(m.busy_ms, totals.iter().sum::<u64>());
    let gen_n: f64 = rows
        .iter()
        .map(|r| r.predicted_per_second.unwrap() * r.predicted_ms.unwrap() / 1000.0)
        .sum();
    let gen_ms: f64 = rows.iter().map(|r| r.predicted_ms.unwrap()).sum();
    assert!((m.avg_gen_tps.unwrap() - gen_n / gen_ms * 1000.0).abs() < 1e-6);
    assert_eq!(
        m.vram_peak_bytes,
        rows.iter().filter_map(|r| r.vram_peak_bytes).max()
    );
    let playground = s
        .clients
        .iter()
        .find(|c| c.client == "Harness Playground")
        .unwrap();
    assert_eq!(playground.requests, 3);
    assert!((playground.share - 3.0 / 9.0).abs() < 1e-9);
    assert_eq!(
        s.clients
            .iter()
            .find(|c| c.client == "opencode")
            .unwrap()
            .requests,
        2
    );
    assert_eq!(
        s.clients
            .iter()
            .find(|c| c.client == "claude-cli")
            .unwrap()
            .requests,
        1
    );
    assert_eq!(s.daily.len(), 1);
    assert_eq!(s.daily[0].requests, 9);

    // Detail and timeline.
    let d = analytics::request(&first.id).unwrap().unwrap();
    let kinds: Vec<&str> = d.timeline.iter().map(|p| p.kind.as_str()).collect();
    assert_eq!(kinds[0], "load", "{:?}", d.timeline);
    assert!(kinds.contains(&"prompt") && kinds.contains(&"generation"));
    assert!(d.content.is_none());

    // Privacy: the marker the prompt and the answer carried is nowhere in the files.
    analytics::shared().unwrap().checkpoint();
    let bytes = db_bytes();
    assert!(!bytes.is_empty());
    assert!(
        !contains(&bytes, &marker),
        "prompt/answer text reached analytics.db"
    );
    assert!(!contains(&bytes, "Count from 1 to 20"));

    // --- Content capture on (per definition, 1 day). ---
    let mut on = def.clone();
    on.capture_content = true;
    on.content_retention_days = Some(1);
    save(on);
    tokio::time::sleep(Duration::from_millis(1200)).await; // definitions are re-read each second
    let marker2 = format!("HSCAP{:016x}", rand_u64());
    let (st, body) = chat(
        &http,
        &base,
        &[("x-harness-client", "capture-test")],
        json!({"model": ALIAS, "max_tokens": 40, "temperature": 0, "stream": true,
               "messages": [{"role": "user", "content": format!("Repeat exactly, nothing else: {marker2}")}]}),
    )
    .await;
    assert_eq!(st, 200);
    let answer: String = body
        .lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .filter_map(|d| serde_json::from_str::<Value>(d).ok())
        .filter_map(|v| {
            v["choices"][0]["delta"]["content"]
                .as_str()
                .map(String::from)
        })
        .collect();
    assert!(answer.contains(&marker2), "{answer}");
    assert!(analytics::flush(Duration::from_secs(10)));
    let cap = analytics::requests(&RequestQuery {
        client: Some("capture-test".into()),
        ..Default::default()
    })
    .unwrap();
    let cap_row = &cap.requests[0];
    assert!(cap_row.content_captured);
    let d = analytics::request(&cap_row.id).unwrap().unwrap();
    let c = d.content.expect("captured content");
    assert!(c.request.contains(&marker2));
    assert!(c.response.contains("data:"), "the SSE stream as received");
    assert!(c.expires_at_ms - c.captured_at_ms == 86_400_000);
    analytics::shared().unwrap().checkpoint();
    assert!(contains(&db_bytes(), &marker2));

    // --- A client that goes away mid-stream: cancelled, not an error. ---
    let resp = http
        .post(format!("{}/v1/chat/completions", base))
        .header("x-harness-client", "quitter")
        .json(&json!({"model": ALIAS, "max_tokens": 400, "stream": true,
               "messages": [{"role": "user", "content": "Write a long story about a lighthouse keeper."}]}))
        .send()
        .await
        .unwrap();
    let mut stream = resp.bytes_stream();
    for _ in 0..3 {
        stream.next().await.unwrap().unwrap();
    }
    drop(stream);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(analytics::flush(Duration::from_secs(10)));
    let q = analytics::requests(&RequestQuery {
        client: Some("quitter".into()),
        ..Default::default()
    })
    .unwrap();
    let quit = &q.requests[0];
    assert!(quit.cancelled, "{quit:?}");
    assert!(!quit.is_error());
    assert!(quit.finish_reason.is_none());
    assert_eq!(
        analytics::requests(&RequestQuery {
            status: Some("cancelled".into()),
            ..Default::default()
        })
        .unwrap()
        .total,
        1
    );

    // --- /metrics (unified exposition). ---
    let text = http
        .get(format!("{}/metrics", base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(text.contains(&format!(
        "harness_gateway_requests_total{{model=\"{ALIAS}\",endpoint=\"/v1/chat/completions\",status=\"200\"}} 9"
    )), "{text}");
    assert!(text.contains(&format!(
        "harness_gateway_ttft_seconds_count{{model=\"{ALIAS}\"}} 10"
    )));
    assert!(text.contains(&format!(
        "harness_gateway_requests_cancelled_total{{model=\"{ALIAS}\"}} 1"
    )));
    assert!(text
        .contains("harness_gateway_request_errors_total{model=\"\",code=\"model_not_found\"} 1"));
    assert!(text.contains(&format!(
        "harness_gateway_model_loaded{{model=\"{ALIAS}\"}} 1"
    )));
    assert!(
        text.contains("llamacpp:prompt_tokens_total{server=\"gw-an-e2e-qwen\"}"),
        "{text}"
    );
    assert!(text.contains("harness_analytics_records_dropped_total 0"));
    assert!(text.contains("harness_system_vram_used_bytes"));

    // --- Retention: two days later with 1 day of raw rows → rollups only. ---
    let before = analytics::summary(None, None).unwrap();
    let report = analytics::shared()
        .unwrap()
        .maintain(analytics::now_ms() + 2 * analytics::DAY_MS, 1)
        .unwrap();
    eprintln!("maintenance: {report:?}");
    assert_eq!(report.rows_deleted, 11);
    assert_eq!(report.contents_deleted, 2); // capture was still on for the cancelled one
    assert_eq!(report.days_rolled_up, 1);
    let info = analytics::info();
    let storage = info.storage.unwrap();
    assert_eq!(storage.raw_rows, 0);
    assert!(storage.rollup_rows >= 4); // (alias, client, endpoint) groups
    let after = analytics::summary(None, None).unwrap();
    assert_eq!(after.rollup_days, 1);
    assert_eq!(after.totals.requests, before.totals.requests);
    assert_eq!(after.totals.errors, before.totals.errors);
    assert_eq!(after.totals.prompt_tokens, before.totals.prompt_tokens);
    assert_eq!(
        after.totals.completion_tokens,
        before.totals.completion_tokens
    );
    assert_eq!(after.totals.busy_ms, before.totals.busy_ms);
    assert_eq!(after.totals.vram_peak_bytes, before.totals.vram_peak_bytes);
    let (ma, mb) = (
        after.models.iter().find(|m| m.alias == ALIAS).unwrap(),
        before.models.iter().find(|m| m.alias == ALIAS).unwrap(),
    );
    assert!((ma.avg_gen_tps.unwrap() - mb.avg_gen_tps.unwrap()).abs() < 1e-6);
    assert_eq!(ma.last_used_ms, mb.last_used_ms);
    assert_eq!(ma.latency_p95_ms, None, "percentiles need raw rows");
    assert_eq!(after.clients, before.clients);
    assert_eq!(after.daily, before.daily);
    // Expired content is overwritten, not left in free pages or the WAL.
    assert!(!contains(&db_bytes(), &marker2));

    analytics::clear().unwrap();
    assert_eq!(analytics::info().storage.unwrap().rollup_rows, 0);

    sampler.abort();
    server.stop().await;
    registry.stop_all().await;
}

fn rand_u64() -> u64 {
    let mut b = [0u8; 8];
    getrandom(&mut b);
    u64::from_le_bytes(b)
}

fn getrandom(buf: &mut [u8]) {
    // Time + address entropy is enough for a unique marker.
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let a = buf.as_ptr() as usize as u128;
    let v = (t ^ (a << 17)).to_le_bytes();
    buf.copy_from_slice(&v[..buf.len()]);
}
