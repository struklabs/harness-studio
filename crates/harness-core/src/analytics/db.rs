//! `analytics.db`: raw request rows, daily rollups, optional captured content.

use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Row, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use super::content::{CapturedContent, StoredContent};
use super::schema;
use super::stats::{self, Phase};
use crate::error_key::err;
use crate::gateway::record::RequestRecord;

/// Schema version this build writes (`PRAGMA user_version`).
pub const SCHEMA_VERSION: i64 = schema::MIGRATIONS.len() as i64;

pub const DAY_MS: u64 = 86_400_000;

/// A request counts as an error when the gateway or llama-server reported one. A client that
/// went away (499 without an error code) is `cancelled`, not an error.
const ERR_EXPR: &str =
    "(CASE WHEN error_code IS NOT NULL OR (status >= 400 AND status <> 499) THEN 1 ELSE 0 END)";

/// Aggregates over raw rows, in the column order of `daily_rollup` after the key columns.
fn agg_columns() -> String {
    format!(
        "COUNT(*), SUM({err}), SUM(oom), SUM(cancelled),
         SUM(COALESCE(prompt_tokens,0)), SUM(COALESCE(completion_tokens,0)),
         SUM(COALESCE(cached_tokens,0)), SUM(total_ms), SUM(COALESCE(ttft_ms,0)), COUNT(ttft_ms),
         SUM(queue_wait_ms), COUNT(load_ms), SUM(COALESCE(load_ms,0)),
         SUM(CASE WHEN predicted_ms > 0 AND predicted_per_second > 0
                  THEN predicted_per_second * predicted_ms / 1000.0 ELSE 0 END),
         SUM(CASE WHEN predicted_ms > 0 AND predicted_per_second > 0 THEN predicted_ms ELSE 0 END),
         SUM(CASE WHEN prompt_ms > 0 AND prompt_per_second > 0
                  THEN prompt_per_second * prompt_ms / 1000.0 ELSE 0 END),
         SUM(CASE WHEN prompt_ms > 0 AND prompt_per_second > 0 THEN prompt_ms ELSE 0 END),
         MAX(vram_peak_bytes), SUM(COALESCE(vram_peak_bytes,0)), COUNT(vram_peak_bytes),
         MAX(ram_peak_bytes), SUM(COALESCE(draft_n,0)), SUM(COALESCE(draft_accepted,0)),
         MAX(started_at_ms + total_ms), MAX(definition_id)",
        err = ERR_EXPR
    )
}

/// The same aggregates over rollup rows (sums of sums, max of max).
const ROLLUP_AGG: &str = "SUM(requests), SUM(errors), SUM(ooms), SUM(cancelled),
    SUM(prompt_tokens), SUM(completion_tokens), SUM(cached_tokens), SUM(total_ms_sum),
    SUM(ttft_ms_sum), SUM(ttft_n), SUM(queue_ms_sum), SUM(load_n), SUM(load_ms_sum),
    SUM(gen_tokens_timed), SUM(gen_ms_sum), SUM(prompt_tokens_timed), SUM(prompt_ms_sum),
    MAX(vram_peak_max), SUM(vram_peak_sum), SUM(vram_n), MAX(ram_peak_max), SUM(draft_n),
    SUM(draft_accepted), MAX(last_used_ms), MAX(definition_id)";

#[cfg(test)]
const AGG_COUNT: usize = 25;

/// Running sums of one group (a model, a client, a day, or everything).
#[derive(Debug, Clone, Default, PartialEq)]
struct Acc {
    requests: u64,
    errors: u64,
    ooms: u64,
    cancelled: u64,
    prompt_tokens: u64,
    completion_tokens: u64,
    cached_tokens: u64,
    total_ms: u64,
    ttft_ms: u64,
    ttft_n: u64,
    queue_ms: u64,
    load_n: u64,
    load_ms: u64,
    gen_tokens: f64,
    gen_ms: f64,
    prompt_tokens_timed: f64,
    prompt_ms: f64,
    vram_max: Option<u64>,
    vram_sum: u64,
    vram_n: u64,
    ram_max: Option<u64>,
    draft_n: u64,
    draft_accepted: u64,
    last_used_ms: Option<u64>,
    definition_id: Option<String>,
}

fn opt_u(row: &Row, i: usize) -> rusqlite::Result<Option<u64>> {
    Ok(row.get::<_, Option<i64>>(i)?.map(|v| v.max(0) as u64))
}

fn u(row: &Row, i: usize) -> rusqlite::Result<u64> {
    Ok(opt_u(row, i)?.unwrap_or(0))
}

fn f(row: &Row, i: usize) -> rusqlite::Result<f64> {
    Ok(row.get::<_, Option<f64>>(i)?.unwrap_or(0.0))
}

fn max_opt(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

impl Acc {
    /// Reads the `AGG_COUNT` aggregate columns starting at `at`.
    fn from_row(row: &Row, at: usize) -> rusqlite::Result<Self> {
        Ok(Self {
            requests: u(row, at)?,
            errors: u(row, at + 1)?,
            ooms: u(row, at + 2)?,
            cancelled: u(row, at + 3)?,
            prompt_tokens: u(row, at + 4)?,
            completion_tokens: u(row, at + 5)?,
            cached_tokens: u(row, at + 6)?,
            total_ms: u(row, at + 7)?,
            ttft_ms: u(row, at + 8)?,
            ttft_n: u(row, at + 9)?,
            queue_ms: u(row, at + 10)?,
            load_n: u(row, at + 11)?,
            load_ms: u(row, at + 12)?,
            gen_tokens: f(row, at + 13)?,
            gen_ms: f(row, at + 14)?,
            prompt_tokens_timed: f(row, at + 15)?,
            prompt_ms: f(row, at + 16)?,
            vram_max: opt_u(row, at + 17)?,
            vram_sum: u(row, at + 18)?,
            vram_n: u(row, at + 19)?,
            ram_max: opt_u(row, at + 20)?,
            draft_n: u(row, at + 21)?,
            draft_accepted: u(row, at + 22)?,
            last_used_ms: opt_u(row, at + 23)?,
            definition_id: row.get(at + 24)?,
        })
    }

    fn add(&mut self, o: &Acc) {
        self.requests += o.requests;
        self.errors += o.errors;
        self.ooms += o.ooms;
        self.cancelled += o.cancelled;
        self.prompt_tokens += o.prompt_tokens;
        self.completion_tokens += o.completion_tokens;
        self.cached_tokens += o.cached_tokens;
        self.total_ms += o.total_ms;
        self.ttft_ms += o.ttft_ms;
        self.ttft_n += o.ttft_n;
        self.queue_ms += o.queue_ms;
        self.load_n += o.load_n;
        self.load_ms += o.load_ms;
        self.gen_tokens += o.gen_tokens;
        self.gen_ms += o.gen_ms;
        self.prompt_tokens_timed += o.prompt_tokens_timed;
        self.prompt_ms += o.prompt_ms;
        self.vram_max = max_opt(self.vram_max, o.vram_max);
        self.vram_sum += o.vram_sum;
        self.vram_n += o.vram_n;
        self.ram_max = max_opt(self.ram_max, o.ram_max);
        self.draft_n += o.draft_n;
        self.draft_accepted += o.draft_accepted;
        self.last_used_ms = max_opt(self.last_used_ms, o.last_used_ms);
        if self.definition_id.is_none() {
            self.definition_id = o.definition_id.clone();
        }
    }

    fn stats(&self, alias: &str, ttft: &mut [f64], latency: &mut [f64]) -> ModelStats {
        let rate = |n: u64| {
            if self.requests == 0 {
                0.0
            } else {
                n as f64 / self.requests as f64
            }
        };
        let (ttft_p50_ms, ttft_p95_ms) = stats::p50_p95(ttft);
        let (latency_p50_ms, latency_p95_ms) = stats::p50_p95(latency);
        ModelStats {
            alias: alias.to_string(),
            definition_id: self.definition_id.clone(),
            requests: self.requests,
            errors: self.errors,
            ooms: self.ooms,
            cancelled: self.cancelled,
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            cached_tokens: self.cached_tokens,
            avg_gen_tps: (self.gen_ms > 0.0).then(|| self.gen_tokens / self.gen_ms * 1000.0),
            avg_prompt_tps: (self.prompt_ms > 0.0)
                .then(|| self.prompt_tokens_timed / self.prompt_ms * 1000.0),
            avg_ttft_ms: (self.ttft_n > 0).then(|| self.ttft_ms as f64 / self.ttft_n as f64),
            ttft_p50_ms,
            ttft_p95_ms,
            latency_p50_ms,
            latency_p95_ms,
            percentile_samples: latency.len() as u64,
            vram_avg_bytes: (self.vram_n > 0).then(|| self.vram_sum / self.vram_n),
            vram_peak_bytes: self.vram_max,
            ram_peak_bytes: self.ram_max,
            error_rate: rate(self.errors),
            oom_rate: rate(self.ooms),
            busy_ms: self.total_ms,
            loads: self.load_n,
            avg_load_ms: (self.load_n > 0).then(|| self.load_ms as f64 / self.load_n as f64),
            spec_accept_rate: (self.draft_n > 0)
                .then(|| self.draft_accepted as f64 / self.draft_n as f64),
            last_used_ms: self.last_used_ms,
            benchmark: None,
        }
    }
}

/// One stored gateway request (metadata only).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RequestRow {
    pub id: String,
    pub started_at_ms: u64,
    pub endpoint: String,
    pub model: Option<String>,
    pub alias: Option<String>,
    pub definition_id: Option<String>,
    pub server_id: Option<String>,
    /// Client label (`X-Harness-Client`, token name, `User-Agent` product; in that order).
    pub client: Option<String>,
    pub user_agent: Option<String>,
    pub harness_client: Option<String>,
    pub token_name: Option<String>,
    pub stream: bool,
    pub status: u16,
    pub error_code: Option<String>,
    pub oom: bool,
    pub cancelled: bool,
    pub queue_wait_ms: u64,
    pub load_ms: Option<u64>,
    pub evicted: Vec<String>,
    pub ttft_ms: Option<u64>,
    pub total_ms: u64,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub prompt_ms: Option<f64>,
    pub predicted_ms: Option<f64>,
    pub prompt_per_second: Option<f64>,
    pub predicted_per_second: Option<f64>,
    pub draft_n: Option<u64>,
    pub draft_accepted: Option<u64>,
    pub finish_reason: Option<String>,
    /// Context window of one slot of the server.
    pub ctx_size: Option<u64>,
    /// (prompt + completion tokens) / `ctx_size`.
    pub context_fill: Option<f64>,
    /// Highest used GPU memory (whole GPU) sampled during the request (1 Hz telemetry).
    pub vram_peak_bytes: Option<u64>,
    /// Highest used system RAM sampled during the request.
    pub ram_peak_bytes: Option<u64>,
    pub backend: Option<String>,
    pub compat: Vec<String>,
    /// The content of this request was captured (see `request_content`).
    pub content_captured: bool,
    /// Slots of the server that answered.
    pub slots: Option<u32>,
    /// `slots` / `tokens` when the request waited for its turn (see `RequestRecord::waited`).
    pub waited: Option<String>,
    // ── T4: tool-call diagnosis (`gateway::tool_diag`) ─────────────────────
    /// The request offered tools, carried tool results or the response made a call.
    pub tool_involved: bool,
    /// Tools the request offered.
    pub tools_offered: u32,
    /// Tool calls the response made.
    pub tool_calls: u32,
    /// Who is at fault: `runtime`, `context`, `model` or `client`; `None` for a clean round.
    pub tool_diag_category: Option<String>,
    /// The exact finding (`call_left_in_content`, `repeated_call`...).
    pub tool_diag_code: Option<String>,
    /// Evidence as JSON text (tool name, excerpt, schema path, linked request...).
    pub tool_diag_detail: Option<String>,
    /// USD at a BYOK cloud provider (its or the user's prices); `None` for local models.
    #[serde(default)]
    pub cost_usd: Option<f64>,
}

