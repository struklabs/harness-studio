//! Usage of the llama-server ports that bypass the gateway ("direct"), seen only as
//! aggregates.
//!
//! A client that talks to a llama-server port itself leaves no request record: nothing
//! passes through the gateway. What stays visible are the server's own `/metrics` counters
//! (`prompt_tokens_total`, `tokens_predicted_total`), which the telemetry poller samples
//! anyway. Those counters also contain the gateway's requests to the same server, so:
//!
//! ```text
//! direct = metrics delta - tokens of the gateway requests recorded for that server
//! ```
//!
//! per sampling interval, for prompt tokens (what the server processed: the usage
//! `prompt_tokens` minus the cached ones) and generated tokens. Nothing is guessed: when the
//! gateway recorded more than the server counted the difference is not negative direct usage
//! but zero. The gateway's side comes from memory ([`note_gateway`], called when a request
//! record is submitted), not from the database, so the two sides are compared at the same
//! moment.
//!
//! # Skew between the two sides
//! llama-server bumps its counters and the gateway submits its record at slightly different
//! moments, so a request can show up in one side one sampling interval before the other. A
//! difference is therefore only counted once it has stayed for two samples
//! ([`DirectTracker`]); a request that is seen on one side first never turns into "direct".
//! Late explicit responses reassign emitted anonymous counters to their client in one DB
//! transaction, including counters already folded by retention. The tracker subtracts
//! processed input; the attributed record keeps cache-inclusive input.
//! The cost is that direct usage appears one sampling interval late and that what is still
//! pending when a server stops is dropped.
//!
//! # What is stored
//! `direct_usage`: tokens per minute and server, with the model label of the server run;
//! minutes older than the raw retention are folded into `direct_daily` (per day and server).
//! There are no requests, clients or timings: only token totals. Servers without `/metrics`
//! (started with metrics off) show nothing.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::db::{day_of, db_err, range_sql, AnalyticsDb, DAY_MS};
use crate::gateway::record::RequestRecord;
use crate::telemetry::Metrics;

const MINUTE_MS: u64 = 60_000;

/// Tokens of one server run in a range.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DirectServer {
    pub server_id: String,
    /// What the server ran (model file name).
    pub label: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DirectDay {
    /// Start of the (UTC) day, Unix ms.
    pub day_start_ms: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

/// Direct usage in the range of an analytics summary: aggregate token totals only, no
/// per-request detail. See the module docs for how it is derived.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DirectUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    /// Per server run, most tokens first.
    pub servers: Vec<DirectServer>,
    pub daily: Vec<DirectDay>,
    pub clients: Vec<DirectClient>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DirectClient {
    pub client: String,
    pub requests: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

/// One explicit direct request. Cached tokens are only used to subtract metrics counters.
#[derive(Debug, Clone, Deserialize)]
pub struct DirectRecord {
    pub endpoint: String,
    pub client: String,
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    #[serde(default)]
    pub cached_tokens: u64,
}

impl DirectUsage {
    pub fn is_empty(&self) -> bool {
        self.prompt_tokens == 0 && self.completion_tokens == 0
    }
}

/// Removes anonymous counters in the same transaction as their attributed replacement.
fn retract_direct(
    conn: &rusqlite::Connection,
    server_id: &str,
    mut tokens: Tokens,
) -> Result<(), String> {
    if tokens == Tokens::default() {
        return Ok(());
    }
    for (table, time) in [("direct_usage", "minute_ms"), ("direct_daily", "day")] {
        let rows = {
            let mut st = conn
                .prepare(&format!(
                    "SELECT {time},prompt_tokens,completion_tokens FROM {table}
                     WHERE server_id=?1 AND client='direct_usage' ORDER BY {time} DESC"
                ))
                .map_err(db_err)?;
            let rows = st
                .query_map([server_id], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, u64>(1)?,
                        r.get::<_, u64>(2)?,
                    ))
                })
                .map_err(db_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db_err)?;
            rows
        };
        for (at, prompt, completion) in rows {
            let p = tokens.prompt.min(prompt);
            let c = tokens.completion.min(completion);
            conn.execute(&format!(
                    "UPDATE {table} SET prompt_tokens=prompt_tokens-?1,completion_tokens=completion_tokens-?2
                     WHERE {time}=?3 AND server_id=?4 AND client='direct_usage'"
                ), params![p, c, at, server_id]).map_err(db_err)?;
            tokens.prompt -= p;
            tokens.completion -= c;
        }
    }
    Ok(())
}

