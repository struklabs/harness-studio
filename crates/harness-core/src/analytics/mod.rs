//! Request analytics: every request that passes the gateway is stored as
//! metadata in `<data root>/analytics.db` (SQLite, WAL; separate from `state.db`).
//!
//! # Recording
//! The gateway calls [`submit`] once per request (the proxy's `RecordGuard`). `submit`
//! updates the in-memory Prometheus counters and puts the record on a bounded queue
//! ([`QUEUE_CAPACITY`]); it never blocks and never touches the disk. When the queue is full
//! (disk too slow) the record is dropped and counted (`RecorderStatus::dropped`,
//! `harness_analytics_records_dropped_total`). A dedicated thread (`harness-analytics`)
//! drains the queue in batches, adds the VRAM/RAM peak of the request window from the 1 Hz
//! telemetry history (see [`set_telemetry`]), and writes one transaction per batch. Records
//! wait [`ENRICH_DELAY`] before they are written so the telemetry sample taken right after a
//! request ends is included.
//!
//! # What is stored
//! `requests` (raw rows): time, endpoint, model / alias / definition, server, client (label,
//! `User-Agent`, `X-Harness-Client`, token name), stream, HTTP status, error code, OOM,
//! cancelled, queue wait, JIT load time, evicted models, TTFT, total time, prompt /
//! completion / cached tokens, llama.cpp prompt and generation time and speed, drafted and
//! accepted tokens, finish reason, slot context size and fill, VRAM / RAM peak, backend,
//! compatibility notes. **Never** the request or response content.
//!
//! `daily_rollup`: per (UTC day, alias, client, endpoint) sums, maxima and counts, made from
//! the raw rows of finished days. Kept forever. `request_content`: only for definitions with
//! `capture_content` on; deleted after `content_retention_days`.
//!
//! # Retention
//! [`AnalyticsDb::maintain`] (the writer thread runs it shortly after start and every hour):
//! rolls finished days up, deletes raw rows older than `raw_retention_days` (default 30)
//! whole days and expired content. `secure_delete` is on, so deleted content is overwritten.
//!
//! # Other tables in the same file
//! `rate_counters` (persistent per-token budgets, [`counters`]) and `direct_usage` / `direct_daily` (token
//! totals of llama-server ports that bypass the gateway, derived from `/metrics`, [`direct`]).
//!
//! `tool_calls` (schema 4, [`tools`]): the tool log of `tools::ToolService`, one row per tool call
//! from any client; kept for the raw retention.
//!
//! # Schema
//! See `schema.rs`; `PRAGMA user_version` migrations like `state.db`. A newer file opens
//! read-only.

mod content;
pub mod counters;
mod db;
pub mod direct;
pub mod prometheus;
mod schema;
pub mod stats;
mod tools;

pub use content::{clean, CapturedContent, ContentCapture, StoredContent, CAPTURE_LIMIT};
pub use db::*;
pub use direct::DirectUsage;
pub use tools::{ToolCallQuery, ToolCallRow};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::error_key::err;
use crate::gateway::record::RequestRecord;
use crate::telemetry::{TelemetryHistoryPoint, TelemetryPoller};

pub const DEFAULT_RAW_RETENTION_DAYS: u32 = 30;
pub const DEFAULT_CONTENT_RETENTION_DAYS: u32 = 7;
/// Records waiting to be written; beyond this they are dropped (and counted).
pub const QUEUE_CAPACITY: usize = 4096;
/// Largest batch per transaction.
const BATCH: usize = 512;
/// How long a record waits before it is written (telemetry catches up).
pub const ENRICH_DELAY: Duration = Duration::from_millis(1200);
const MAINTENANCE_EVERY: Duration = Duration::from_secs(3600);
const FIRST_MAINTENANCE: Duration = Duration::from_secs(30);

/// `settings.analytics`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AnalyticsSettings {
    /// Record gateway request metadata (on by default; content is a separate, per-definition
    /// opt-in).
    pub enabled: bool,
    /// Days raw request rows are kept (1–3650); daily rollups are kept forever.
    pub raw_retention_days: u32,
}

impl Default for AnalyticsSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            raw_retention_days: DEFAULT_RAW_RETENTION_DAYS,
        }
    }
}

impl AnalyticsSettings {
    pub fn retention_days(&self) -> u32 {
        self.raw_retention_days.clamp(1, 3650)
    }
}

/// Counters of the recorder since this process started.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RecorderStatus {
    /// The writer thread has started (after the first request).
    pub running: bool,
    pub written: u64,
    /// Dropped because the queue was full.
    pub dropped: u64,
    /// Skipped because recording is turned off.
    pub skipped: u64,
    pub write_errors: u64,
    pub last_error: Option<String>,
}