impl RequestRow {
    /// Metadata of a gateway record (no content).
    pub fn from_record(r: &RequestRecord) -> Self {
        let used = r.usage.prompt_tokens.unwrap_or(0) + r.usage.completion_tokens.unwrap_or(0);
        Self {
            id: r.id.clone(),
            started_at_ms: r.started_at_ms,
            endpoint: r.endpoint.clone(),
            model: r.model.clone(),
            alias: r.alias.clone(),
            definition_id: r.definition_id.clone(),
            server_id: r.server_id.clone(),
            client: r.client.label.clone(),
            user_agent: r.client.user_agent.clone(),
            harness_client: r.client.harness_client.clone(),
            token_name: r.client.token_name.clone(),
            stream: r.stream,
            status: r.status,
            error_code: r.error_code.clone(),
            oom: r.oom
                || r.error_code
                    .as_deref()
                    .is_some_and(|c| c.contains("memory")),
            cancelled: r.cancelled,
            queue_wait_ms: r.queue_wait_ms,
            load_ms: r.load_ms,
            evicted: r.evicted.clone(),
            ttft_ms: r.ttft_ms,
            total_ms: r.total_ms,
            prompt_tokens: r.usage.prompt_tokens,
            completion_tokens: r.usage.completion_tokens,
            cached_tokens: r.usage.cached_tokens,
            prompt_ms: r.usage.prompt_ms,
            predicted_ms: r.usage.predicted_ms,
            prompt_per_second: r.usage.prompt_per_second,
            predicted_per_second: r.usage.predicted_per_second,
            draft_n: r.usage.draft_n,
            draft_accepted: r.usage.draft_n_accepted,
            finish_reason: r.usage.finish_reason.clone(),
            ctx_size: r.ctx_size,
            context_fill: r
                .ctx_size
                .filter(|c| *c > 0 && used > 0)
                .map(|c| used as f64 / c as f64),
            vram_peak_bytes: None,
            ram_peak_bytes: None,
            backend: r.backend.clone(),
            compat: r.compat.clone(),
            content_captured: false,
            slots: r.slots,
            waited: r.waited.clone(),
            tool_involved: r.tool_involved,
            tools_offered: r.tools_offered,
            tool_calls: r.tool_calls,
            tool_diag_category: r.tool_diag_category.clone(),
            tool_diag_code: r.tool_diag_code.clone(),
            tool_diag_detail: r.tool_diag_detail.clone(),
            cost_usd: r.cost_usd,
        }
    }

    /// Accepted / drafted tokens of speculative decoding.
    pub fn spec_accept_rate(&self) -> Option<f64> {
        match (self.draft_n, self.draft_accepted) {
            (Some(n), Some(a)) if n > 0 => Some(a as f64 / n as f64),
            _ => None,
        }
    }

    pub fn is_error(&self) -> bool {
        self.error_code.is_some() || (self.status >= 400 && self.status != 499)
    }
}

/// What the recorder writes for one request.
#[derive(Debug, Clone)]
pub struct NewRequest {
    pub row: RequestRow,
    pub content: Option<CapturedContent>,
}

/// Agent-related signals of one alias (see [`AnalyticsDb::agent_signals`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentSignals {
    pub alias: String,
    /// Answers that were native (structured) tool calls.
    pub native_tool_calls: u64,
    /// Prompt processing speed, tokens/s (`None` without timed requests).
    pub prompt_tps: Option<f64>,
    /// Uncached prompt tokens the speed is measured over.
    pub prompt_tokens_timed: u64,
    /// Largest prompt seen.
    pub max_prompt_tokens: Option<u64>,
    /// Tool rounds (tools offered) with a clean call.
    pub tool_ok: u64,
    /// Tool rounds that failed through the model or its chat template (`runtime` / `model`
    /// findings of `gateway::tool_diag`).
    pub tool_bad: u64,
}

/// Queue behaviour of one definition over a time window and, when it keeps making requests
/// wait for a free slot, how many slots to try at its next load (see
/// [`AnalyticsDb::queue_hints`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct QueueHint {
    pub definition_id: String,
    pub alias: String,
    /// Slots of the server at its most recent request in the window; only requests served with
    /// that slot count are counted (an earlier load with other slots says nothing about it).
    pub slots: u32,
    pub requests: u64,
    /// Requests that found every slot busy.
    pub waited_slots: u64,
    /// Requests that had a slot but waited for room in the shared KV pool (more slots would
    /// not help these).
    pub waited_tokens: u64,
    /// `waited_slots / requests`.
    pub wait_share: f64,
    /// Mean queue time of the slot-waiting requests, ms (loads excluded).
    pub avg_wait_ms: f64,
    /// Slots worth trying at the next load; `None` while the queue is no problem.
    pub suggested_slots: Option<u32>,
}

/// Requests of a definition (with one slot count) needed before a hint is made.
pub const HINT_MIN_REQUESTS: u64 = 10;
/// Share of requests that waited for a slot before more slots are suggested.
pub const HINT_MIN_SHARE: f64 = 0.2;
/// Mean wait (ms) below which the queue is not worth a reload.
pub const HINT_MIN_WAIT_MS: f64 = 500.0;
/// Most slots Harness suggests (the "Eşzamanlı istek" presets stop at 8).
pub const HINT_MAX_SLOTS: u32 = 8;

impl QueueHint {
    /// Applies the rule: enough requests, a fifth of them waited for a slot, the wait was
    /// noticeable, and the next preset (2, 4, 8) is above the current slots.
    pub fn suggest(&mut self) {
        self.suggested_slots = None;
        if self.requests < HINT_MIN_REQUESTS
            || self.wait_share < HINT_MIN_SHARE
            || self.avg_wait_ms < HINT_MIN_WAIT_MS
            || self.slots >= HINT_MAX_SLOTS
        {
            return;
        }
        self.suggested_slots = [2, 4, 8].into_iter().find(|n| *n > self.slots);
    }
}