impl AnalyticsDb {
    /// Adds tokens to the minute of `at_ms`.
    pub fn direct_add(
        &self,
        at_ms: u64,
        server_id: &str,
        label: &str,
        prompt_tokens: u64,
        completion_tokens: u64,
    ) -> Result<(), String> {
        self.direct_add_client(
            at_ms,
            server_id,
            label,
            &DirectClient {
                client: "direct_usage".into(),
                requests: 0,
                prompt_tokens,
                completion_tokens,
            },
        )
    }

    pub fn direct_add_client(
        &self,
        at_ms: u64,
        server_id: &str,
        label: &str,
        usage: &DirectClient,
    ) -> Result<(), String> {
        self.direct_add_reconciled(at_ms, server_id, label, usage, Tokens::default())
    }

    fn direct_add_reconciled(
        &self,
        at_ms: u64,
        server_id: &str,
        label: &str,
        usage: &DirectClient,
        correction: Tokens,
    ) -> Result<(), String> {
        self.writable()?;
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(db_err)?;
        retract_direct(&tx, server_id, correction)?;
        tx.execute(
            "INSERT INTO direct_usage(minute_ms,day,server_id,label,client,requests,prompt_tokens,completion_tokens)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(minute_ms,server_id,client) DO UPDATE SET
             requests=requests+excluded.requests, prompt_tokens=prompt_tokens+excluded.prompt_tokens,
             completion_tokens=completion_tokens+excluded.completion_tokens,label=excluded.label",
            params![ (at_ms / MINUTE_MS * MINUTE_MS) as i64, day_of(at_ms), server_id, label,
             usage.client, usage.requests.min(i64::MAX as u64) as i64,
             usage.prompt_tokens.min(i64::MAX as u64) as i64,usage.completion_tokens.min(i64::MAX as u64) as i64]
        ).map_err(db_err)?;
        tx.commit().map_err(db_err)
    }