static WRITTEN: AtomicU64 = AtomicU64::new(0);
static DROPPED: AtomicU64 = AtomicU64::new(0);
static SKIPPED: AtomicU64 = AtomicU64::new(0);
static WRITE_ERRORS: AtomicU64 = AtomicU64::new(0);

fn last_error() -> &'static Mutex<Option<String>> {
    static E: Mutex<Option<String>> = Mutex::new(None);
    &E
}

pub fn recorder_status() -> RecorderStatus {
    RecorderStatus {
        running: SENDER.get().is_some(),
        written: WRITTEN.load(Ordering::Relaxed),
        dropped: DROPPED.load(Ordering::Relaxed),
        skipped: SKIPPED.load(Ordering::Relaxed),
        write_errors: WRITE_ERRORS.load(Ordering::Relaxed),
        last_error: last_error().lock().ok().and_then(|e| e.clone()),
    }
}

/// Unix milliseconds now.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `<data root>/analytics.db`.
pub fn db_file() -> PathBuf {
    crate::paths::data_root().join("analytics.db")
}

/// The process-wide handle to `analytics.db`, opened (and migrated) on first use. Call after
/// `paths::init`.
pub fn shared() -> Result<Arc<AnalyticsDb>, String> {
    static SHARED: OnceLock<Result<Arc<AnalyticsDb>, String>> = OnceLock::new();
    SHARED
        .get_or_init(|| AnalyticsDb::open(&db_file()).map(Arc::new))
        .clone()
}

type Telemetry = Arc<tokio::sync::Mutex<TelemetryPoller>>;

fn telemetry_slot() -> &'static Mutex<Option<Telemetry>> {
    static T: Mutex<Option<Telemetry>> = Mutex::new(None);
    &T
}

/// The telemetry poller whose 1 Hz history gives each request its VRAM/RAM peak (and the
/// `/metrics` exposition its llama-server values). The GUI and `harness serve` set it.
pub fn set_telemetry(poller: Telemetry) {
    *telemetry_slot().lock().unwrap_or_else(|e| e.into_inner()) = Some(poller);
}

pub(crate) fn telemetry() -> Option<Telemetry> {
    telemetry_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

enum Msg {
    Record(Box<RequestRecord>, Option<Box<CapturedContent>>, Instant),
    Flush(SyncSender<()>),
}

static SENDER: OnceLock<SyncSender<Msg>> = OnceLock::new();

fn sender() -> &'static SyncSender<Msg> {
    SENDER.get_or_init(|| {
        let (tx, rx) = sync_channel(QUEUE_CAPACITY);
        let spawned = std::thread::Builder::new()
            .name("harness-analytics".into())
            .spawn(move || writer(rx));
        if let Err(e) = spawned {
            set_error(err("analytics.writer_failed").with("error", e).into());
        }
        tx
    })
}

fn set_error(e: String) {
    eprintln!("[Harness Analytics] {}", e);
    if let Ok(mut l) = last_error().lock() {
        *l = Some(e);
    }
}

/// Hands one finished gateway request to the recorder. Never blocks: a full queue drops the
/// record (counted). `content` is `Some` only for definitions with content capture on.
pub fn submit(record: RequestRecord, content: Option<CapturedContent>) {
    prometheus::observe(&record);
    direct::note_gateway(&record);
    let msg = Msg::Record(Box::new(record), content.map(Box::new), Instant::now());
    enqueue(sender(), msg, &DROPPED);
}