/// Statistics of one model (alias) over a time range.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelStats {
    /// Alias (`*` for the total row; empty for requests that never resolved a model).
    pub alias: String,
    pub definition_id: Option<String>,
    pub requests: u64,
    pub errors: u64,
    pub ooms: u64,
    pub cancelled: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_tokens: u64,
    /// Generated tokens / generation time over every timed request (token-weighted).
    pub avg_gen_tps: Option<f64>,
    /// Processed prompt tokens / prompt time (token-weighted).
    pub avg_prompt_tps: Option<f64>,
    pub avg_ttft_ms: Option<f64>,
    /// Percentiles come from raw rows only (the last `retention_days`).
    pub ttft_p50_ms: Option<f64>,
    pub ttft_p95_ms: Option<f64>,
    pub latency_p50_ms: Option<f64>,
    pub latency_p95_ms: Option<f64>,
    /// Raw rows the latency percentiles are computed from.
    pub percentile_samples: u64,
    /// Mean of the per-request VRAM peaks.
    pub vram_avg_bytes: Option<u64>,
    pub vram_peak_bytes: Option<u64>,
    pub ram_peak_bytes: Option<u64>,
    pub error_rate: f64,
    pub oom_rate: f64,
    /// Sum of request durations (time the model was busy for clients).
    pub busy_ms: u64,
    /// JIT loads the requests caused.
    pub loads: u64,
    pub avg_load_ms: Option<f64>,
    pub spec_accept_rate: Option<f64>,
    pub last_used_ms: Option<u64>,
    /// Latest completed llama-bench run of the definition's model file.
    pub benchmark: Option<BenchmarkRef>,
}

/// A benchmark result next to real usage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BenchmarkRef {
    pub run_id: String,
    pub finished_at: u64,
    /// Best prompt-processing result (`pp` test) of the run, t/s.
    pub prompt_tps: Option<f64>,
    pub n_prompt: Option<u32>,
    /// Best generation result (`tg` test) of the run, t/s.
    pub gen_tps: Option<f64>,
    pub n_gen: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ClientShare {
    /// Client label (empty when unknown).
    pub client: String,
    pub requests: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    /// Share of all requests in the range (0–1).
    pub share: f64,
    /// Share of all tokens in the range (0–1).
    pub token_share: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DailyPoint {
    /// Start of the (UTC) day, Unix ms.
    pub day_start_ms: u64,
    pub requests: u64,
    pub errors: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

/// `GET /analytics/summary`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AnalyticsSummary {
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
    pub generated_at_ms: u64,
    /// Gateway statistics (`alias` = `*`); requests also include attributed direct clients.
    /// Timings and error rates only cover gateway requests with recorded outcomes.
    pub totals: ModelStats,
    /// Per gateway model, most requests first.
    pub models: Vec<ModelStats>,
    pub clients: Vec<ClientShare>,
    pub daily: Vec<DailyPoint>,
    /// Oldest raw row still stored; percentiles only cover raw rows.
    pub raw_since_ms: Option<u64>,
    /// Days in the range that only survive as daily rollups (older than the raw retention).
    pub rollup_days: u64,
    pub retention_days: u32,
    pub recorder: super::RecorderStatus,
    /// Token totals of the llama-server ports that bypass the gateway (aggregate only; see
    /// `analytics::direct`).
    #[serde(default)]
    pub direct: super::direct::DirectUsage,
    /// What BYOK cloud models cost in the range (raw rows only: rollups do not keep prices).
    #[serde(default)]
    pub cost: CostSummary,
}

/// Cost of the cloud-provider requests in a range, USD.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CostSummary {
    pub total_usd: f64,
    /// Per gateway model, most expensive first.
    pub models: Vec<ModelCost>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelCost {
    pub alias: String,
    /// Requests with a known cost.
    pub requests: u64,
    pub cost_usd: f64,
}

/// Filters of `GET /analytics/requests`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema, utoipa::IntoParams))]
#[cfg_attr(feature = "openapi", into_params(parameter_in = Query))]
pub struct RequestQuery {
    /// Alias (exact).
    pub model: Option<String>,
    /// Client label (exact).
    pub client: Option<String>,
    /// Endpoint path (exact, e.g. `/v1/chat/completions`).
    pub endpoint: Option<String>,
    /// `ok`, `error`, `oom` or `cancelled`.
    pub status: Option<String>,
    /// `true`: only requests with a tool-call finding (`tool_diag_category` set); `false`: only
    /// tool requests without one. `None`: no filter.
    pub tool_issue: Option<bool>,
    /// Only requests with this finder category (`runtime`, `context`, `model`, `client`).
    pub tool_category: Option<String>,
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
    /// Rows started before this time (paging: `started_at_ms` of the last row).
    pub before_ms: Option<u64>,
    /// Default 100, at most 1000.
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RequestPage {
    pub requests: Vec<RequestRow>,
    /// Rows matching the filters (all pages).
    pub total: u64,
    /// Values present in the stored rows, for filter pickers.
    pub models: Vec<String>,
    pub clients: Vec<String>,
    pub endpoints: Vec<String>,
}

/// `GET /analytics/requests/{id}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RequestDetail {
    pub request: RequestRow,
    pub spec_accept_rate: Option<f64>,
    /// load → queue → prompt → generation → other, in ms from arrival.
    pub timeline: Vec<Phase>,
    /// Captured content (only for definitions with content capture on, until it expires).
    pub content: Option<StoredContent>,
}

/// What a maintenance run did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MaintenanceReport {
    pub days_rolled_up: u64,
    pub rows_deleted: u64,
    pub contents_deleted: u64,
}

/// Size and range of the stored data.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StorageInfo {
    pub raw_rows: u64,
    pub rollup_rows: u64,
    pub content_rows: u64,
    pub oldest_raw_ms: Option<u64>,
    pub oldest_rollup_day_ms: Option<u64>,
    /// `analytics.db` + WAL, bytes.
    pub file_bytes: u64,
    pub path: String,
}

pub fn day_of(ms: u64) -> i64 {
    (ms / DAY_MS) as i64
}

/// Handle to `analytics.db`. One connection behind a mutex (single writer; the writer
/// thread and the read queries are short).
pub struct AnalyticsDb {
    conn: Mutex<Connection>,
    newer_version: Option<i64>,
    path: PathBuf,
}

const ROW_COLUMNS: &str = "id, started_at_ms, endpoint, model, alias, definition_id, server_id,
    client, user_agent, harness_client, token_name, stream, status, error_code, oom, cancelled,
    queue_wait_ms, load_ms, evicted_json, ttft_ms, total_ms, prompt_tokens, completion_tokens,
    cached_tokens, prompt_ms, predicted_ms, prompt_per_second, predicted_per_second, draft_n,
    draft_accepted, finish_reason, ctx_size, context_fill, vram_peak_bytes, ram_peak_bytes,
    backend, compat_json, content_captured, slots, waited,
    tool_involved, tools_offered, tool_calls, tool_diag_category, tool_diag_code,
    tool_diag_detail, cost_usd";

