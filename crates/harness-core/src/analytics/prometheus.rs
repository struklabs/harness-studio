//! Unified Prometheus exposition (`GET /metrics` on the gateway, `GET /api/harness/v1/metrics`
//! on the Control API).
//!
//! * `harness_gateway_*`: counters and histograms of the requests that passed the gateway
//!   since this process started (updated in [`observe`], before the analytics queue, so
//!   they count every request even if the database is slow), and gauges of the scheduler.
//! * `harness_analytics_*`: recorder counters (written, dropped, errors).
//! * `harness_system_*`: GPU / RAM usage of the latest 1 Hz telemetry sample.
//! * `llamacpp:*{server="<id>"}`: every llama-server's own `/metrics` as last sampled by the
//!   telemetry poller (servers started with metrics on; direct-endpoint traffic shows here).
//!
//! Names are documented in `private hs-internal architecture notes` §13.5.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::Mutex;

use crate::gateway::record::RequestRecord;
use crate::gateway::scheduler::{ModelSlot, SlotState};
use crate::telemetry::{Metrics, TelemetrySnapshot};

/// A metric family: name, help, value of one item.
type Family<T, V> = (&'static str, &'static str, fn(&T) -> V);

/// Histogram upper bounds, seconds.
pub const BUCKETS: [f64; 12] = [
    0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0,
];

#[derive(Debug, Clone, Default)]
struct Histogram {
    counts: [u64; BUCKETS.len()],
    count: u64,
    sum: f64,
}

impl Histogram {
    fn observe(&mut self, secs: f64) {
        for (i, b) in BUCKETS.iter().enumerate() {
            if secs <= *b {
                self.counts[i] += 1;
            }
        }
        self.count += 1;
        self.sum += secs;
    }
}

#[derive(Debug, Clone, Default)]
struct ModelCounters {
    /// (endpoint, status) → requests.
    requests: BTreeMap<(String, u16), u64>,
    errors: BTreeMap<String, u64>,
    cancelled: u64,
    oom: u64,
    prompt_tokens: u64,
    completion_tokens: u64,
    cached_tokens: u64,
    duration: Histogram,
    ttft: Histogram,
    queue: Histogram,
    load: Histogram,
}

fn counters() -> &'static Mutex<BTreeMap<String, ModelCounters>> {
    static C: Mutex<BTreeMap<String, ModelCounters>> = Mutex::new(BTreeMap::new());
    &C
}

/// Counts one finished gateway request.
pub fn observe(r: &RequestRecord) {
    let alias = r.alias.clone().unwrap_or_default();
    let mut all = counters().lock().unwrap_or_else(|e| e.into_inner());
    let c = all.entry(alias).or_default();
    *c.requests
        .entry((r.endpoint.clone(), r.status))
        .or_default() += 1;
    if let Some(e) = &r.error_code {
        *c.errors.entry(e.clone()).or_default() += 1;
    }
    if r.cancelled {
        c.cancelled += 1;
    }
    if r.oom {
        c.oom += 1;
    }
    c.prompt_tokens += r.usage.prompt_tokens.unwrap_or(0);
    c.completion_tokens += r.usage.completion_tokens.unwrap_or(0);
    c.cached_tokens += r.usage.cached_tokens.unwrap_or(0);
    c.duration.observe(r.total_ms as f64 / 1000.0);
    if let Some(t) = r.ttft_ms {
        c.ttft.observe(t as f64 / 1000.0);
    }
    c.queue.observe(r.queue_wait_ms as f64 / 1000.0);
    if let Some(l) = r.load_ms {
        c.load.observe(l as f64 / 1000.0);
    }
}

/// Escapes a label value (`\`, `"`, newline).
pub fn escape(v: &str) -> String {
    v.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn header(out: &mut String, name: &str, kind: &str, help: &str) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} {kind}");
}

fn histogram(out: &mut String, name: &str, help: &str, all: &[(&String, &Histogram)]) {
    header(out, name, "histogram", help);
    for (model, h) in all {
        let m = escape(model);
        for (i, b) in BUCKETS.iter().enumerate() {
            let _ = writeln!(
                out,
                "{name}_bucket{{model=\"{m}\",le=\"{b}\"}} {}",
                h.counts[i]
            );
        }
        let _ = writeln!(
            out,
            "{name}_bucket{{model=\"{m}\",le=\"+Inf\"}} {}",
            h.count
        );
        let _ = writeln!(out, "{name}_sum{{model=\"{m}\"}} {}", h.sum);
        let _ = writeln!(out, "{name}_count{{model=\"{m}\"}} {}", h.count);
    }
}

