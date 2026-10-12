//! Pure math of the analytics: percentiles and the phases of one request.

use serde::{Deserialize, Serialize};

/// Percentile `p` (0–100) of **sorted** values by linear interpolation between the closest
/// ranks (Hyndman–Fan type 7, the default of NumPy and spreadsheets): rank
/// `h = (n − 1) · p / 100`, value `x[⌊h⌋] + (h − ⌊h⌋) · (x[⌊h⌋+1] − x[⌊h⌋])`.
/// `None` for no values.
pub fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    let n = sorted.len();
    if n == 0 {
        return None;
    }
    let p = p.clamp(0.0, 100.0);
    let h = (n - 1) as f64 * p / 100.0;
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(n - 1);
    Some(sorted[lo] + (h - lo as f64) * (sorted[hi] - sorted[lo]))
}

/// Sorts `values` and returns (P50, P95).
pub fn p50_p95(values: &mut [f64]) -> (Option<f64>, Option<f64>) {
    values.sort_by(|a, b| a.total_cmp(b));
    (percentile(values, 50.0), percentile(values, 95.0))
}

/// One segment of a request's timeline, in ms from its arrival at the gateway.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Phase {
    /// `load` (JIT load of the model), `queue` (waiting for a free slot), `prompt` (prompt
    /// processing), `generation` (token generation), `other` (proxy, network, sampling
    /// overhead after the model's own timings).
    pub kind: String,
    pub start_ms: f64,
    pub duration_ms: f64,
}

/// Phases of a request from the times the gateway measured (wait, load, total) and the
/// timings llama-server reported (prompt, generation). The load happens inside the wait
/// (a request waits while its model loads), so `queue` is the wait minus the load.
pub fn timeline(
    queue_wait_ms: u64,
    load_ms: Option<u64>,
    prompt_ms: Option<f64>,
    predicted_ms: Option<f64>,
    total_ms: u64,
) -> Vec<Phase> {
    let mut out = Vec::new();
    let mut at = 0.0;
    let mut push = |kind: &str, ms: f64, at: &mut f64| {
        if ms > 0.0 {
            out.push(Phase {
                kind: kind.to_string(),
                start_ms: *at,
                duration_ms: ms,
            });
            *at += ms;
        }
    };
    let wait = queue_wait_ms as f64;
    let load = (load_ms.unwrap_or(0) as f64).min(wait);
    push("load", load, &mut at);
    push("queue", wait - load, &mut at);
    push("prompt", prompt_ms.unwrap_or(0.0).max(0.0), &mut at);
    push("generation", predicted_ms.unwrap_or(0.0).max(0.0), &mut at);
    let rest = total_ms as f64 - at;
    push("other", rest, &mut at);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_matches_type_7_by_hand() {
        // 1..=20: h(50) = 9.5 → (10 + 11) / 2; h(95) = 18.05 → 19 + 0.05 · 1.
        let v: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(percentile(&v, 50.0), Some(10.5));
        assert!((percentile(&v, 95.0).unwrap() - 19.05).abs() < 1e-9);
        assert_eq!(percentile(&v, 0.0), Some(1.0));
        assert_eq!(percentile(&v, 100.0), Some(20.0));
        // One value, two values.
        assert_eq!(percentile(&[7.0], 95.0), Some(7.0));
        assert_eq!(percentile(&[10.0, 20.0], 50.0), Some(15.0));
        assert!((percentile(&[10.0, 20.0], 95.0).unwrap() - 19.5).abs() < 1e-9);
        assert_eq!(percentile(&[], 50.0), None);
        // Unsorted input through p50_p95.
        let mut u = vec![300.0, 100.0, 200.0, 400.0, 500.0];
        let (p50, p95) = p50_p95(&mut u);
        assert_eq!(p50, Some(300.0));
        assert!((p95.unwrap() - 480.0).abs() < 1e-9);
    }

    #[test]
    fn timeline_orders_load_queue_prompt_generation() {
        let t = timeline(2600, Some(2500), Some(40.0), Some(800.0), 3500);
        let kinds: Vec<&str> = t.iter().map(|p| p.kind.as_str()).collect();
        assert_eq!(kinds, ["load", "queue", "prompt", "generation", "other"]);
        assert_eq!(t[1].start_ms, 2500.0);
        assert_eq!(t[1].duration_ms, 100.0);
        assert_eq!(t[2].start_ms, 2600.0);
        assert_eq!(t[3].start_ms, 2640.0);
        assert!((t[4].duration_ms - 60.0).abs() < 1e-9);
        // No wait, no timings (an embedding): everything is `other`.
        let t = timeline(0, None, None, None, 30);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].kind, "other");
    }
}