fn json_list(v: Option<String>) -> Vec<String> {
    v.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn read_row(r: &Row) -> rusqlite::Result<RequestRow> {
    Ok(RequestRow {
        id: r.get(0)?,
        started_at_ms: u(r, 1)?,
        endpoint: r.get(2)?,
        model: r.get(3)?,
        alias: r.get(4)?,
        definition_id: r.get(5)?,
        server_id: r.get(6)?,
        client: r.get(7)?,
        user_agent: r.get(8)?,
        harness_client: r.get(9)?,
        token_name: r.get(10)?,
        stream: r.get::<_, i64>(11)? != 0,
        status: u(r, 12)? as u16,
        error_code: r.get(13)?,
        oom: r.get::<_, i64>(14)? != 0,
        cancelled: r.get::<_, i64>(15)? != 0,
        queue_wait_ms: u(r, 16)?,
        load_ms: opt_u(r, 17)?,
        evicted: json_list(r.get(18)?),
        ttft_ms: opt_u(r, 19)?,
        total_ms: u(r, 20)?,
        prompt_tokens: opt_u(r, 21)?,
        completion_tokens: opt_u(r, 22)?,
        cached_tokens: opt_u(r, 23)?,
        prompt_ms: r.get(24)?,
        predicted_ms: r.get(25)?,
        prompt_per_second: r.get(26)?,
        predicted_per_second: r.get(27)?,
        draft_n: opt_u(r, 28)?,
        draft_accepted: opt_u(r, 29)?,
        finish_reason: r.get(30)?,
        ctx_size: opt_u(r, 31)?,
        context_fill: r.get(32)?,
        vram_peak_bytes: opt_u(r, 33)?,
        ram_peak_bytes: opt_u(r, 34)?,
        backend: r.get(35)?,
        compat: json_list(r.get(36)?),
        content_captured: r.get::<_, i64>(37)? != 0,
        slots: opt_u(r, 38)?.map(|n| n as u32),
        waited: r.get(39)?,
        tool_involved: r.get::<_, i64>(40)? != 0,
        tools_offered: opt_u(r, 41)?.unwrap_or(0) as u32,
        tool_calls: opt_u(r, 42)?.unwrap_or(0) as u32,
        tool_diag_category: r.get(43)?,
        tool_diag_code: r.get(44)?,
        tool_diag_detail: r.get(45)?,
        cost_usd: r.get(46)?,
    })
}

fn i(v: Option<u64>) -> Option<i64> {
    v.map(|v| v.min(i64::MAX as u64) as i64)
}

pub(super) fn db_err(e: rusqlite::Error) -> String {
    err("analytics.db_error").with("error", e).into()
}

/// Builds `WHERE` for the request filters (time range on `started_at_ms`).
fn where_clause(q: &RequestQuery) -> (String, Vec<rusqlite::types::Value>) {
    use rusqlite::types::Value;
    let mut parts: Vec<String> = Vec::new();
    let mut args: Vec<Value> = Vec::new();
    if let Some(m) = q.model.as_deref().filter(|s| !s.is_empty()) {
        parts.push("alias = ?".into());
        args.push(Value::Text(m.to_string()));
    }
    if let Some(c) = q.client.as_deref().filter(|s| !s.is_empty()) {
        parts.push("client = ?".into());
        args.push(Value::Text(c.to_string()));
    }
    if let Some(e) = q.endpoint.as_deref().filter(|s| !s.is_empty()) {
        parts.push("endpoint = ?".into());
        args.push(Value::Text(e.to_string()));
    }
    match q.status.as_deref() {
        Some("ok") => parts.push(format!("{} = 0 AND cancelled = 0", ERR_EXPR)),
        Some("error") => parts.push(format!("{} = 1", ERR_EXPR)),
        Some("oom") => parts.push("oom = 1".into()),
        Some("cancelled") => parts.push("cancelled = 1".into()),
        _ => {}
    }
    match q.tool_issue {
        Some(true) => parts.push("tool_diag_category IS NOT NULL".into()),
        Some(false) => parts.push("tool_involved = 1 AND tool_diag_category IS NULL".into()),
        None => {}
    }
    if let Some(c) = &q.tool_category {
        parts.push("tool_diag_category = ?".into());
        args.push(Value::Text(c.clone()));
    }
    if let Some(v) = q.from_ms {
        parts.push("started_at_ms >= ?".into());
        args.push(Value::Integer(v as i64));
    }
    if let Some(v) = q.to_ms {
        parts.push("started_at_ms < ?".into());
        args.push(Value::Integer(v as i64));
    }
    if let Some(v) = q.before_ms {
        parts.push("started_at_ms < ?".into());
        args.push(Value::Integer(v as i64));
    }
    let sql = if parts.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", parts.join(" AND "))
    };
    (sql, args)
}

/// Time range as SQL on raw rows and on rollup days.
pub(super) fn range_sql(from_ms: Option<u64>, to_ms: Option<u64>) -> (String, String) {
    let raw = format!(
        "started_at_ms >= {} AND started_at_ms < {}",
        from_ms.unwrap_or(0).min(i64::MAX as u64),
        to_ms.unwrap_or(i64::MAX as u64).min(i64::MAX as u64)
    );
    // A rollup day counts when it starts inside the range (days are whole).
    let from_day = from_ms.map(|m| m.div_ceil(DAY_MS) as i64).unwrap_or(0);
    let to_day = to_ms.map(day_of).unwrap_or(i64::MAX);
    let rollup = format!(
        "day >= {} AND day < {} AND day NOT IN (SELECT DISTINCT day FROM requests)",
        from_day, to_day
    );
    (raw, rollup)
}