    /// Direct usage in `[from_ms, to_ms)`: minute rows plus the daily rollups of days whose
    /// minutes are gone.
    pub fn direct_usage(
        &self,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
    ) -> Result<DirectUsage, String> {
        let (_, rollup) = range_sql(from_ms, to_ms);
        let rollup = rollup.replace(
            "SELECT DISTINCT day FROM requests",
            "SELECT DISTINCT day FROM direct_usage",
        );
        let raw = format!(
            "minute_ms >= {} AND minute_ms < {}",
            from_ms.unwrap_or(0).min(i64::MAX as u64),
            to_ms.unwrap_or(i64::MAX as u64).min(i64::MAX as u64)
        );
        let conn = self.lock();
        let mut servers: BTreeMap<String, DirectServer> = BTreeMap::new();
        let mut days: BTreeMap<i64, DirectDay> = BTreeMap::new();
        let mut clients: BTreeMap<String, DirectClient> = BTreeMap::new();
        let sources = [
            format!(
                "SELECT day, server_id, label, prompt_tokens, completion_tokens, client, requests
                 FROM direct_usage WHERE {raw}"
            ),
            format!(
                "SELECT day, server_id, label, prompt_tokens, completion_tokens, client, requests
                 FROM direct_daily WHERE {rollup}"
            ),
        ];
        for sql in &sources {
            let mut st = conn.prepare(sql).map_err(db_err)?;
            let rows = st
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?.max(0) as u64,
                        r.get::<_, i64>(4)?.max(0) as u64,
                        r.get::<_, String>(5)?,
                        r.get::<_, i64>(6)?.max(0) as u64,
                    ))
                })
                .map_err(db_err)?;
            for row in rows {
                let (day, id, label, p, c, client, requests) = row.map_err(db_err)?;
                let entry = clients
                    .entry(client.clone())
                    .or_insert_with(|| DirectClient {
                        client,
                        ..Default::default()
                    });
                entry.requests += requests;
                entry.prompt_tokens += p;
                entry.completion_tokens += c;
                let s = servers.entry(id.clone()).or_insert_with(|| DirectServer {
                    server_id: id,
                    ..Default::default()
                });
                s.label = label;
                s.prompt_tokens += p;
                s.completion_tokens += c;
                let d = days.entry(day).or_insert_with(|| DirectDay {
                    day_start_ms: day.max(0) as u64 * DAY_MS,
                    ..Default::default()
                });
                d.prompt_tokens += p;
                d.completion_tokens += c;
            }
        }
        let mut servers: Vec<DirectServer> = servers.into_values().collect();
        servers.sort_by(|a, b| {
            (b.prompt_tokens + b.completion_tokens)
                .cmp(&(a.prompt_tokens + a.completion_tokens))
                .then(a.server_id.cmp(&b.server_id))
        });
        Ok(DirectUsage {
            prompt_tokens: servers.iter().map(|s| s.prompt_tokens).sum(),
            completion_tokens: servers.iter().map(|s| s.completion_tokens).sum(),
            servers,
            daily: days.into_values().collect(),
            clients: clients.into_values().collect(),
        })
    }

    /// Folds the minute rows of days before `cutoff_day` into `direct_daily`. Called inside
    /// the maintenance transaction.
    pub(super) fn fold_direct(
        tx: &rusqlite::Transaction<'_>,
        cutoff_day: i64,
    ) -> Result<(), String> {
        tx.execute(
            "INSERT INTO direct_daily (day, server_id, label, prompt_tokens, completion_tokens, client, requests)
             SELECT day, server_id, MAX(label), SUM(prompt_tokens), SUM(completion_tokens), client, SUM(requests)
             FROM direct_usage WHERE day < ?1 GROUP BY day, server_id, client
             ON CONFLICT(day, server_id, client) DO UPDATE SET
                prompt_tokens = prompt_tokens + excluded.prompt_tokens,
                completion_tokens = completion_tokens + excluded.completion_tokens,
                label = excluded.label, requests = requests + excluded.requests",
            [cutoff_day],
        )
        .map_err(db_err)?;
        tx.execute("DELETE FROM direct_usage WHERE day < ?1", [cutoff_day])
            .map_err(db_err)?;
        Ok(())
    }
}

/// Token counters of one dimension, as (prompt tokens the server processed, generated).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tokens {
    pub prompt: u64,
    pub completion: u64,
}

impl Tokens {
    fn minus(self, other: Tokens) -> (i64, i64) {
        (
            self.prompt as i64 - other.prompt as i64,
            self.completion as i64 - other.completion as i64,
        )
    }
}

/// The server-side counters of a `/metrics` sample (`None` when it has none of them).
pub fn server_counters(m: &Metrics) -> Option<Tokens> {
    let prompt = m.get("prompt_tokens_total");
    let gen = m.get("tokens_predicted_total");
    if prompt.is_none() && gen.is_none() {
        return None;
    }
    Some(Tokens {
        prompt: prompt.copied().unwrap_or(0.0).max(0.0) as u64,
        completion: gen.copied().unwrap_or(0.0).max(0.0) as u64,
    })
}

/// One signed dimension of the running difference between the server and the gateway.
#[derive(Debug, Clone, Copy, Default)]
struct Lane {
    /// Server minus gateway since the baseline (floored, see [`Lane::step`]).
    diff: i64,
    previous: i64,
    emitted: i64,
    correction: u64,
}

impl Lane {
    /// One sampling interval: the server counted `server`, the gateway recorded `gateway`
    /// tokens. Returns the tokens that became direct usage.
    fn step(&mut self, server: u64, gateway: u64) -> u64 {
        let mut diff = self.diff + server as i64 - gateway as i64;
        // The gateway can be ahead by what it recorded in this interval (skew), no more.
        diff = diff.max(self.emitted - gateway as i64);
        let retract = (self.emitted - diff.max(0)).max(0);
        self.emitted -= retract;
        self.correction += retract as u64;
        // Settled: unchanged over two samples. A difference seen only once may be a request
        // that the other side books one interval later.
        let settled = diff.min(self.previous);
        let emit = (settled - self.emitted).max(0);
        self.emitted += emit;
        self.previous = diff;
        self.diff = diff;
        emit as u64
    }
}

