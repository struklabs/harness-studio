//! Measured performance per model and device, kept locally in `config/model-perf.json`.
//!
//! The recommendations in Model Hub, Library and Performance combine estimates (hardware fit,
//! file size) with what was really measured on this machine:
//!
//! - load time: a server of the model became ready, or the gateway loaded it just in time;
//! - benchmark tokens per second: prompt processing and generation of a finished `llama-bench`
//!   run (older runs in the history are folded in when listing);
//! - image and audio speed: seconds one successful gateway request took;
//! - tool-calling reliability: the result of the small probe suite in [`crate::tool_probe`].
//!
//! Nothing leaves the machine. A record is keyed by model name ([`model_key`]) and device
//! label ([`current_device`]), so the same model on another GPU keeps its own numbers.

use crate::benchmark::{BenchResult, BenchmarkRun, RunStatus};
use crate::tool_probe::ToolProbeResult;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// `config/model-perf.json`: `{schema_version, records: [...]}`.
pub const SCHEMA: crate::store::Schema = crate::store::Schema {
    file: "model-perf.json",
    current: 1,
    envelope: crate::store::Envelope::Wrapped("records"),
    migrations: &[],
};

/// Records kept; the least recently updated ones go first.
const MAX_RECORDS: usize = 500;

/// Weight of a new sample against the stored value (exponential moving average).
const NEW_SAMPLE_WEIGHT: f64 = 0.4;

/// Measurements of one model on one device. Every figure is optional: a record only holds what
/// was measured so far.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelPerfRecord {
    /// Normalized model name ([`model_key`]).
    pub model_key: String,
    /// File or model name as last seen, for display.
    pub model_name: String,
    /// GPU (or CPU) the numbers belong to ([`current_device`]).
    pub device: String,
    /// Unix seconds of the last update.
    pub updated_at: u64,
    /// Time from start to ready, ms.
    #[serde(default)]
    pub load_ms: Option<u64>,
    /// llama-bench prompt processing, tokens per second.
    #[serde(default)]
    pub bench_pp_tps: Option<f64>,
    /// llama-bench generation, tokens per second.
    #[serde(default)]
    pub bench_tg_tps: Option<f64>,
    /// Seconds one image request took.
    #[serde(default)]
    pub image_secs: Option<f64>,
    /// Seconds one speech or transcription request took.
    #[serde(default)]
    pub audio_secs: Option<f64>,
    /// Latest tool-call probe.
    #[serde(default)]
    pub tool_probe: Option<ToolProbeResult>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Comparable model name: the name the gateway serves it under (`process::model_alias`),
/// lower case, no directory and no `.gguf`/`.bin`/`.safetensors`/`.onnx` extension. The
/// frontend normalizes the same way.
pub fn normalize_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    let lower = base.to_lowercase();
    for ext in [".gguf", ".bin", ".safetensors", ".onnx"] {
        if let Some(stem) = lower.strip_suffix(ext) {
            return stem.to_string();
        }
    }
    lower
}