impl AnalyticsDb {
    /// Opens (creating if needed) the database at `path` and applies pending migrations.
    /// A file of a newer schema opens read-only.
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                String::from(
                    err("fs.create_dir_failed")
                        .with("path", parent.display())
                        .with("error", e),
                )
            })?;
        }
        let mut conn = Connection::open(path).map_err(db_err)?;
        conn.busy_timeout(Duration::from_secs(5)).map_err(db_err)?;
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(db_err)?;
        if version > SCHEMA_VERSION {
            drop(conn);
            let ro = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .map_err(db_err)?;
            ro.busy_timeout(Duration::from_secs(5)).map_err(db_err)?;
            return Ok(Self {
                conn: Mutex::new(ro),
                newer_version: Some(version),
                path: path.to_path_buf(),
            });
        }
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(db_err)?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(db_err)?;
        // Deleted content (expired captures, cleared data) is overwritten, not left in free
        // pages.
        conn.pragma_update(None, "secure_delete", true)
            .map_err(db_err)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
        let version: i64 = tx
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(db_err)?;
        if version < SCHEMA_VERSION {
            for sql in &schema::MIGRATIONS[version as usize..] {
                tx.execute_batch(sql).map_err(db_err)?;
            }
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)
                .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(Self {
            conn: Mutex::new(conn),
            newer_version: None,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_read_only(&self) -> bool {
        self.newer_version.is_some()
    }

    pub(super) fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(super) fn writable(&self) -> Result<(), String> {
        match self.newer_version {
            Some(v) => Err(err("analytics.read_only")
                .num("found", v)
                .num("supported", SCHEMA_VERSION)
                .into()),
            None => Ok(()),
        }
    }

    /// Writes a batch in one transaction (duplicates by id are ignored).
    pub fn insert(&self, batch: &[NewRequest]) -> Result<usize, String> {
        self.writable()?;
        let mut conn = self.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
        let mut written = 0;
        {
            let mut ins = tx
                .prepare_cached(&format!(
                    "INSERT OR IGNORE INTO requests (day, {ROW_COLUMNS}) VALUES (?1, ?2, ?3, ?4,
                     ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
                     ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35,
                     ?36, ?37, ?38, ?39, ?40, ?41, ?42, ?43, ?44, ?45, ?46, ?47, ?48)"
                ))
                .map_err(db_err)?;
            let mut content = tx
                .prepare_cached(
                    "INSERT OR REPLACE INTO request_content (request_id, captured_at_ms,
                     expires_at_ms, request_body, response_body, request_truncated,
                     response_truncated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )
                .map_err(db_err)?;
            for n in batch {
                let r = &n.row;
                let evicted = (!r.evicted.is_empty())
                    .then(|| serde_json::to_string(&r.evicted).unwrap_or_default());
                let compat = (!r.compat.is_empty())
                    .then(|| serde_json::to_string(&r.compat).unwrap_or_default());
                let changed = ins
                    .execute(params![
                        day_of(r.started_at_ms),
                        r.id,
                        r.started_at_ms as i64,
                        r.endpoint,
                        r.model,
                        r.alias,
                        r.definition_id,
                        r.server_id,
                        r.client,
                        r.user_agent,
                        r.harness_client,
                        r.token_name,
                        r.stream,
                        r.status,
                        r.error_code,
                        r.oom,
                        r.cancelled,
                        r.queue_wait_ms as i64,
                        i(r.load_ms),
                        evicted,
                        i(r.ttft_ms),
                        r.total_ms as i64,
                        i(r.prompt_tokens),
                        i(r.completion_tokens),
                        i(r.cached_tokens),
                        r.prompt_ms,
                        r.predicted_ms,
                        r.prompt_per_second,
                        r.predicted_per_second,
                        i(r.draft_n),
                        i(r.draft_accepted),
                        r.finish_reason,
                        i(r.ctx_size),
                        r.context_fill,
                        i(r.vram_peak_bytes),
                        i(r.ram_peak_bytes),
                        r.backend,
                        compat,
                        n.content.is_some(),
                        r.slots,
                        r.waited,
                        r.tool_involved as i64,
                        r.tools_offered,
                        r.tool_calls,
                        r.tool_diag_category,
                        r.tool_diag_code,
                        r.tool_diag_detail,
                        r.cost_usd,
                    ])
                    .map_err(db_err)?;
                written += changed;
                if let (Some(c), true) = (&n.content, changed > 0) {
                    let now = super::now_ms();
                    let expires = now + u64::from(c.retention_days) * DAY_MS;
                    content
                        .execute(params![
                            r.id,
                            now as i64,
                            expires as i64,
                            c.request,
                            c.response,
                            c.request_truncated,
                            c.response_truncated,
                        ])
                        .map_err(db_err)?;
                }
            }
        }
        tx.commit().map_err(db_err)?;
        Ok(written)
    }

    /// Per-model statistics, client shares and a daily series for `[from_ms, to_ms)`.
    /// Raw rows give exact values and percentiles; days older than the raw retention come
    /// from the daily rollups (counts, sums, averages; no percentiles).
    pub fn summary(
        &self,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
    ) -> Result<AnalyticsSummary, String> {
        let conn = self.lock();
        let (raw, rollup) = range_sql(from_ms, to_ms);
        let aggs = agg_columns();

        let mut by_model: BTreeMap<String, Acc> = BTreeMap::new();
        let query_accs = |sql: &str, map: &mut BTreeMap<String, Acc>| -> Result<(), String> {
            let mut st = conn.prepare(sql).map_err(db_err)?;
            let rows = st
                .query_map([], |r| Ok((r.get::<_, String>(0)?, Acc::from_row(r, 1)?)))
                .map_err(db_err)?;
            for row in rows {
                let (key, acc) = row.map_err(db_err)?;
                map.entry(key).or_default().add(&acc);
            }
            Ok(())
        };
        query_accs(
            &format!("SELECT COALESCE(alias,''), {aggs} FROM requests WHERE {raw} GROUP BY 1"),
            &mut by_model,
        )?;
        query_accs(
            &format!("SELECT alias, {ROLLUP_AGG} FROM daily_rollup WHERE {rollup} GROUP BY 1"),
            &mut by_model,
        )?;
        let mut clients: BTreeMap<String, Acc> = BTreeMap::new();
        query_accs(
            &format!("SELECT COALESCE(client,''), {aggs} FROM requests WHERE {raw} GROUP BY 1"),
            &mut clients,
        )?;
        query_accs(
            &format!("SELECT client, {ROLLUP_AGG} FROM daily_rollup WHERE {rollup} GROUP BY 1"),
            &mut clients,
        )?;
        let mut days: BTreeMap<String, Acc> = BTreeMap::new();
        query_accs(
            &format!("SELECT printf('%012d', day), {aggs} FROM requests WHERE {raw} GROUP BY day"),
            &mut days,
        )?;
        query_accs(
            &format!(
                "SELECT printf('%012d', day), {ROLLUP_AGG} FROM daily_rollup WHERE {rollup} GROUP BY day"
            ),
            &mut days,
        )?;
        let rollup_days: u64 = conn
            .query_row(
                &format!("SELECT COUNT(DISTINCT day) FROM daily_rollup WHERE {rollup}"),
                [],
                |r| u(r, 0),
            )
            .map_err(db_err)?;

        let mut cost = CostSummary::default();
        {
            let mut st = conn
                .prepare(&format!(
                    "SELECT COALESCE(alias,''), COUNT(*), SUM(cost_usd) FROM requests
                     WHERE {raw} AND cost_usd IS NOT NULL GROUP BY 1"
                ))
                .map_err(db_err)?;
            let rows = st
                .query_map([], |r| {
                    Ok(ModelCost {
                        alias: r.get(0)?,
                        requests: u(r, 1)?,
                        cost_usd: r.get::<_, f64>(2)?,
                    })
                })
                .map_err(db_err)?;
            for row in rows {
                cost.models.push(row.map_err(db_err)?);
            }
            cost.models.sort_by(|a, b| {
                b.cost_usd
                    .total_cmp(&a.cost_usd)
                    .then(a.alias.cmp(&b.alias))
            });
            cost.total_usd = cost.models.iter().map(|m| m.cost_usd).sum();
        }

        // Percentile inputs: raw rows only.
        let mut ttft: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        let mut latency: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        let mut all_ttft = Vec::new();
        let mut all_latency = Vec::new();
        {
            let mut st = conn
                .prepare(&format!(
                    "SELECT COALESCE(alias,''), ttft_ms, total_ms FROM requests WHERE {raw}"
                ))
                .map_err(db_err)?;
            let rows = st
                .query_map([], |r| Ok((r.get::<_, String>(0)?, opt_u(r, 1)?, u(r, 2)?)))
                .map_err(db_err)?;
            for row in rows {
                let (alias, t, total) = row.map_err(db_err)?;
                if let Some(t) = t {
                    ttft.entry(alias.clone()).or_default().push(t as f64);
                    all_ttft.push(t as f64);
                }
                latency.entry(alias).or_default().push(total as f64);
                all_latency.push(total as f64);
            }
        }
        let raw_since_ms: Option<u64> = conn
            .query_row("SELECT MIN(started_at_ms) FROM requests", [], |r| {
                opt_u(r, 0)
            })
            .map_err(db_err)?;
        drop(conn);

        let mut total = Acc::default();
        let mut models: Vec<ModelStats> = by_model
            .iter()
            .map(|(alias, acc)| {
                total.add(acc);
                acc.stats(
                    alias,
                    ttft.get_mut(alias)
                        .map(|v| v.as_mut_slice())
                        .unwrap_or(&mut []),
                    latency
                        .get_mut(alias)
                        .map(|v| v.as_mut_slice())
                        .unwrap_or(&mut []),
                )
            })
            .collect();
        total.definition_id = None;
        models.sort_by(|a, b| b.requests.cmp(&a.requests).then(a.alias.cmp(&b.alias)));
        let mut totals = total.stats("*", &mut all_ttft, &mut all_latency);
        let direct = self.direct_usage(from_ms, to_ms)?;
        let all_tokens = (totals.prompt_tokens
            + totals.completion_tokens
            + direct.prompt_tokens
            + direct.completion_tokens)
            .max(1) as f64;
        let mut clients: Vec<ClientShare> = clients
            .into_iter()
            .map(|(client, acc)| {
                let total_tokens = acc.prompt_tokens + acc.completion_tokens;
                ClientShare {
                    client,
                    requests: acc.requests,
                    prompt_tokens: acc.prompt_tokens,
                    completion_tokens: acc.completion_tokens,
                    total_tokens,
                    share: 0.0,
                    token_share: total_tokens as f64 / all_tokens,
                }
            })
            .collect();
        for d in direct.clients {
            if let Some(c) = clients.iter_mut().find(|c| c.client == d.client) {
                c.requests += d.requests;
                c.prompt_tokens += d.prompt_tokens;
                c.completion_tokens += d.completion_tokens;
                c.total_tokens = c.prompt_tokens + c.completion_tokens;
                c.token_share = c.total_tokens as f64 / all_tokens;
            } else {
                let total_tokens = d.prompt_tokens + d.completion_tokens;
                clients.push(ClientShare {
                    client: d.client,
                    requests: d.requests,
                    prompt_tokens: d.prompt_tokens,
                    completion_tokens: d.completion_tokens,
                    total_tokens,
                    share: 0.0,
                    token_share: total_tokens as f64 / all_tokens,
                });
            }
        }
        totals.requests = clients.iter().map(|c| c.requests).sum();
        let all = totals.requests.max(1) as f64;
        for client in &mut clients {
            client.share = client.requests as f64 / all;
        }
        clients.sort_by(|a, b| b.requests.cmp(&a.requests).then(a.client.cmp(&b.client)));
        let daily = days
            .into_iter()
            .map(|(day, acc)| DailyPoint {
                day_start_ms: day.parse::<u64>().unwrap_or(0) * DAY_MS,
                requests: acc.requests,
                errors: acc.errors,
                prompt_tokens: acc.prompt_tokens,
                completion_tokens: acc.completion_tokens,
            })
            .collect();
        Ok(AnalyticsSummary {
            from_ms,
            to_ms,
            generated_at_ms: super::now_ms(),
            totals,
            models,
            clients,
            daily,
            raw_since_ms,
            rollup_days,
            cost,
            ..Default::default()
        })
    }

    /// Newest requests matching the filters.
    pub fn requests(&self, q: &RequestQuery) -> Result<RequestPage, String> {
        let conn = self.lock();
        let (filter, args) = where_clause(q);
        let limit = q.limit.unwrap_or(100).clamp(1, 1000);
        let mut st = conn
            .prepare(&format!(
                "SELECT {ROW_COLUMNS} FROM requests {filter}
                 ORDER BY started_at_ms DESC, id DESC LIMIT {limit}"
            ))
            .map_err(db_err)?;
        let requests = st
            .query_map(rusqlite::params_from_iter(args.iter()), read_row)
            .map_err(db_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)?;
        // The total ignores the paging cursor.
        let (count_filter, count_args) = where_clause(&RequestQuery {
            before_ms: None,
            ..q.clone()
        });
        let total: u64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM requests {count_filter}"),
                rusqlite::params_from_iter(count_args.iter()),
                |r| u(r, 0),
            )
            .map_err(db_err)?;
        let distinct = |col: &str| -> Result<Vec<String>, String> {
            let mut st = conn
                .prepare(&format!(
                    "SELECT DISTINCT {col} FROM requests WHERE {col} IS NOT NULL ORDER BY 1 LIMIT 200"
                ))
                .map_err(db_err)?;
            let v = st
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(db_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db_err)?;
            Ok(v)
        };
        Ok(RequestPage {
            requests,
            total,
            models: distinct("alias")?,
            clients: distinct("client")?,
            endpoints: distinct("endpoint")?,
        })
    }

    /// One request with its timeline and captured content (if any, and not expired).
    pub fn request(&self, id: &str) -> Result<Option<RequestDetail>, String> {
        let conn = self.lock();
        let row = conn
            .query_row(
                &format!("SELECT {ROW_COLUMNS} FROM requests WHERE id = ?1"),
                [id],
                read_row,
            )
            .optional()
            .map_err(db_err)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let content = conn
            .query_row(
                "SELECT captured_at_ms, expires_at_ms, request_body, response_body,
                        request_truncated, response_truncated
                 FROM request_content WHERE request_id = ?1 AND expires_at_ms > ?2",
                params![id, super::now_ms() as i64],
                |r| {
                    Ok(StoredContent {
                        captured_at_ms: u(r, 0)?,
                        expires_at_ms: u(r, 1)?,
                        request: r.get(2)?,
                        response: r.get(3)?,
                        request_truncated: r.get::<_, i64>(4)? != 0,
                        response_truncated: r.get::<_, i64>(5)? != 0,
                    })
                },
            )
            .optional()
            .map_err(db_err)?;
        Ok(Some(RequestDetail {
            spec_accept_rate: row.spec_accept_rate(),
            timeline: stats::timeline(
                row.queue_wait_ms,
                row.load_ms,
                row.prompt_ms,
                row.predicted_ms,
                row.total_ms,
            ),
            request: row,
            content,
        }))
    }

    /// Rolls every finished day that still has raw rows up into `daily_rollup` (replacing
    /// its rows, so re-running is harmless), then deletes raw rows older than
    /// `raw_retention_days` whole days and expired captured content. `now_ms` is a
    /// parameter so the retention can be exercised without waiting 30 days.
    pub fn maintain(
        &self,
        now_ms: u64,
        raw_retention_days: u32,
    ) -> Result<MaintenanceReport, String> {
        self.writable()?;
        let today = day_of(now_ms);
        let cutoff = today - i64::from(raw_retention_days.max(1));
        let mut conn = self.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
        let days: Vec<i64> = {
            let mut st = tx
                .prepare("SELECT DISTINCT day FROM requests WHERE day < ?1")
                .map_err(db_err)?;
            let v = st
                .query_map([today], |r| r.get(0))
                .map_err(db_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db_err)?;
            v
        };
        let aggs = agg_columns();
        for day in &days {
            tx.execute("DELETE FROM daily_rollup WHERE day = ?1", [day])
                .map_err(db_err)?;
            tx.execute(
                &format!(
                    "INSERT INTO daily_rollup SELECT day, COALESCE(alias,''), COALESCE(client,''),
                     endpoint, {aggs} FROM requests WHERE day = ?1
                     GROUP BY day, COALESCE(alias,''), COALESCE(client,''), endpoint"
                ),
                [day],
            )
            .map_err(db_err)?;
        }
        let rows_deleted = tx
            .execute("DELETE FROM requests WHERE day < ?1", [cutoff])
            .map_err(db_err)? as u64;
        tx.execute("DELETE FROM tool_calls WHERE day < ?1", [cutoff])
            .map_err(db_err)?;
        AnalyticsDb::fold_direct(&tx, cutoff)?;
        let contents_deleted = tx
            .execute(
                "DELETE FROM request_content WHERE expires_at_ms <= ?1
                 OR request_id NOT IN (SELECT id FROM requests)",
                [now_ms as i64],
            )
            .map_err(db_err)? as u64;
        tx.commit().map_err(db_err)?;
        if rows_deleted > 0 || contents_deleted > 0 {
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }
        Ok(MaintenanceReport {
            days_rolled_up: days.len() as u64,
            rows_deleted,
            contents_deleted,
        })
    }

    /// Deletes every row (raw, rollups, content) and shrinks the file.
    pub fn clear(&self) -> Result<(), String> {
        self.writable()?;
        let conn = self.lock();
        conn.execute_batch(
            "BEGIN IMMEDIATE;
             DELETE FROM requests; DELETE FROM daily_rollup; DELETE FROM request_content;
             DELETE FROM direct_usage; DELETE FROM direct_daily; DELETE FROM tool_calls;
             COMMIT;",
        )
        .map_err(db_err)?;
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;");
        Ok(())
    }

    /// Writes the WAL back into the main file (tests read the file bytes).
    /// Per alias, what agent tools care about (raw rows only): how many answers were native
    /// tool calls (`finish_reason` `tool_calls` / `tool_use` / `function_call`) and the prompt
    /// processing speed over timed requests (uncached prompt tokens / prompt time, so a big
    /// prompt weighs more than a short one).
    pub fn agent_signals(&self) -> Result<Vec<AgentSignals>, String> {
        let conn = self.lock();
        let mut st = conn
            .prepare(
                "SELECT alias,
                    SUM(CASE WHEN finish_reason IN ('tool_calls', 'tool_use', 'function_call') THEN 1 ELSE 0 END),
                    SUM(CASE WHEN prompt_ms > 0 AND prompt_tokens > COALESCE(cached_tokens, 0)
                        THEN prompt_tokens - COALESCE(cached_tokens, 0) ELSE 0 END),
                    SUM(CASE WHEN prompt_ms > 0 AND prompt_tokens > COALESCE(cached_tokens, 0)
                        THEN prompt_ms ELSE 0 END),
                    MAX(prompt_tokens),
                    SUM(CASE WHEN tools_offered > 0 AND tool_calls > 0 AND cancelled = 0
                        AND (tool_diag_category IS NULL OR tool_diag_category NOT IN ('runtime', 'model'))
                        THEN 1 ELSE 0 END),
                    SUM(CASE WHEN tools_offered > 0 AND cancelled = 0
                        AND tool_diag_category IN ('runtime', 'model') THEN 1 ELSE 0 END)
                 FROM requests WHERE alias IS NOT NULL AND alias <> '' GROUP BY alias",
            )
            .map_err(db_err)?;
        let rows = st
            .query_map([], |r| {
                let tokens = f(r, 2)?;
                let ms = f(r, 3)?;
                Ok(AgentSignals {
                    alias: r.get(0)?,
                    native_tool_calls: u(r, 1)?,
                    prompt_tps: (ms > 0.0 && tokens > 0.0).then(|| tokens * 1000.0 / ms),
                    prompt_tokens_timed: tokens as u64,
                    max_prompt_tokens: opt_u(r, 4)?,
                    tool_ok: u(r, 5)?,
                    tool_bad: u(r, 6)?,
                })
            })
            .map_err(db_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    /// Queue history per definition for requests since `since_ms`: how many waited for a free
    /// slot and whether more slots would have helped (raw rows only; requests recorded before
    /// the slot count was stored are ignored).
    pub fn queue_hints(&self, since_ms: u64) -> Result<Vec<QueueHint>, String> {
        let conn = self.lock();
        let mut st = conn
            .prepare(
                "SELECT definition_id, MAX(alias), slots, COUNT(*),
                    SUM(CASE WHEN waited = 'slots' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN waited = 'tokens' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN waited = 'slots' AND load_ms IS NULL THEN queue_wait_ms ELSE 0 END),
                    SUM(CASE WHEN waited = 'slots' AND load_ms IS NULL THEN 1 ELSE 0 END),
                    MAX(started_at_ms)
                 FROM requests
                 WHERE definition_id IS NOT NULL AND slots IS NOT NULL AND started_at_ms >= ?1
                    AND status < 500
                 GROUP BY definition_id, slots",
            )
            .map_err(db_err)?;
        let rows = st
            .query_map([since_ms as i64], |r| {
                let timed = u(r, 7)?;
                let wait_sum = f(r, 6)?;
                Ok((
                    u(r, 8)?,
                    QueueHint {
                        definition_id: r.get(0)?,
                        alias: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        slots: u(r, 2)? as u32,
                        requests: u(r, 3)?,
                        waited_slots: u(r, 4)?,
                        waited_tokens: u(r, 5)?,
                        wait_share: 0.0,
                        avg_wait_ms: if timed > 0 {
                            wait_sum / timed as f64
                        } else {
                            0.0
                        },
                        suggested_slots: None,
                    },
                ))
            })
            .map_err(db_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)?;
        // Per definition only the group of the latest slot count.
        let mut latest: std::collections::BTreeMap<String, (u64, QueueHint)> = Default::default();
        for (last, hint) in rows {
            match latest.get(&hint.definition_id) {
                Some((t, _)) if *t >= last => {}
                _ => {
                    latest.insert(hint.definition_id.clone(), (last, hint));
                }
            }
        }
        let mut out: Vec<QueueHint> = latest
            .into_values()
            .map(|(_, mut h)| {
                h.wait_share = if h.requests > 0 {
                    h.waited_slots as f64 / h.requests as f64
                } else {
                    0.0
                };
                h.suggest();
                h
            })
            .collect();
        out.sort_by(|a, b| a.alias.cmp(&b.alias));
        Ok(out)
    }

    pub fn checkpoint(&self) {
        let _ = self
            .lock()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }

    pub fn storage(&self) -> Result<StorageInfo, String> {
        let conn = self.lock();
        let count = |sql: &str| -> Result<u64, String> {
            conn.query_row(sql, [], |r| u(r, 0)).map_err(db_err)
        };
        let raw_rows = count("SELECT COUNT(*) FROM requests")?;
        let rollup_rows = count("SELECT COUNT(*) FROM daily_rollup")?;
        let content_rows = count("SELECT COUNT(*) FROM request_content")?;
        let oldest_raw_ms = conn
            .query_row("SELECT MIN(started_at_ms) FROM requests", [], |r| {
                opt_u(r, 0)
            })
            .map_err(db_err)?;
        let oldest_rollup_day_ms = conn
            .query_row("SELECT MIN(day) FROM daily_rollup", [], |r| opt_u(r, 0))
            .map_err(db_err)?
            .map(|d| d * DAY_MS);
        drop(conn);
        let size = |p: PathBuf| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        let mut wal = self.path.clone().into_os_string();
        wal.push("-wal");
        Ok(StorageInfo {
            raw_rows,
            rollup_rows,
            content_rows,
            oldest_raw_ms,
            oldest_rollup_day_ms,
            file_bytes: size(self.path.clone()) + size(PathBuf::from(wal)),
            path: self.path.to_string_lossy().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_migrates_and_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("analytics.db");
        let db = AnalyticsDb::open(&path).unwrap();
        assert!(!db.is_read_only());
        drop(db);
        let db = AnalyticsDb::open(&path).unwrap();
        let v: i64 = db
            .lock()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        assert_eq!(db.storage().unwrap().raw_rows, 0);
        // Empty database: an empty summary, not an error.
        let s = db.summary(None, None).unwrap();
        assert_eq!(s.totals.requests, 0);
        assert!(s.models.is_empty());
        assert_eq!(s.totals.ttft_p95_ms, None);
        drop(db);
        // A newer schema opens read-only and refuses writes.
        {
            let c = Connection::open(&path).unwrap();
            c.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
                .unwrap();
        }
        let db = AnalyticsDb::open(&path).unwrap();
        assert!(db.is_read_only());
        assert!(db.clear().is_err());
        assert!(db.maintain(0, 30).is_err());
    }

    /// A database written before the tool diagnosis (schema 4, with data) gets the columns,
    /// keeps its rows (they read as "no tools"), and new rows round-trip, filter and count.
    #[test]
    fn a_v4_database_migrates_and_the_tool_diagnosis_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("analytics.db");
        {
            let conn = Connection::open(&path).unwrap();
            for sql in &schema::MIGRATIONS[..4] {
                conn.execute_batch(sql).unwrap();
            }
            conn.execute(
                "INSERT INTO requests (id, started_at_ms, day, endpoint, alias, status, total_ms)
                 VALUES ('old', 1000, 0, '/v1/chat/completions', 'qwen', 200, 5)",
                [],
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 4).unwrap();
        }
        let db = AnalyticsDb::open(&path).unwrap();
        assert!(!db.is_read_only());
        let all = db.requests(&RequestQuery::default()).unwrap();
        assert_eq!(all.requests.len(), 1);
        let old = &all.requests[0];
        assert!(!old.tool_involved);
        assert_eq!((old.tools_offered, old.tool_calls), (0, 0));
        assert_eq!(old.tool_diag_category, None);

        let rec = |id: &str, at: u64, calls: u32, diag: Option<(&str, &str)>| {
            let mut r = RequestRecord {
                id: id.into(),
                started_at_ms: at,
                endpoint: "/v1/chat/completions".into(),
                alias: Some("coder".into()),
                status: 200,
                tool_involved: true,
                tools_offered: 3,
                tool_calls: calls,
                ..Default::default()
            };
            if let Some((cat, code)) = diag {
                r.tool_diag_category = Some(cat.into());
                r.tool_diag_code = Some(code.into());
                r.tool_diag_detail = Some("{\"tool\":\"edit\"}".into());
            }
            NewRequest {
                row: RequestRow::from_record(&r),
                content: None,
            }
        };
        db.insert(&[
            rec("ok1", 2000, 1, None),
            rec("ok2", 3000, 1, None),
            rec("text", 3500, 0, None),
            rec("rt", 4000, 0, Some(("runtime", "call_left_in_content"))),
            rec("md", 5000, 1, Some(("model", "repeated_call"))),
            rec("cl", 6000, 1, Some(("client", "tool_result_error"))),
        ])
        .unwrap();
        let issues = db
            .requests(&RequestQuery {
                tool_issue: Some(true),
                ..Default::default()
            })
            .unwrap();
        let ids: Vec<&str> = issues.requests.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["cl", "md", "rt"]);
        assert_eq!(
            issues.requests[2].tool_diag_code.as_deref(),
            Some("call_left_in_content")
        );
        assert_eq!(
            issues.requests[0].tool_diag_detail.as_deref(),
            Some("{\"tool\":\"edit\"}")
        );
        // Clean tool rounds: the old row (no tools) is not among them.
        let clean = db
            .requests(&RequestQuery {
                tool_issue: Some(false),
                ..Default::default()
            })
            .unwrap();
        let ids: Vec<&str> = clean.requests.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["text", "ok2", "ok1"]);
        let model = db
            .requests(&RequestQuery {
                tool_category: Some("model".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(model.requests.len(), 1);
        // Per alias: two clean calls; the runtime and the model fault count against the model,
        // the client's fault and the text answer do not.
        let sig = db.agent_signals().unwrap();
        let coder = sig.iter().find(|s| s.alias == "coder").unwrap();
        assert_eq!((coder.tool_ok, coder.tool_bad), (3, 2));
    }

    #[test]
    fn cloud_request_costs_are_stored_and_summed_per_model() {
        let dir = tempfile::tempdir().unwrap();
        let db = AnalyticsDb::open(&dir.path().join("a.db")).unwrap();
        let now = super::super::now_ms();
        let row = |id: &str, alias: &str, cost: Option<f64>| NewRequest {
            row: RequestRow {
                id: id.into(),
                started_at_ms: now,
                endpoint: "/v1/chat/completions".into(),
                alias: Some(alias.into()),
                status: 200,
                total_ms: 5,
                prompt_tokens: Some(1000),
                completion_tokens: Some(500),
                cost_usd: cost,
                ..Default::default()
            },
            content: None,
        };
        db.insert(&[
            row("a", "cloud:oa/gpt", Some(0.006)),
            row("b", "cloud:oa/gpt", Some(0.004)),
            row("c", "cloud:an/claude", Some(0.5)),
            row("d", "local", None),
        ])
        .unwrap();
        let page = db.requests(&RequestQuery::default()).unwrap();
        assert_eq!(
            page.requests
                .iter()
                .filter(|r| r.cost_usd.is_some())
                .count(),
            3
        );
        let cost = db.summary(None, None).unwrap().cost;
        assert_eq!(cost.models.len(), 2);
        assert_eq!(cost.models[0].alias, "cloud:an/claude");
        assert_eq!(cost.models[1].requests, 2);
        assert!((cost.total_usd - 0.51).abs() < 1e-9);
    }

    #[test]
    fn agent_signals_count_native_tool_calls_and_weigh_prompt_speed() {
        let dir = tempfile::tempdir().unwrap();
        let db = AnalyticsDb::open(&dir.path().join("a.db")).unwrap();
        {
            let conn = db.lock();
            let add = |id: &str, alias: &str, finish: &str, pt: i64, cached: i64, pms: f64| {
                conn.execute(
                    "INSERT INTO requests (id, started_at_ms, day, endpoint, alias, status, total_ms,
                        prompt_tokens, cached_tokens, prompt_ms, finish_reason)
                     VALUES (?1, 1, 0, '/v1/chat/completions', ?2, 200, 10, ?3, ?4, ?5, ?6)",
                    rusqlite::params![id, alias, pt, cached, pms, finish],
                )
                .unwrap();
            };
            // A 29k-token agent prompt at ~430 t/s, then the same prompt from the cache.
            add("a", "coder", "stop", 29_070, 0, 67_600.0);
            add("b", "coder", "stop", 29_100, 29_000, 250.0);
            add("c", "qwen3", "tool_calls", 1_000, 0, 1_000.0);
            add("d", "qwen3", "tool_use", 100, 0, 100.0);
        }
        let mut s = db.agent_signals().unwrap();
        s.sort_by(|a, b| a.alias.cmp(&b.alias));
        assert_eq!(s[0].alias, "coder");
        assert_eq!(s[0].native_tool_calls, 0);
        let tps = s[0].prompt_tps.unwrap();
        assert!((tps - 29_170.0 * 1000.0 / 67_850.0).abs() < 0.01, "{tps}");
        assert_eq!(s[0].max_prompt_tokens, Some(29_100));
        assert_eq!(s[1].native_tool_calls, 2);
        assert_eq!(s[1].prompt_tps, Some(1000.0));
    }

    #[test]
    fn queue_hints_follow_the_latest_slot_count_and_the_rule() {
        let dir = tempfile::tempdir().unwrap();
        let db = AnalyticsDb::open(&dir.path().join("a.db")).unwrap();
        {
            let conn = db.lock();
            let add = |n: u32, def: &str, slots: i64, waited: Option<&str>, wait: i64, t: i64| {
                conn.execute(
                    "INSERT INTO requests (id, started_at_ms, day, endpoint, alias, definition_id,
                        status, total_ms, queue_wait_ms, slots, waited)
                     VALUES (?1, ?2, 0, '/v1/chat/completions', ?3, ?3, 200, 10, ?4, ?5, ?6)",
                    rusqlite::params![format!("{def}-{n}-{t}"), t, def, wait, slots, waited],
                )
                .unwrap();
            };
            // "busy": 1 slot, 12 requests, 5 of them waited ~2 s -> suggest 2. An older load
            // with 4 slots must not count.
            for n in 0..12 {
                let waited = (n < 5).then_some("slots");
                add(
                    n,
                    "busy",
                    1,
                    waited,
                    if n < 5 { 2000 } else { 0 },
                    1000 + n as i64,
                );
            }
            for n in 0..30 {
                add(n, "busy", 4, None, 0, 10 + n as i64);
            }
            // "calm": waits are rare.
            for n in 0..20 {
                add(
                    n,
                    "calm",
                    2,
                    (n == 0).then_some("slots"),
                    3000,
                    1000 + n as i64,
                );
            }
            // "pool": waits are for tokens only; more slots would not help.
            for n in 0..20 {
                add(n, "pool", 2, Some("tokens"), 4000, 1000 + n as i64);
            }
            // "few": too little history.
            for n in 0..4 {
                add(n, "few", 1, Some("slots"), 5000, 1000 + n as i64);
            }
            // "max": already at the most.
            for n in 0..20 {
                add(n, "max", 8, Some("slots"), 5000, 1000 + n as i64);
            }
            // Before the window.
            add(0, "old", 1, Some("slots"), 5000, 1);
        }
        let hints = db.queue_hints(100).unwrap();
        let get = |a: &str| hints.iter().find(|h| h.alias == a).unwrap().clone();
        let busy = get("busy");
        assert_eq!((busy.slots, busy.requests, busy.waited_slots), (1, 12, 5));
        assert!((busy.wait_share - 5.0 / 12.0).abs() < 1e-9);
        assert_eq!(busy.avg_wait_ms, 2000.0);
        assert_eq!(busy.suggested_slots, Some(2));
        assert_eq!(get("calm").suggested_slots, None);
        let pool = get("pool");
        assert_eq!((pool.waited_slots, pool.waited_tokens), (0, 20));
        assert_eq!(pool.suggested_slots, None);
        assert_eq!(get("few").suggested_slots, None);
        assert_eq!(get("max").suggested_slots, None);
        assert!(hints.iter().all(|h| h.alias != "old"));
    }

    #[test]
    fn agg_column_count_matches_rollup_table() {
        let dir = tempfile::tempdir().unwrap();
        let db = AnalyticsDb::open(&dir.path().join("a.db")).unwrap();
        let conn = db.lock();
        let cols: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('daily_rollup')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        // day, alias, client, endpoint + the aggregates.
        assert_eq!(cols as usize, 4 + AGG_COUNT);
    }
    #[test]
    fn attributed_direct_clients_merge_with_gateway_and_survive_retention() {
        use crate::analytics::direct::DirectClient;
        let dir = tempfile::tempdir().unwrap();
        let db = AnalyticsDb::open(&dir.path().join("analytics.db")).unwrap();
        let at = 100 * DAY_MS;
        db.direct_add_client(
            at,
            "s",
            "m",
            &DirectClient {
                client: "harness-auto-tools".into(),
                requests: 2,
                prompt_tokens: 20,
                completion_tokens: 10,
            },
        )
        .unwrap();
        db.direct_add_client(
            at,
            "s",
            "m",
            &DirectClient {
                client: "harness-tool-creator".into(),
                requests: 1,
                prompt_tokens: 5,
                completion_tokens: 5,
            },
        )
        .unwrap();
        db.direct_add(at, "s", "m", 2, 3).unwrap();
        db.lock().execute(
            "INSERT INTO requests(id,started_at_ms,day,endpoint,alias,client,status,total_ms,prompt_tokens,completion_tokens)
             VALUES('labelled',?1,100,'/v1/chat/completions','m','harness-auto-tools',200,10,7,3)", [at as i64]
        ).unwrap();
        let summary = db.summary(Some(at), Some(at + DAY_MS)).unwrap();
        assert_eq!(summary.clients.len(), 3);
        assert_eq!(summary.totals.requests, 4);
        assert_eq!(summary.clients.iter().map(|c| c.requests).sum::<u64>(), 4);
        assert_eq!(
            summary
                .clients
                .iter()
                .find(|c| c.client == "harness-auto-tools")
                .unwrap()
                .share,
            0.75
        );
        assert_eq!(
            summary
                .clients
                .iter()
                .find(|c| c.client == "harness-tool-creator")
                .unwrap()
                .share,
            0.25
        );
        assert_eq!(
            summary.clients.iter().map(|c| c.total_tokens).sum::<u64>(),
            55
        );
        assert!((summary.clients.iter().map(|c| c.token_share).sum::<f64>() - 1.0).abs() < 1e-9);
        assert_eq!(
            summary
                .clients
                .iter()
                .find(|c| c.client == "harness-auto-tools")
                .unwrap()
                .requests,
            3
        );
        assert!(db
            .summary(Some(at + DAY_MS), None)
            .unwrap()
            .clients
            .is_empty());
        db.maintain(at + 40 * DAY_MS, 30).unwrap();
        let folded = db.summary(Some(at), Some(at + DAY_MS)).unwrap();
        assert_eq!(
            serde_json::to_value(folded.clients).unwrap(),
            serde_json::to_value(summary.clients).unwrap()
        );
        db.clear().unwrap();
        assert!(db.summary(None, None).unwrap().clients.is_empty());
    }

    #[test]
    fn summary_includes_direct_usage_as_client_with_proper_token_share() {
        let dir = tempfile::tempdir().unwrap();
        let db = AnalyticsDb::open(&dir.path().join("a.db")).unwrap();
        let day0 = 100 * DAY_MS;
        let min_ms = 60_000;
        let min0 = day0 + min_ms;

        // Add regular gateway usage
        {
            let conn = db.lock();
            conn.execute(
                "INSERT INTO requests (id, started_at_ms, day, endpoint, alias, status, total_ms,
                    prompt_tokens, completion_tokens, cached_tokens, prompt_ms)
                 VALUES ('r1', ?1, 0, '/v1/chat', 'model1', 200, 10, 50, 50, 0, 10.0)",
                rusqlite::params![i64::try_from(min0).unwrap()],
            )
            .unwrap();
        }

        // Add direct usage (no gateway requests)
        db.direct_add(min0 + min_ms, "server1", "model1", 100, 100)
            .unwrap();

        // Gateway tokens: 100 (50 prompt + 50 comp)
        // Direct tokens: 200 (100 prompt + 100 comp)
        // Total tokens: 300
        let s = db.summary(None, None).unwrap();

        let client = s
            .clients
            .iter()
            .find(|c| c.client == "direct_usage")
            .expect("direct_usage client missing");
        assert_eq!(client.requests, 0); // direct usage has 0 requests
        assert_eq!(client.prompt_tokens, 100);
        assert_eq!(client.completion_tokens, 100);
        assert_eq!(client.total_tokens, 200);
        assert_eq!(client.share, 0.0); // requests share is 0
        assert_eq!(client.token_share, 200.0 / 300.0);

        // Also check an empty range, should have 0 for direct usage and no client for it
        let s_empty = db
            .summary(Some(min0 + 10 * min_ms), Some(min0 + 11 * min_ms))
            .unwrap();
        assert!(s_empty
            .clients
            .iter()
            .find(|c| c.client == "direct_usage")
            .is_none());
        assert_eq!(s_empty.direct.prompt_tokens, 0);

        // Check range that only includes direct usage
        let s_direct_only = db
            .summary(Some(min0 + min_ms), Some(min0 + 2 * min_ms))
            .unwrap();
        let client = s_direct_only
            .clients
            .iter()
            .find(|c| c.client == "direct_usage")
            .unwrap();
        assert_eq!(client.token_share, 1.0); // 200 / 200
    }
}