#[derive(Debug, Clone, Default)]
struct ServerRun {
    last: Option<(Tokens, Tokens)>,
    prompt: Lane,
    completion: Lane,
}

/// Direct usage as it is derived from successive samples; see the module docs. Pure: it
/// holds no clock and does no I/O.
#[derive(Debug, Default)]
pub struct DirectTracker {
    servers: HashMap<String, ServerRun>,
}

impl DirectTracker {
    fn note_explicit(&mut self, server_id: &str, tokens: Tokens) -> Tokens {
        let Some(run) = self.servers.get_mut(server_id) else {
            return Tokens::default();
        };
        for (lane, known) in [
            (&mut run.prompt, tokens.prompt),
            (&mut run.completion, tokens.completion),
        ] {
            lane.diff -= known as i64;
            lane.previous -= known as i64;
            let retract = (lane.emitted - lane.diff.max(0)).max(0);
            lane.emitted -= retract;
            lane.correction += retract as u64;
        }
        if let Some((_, gateway)) = &mut run.last {
            gateway.prompt += tokens.prompt;
            gateway.completion += tokens.completion;
        }
        self.take_correction(server_id)
    }
    fn take_correction(&mut self, server_id: &str) -> Tokens {
        let Some(run) = self.servers.get_mut(server_id) else {
            return Tokens::default();
        };
        Tokens {
            prompt: std::mem::take(&mut run.prompt.correction),
            completion: std::mem::take(&mut run.completion.correction),
        }
    }
    /// One `/metrics` sample of `server_id` with the gateway's running totals for it at the
    /// same moment. Returns the direct tokens of this interval, if any. The first sample of
    /// a server (and the first after its counters went backwards, i.e. a restart) only sets
    /// the baseline.
    pub fn observe(
        &mut self,
        server_id: &str,
        metrics: &Metrics,
        gateway: Tokens,
    ) -> Option<Tokens> {
        let cur = server_counters(metrics)?;
        let run = self.servers.entry(server_id.to_string()).or_default();
        let Some((prev_server, prev_gateway)) = run.last else {
            *run = ServerRun {
                last: Some((cur, gateway)),
                ..Default::default()
            };
            return None;
        };
        if cur.prompt < prev_server.prompt || cur.completion < prev_server.completion {
            *run = ServerRun {
                last: Some((cur, gateway)),
                ..Default::default()
            };
            return None;
        }
        let (dp, dc) = cur.minus(prev_server);
        let (gp, gc) = gateway.minus(prev_gateway);
        run.last = Some((cur, gateway));
        let prompt = run.prompt.step(dp.max(0) as u64, gp.max(0) as u64);
        let completion = run.completion.step(dc.max(0) as u64, gc.max(0) as u64);
        (prompt > 0 || completion > 0).then_some(Tokens { prompt, completion })
    }

    /// Forgets servers that are not in `active` any more.
    pub fn retain(&mut self, active: &[String]) {
        self.servers.retain(|id, _| active.contains(id));
    }
}

fn gateway_totals() -> &'static Mutex<HashMap<String, Tokens>> {
    static CELL: std::sync::OnceLock<Mutex<HashMap<String, Tokens>>> = std::sync::OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Tokens the gateway recorded for a server since this process started, in the unit of the
/// server's `/metrics`: prompt tokens it processed (usage `prompt_tokens` minus cached) and
/// generated tokens.
pub fn tokens_of(record: &RequestRecord) -> Tokens {
    let prompt = record.usage.prompt_tokens.unwrap_or(0);
    Tokens {
        prompt: prompt.saturating_sub(record.usage.cached_tokens.unwrap_or(0)),
        completion: record.usage.completion_tokens.unwrap_or(0),
    }
}

/// Counts a finished gateway request against its server (called with every submitted
/// record).
pub fn note_gateway(record: &RequestRecord) {
    let Some(id) = record.server_id.as_deref() else {
        return;
    };
    let t = tokens_of(record);
    let mut all = gateway_totals().lock().unwrap_or_else(|e| e.into_inner());
    let e = all.entry(id.to_string()).or_default();
    e.prompt += t.prompt;
    e.completion += t.completion;
}

fn gateway_of(server_id: &str) -> Tokens {
    gateway_totals()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(server_id)
        .copied()
        .unwrap_or_default()
}