/// [`normalize_name`] of the name a model file is served under.
pub fn model_key(model_path: &Path) -> String {
    let name = crate::process::model_alias(model_path).unwrap_or_else(|| {
        model_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    normalize_name(&name)
}

/// The device the numbers on this machine belong to: the primary GPU, else the CPU.
pub fn current_device() -> String {
    let hw = crate::hardware::detect_hardware();
    let gpu = hw
        .primary_gpu_index
        .and_then(|i| hw.gpus.iter().find(|g| g.index == i))
        .or_else(|| hw.gpus.first());
    match gpu {
        Some(g) if !g.name.trim().is_empty() => g.name.trim().to_string(),
        _ => hw.cpu_name.trim().to_string(),
    }
}

fn store() -> crate::store::JsonStore {
    crate::store::JsonStore::new(crate::paths::config_dir().join(SCHEMA.file))
}

/// Stored records, newest first.
pub fn load() -> Vec<ModelPerfRecord> {
    let mut records = store()
        .load_versioned::<Vec<ModelPerfRecord>>(&SCHEMA)
        .ok()
        .flatten()
        .map(|v| v.data)
        .unwrap_or_default();
    records.sort_by_key(|r| std::cmp::Reverse(r.updated_at));
    records
}

/// Stored records with the figures of the benchmark history folded in for the current device.
pub fn list_with_history(runs: &[BenchmarkRun]) -> Vec<ModelPerfRecord> {
    merge_history(load(), runs, &current_device())
}

/// Folds finished benchmark runs into `records` where a record has no benchmark figure yet.
/// Runs are newest first; the newest run of a model wins.
pub fn merge_history(
    mut records: Vec<ModelPerfRecord>,
    runs: &[BenchmarkRun],
    device: &str,
) -> Vec<ModelPerfRecord> {
    for run in runs.iter().filter(|r| r.status == RunStatus::Completed) {
        let (pp, tg) = bench_speeds(&run.results);
        if pp.is_none() && tg.is_none() {
            continue;
        }
        let key = model_key(&run.request.model_path);
        let at = run.finished_at;
        match records
            .iter_mut()
            .find(|r| r.model_key == key && r.device == device)
        {
            Some(r) => {
                if r.bench_pp_tps.is_none() && r.bench_tg_tps.is_none() {
                    r.bench_pp_tps = pp;
                    r.bench_tg_tps = tg;
                }
            }
            None => records.push(ModelPerfRecord {
                model_key: key,
                model_name: run.model_name.clone(),
                device: device.to_string(),
                updated_at: at,
                bench_pp_tps: pp,
                bench_tg_tps: tg,
                ..Default::default()
            }),
        }
    }
    records.sort_by_key(|r| std::cmp::Reverse(r.updated_at));
    records
}

/// Best prompt-processing and generation speed of a run's rows (the faster side when the run
/// compared CPU and GPU: the speed the model reaches on this machine).
pub fn bench_speeds(results: &[BenchResult]) -> (Option<f64>, Option<f64>) {
    let best = |pred: &dyn Fn(&BenchResult) -> bool| {
        results
            .iter()
            .filter(|r| r.avg_ts > 0.0 && pred(r))
            .map(|r| r.avg_ts)
            .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))))
    };
    (
        best(&|r| r.n_prompt > 0 && r.n_gen == 0),
        best(&|r| r.n_gen > 0 && r.n_prompt == 0),
    )
}

fn blend(old: Option<f64>, new: f64) -> f64 {
    match old {
        Some(o) if o > 0.0 => o * (1.0 - NEW_SAMPLE_WEIGHT) + new * NEW_SAMPLE_WEIGHT,
        _ => new,
    }
}

/// Updates (or creates) the record of `key` on `device`. Best effort: a failing disk never
/// breaks the measurement it came from.
pub fn update(
    key: &str,
    name: &str,
    device: &str,
    change: impl FnOnce(&mut ModelPerfRecord),
) -> Result<(), String> {
    if key.is_empty() {
        return Ok(());
    }
    store().update_versioned::<Vec<ModelPerfRecord>, _>(&SCHEMA, |records| {
        let idx = match records
            .iter()
            .position(|r| r.model_key == key && r.device == device)
        {
            Some(i) => i,
            None => {
                records.push(ModelPerfRecord {
                    model_key: key.to_string(),
                    device: device.to_string(),
                    ..Default::default()
                });
                records.len() - 1
            }
        };
        let r = &mut records[idx];
        if !name.is_empty() {
            r.model_name = name.to_string();
        }
        change(r);
        r.updated_at = now_secs();
        if records.len() > MAX_RECORDS {
            records.sort_by_key(|r| std::cmp::Reverse(r.updated_at));
            records.truncate(MAX_RECORDS);
        }
        Ok(())
    })
}

/// A server of this model became ready after `ms`.
pub fn record_load(model_path: &Path, ms: u64) {
    let name = model_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let _ = update(&model_key(model_path), &name, &current_device(), |r| {
        r.load_ms = Some(blend(r.load_ms.map(|v| v as f64), ms as f64).round() as u64);
    });
}

/// A `llama-bench` run finished.
pub fn record_bench(run: &BenchmarkRun) {
    if run.status != RunStatus::Completed {
        return;
    }
    let (pp, tg) = bench_speeds(&run.results);
    if pp.is_none() && tg.is_none() {
        return;
    }
    let _ = update(
        &model_key(&run.request.model_path),
        &run.model_name,
        &current_device(),
        |r| {
            // A benchmark is a deliberate measurement: it replaces the figure.
            if pp.is_some() {
                r.bench_pp_tps = pp;
            }
            if tg.is_some() {
                r.bench_tg_tps = tg;
            }
        },
    );
}