/// Everything the exposition shows besides the request counters.
#[derive(Debug, Default)]
pub struct Inputs {
    pub gateway_running: bool,
    pub slots: Vec<ModelSlot>,
    pub system: Option<TelemetrySnapshot>,
    /// Latest raw `/metrics` of each llama-server (`llamacpp:` prefix stripped).
    pub servers: Vec<(String, Metrics)>,
    pub recorder: super::RecorderStatus,
}

/// Renders the text exposition format (version 0.0.4).
pub fn render_with(inputs: &Inputs) -> String {
    let mut out = String::new();
    let all = counters().lock().unwrap_or_else(|e| e.into_inner()).clone();

    header(
        &mut out,
        "harness_gateway_up",
        "gauge",
        "1 when the gateway listens.",
    );
    let _ = writeln!(
        out,
        "harness_gateway_up {}",
        u8::from(inputs.gateway_running)
    );

    header(
        &mut out,
        "harness_gateway_requests_total",
        "counter",
        "Gateway requests by model, endpoint and HTTP status.",
    );
    for (model, c) in &all {
        for ((endpoint, status), n) in &c.requests {
            let _ = writeln!(
                out,
                "harness_gateway_requests_total{{model=\"{}\",endpoint=\"{}\",status=\"{}\"}} {}",
                escape(model),
                escape(endpoint),
                status,
                n
            );
        }
    }
    header(
        &mut out,
        "harness_gateway_request_errors_total",
        "counter",
        "Gateway requests that failed, by model and error code.",
    );
    for (model, c) in &all {
        for (code, n) in &c.errors {
            let _ = writeln!(
                out,
                "harness_gateway_request_errors_total{{model=\"{}\",code=\"{}\"}} {}",
                escape(model),
                escape(code),
                n
            );
        }
    }
    let simple: [Family<ModelCounters, u64>; 5] = [
        (
            "harness_gateway_requests_cancelled_total",
            "Requests the client abandoned (upstream cancelled).",
            |c| c.cancelled,
        ),
        (
            "harness_gateway_oom_total",
            "Requests that ran out of memory (no room to load, or a load hit OOM).",
            |c| c.oom,
        ),
        (
            "harness_gateway_prompt_tokens_total",
            "Prompt tokens of gateway requests.",
            |c| c.prompt_tokens,
        ),
        (
            "harness_gateway_completion_tokens_total",
            "Generated tokens of gateway requests.",
            |c| c.completion_tokens,
        ),
        (
            "harness_gateway_cached_tokens_total",
            "Prompt tokens served from the KV cache.",
            |c| c.cached_tokens,
        ),
    ];
    for (name, help, get) in simple {
        header(&mut out, name, "counter", help);
        for (model, c) in &all {
            let _ = writeln!(out, "{name}{{model=\"{}\"}} {}", escape(model), get(c));
        }
    }
    let hist = |f: fn(&ModelCounters) -> &Histogram| -> Vec<(&String, &Histogram)> {
        all.iter().map(|(m, c)| (m, f(c))).collect()
    };
    histogram(
        &mut out,
        "harness_gateway_request_duration_seconds",
        "Time from arrival to the end of the response.",
        &hist(|c| &c.duration),
    );
    histogram(
        &mut out,
        "harness_gateway_ttft_seconds",
        "Time from arrival to the first response byte.",
        &hist(|c| &c.ttft),
    );
    histogram(
        &mut out,
        "harness_gateway_queue_wait_seconds",
        "Time waiting for a slot (including a JIT load).",
        &hist(|c| &c.queue),
    );
    histogram(
        &mut out,
        "harness_gateway_model_load_seconds",
        "JIT loads caused by requests.",
        &hist(|c| &c.load),
    );

    let gauges: [Family<ModelSlot, f64>; 3] = [
        (
            "harness_gateway_model_loaded",
            "1 when the model is loaded and ready on a server.",
            |s| f64::from(u8::from(s.state == SlotState::Ready)),
        ),
        (
            "harness_gateway_in_flight_requests",
            "Requests running on the model.",
            |s| f64::from(s.in_flight),
        ),
        (
            "harness_gateway_queued_requests",
            "Requests waiting for the model.",
            |s| f64::from(s.queued),
        ),
    ];
    for (name, help, get) in gauges {
        header(&mut out, name, "gauge", help);
        for s in &inputs.slots {
            let _ = writeln!(out, "{name}{{model=\"{}\"}} {}", escape(&s.alias), get(s));
        }
    }

    let r = &inputs.recorder;
    for (name, help, v) in [
        (
            "harness_analytics_records_written_total",
            "Request rows written to analytics.db.",
            r.written,
        ),
        (
            "harness_analytics_records_dropped_total",
            "Request rows dropped because the analytics queue was full.",
            r.dropped,
        ),
        (
            "harness_analytics_write_errors_total",
            "Failed analytics.db writes.",
            r.write_errors,
        ),
    ] {
        header(&mut out, name, "counter", help);
        let _ = writeln!(out, "{name} {v}");
    }

    if let Some(s) = &inputs.system {
        for (name, help, v) in [
            (
                "harness_system_vram_used_bytes",
                "Used memory of the primary GPU.",
                s.vram_used_bytes,
            ),
            (
                "harness_system_vram_total_bytes",
                "Memory of the primary GPU.",
                s.vram_total_bytes,
            ),
            (
                "harness_system_ram_used_bytes",
                "Used system RAM.",
                s.system_ram_used_bytes,
            ),
            (
                "harness_system_ram_total_bytes",
                "System RAM.",
                s.system_ram_total_bytes,
            ),
        ] {
            header(&mut out, name, "gauge", help);
            let _ = writeln!(out, "{name} {v}");
        }
    }

    // llama-server metrics, one family per name, a `server` label per instance.
    let mut families: BTreeMap<&str, Vec<(&str, f64)>> = BTreeMap::new();
    for (server, metrics) in &inputs.servers {
        for (name, v) in metrics {
            families
                .entry(name.as_str())
                .or_default()
                .push((server.as_str(), *v));
        }
    }
    for (name, values) in families {
        let valid = name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':');
        if !valid {
            continue;
        }
        let _ = writeln!(out, "# TYPE llamacpp:{name} untyped");
        for (server, v) in values {
            let _ = writeln!(out, "llamacpp:{name}{{server=\"{}\"}} {v}", escape(server));
        }
    }
    out
}