fn tracker() -> &'static Mutex<DirectTracker> {
    static CELL: std::sync::OnceLock<Mutex<DirectTracker>> = std::sync::OnceLock::new();
    CELL.get_or_init(|| Mutex::new(DirectTracker::default()))
}

/// A server run to watch: its id, what it runs and its latest `/metrics`.
pub struct Sample<'a> {
    pub server_id: &'a str,
    pub label: &'a str,
    pub metrics: &'a Metrics,
}

/// Called after every telemetry round with the servers that answered `/metrics` and the ids
/// of all active servers. Writes the direct tokens of this interval (when recording is on).
pub fn observe(samples: &[Sample<'_>], active: &[String]) {
    let now = super::now_ms();
    let mut t = tracker().lock().unwrap_or_else(|e| e.into_inner());
    let found: Vec<(String, String, Tokens, Tokens)> = {
        t.retain(active);
        samples
            .iter()
            .filter_map(|s| {
                let emitted = t.observe(s.server_id, s.metrics, gateway_of(s.server_id));
                let correction = t.take_correction(s.server_id);
                (emitted.is_some() || correction != Tokens::default()).then(|| {
                    (
                        s.server_id.to_string(),
                        s.label.to_string(),
                        emitted.unwrap_or_default(),
                        correction,
                    )
                })
            })
            .collect()
    };
    if found.is_empty() || !crate::config::AppSettings::load().analytics.enabled {
        return;
    }
    let db = match super::shared() {
        Ok(db) => db,
        Err(e) => {
            super::set_error(e);
            return;
        }
    };
    for (id, label, d, correction) in found {
        if let Err(e) = db.direct_add_reconciled(
            now,
            &id,
            &label,
            &DirectClient {
                client: "direct_usage".into(),
                requests: 0,
                prompt_tokens: d.prompt,
                completion_tokens: d.completion,
            },
            correction,
        ) {
            super::set_error(e);
        }
    }
}

// Match explicit clients to observed servers so metrics do not count their tokens again.
fn endpoints() -> &'static Mutex<HashMap<String, String>> {
    static CELL: std::sync::OnceLock<Mutex<HashMap<String, String>>> = std::sync::OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}