/// Non-blocking hand-off to the writer; a full (slow disk) or closed queue counts a drop.
fn enqueue(tx: &SyncSender<Msg>, msg: Msg, dropped: &AtomicU64) {
    match tx.try_send(msg) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
            dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Writes everything queued so far (without the enrichment delay) and waits for it, at most
/// `timeout`. `false` on timeout. For tests and before a "clear".
pub fn flush(timeout: Duration) -> bool {
    let (tx, rx) = sync_channel(1);
    let deadline = Instant::now() + timeout;
    let mut msg = Msg::Flush(tx);
    loop {
        match sender().try_send(msg) {
            Ok(()) => break,
            Err(TrySendError::Full(m)) if Instant::now() < deadline => {
                msg = m;
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return false,
        }
    }
    rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .is_ok()
}

struct Pending {
    record: Box<RequestRecord>,
    content: Option<Box<CapturedContent>>,
    at: Instant,
}

fn writer(rx: Receiver<Msg>) {
    let mut pending: Vec<Pending> = Vec::new();
    let mut next_maintenance = Instant::now() + FIRST_MAINTENANCE;
    loop {
        let now = Instant::now();
        let due = pending
            .first()
            .map(|p| p.at + ENRICH_DELAY)
            .unwrap_or(next_maintenance)
            .min(next_maintenance);
        let mut flushes: Vec<SyncSender<()>> = Vec::new();
        match rx.recv_timeout(due.saturating_duration_since(now)) {
            Ok(msg) => {
                fn take(m: Msg, pending: &mut Vec<Pending>, flushes: &mut Vec<SyncSender<()>>) {
                    match m {
                        Msg::Record(record, content, at) => pending.push(Pending {
                            record,
                            content,
                            at,
                        }),
                        Msg::Flush(ack) => flushes.push(ack),
                    }
                }
                take(msg, &mut pending, &mut flushes);
                while pending.len() < BATCH * 4 {
                    match rx.try_recv() {
                        Ok(m) => take(m, &mut pending, &mut flushes),
                        Err(_) => break,
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let now = Instant::now();
        let ready = if flushes.is_empty() {
            pending
                .iter()
                .take_while(|p| now >= p.at + ENRICH_DELAY)
                .count()
        } else {
            pending.len()
        };
        if ready > 0 {
            let batch: Vec<Pending> = pending.drain(..ready).collect();
            for chunk in batch.chunks(BATCH) {
                write_batch(chunk);
            }
            crate::events::publish(crate::events::CoreEvent::AnalyticsWritten {
                count: batch.len(),
            });
        }
        for ack in flushes {
            let _ = ack.send(());
        }
        if Instant::now() >= next_maintenance {
            next_maintenance = Instant::now() + MAINTENANCE_EVERY;
            if let Err(e) = maintain_now() {
                set_error(e);
            }
        }
    }
}

/// Runs retention now with the saved settings.
pub fn maintain_now() -> Result<MaintenanceReport, String> {
    let days = crate::config::AppSettings::load()
        .analytics
        .retention_days();
    shared()?.maintain(now_ms(), days)
}

fn write_batch(batch: &[Pending]) {
    let settings = crate::config::AppSettings::load().analytics;
    if !settings.enabled {
        SKIPPED.fetch_add(batch.len() as u64, Ordering::Relaxed);
        return;
    }
    let mut rows: Vec<NewRequest> = batch
        .iter()
        .map(|p| NewRequest {
            row: RequestRow::from_record(&p.record),
            content: p.content.as_deref().cloned(),
        })
        .collect();
    enrich(&mut rows);
    let result = shared().and_then(|db| db.insert(&rows));
    match result {
        Ok(n) => {
            WRITTEN.fetch_add(n as u64, Ordering::Relaxed);
        }
        Err(e) => {
            WRITE_ERRORS.fetch_add(1, Ordering::Relaxed);
            set_error(e);
        }
    }
}

/// VRAM / RAM peak over each request's window from the telemetry history of its server. A
/// request shorter than the sampling interval takes the sample closest to its end (within
/// 2 s).
fn enrich(rows: &mut [NewRequest]) {
    let Some(poller) = telemetry() else {
        return;
    };
    let mut histories: HashMap<String, Vec<TelemetryHistoryPoint>> = HashMap::new();
    {
        let p = poller.blocking_lock();
        for r in rows.iter() {
            if let Some(id) = &r.row.server_id {
                histories
                    .entry(id.clone())
                    .or_insert_with(|| p.get_history(id, None));
            }
        }
    }
    for r in rows.iter_mut() {
        let Some(history) = r.row.server_id.as_ref().and_then(|id| histories.get(id)) else {
            continue;
        };
        if let Some((vram, ram)) = peak_in_window(history, r.row.started_at_ms, r.row.total_ms) {
            r.row.vram_peak_bytes = Some(vram);
            r.row.ram_peak_bytes = Some(ram);
        }
    }
}

/// Highest (VRAM, RAM) of the samples within `[start, start + total]` (seconds, inclusive);
/// else the sample nearest to the end if it is at most 2 s away.
pub fn peak_in_window(
    history: &[TelemetryHistoryPoint],
    started_at_ms: u64,
    total_ms: u64,
) -> Option<(u64, u64)> {
    let memory = |p: &TelemetryHistoryPoint| {
        (
            p.server_vram_used_bytes.unwrap_or(p.vram_used_bytes),
            p.server_ram_used_bytes.unwrap_or(p.ram_used_bytes),
        )
    };
    let measured = |p: &TelemetryHistoryPoint| {
        p.server_vram_used_bytes.is_some()
            || p.server_ram_used_bytes.is_some()
            || p.vram_used_bytes > 0
            || p.ram_used_bytes > 0
    };
    let from = started_at_ms / 1000;
    let to = (started_at_ms + total_ms).div_ceil(1000);
    let inside: Vec<&TelemetryHistoryPoint> = history
        .iter()
        .filter(|p| p.timestamp >= from && p.timestamp <= to)
        .filter(|p| measured(p))
        .collect();
    if !inside.is_empty() {
        return Some((
            inside.iter().map(|p| memory(p).0).max().unwrap_or(0),
            inside.iter().map(|p| memory(p).1).max().unwrap_or(0),
        ));
    }
    history
        .iter()
        .filter(|p| measured(p))
        .min_by_key(|p| p.timestamp.abs_diff(to))
        .filter(|p| p.timestamp.abs_diff(to) <= 2)
        .map(memory)
}

/// Summary over `[from_ms, to_ms)` with benchmark results of each model attached.
pub fn summary(from_ms: Option<u64>, to_ms: Option<u64>) -> Result<AnalyticsSummary, String> {
    let mut s = shared()?.summary(from_ms, to_ms)?;
    s.retention_days = crate::config::AppSettings::load()
        .analytics
        .retention_days();
    s.recorder = recorder_status();
    s.direct = shared()?.direct_usage(from_ms, to_ms)?;
    attach_benchmarks(&mut s);
    Ok(s)
}

/// The latest completed llama-bench run of each model's file (best `pp` and `tg` rows).
fn attach_benchmarks(s: &mut AnalyticsSummary) {
    if s.models.is_empty() {
        return;
    }
    let runs = crate::benchmark::load_history();
    if runs.is_empty() {
        return;
    }
    let defs = crate::domain::store::Domain::global()
        .definitions()
        .unwrap_or_default();
    for m in &mut s.models {
        let def = defs
            .iter()
            .find(|d| m.definition_id.as_deref() == Some(d.id.as_str()) || d.alias == m.alias);
        let Some(path) = def.and_then(|d| d.base.local_path()) else {
            continue;
        };
        let key = crate::library::state_key(path);
        let run = runs
            .iter()
            .filter(|r| r.status == crate::benchmark::RunStatus::Completed)
            .filter(|r| crate::library::state_key(&r.request.model_path) == key)
            .max_by_key(|r| r.finished_at);
        let Some(run) = run else {
            continue;
        };
        let pp = run
            .results
            .iter()
            .filter(|r| r.n_prompt > 0 && r.n_gen == 0)
            .max_by(|a, b| a.avg_ts.total_cmp(&b.avg_ts));
        let tg = run
            .results
            .iter()
            .filter(|r| r.n_gen > 0 && r.n_prompt == 0)
            .max_by(|a, b| a.avg_ts.total_cmp(&b.avg_ts));
        if pp.is_none() && tg.is_none() {
            continue;
        }
        m.benchmark = Some(BenchmarkRef {
            run_id: run.id.clone(),
            finished_at: run.finished_at,
            prompt_tps: pp.map(|r| r.avg_ts),
            n_prompt: pp.map(|r| r.n_prompt),
            gen_tps: tg.map(|r| r.avg_ts),
            n_gen: tg.map(|r| r.n_gen),
        });
    }
}

/// Recent requests (Request inspector).
pub fn requests(q: &RequestQuery) -> Result<RequestPage, String> {
    shared()?.requests(q)
}

/// Per-alias native tool calls and prompt speed (agent integrations).
pub fn agent_signals() -> Result<Vec<AgentSignals>, String> {
    shared()?.agent_signals()
}

/// Slot recommendations from the last week of the queue history (see [`QueueHint`]).
pub fn queue_hints() -> Result<Vec<QueueHint>, String> {
    let week = 7 * 24 * 3600 * 1000;
    shared()?.queue_hints(now_ms().saturating_sub(week))
}

/// One request with timeline and captured content.
pub fn request(id: &str) -> Result<Option<RequestDetail>, String> {
    shared()?.request(id)
}

/// Deletes all analytics data (raw rows, rollups, content). Queued records are written first
/// so nothing older survives the clear.
pub fn clear() -> Result<(), String> {
    flush(Duration::from_secs(5));
    shared()?.clear()
}

/// Settings, storage size and recorder counters (Analytics screen).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AnalyticsInfo {
    pub settings: AnalyticsSettings,
    pub storage: Option<StorageInfo>,
    pub recorder: RecorderStatus,
}

pub fn info() -> AnalyticsInfo {
    AnalyticsInfo {
        settings: crate::config::AppSettings::load().analytics,
        storage: shared().and_then(|db| db.storage()).ok(),
        recorder: recorder_status(),
    }
}

/// Saves `settings.analytics` (retention clamped to 1–3650 days).
pub fn save_settings(settings: AnalyticsSettings) -> Result<AnalyticsSettings, String> {
    let mut all = crate::config::AppSettings::load();
    all.analytics = AnalyticsSettings {
        enabled: settings.enabled,
        raw_retention_days: settings.retention_days(),
    };
    all.save()?;
    Ok(all.analytics)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(ts: u64, vram: u64, ram: u64) -> TelemetryHistoryPoint {
        TelemetryHistoryPoint {
            timestamp: ts,
            predicted_tokens_per_sec: 0.0,
            prompt_tokens_per_sec: 0.0,
            vram_used_bytes: vram,
            server_vram_used_bytes: None,
            vram_total_bytes: 0,
            ram_used_bytes: ram,
            server_ram_used_bytes: None,
            ram_total_bytes: 0,
            active_slots: 0,
            total_slots: 0,
            kv_usage_ratio: None,
            requests_processing: 0,
            requests_deferred: 0,
        }
    }

    #[test]
    fn full_queue_drops_and_counts_without_blocking() {
        // A writer that never drains stands in for a very slow disk.
        let (tx, rx) = sync_channel::<Msg>(3);
        let dropped = AtomicU64::new(0);
        let flush_msg = || Msg::Flush(sync_channel(1).0);
        let started = Instant::now();
        for _ in 0..10 {
            enqueue(&tx, flush_msg(), &dropped);
        }
        assert!(started.elapsed() < Duration::from_secs(1), "must not block");
        assert_eq!(dropped.load(Ordering::Relaxed), 7);
        // The writer catches up: space frees and nothing more is dropped.
        while rx.try_recv().is_ok() {}
        enqueue(&tx, flush_msg(), &dropped);
        assert_eq!(dropped.load(Ordering::Relaxed), 7);
        // A closed queue also counts.
        drop(rx);
        enqueue(&tx, flush_msg(), &dropped);
        assert_eq!(dropped.load(Ordering::Relaxed), 8);
    }

    #[test]
    fn peak_uses_window_then_nearest_sample() {
        let h = vec![
            point(100, 5, 50),
            point(101, 9, 40),
            point(102, 7, 60),
            point(110, 1, 1),
        ];
        // 100.2 s .. 102.1 s → whole seconds 100..=103: samples 100, 101, 102.
        assert_eq!(peak_in_window(&h, 100_200, 1_900), Some((9, 60)));
        // 105.0 s .. 105.3 s: nothing inside, nearest (110) is 4 s away → none.
        assert_eq!(peak_in_window(&h, 105_000, 300), None);
        // 109.0 .. 109.1: nearest is 110, 0 s from the rounded end → used.
        assert_eq!(peak_in_window(&h, 109_000, 100), Some((1, 1)));
    }

    #[test]
    fn peak_prefers_server_shares_in_window_and_nearest_sample() {
        let mut first = point(100, 14 << 30, 32 << 30);
        first.server_vram_used_bytes = Some(600 << 20);
        first.server_ram_used_bytes = Some(900 << 20);
        let mut second = point(101, 15 << 30, 33 << 30);
        second.server_vram_used_bytes = Some(500 << 20);
        second.server_ram_used_bytes = Some(1000 << 20);
        assert_eq!(
            peak_in_window(&[first.clone(), second], 100_000, 1000),
            Some((600 << 20, 1000 << 20))
        );
        assert_eq!(
            peak_in_window(&[first], 98_000, 0),
            Some((600 << 20, 900 << 20))
        );
    }

    #[test]
    fn peak_preserves_zero_shares_and_falls_back_per_memory_kind() {
        let mut sample = point(100, 14 << 30, 32 << 30);
        sample.server_vram_used_bytes = Some(0);
        assert_eq!(
            peak_in_window(&[sample.clone()], 100_000, 0),
            Some((0, 32 << 30))
        );
        sample.server_ram_used_bytes = Some(0);
        sample.vram_used_bytes = 0;
        sample.ram_used_bytes = 0;
        assert_eq!(peak_in_window(&[sample.clone()], 100_000, 0), Some((0, 0)));
        assert_eq!(peak_in_window(&[sample], 98_000, 0), Some((0, 0)));
    }
}