/// Renders with the live inputs of this process (gateway scheduler, telemetry, recorder).
pub async fn render() -> String {
    let scheduler = crate::gateway::scheduler();
    let slots = match &scheduler {
        Some(s) => s.snapshot().await,
        None => Vec::new(),
    };
    let (system, servers) = match super::telemetry() {
        Some(t) => {
            let p = t.lock().await;
            (Some(p.last("")), p.raw_metrics())
        }
        None => (None, Vec::new()),
    };
    render_with(&Inputs {
        gateway_running: crate::gateway::status().running,
        slots,
        system,
        servers,
        recorder: super::recorder_status(),
    })
}

/// Content type of the exposition.
pub const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposition_is_well_formed() {
        let mut m = Metrics::new();
        m.insert("prompt_tokens_total".into(), 42.0);
        m.insert("bad name{".into(), 1.0);
        let text = render_with(&Inputs {
            gateway_running: true,
            servers: vec![("gw-a\"b".into(), m)],
            ..Default::default()
        });
        assert!(text.contains("harness_gateway_up 1"));
        assert!(text.contains("llamacpp:prompt_tokens_total{server=\"gw-a\\\"b\"} 42"));
        assert!(!text.contains("bad name"));
        // Every sample line is `name{labels} value` or `name value`.
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            let value = line.rsplit(' ').next().unwrap();
            assert!(value.parse::<f64>().is_ok(), "{line}");
        }
    }

    #[test]
    fn histogram_buckets_are_cumulative() {
        let mut h = Histogram::default();
        for s in [0.03, 0.2, 0.2, 7.0, 400.0] {
            h.observe(s);
        }
        assert_eq!(h.counts[0], 1); // ≤ 0.05
        assert_eq!(h.counts[2], 3); // ≤ 0.25
        assert_eq!(h.counts[7], 4); // ≤ 10
        assert_eq!(h.counts[11], 4); // ≤ 300; 400 only in +Inf
        assert_eq!(h.count, 5);
    }
}