fn endpoint_key(url: &str) -> String {
    let Ok(mut parsed) = reqwest::Url::parse(url) else {
        return url
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .to_string();
    };
    if parsed.host_str() == Some("localhost") {
        let _ = parsed.set_host(Some("127.0.0.1"));
    }
    parsed
        .as_str()
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .to_string()
}
pub fn register_servers(servers: &[(String, crate::process::ActiveServerInfo)]) {
    let mut map = endpoints().lock().unwrap_or_else(|e| e.into_inner());
    map.clear();
    for (id, info) in servers {
        map.insert(
            endpoint_key(&format!("http://{}:{}", info.connect_host, info.port)),
            id.clone(),
        );
    }
}
/// Only call for responses without the gateway's x-harness-request-id header.
/// Recording is optional; analytics errors must never fail a model answer.
pub fn record_client(record: &DirectRecord) -> Result<(), String> {
    if !crate::config::AppSettings::load().analytics.enabled {
        return Ok(());
    }
    let key = endpoint_key(&record.endpoint);
    let id = endpoints()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
        .cloned();
    let db = super::shared()?;
    let mut tracker = tracker().lock().unwrap_or_else(|e| e.into_inner());
    let correction = if let Some(id) = &id {
        let tokens = Tokens {
            prompt: record.prompt_tokens.saturating_sub(record.cached_tokens),
            completion: record.completion_tokens,
        };
        let mut all = gateway_totals().lock().unwrap_or_else(|e| e.into_inner());
        let entry = all.entry(id.clone()).or_default();
        entry.prompt += tokens.prompt;
        entry.completion += tokens.completion;
        tracker.note_explicit(id, tokens)
    } else {
        Tokens::default()
    };
    db.direct_add_reconciled(
        super::now_ms(),
        id.as_deref().unwrap_or(&key),
        &record.model,
        &DirectClient {
            client: record.client.clone(),
            requests: 1,
            prompt_tokens: record.prompt_tokens,
            completion_tokens: record.completion_tokens,
        },
        correction,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::parse_prometheus;

    fn data(name: &str) -> Metrics {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name);
        parse_prometheus(&std::fs::read_to_string(path).unwrap())
    }

    fn t(prompt: u64, completion: u64) -> Tokens {
        Tokens { prompt, completion }
    }

    fn metrics(prompt: u64, completion: u64) -> Metrics {
        Metrics::from([
            ("prompt_tokens_total".to_string(), prompt as f64),
            ("tokens_predicted_total".to_string(), completion as f64),
        ])
    }

    #[test]
    fn real_metrics_samples_give_the_counters() {
        // An idle b11236 server and one that ran a few drafted requests (real captures).
        assert_eq!(server_counters(&data("b11236.metrics.txt")), Some(t(0, 0)));
        let used = data("b11236-qwen3-4b-draft.metrics.txt");
        assert_eq!(server_counters(&used), Some(t(26, 110)));
        assert_eq!(server_counters(&Metrics::new()), None);
    }

    #[test]
    fn real_counters_become_direct_usage_after_two_samples() {
        let mut tr = DirectTracker::default();
        let idle = data("b11236.metrics.txt");
        let used = data("b11236-qwen3-4b-draft.metrics.txt");
        // Baseline, then the server's counters jump with no gateway request behind them.
        assert_eq!(tr.observe("s", &idle, t(0, 0)), None);
        assert_eq!(
            tr.observe("s", &used, t(0, 0)),
            None,
            "seen once: not settled"
        );
        assert_eq!(tr.observe("s", &used, t(0, 0)), Some(t(26, 110)));
        assert_eq!(tr.observe("s", &used, t(0, 0)), None, "counted once");
    }

    #[test]
    fn gateway_tokens_are_subtracted_and_never_make_it_negative() {
        let mut tr = DirectTracker::default();
        tr.observe("s", &metrics(100, 50), t(100, 50));
        // The server did 40+30 more; the gateway recorded 40+30 of it: nothing direct.
        for _ in 0..3 {
            assert_eq!(tr.observe("s", &metrics(140, 80), t(140, 80)), None);
        }
        // The gateway recorded more than the server counted: still nothing, no negatives.
        for _ in 0..3 {
            assert_eq!(tr.observe("s", &metrics(150, 90), t(200, 120)), None);
        }
        // Real direct usage next to gateway traffic: server +70/+25, gateway +50/+10.
        assert_eq!(tr.observe("s", &metrics(220, 115), t(250, 130)), None);
        assert_eq!(
            tr.observe("s", &metrics(220, 115), t(250, 130)),
            Some(t(20, 15))
        );
    }

    #[test]
    fn a_request_booked_one_interval_apart_is_not_direct() {
        // The server counts first, the gateway one sample later.
        let mut tr = DirectTracker::default();
        tr.observe("s", &metrics(0, 0), t(0, 0));
        assert_eq!(tr.observe("s", &metrics(100, 40), t(0, 0)), None);
        assert_eq!(tr.observe("s", &metrics(100, 40), t(100, 40)), None);
        assert_eq!(tr.observe("s", &metrics(100, 40), t(100, 40)), None);
        // The gateway records first, the server one sample later.
        assert_eq!(tr.observe("s", &metrics(100, 40), t(200, 90)), None);
        assert_eq!(tr.observe("s", &metrics(200, 90), t(200, 90)), None);
        assert_eq!(tr.observe("s", &metrics(200, 90), t(200, 90)), None);
        // Direct usage afterwards is still found.
        assert_eq!(tr.observe("s", &metrics(230, 90), t(200, 90)), None);
        assert_eq!(
            tr.observe("s", &metrics(230, 90), t(200, 90)),
            Some(t(30, 0))
        );
    }

    #[test]
    fn counters_going_backwards_are_a_restart() {
        let mut tr = DirectTracker::default();
        tr.observe("s", &metrics(500, 500), t(0, 0));
        // Restarted: counters start at 0 again; that is a new baseline, not usage.
        assert_eq!(tr.observe("s", &metrics(10, 5), t(0, 0)), None);
        assert_eq!(tr.observe("s", &metrics(10, 5), t(0, 0)), None);
        assert_eq!(tr.observe("s", &metrics(20, 5), t(0, 0)), None);
        assert_eq!(tr.observe("s", &metrics(20, 5), t(0, 0)), Some(t(10, 0)));
        // A server without those counters yields nothing.
        assert_eq!(tr.observe("x", &Metrics::new(), t(0, 0)), None);
        tr.retain(&[]);
        assert!(tr.servers.is_empty());
    }

    #[test]
    fn gateway_records_count_processed_prompt_tokens() {
        let r = RequestRecord {
            usage: crate::gateway::record::Usage {
                prompt_tokens: Some(17),
                cached_tokens: Some(9),
                completion_tokens: Some(30),
                ..Default::default()
            },
            ..Default::default()
        };
        // The real chat response behind `b11236-qwen3-4b-draft.metrics.txt`: 17 prompt
        // tokens of which 9 cached = `prompt_n` 8 processed.
        assert_eq!(tokens_of(&r), t(8, 30));
    }

    /// Real `/metrics` of llama-server b11026 (Qwen2.5-0.5B) after the same chat was sent
    /// twice straight to the port: usage 41 prompt (0 cached) + 40 generated, then 41 prompt
    /// (40 cached) + 40 generated. The server's `prompt_tokens_total` is 42 = 41 + 1
    /// processed, so the gateway's side of the subtraction is prompt minus cached.
    #[test]
    fn real_two_chat_run_matches_the_gateway_unit() {
        let after = data("b11026-qwen2.5-0.5b-two-chats.metrics.txt");
        assert_eq!(server_counters(&after), Some(t(42, 80)));
        let usage = |prompt, cached, completion| RequestRecord {
            server_id: Some("s".into()),
            usage: crate::gateway::record::Usage {
                prompt_tokens: Some(prompt),
                cached_tokens: Some(cached),
                completion_tokens: Some(completion),
                ..Default::default()
            },
            ..Default::default()
        };
        let (a, b) = (tokens_of(&usage(41, 0, 40)), tokens_of(&usage(41, 40, 40)));
        assert_eq!((a.prompt + b.prompt, a.completion + b.completion), (42, 80));

        let before = metrics(0, 0);
        // Both chats went through the gateway: nothing is direct.
        let mut tr = DirectTracker::default();
        tr.observe("s", &before, t(0, 0));
        for _ in 0..3 {
            assert_eq!(tr.observe("s", &after, t(42, 80)), None);
        }
        // Only the first one did: the second (1 processed, 40 generated) was direct.
        let mut tr = DirectTracker::default();
        tr.observe("s", &before, t(0, 0));
        assert_eq!(tr.observe("s", &after, a), None);
        assert_eq!(tr.observe("s", &after, a), Some(t(1, 40)));
    }

    #[test]
    fn stored_per_minute_then_folded_into_days() {
        let dir = tempfile::tempdir().unwrap();
        let db = AnalyticsDb::open(&dir.path().join("a.db")).unwrap();
        let day0 = 100 * DAY_MS;
        db.direct_add(day0 + 5 * MINUTE_MS + 100, "a", "qwen", 10, 4)
            .unwrap();
        db.direct_add(day0 + 5 * MINUTE_MS + 59_000, "a", "qwen", 5, 1)
            .unwrap();
        db.direct_add(day0 + 6 * MINUTE_MS, "b", "gemma", 1, 1)
            .unwrap();
        db.direct_add(day0 + DAY_MS + 1, "a", "qwen", 7, 7).unwrap();
        let u = db.direct_usage(None, None).unwrap();
        assert_eq!((u.prompt_tokens, u.completion_tokens), (23, 13));
        assert_eq!(u.servers[0].server_id, "a");
        assert_eq!(u.servers[0].prompt_tokens, 22);
        assert_eq!(u.daily.len(), 2);
        // One row per minute and server.
        let rows: i64 = db
            .lock()
            .query_row("SELECT COUNT(*) FROM direct_usage", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 3);
        // Range: the first day only.
        let first = db.direct_usage(Some(day0), Some(day0 + DAY_MS)).unwrap();
        assert_eq!((first.prompt_tokens, first.completion_tokens), (16, 6));
        // Retention fold: totals stay the same, minute rows of old days are gone.
        db.maintain(day0 + 40 * DAY_MS, 30).unwrap();
        let rows: i64 = db
            .lock()
            .query_row("SELECT COUNT(*) FROM direct_usage", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0);
        let u = db.direct_usage(None, None).unwrap();
        assert_eq!((u.prompt_tokens, u.completion_tokens), (23, 13));
        assert_eq!(u.daily.len(), 2);
        let first = db.direct_usage(Some(day0), Some(day0 + DAY_MS)).unwrap();
        assert_eq!((first.prompt_tokens, first.completion_tokens), (16, 6));
        // `clear` removes it too.
        db.clear().unwrap();
        assert!(db.direct_usage(None, None).unwrap().is_empty());
    }

    #[test]
    fn endpoint_normalization_only_rewrites_the_loopback_host() {
        assert_eq!(
            endpoint_key("http://localhost:8080/v1/"),
            "http://127.0.0.1:8080"
        );
        assert_eq!(
            endpoint_key("https://localhost.example.com/v1"),
            "https://localhost.example.com"
        );
        assert_eq!(
            endpoint_key("https://example.com/localhost/v1"),
            "https://example.com/localhost"
        );
    }

    #[test]
    fn late_explicit_usage_is_reconciled_in_raw_and_folded_storage() {
        for (folded, immediate) in [(false, false), (true, false), (false, true), (true, true)] {
            let dir = tempfile::tempdir().unwrap();
            let db = AnalyticsDb::open(&dir.path().join("a.db")).unwrap();
            let at = 100 * DAY_MS;
            let mut tracker = DirectTracker::default();
            tracker.observe("s", &metrics(0, 0), t(0, 0));
            tracker.observe("s", &metrics(26, 110), t(0, 0));
            let anonymous = tracker.observe("s", &metrics(26, 110), t(0, 0)).unwrap();
            db.direct_add(at, "s", "m", anonymous.prompt, anonymous.completion)
                .unwrap();
            if folded {
                db.maintain(at + 40 * DAY_MS, 30).unwrap();
            }
            let correction = if immediate {
                tracker.note_explicit("s", t(26, 110))
            } else {
                assert_eq!(tracker.observe("s", &metrics(26, 110), t(26, 110)), None);
                tracker.take_correction("s")
            };
            assert_eq!(correction, t(26, 110));
            db.direct_add_reconciled(
                at,
                "s",
                "m",
                &DirectClient {
                    client: "creator".into(),
                    requests: 1,
                    prompt_tokens: 62,
                    completion_tokens: 110,
                },
                correction,
            )
            .unwrap();
            let usage = db.direct_usage(None, None).unwrap();
            assert_eq!((usage.prompt_tokens, usage.completion_tokens), (62, 110));
            assert_eq!(
                usage
                    .clients
                    .iter()
                    .find(|c| c.client == "creator")
                    .unwrap()
                    .requests,
                1
            );
            assert_eq!(tracker.take_correction("s"), t(0, 0));
            assert_eq!(tracker.observe("s", &metrics(26, 110), t(26, 110)), None);
            tracker.observe("s", &metrics(36, 115), t(26, 110));
            assert_eq!(
                tracker.observe("s", &metrics(36, 115), t(26, 110)),
                Some(t(10, 5))
            );
        }
    }

    #[test]
    fn failed_attribution_does_not_remove_anonymous_usage() {
        let dir = tempfile::tempdir().unwrap();
        let db = AnalyticsDb::open(&dir.path().join("a.db")).unwrap();
        db.direct_add(100 * DAY_MS, "s", "m", 26, 110).unwrap();
        db.lock()
            .execute_batch(
                "CREATE TRIGGER reject_client BEFORE INSERT ON direct_usage
             WHEN NEW.client='creator' BEGIN SELECT RAISE(ABORT,'write failed'); END;",
            )
            .unwrap();
        assert!(db
            .direct_add_reconciled(
                100 * DAY_MS,
                "s",
                "m",
                &DirectClient {
                    client: "creator".into(),
                    requests: 1,
                    prompt_tokens: 26,
                    completion_tokens: 110,
                },
                t(26, 110)
            )
            .is_err());
        let usage = db.direct_usage(None, None).unwrap();
        assert_eq!((usage.prompt_tokens, usage.completion_tokens), (26, 110));
        assert_eq!(usage.clients.len(), 1);
    }
}