/// The tool-call probe finished.
pub fn record_tool_probe(model_path: &Path, result: ToolProbeResult) {
    let name = model_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let _ = update(&model_key(model_path), &name, &current_device(), |r| {
        r.tool_probe = Some(result);
    });
}

/// What a finished gateway request says about speed: image and audio requests that succeeded,
/// and just-in-time loads. Called for every published request; most return at once.
pub fn observe_request(rec: &crate::gateway::record::RequestRecord) {
    let Some(name) = rec.alias.as_deref().or(rec.model.as_deref()) else {
        return;
    };
    if !(200..300).contains(&rec.status) || rec.cancelled {
        return;
    }
    let secs = rec.total_ms as f64 / 1000.0;
    let image = rec.endpoint.starts_with("/v1/images/");
    let audio = rec.endpoint.starts_with("/v1/audio/");
    if !image && !audio && rec.load_ms.is_none() {
        return;
    }
    let key = normalize_name(name);
    let _ = update(&key, name, &current_device(), |r| {
        if image && secs > 0.0 {
            r.image_secs = Some(blend(r.image_secs, secs));
        }
        if audio && secs > 0.0 {
            r.audio_secs = Some(blend(r.audio_secs, secs));
        }
        if let Some(ms) = rec.load_ms.filter(|m| *m > 0) {
            r.load_ms = Some(blend(r.load_ms.map(|v| v as f64), ms as f64).round() as u64);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::BenchmarkRequest;
    use std::path::PathBuf;

    fn row(n_prompt: u32, n_gen: u32, ts: f64) -> BenchResult {
        BenchResult {
            n_prompt,
            n_gen,
            avg_ts: ts,
            ..Default::default()
        }
    }

    fn run(path: &str, status: RunStatus, results: Vec<BenchResult>, at: u64) -> BenchmarkRun {
        BenchmarkRun {
            id: path.into(),
            started_at: at,
            finished_at: at,
            model_name: path.into(),
            request: BenchmarkRequest {
                model_path: PathBuf::from(path),
                ..Default::default()
            },
            results,
            status,
            error: None,
        }
    }

    #[test]
    fn names_are_normalized_like_the_gateway_serves_them() {
        assert_eq!(normalize_name("Qwen3-4B-Q4_K_M.gguf"), "qwen3-4b-q4_k_m");
        assert_eq!(normalize_name("C:\\m\\Foo.GGUF"), "foo");
        assert_eq!(normalize_name("/a/b/ggml-base.bin"), "ggml-base");
        assert_eq!(
            model_key(Path::new("/m/Big-00001-of-00003.gguf")),
            "big",
            "split sets share one key"
        );
    }

    #[test]
    fn bench_speeds_take_the_faster_side() {
        let rows = [
            row(512, 0, 100.0),
            row(512, 0, 900.0),
            row(0, 128, 8.0),
            row(0, 128, 40.0),
            row(0, 0, 5.0),
        ];
        assert_eq!(bench_speeds(&rows), (Some(900.0), Some(40.0)));
        assert_eq!(bench_speeds(&[]), (None, None));
    }

    #[test]
    fn history_fills_only_missing_figures_and_skips_failed_runs() {
        let device = "GPU A";
        let runs = [
            run(
                "/m/a.gguf",
                RunStatus::Completed,
                vec![row(512, 0, 300.0), row(0, 64, 30.0)],
                20,
            ),
            run("/m/b.gguf", RunStatus::Error, vec![row(0, 64, 99.0)], 30),
        ];
        let existing = ModelPerfRecord {
            model_key: "c".into(),
            device: device.into(),
            updated_at: 5,
            load_ms: Some(1200),
            ..Default::default()
        };
        let merged = merge_history(vec![existing.clone()], &runs, device);
        assert_eq!(merged.len(), 2, "the failed run adds nothing");
        assert_eq!(merged[0].model_key, "a");
        assert_eq!(merged[0].bench_tg_tps, Some(30.0));
        assert_eq!(merged[1], existing);

        // A stored figure is kept over an older run's.
        let mut stored = merged[0].clone();
        stored.bench_tg_tps = Some(55.0);
        let again = merge_history(vec![stored], &runs, device);
        assert_eq!(again[0].bench_tg_tps, Some(55.0));
    }

    #[test]
    fn samples_blend_into_the_stored_value() {
        assert_eq!(blend(None, 10.0), 10.0);
        let b = blend(Some(10.0), 20.0);
        assert!((b - 14.0).abs() < 1e-9, "{b}");
    }
}
