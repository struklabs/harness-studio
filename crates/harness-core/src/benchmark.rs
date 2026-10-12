//! `llama-bench` integration: runs parameter sweeps in the background as jobs of the
//! llama-bench engine (`engine::llama_bench`, driven by `engine::job::run_job`), streams
//! results as they finish (`-o jsonl`) and keeps a local history of runs.

use crate::engine::job::{run_job, JobCommand, JobEvent, JobOutcome, JobProgress};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

const MAX_HISTORY: usize = 100;
/// A sweep is already running (one at a time).
pub const ERR_ALREADY_RUNNING: &str = "benchmark.already_running";
const LOG_TAIL: usize = 60;
/// Whole manual sweep: 15 minutes allows several-minute low-end CPU runs and model loads,
/// while bounding a stuck GPU driver. Large sweeps can be split into smaller requests.
pub const MANUAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);
const CACHE_TYPES: &[&str] = &[
    "f32", "f16", "bf16", "q8_0", "q4_0", "q4_1", "iq4_nl", "q5_0", "q5_1",
];

pub fn bench_binary_name() -> String {
    crate::engine::llama_bench().release_source().binary_file()
}

/// `llama-bench` ships next to `llama-server` in the official release archives; the
/// llama-bench engine finds it.
pub fn bench_binary_for(server_binary: &Path) -> Result<PathBuf, String> {
    crate::engine::llama_bench().job_binary(server_binary)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BenchmarkRequest {
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub model_path: PathBuf,
    /// Each list is a sweep: llama-bench runs every combination.
    pub n_prompt: Vec<u32>,
    pub n_gen: Vec<u32>,
    pub n_depth: Vec<u32>,
    pub gpu_layers: Vec<i32>,
    /// "on" | "off" | "auto"
    pub flash_attn: Vec<String>,
    pub cache_type_k: Vec<String>,
    pub cache_type_v: Vec<String>,
    pub threads: Option<u32>,
    pub batch_size: Option<u32>,
    pub ubatch_size: Option<u32>,
    pub repetitions: Option<u32>,
    /// Servers that held GPU memory when the run started (set by the front end that knows the
    /// registry, not by the caller): their load skews the numbers, so the saved run keeps it.
    pub concurrent_servers: u32,
}

/// A running server a benchmark would compete with.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BenchServer {
    pub id: String,
    pub name: String,
}

/// What a benchmark would share the GPU with, read before it starts.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BenchPreflight {
    pub servers: Vec<BenchServer>,
    pub vram_free_bytes: Option<u64>,
    pub vram_total_bytes: Option<u64>,
    /// Running servers hold GPU memory (measured: Qwen3-4B loaded gave 10.98 t/s generation,
    /// 148.9 t/s on an idle GPU), so the numbers would mislead.
    pub contended: bool,
}

/// `usage` is the OS `(used, total)` dedicated VRAM of the main adapter.
pub fn preflight(servers: Vec<BenchServer>, usage: Option<(u64, u64)>) -> BenchPreflight {
    BenchPreflight {
        contended: !servers.is_empty(),
        servers,
        vram_free_bytes: usage.map(|(used, total)| total.saturating_sub(used)),
        vram_total_bytes: usage.map(|(_, total)| total),
    }
}

/// One llama-bench result row (a subset of its JSON output).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BenchResult {
    pub model_type: String,
    pub model_size: u64,
    pub model_n_params: u64,
    pub backends: String,
    pub gpu_info: String,
    pub cpu_info: String,
    pub build_commit: String,
    pub build_number: Option<u64>,
    pub n_prompt: u32,
    pub n_gen: u32,
    pub n_depth: u32,
    pub n_gpu_layers: i32,
    pub n_threads: u32,
    pub n_batch: u32,
    pub n_ubatch: u32,
    pub type_k: String,
    pub type_v: String,
    /// -1 auto / 0 off / 1 on (older builds report a bool).
    pub flash_attn: serde_json::Value,
    pub avg_ts: f64,
    pub stddev_ts: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum RunStatus {
    Completed,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BenchmarkRun {
    pub id: String,
    pub started_at: u64,
    pub finished_at: u64,
    pub model_name: String,
    pub request: BenchmarkRequest,
    pub results: Vec<BenchResult>,
    pub status: RunStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BenchmarkStatus {
    /// Id of the current (or latest) run; the saved run gets the same id.
    pub run_id: Option<String>,
    pub running: bool,
    pub current: u32,
    pub total: u32,
    /// Latest progress line, e.g. "generation run 2/5".
    pub phase: String,
    pub results: Vec<BenchResult>,
    pub log_tail: Vec<String>,
    /// Set when the latest run finished (saved to history).
    pub last_run: Option<BenchmarkRun>,
}

#[derive(Default)]
struct Inner {
    run_id: Option<String>,
    running: bool,
    current: u32,
    total: u32,
    phase: String,
    results: Vec<BenchResult>,
    log_tail: VecDeque<String>,
    last_run: Option<BenchmarkRun>,
    cancelled: bool,
    /// The run hit its time limit and was stopped ([`BenchmarkManager::start_limited`]).
    timed_out: bool,
}

#[derive(Clone, Default)]
pub struct BenchmarkManager {
    inner: Arc<Mutex<Inner>>,
    /// Cancels the running job (kills its process tree); `None` while idle.
    cancel: Arc<Mutex<Option<Arc<watch::Sender<bool>>>>>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn join<T: ToString>(values: &[T]) -> Option<String> {
    (!values.is_empty()).then(|| {
        values
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",")
    })
}

/// Builds llama-bench arguments. `fa_on_off` = the build accepts `on|off|auto`
/// (older builds only take `0|1`).
pub fn build_args(req: &BenchmarkRequest, fa_on_off: bool) -> Result<Vec<String>, String> {
    let mut args = vec![
        "-m".to_string(),
        req.model_path.to_string_lossy().to_string(),
        "-o".to_string(),
        "jsonl".to_string(),
        "--progress".to_string(),
    ];
    let mut push = |flag: &str, value: Option<String>| {
        if let Some(v) = value {
            args.push(flag.to_string());
            args.push(v);
        }
    };
    push("-p", join(&req.n_prompt));
    push("-n", join(&req.n_gen));
    push("-d", join(&req.n_depth));
    push("-ngl", join(&req.gpu_layers));
    for (flag, types) in [("-ctk", &req.cache_type_k), ("-ctv", &req.cache_type_v)] {
        if let Some(bad) = types.iter().find(|t| !CACHE_TYPES.contains(&t.as_str())) {
            return Err(crate::error_key::err("benchmark.invalid_cache_type")
                .with("value", bad)
                .into());
        }
        push(flag, join(types));
    }
    let mut fa = Vec::new();
    for v in &req.flash_attn {
        let v = match (v.as_str(), fa_on_off) {
            ("on" | "off" | "auto", true) => v.clone(),
            ("on", false) => "1".to_string(),
            ("off" | "auto", false) => "0".to_string(),
            _ => {
                return Err(crate::error_key::err("benchmark.invalid_flash_attn")
                    .with("value", v)
                    .into())
            }
        };
        if !fa.contains(&v) {
            fa.push(v);
        }
    }
    push("-fa", join(&fa));
    push("-t", req.threads.map(|v| v.to_string()));
    push("-b", req.batch_size.map(|v| v.to_string()));
    push("-ub", req.ubatch_size.map(|v| v.to_string()));
    push("-r", req.repetitions.map(|v| v.clamp(1, 50).to_string()));
    Ok(args)
}

/// Parses "llama-bench: benchmark 3/8: prompt run 1/5" into (3, 8, "prompt run 1/5").
pub fn parse_progress(line: &str) -> Option<(u32, u32, String)> {
    let rest = line.split("benchmark ").nth(1)?;
    let (counts, phase) = rest.split_once(':')?;
    let (cur, total) = counts.trim().split_once('/')?;
    Some((
        cur.parse().ok()?,
        total.parse().ok()?,
        phase.trim().to_string(),
    ))
}

impl BenchmarkManager {
    pub fn status(&self) -> BenchmarkStatus {
        let inner = self.inner.lock().unwrap();
        BenchmarkStatus {
            run_id: inner.run_id.clone(),
            running: inner.running,
            current: inner.current,
            total: inner.total,
            phase: inner.phase.clone(),
            results: inner.results.clone(),
            log_tail: inner.log_tail.iter().cloned().collect(),
            last_run: inner.last_run.clone(),
        }
    }

    fn log(&self, line: String) {
        let mut inner = self.inner.lock().unwrap();
        if inner.log_tail.len() >= LOG_TAIL {
            inner.log_tail.pop_front();
        }
        inner.log_tail.push_back(line);
    }

    /// Starts a sweep in the background; returns the run id (progress via [`Self::status`] and
    /// the event bus, the finished run lands in the history under the same id).
    pub fn start(&self, bench_bin: PathBuf, req: BenchmarkRequest) -> Result<String, String> {
        self.start_limited(bench_bin, req, Some(MANUAL_TIMEOUT))
    }

    /// [`Self::start`] with a wall-clock limit: a run still going after `limit` is stopped and
    /// recorded as an error (`benchmark.timeout`) with the rows it finished. A GPU driver that
    /// stops answering would otherwise keep llama-bench waiting forever.
    ///
    /// The run is a job of the [`crate::engine::llama_bench`] engine ([`run_job`]): its
    /// output lines give progress and result rows, and cancelling (or the limit) kills the
    /// whole process tree.
    pub fn start_limited(
        &self,
        bench_bin: PathBuf,
        req: BenchmarkRequest,
        limit: Option<std::time::Duration>,
    ) -> Result<String, String> {
        if !req.model_path.is_file() {
            return Err(crate::error_key::err("process.model_not_found")
                .with("path", req.model_path.display())
                .into());
        }
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let started_at = now_secs();
        let run_id = format!(
            "{}-{}-{}",
            started_at,
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        );
        {
            let mut inner = self.inner.lock().unwrap();
            if inner.running {
                return Err(ERR_ALREADY_RUNNING.to_string());
            }
            *inner = Inner {
                run_id: Some(run_id.clone()),
                running: true,
                phase: "başlatılıyor".to_string(),
                last_run: inner.last_run.take(),
                ..Default::default()
            };
        }

        let fail = |this: &Self, msg: String| {
            this.inner.lock().unwrap().running = false;
            Err(msg)
        };
        let help = crate::process::ServerCaps::help_text(&bench_bin);
        let args = match build_args(&req, help.contains("on|off")) {
            Ok(a) => a,
            Err(e) => return fail(self, e),
        };
        let cmd = JobCommand {
            binary: bench_bin.clone(),
            args,
            cwd: Some(bench_bin.parent().unwrap_or(Path::new(".")).to_path_buf()),
            outputs: Vec::new(),
        };
        self.log(format!("$ llama-bench {}", cmd.args.join(" ")));

        let (cancel_tx, cancel_rx) = watch::channel(false);
        let cancel_tx = Arc::new(cancel_tx);
        *self.cancel.lock().unwrap() = Some(cancel_tx.clone());

        let this = self.clone();
        let id = run_id.clone();
        let spawned = std::thread::Builder::new()
            .name("harness-benchmark".into())
            .spawn(move || {
                let outcome = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt.block_on(this.run(&cmd, cancel_rx, cancel_tx, async move {
                        match limit {
                            Some(limit) => tokio::time::sleep(limit).await,
                            None => std::future::pending().await,
                        }
                    })),
                    Err(e) => JobOutcome::Failed {
                        reason: "benchmark.runtime_failed".into(),
                        exit_code: None,
                        tail: vec![e.to_string()],
                    },
                };
                this.finish(id, started_at, req, outcome, limit);
            });
        if let Err(e) = spawned {
            *self.cancel.lock().unwrap() = None;
            return fail(
                self,
                crate::error_key::err("process.spawn_failed")
                    .with("program", "llama-bench")
                    .with("error", e)
                    .into(),
            );
        }
        Ok(run_id)
    }

    /// Runs the job: result rows and progress from its output, the limit as a timer that
    /// cancels it.
    async fn run(
        &self,
        cmd: &JobCommand,
        cancel_rx: watch::Receiver<bool>,
        cancel_tx: Arc<watch::Sender<bool>>,
        timeout: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> JobOutcome {
        let engine = crate::engine::llama_bench();
        // Keep the deadline injectable so tests can allow process startup before
        // timing the hang. Public runs still use the wall-clock limit above.
        let this = self.clone();
        let timer = tokio::spawn(async move {
            timeout.await;
            this.inner.lock().unwrap().timed_out = true;
            let _ = cancel_tx.send(true);
        });
        let this = self.clone();
        let outcome = run_job(
            cmd,
            |line| engine.parse_progress(line),
            cancel_rx,
            move |event| match event {
                JobEvent::Progress(p) => this.progress(p),
                JobEvent::Line(line) => {
                    if engine.parse_progress(&line).is_some() {
                        return;
                    }
                    match serde_json::from_str::<BenchResult>(line.trim()) {
                        Ok(result) => this.inner.lock().unwrap().results.push(result),
                        Err(_) if !line.trim().is_empty() => this.log(line),
                        Err(_) => {}
                    }
                }
            },
        )
        .await;
        timer.abort();
        outcome
    }

    fn progress(&self, p: JobProgress) {
        let (current, total) = (p.done as u32, p.total.unwrap_or(0) as u32);
        let phase = p.phase.unwrap_or_default();
        let mut inner = self.inner.lock().unwrap();
        inner.current = current;
        inner.total = total;
        inner.phase = phase.clone();
        let run_id = inner.run_id.clone().unwrap_or_default();
        drop(inner);
        crate::events::publish(crate::events::CoreEvent::BenchmarkProgress {
            run_id,
            current,
            total,
            phase,
        });
    }

    /// Records the finished run (history, status, event).
    fn finish(
        &self,
        id: String,
        started_at: u64,
        req: BenchmarkRequest,
        outcome: JobOutcome,
        limit: Option<std::time::Duration>,
    ) {
        *self.cancel.lock().unwrap() = None;
        let mut inner = self.inner.lock().unwrap();
        let timed_out = inner.timed_out;
        let (status, error) = if inner.cancelled {
            (RunStatus::Cancelled, None)
        } else if timed_out {
            (
                RunStatus::Error,
                Some(
                    crate::error_key::err("benchmark.timeout")
                        .with("seconds", limit.map(|l| l.as_secs()).unwrap_or(0))
                        .into(),
                ),
            )
        } else {
            match outcome {
                JobOutcome::Completed { .. } => (RunStatus::Completed, None),
                JobOutcome::Cancelled => (RunStatus::Cancelled, None),
                JobOutcome::Failed {
                    reason,
                    exit_code,
                    tail,
                } => {
                    let error: String = match reason.as_str() {
                        "jobs.exit_failed" => {
                            let tail: Vec<String> =
                                inner.log_tail.iter().rev().take(8).rev().cloned().collect();
                            crate::error_key::err("benchmark.exit_failed")
                                .with(
                                    "status",
                                    exit_code
                                        .map(|c| format!("exit code: {c}"))
                                        .unwrap_or_default(),
                                )
                                .with("log", tail.join("\n"))
                                .into()
                        }
                        "jobs.spawn_failed" => crate::error_key::err("process.spawn_failed")
                            .with("program", "llama-bench")
                            .with("error", tail.join(" "))
                            .into(),
                        "jobs.wait_failed" => crate::error_key::err("process.wait_failed")
                            .with("error", tail.join(" "))
                            .into(),
                        _ => crate::error_key::err("benchmark.process_lost").into(),
                    };
                    (RunStatus::Error, Some(error))
                }
            }
        };
        let run = BenchmarkRun {
            id,
            started_at,
            finished_at: now_secs(),
            model_name: req
                .model_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            request: req,
            results: inner.results.clone(),
            status,
            error,
        };
        inner.running = false;
        inner.phase.clear();
        inner.last_run = Some(run.clone());
        drop(inner);
        // Keep cancelled runs only if they produced something.
        if run.status != RunStatus::Cancelled || !run.results.is_empty() {
            let _ = append_history(run.clone());
        }
        crate::events::publish(crate::events::CoreEvent::BenchmarkFinished { run: Box::new(run) });
    }

    pub fn cancel(&self) {
        let mut inner = self.inner.lock().unwrap();
        if !inner.running {
            return;
        }
        inner.cancelled = true;
        drop(inner);
        if let Some(tx) = self.cancel.lock().unwrap().as_ref() {
            let _ = tx.send(true);
        }
    }
}

/// `benchmarks.json`: v1 was a bare array; v2 is `{schema_version, runs: [...]}`.
pub const HISTORY_SCHEMA: crate::store::Schema = crate::store::Schema {
    file: "benchmarks.json",
    current: 2,
    envelope: crate::store::Envelope::Wrapped("runs"),
    migrations: &[history_v1_to_v2],
};

fn history_v1_to_v2(doc: serde_json::Value) -> Result<serde_json::Value, String> {
    Ok(crate::store::wrap_legacy(doc, "runs"))
}

fn history_file() -> PathBuf {
    crate::paths::config_dir().join("benchmarks.json")
}

fn history_store() -> crate::store::JsonStore {
    crate::store::JsonStore::new(history_file())
}

pub fn load_history() -> Vec<BenchmarkRun> {
    history_store()
        .load_versioned::<Vec<BenchmarkRun>>(&HISTORY_SCHEMA)
        .ok()
        .flatten()
        .map(|v| v.data)
        .unwrap_or_default()
}

/// Newest first, capped at `MAX_HISTORY`.
fn append_history(run: BenchmarkRun) -> Result<(), String> {
    history_store().update_versioned::<Vec<BenchmarkRun>, _>(&HISTORY_SCHEMA, |runs| {
        runs.insert(0, run);
        runs.truncate(MAX_HISTORY);
        Ok(())
    })
}

/// Prompt processing speed (tokens/s) the newest benchmark of this model measured, for
/// estimating how long a big prompt takes. Rows of the run are the pure-prompt (`pp`) ones;
/// `gpu_layers` is the `-ngl` the model is loaded with (picks the CPU or the GPU rows), `None`
/// = the direction whose generation was faster. The longest prompt size wins (speed drops with
/// length, so the short sizes would flatter the estimate).
pub fn prompt_speed(
    runs: &[BenchmarkRun],
    model_path: &Path,
    gpu_layers: Option<u32>,
) -> Option<f64> {
    let key = crate::library::state_key(model_path);
    let run = runs
        .iter()
        .filter(|r| crate::library::state_key(&r.request.model_path) == key)
        .filter(|r| {
            r.results
                .iter()
                .any(|x| x.n_prompt > 0 && x.n_gen == 0 && x.avg_ts > 0.0)
        })
        .max_by_key(|r| r.finished_at)?;
    let pp = |on_gpu: Option<bool>| {
        run.results
            .iter()
            .filter(|x| x.n_prompt > 0 && x.n_gen == 0 && x.avg_ts > 0.0)
            .filter(|x| on_gpu.is_none_or(|g| (x.n_gpu_layers != 0) == g))
            .max_by(|a, b| {
                a.n_prompt
                    .cmp(&b.n_prompt)
                    .then(a.avg_ts.total_cmp(&b.avg_ts))
            })
    };
    let faster_on_gpu = || {
        let tg = |g: bool| {
            run.results
                .iter()
                .filter(|x| x.n_gen > 0 && x.n_prompt == 0 && (x.n_gpu_layers != 0) == g)
                .map(|x| x.avg_ts)
                .fold(f64::NAN, f64::max)
        };
        // NaN (no rows for a side) loses against any number.
        tg(true) > tg(false) || tg(false).is_nan() && !tg(true).is_nan()
    };
    let side = gpu_layers.map(|n| n > 0).unwrap_or_else(faster_on_gpu);
    pp(Some(side)).or_else(|| pp(None)).map(|x| x.avg_ts)
}

pub fn delete_run(id: &str) -> Result<Vec<BenchmarkRun>, String> {
    history_store().update_versioned::<Vec<BenchmarkRun>, _>(&HISTORY_SCHEMA, |runs| {
        runs.retain(|r| r.id != id);
        Ok(runs.clone())
    })
}

pub fn clear_history() -> Result<(), String> {
    history_store().save_versioned(&HISTORY_SCHEMA, &Vec::<BenchmarkRun>::new())
}

#[cfg(test)]
mod tests {
    #[test]
    fn preflight_flags_running_servers_and_reports_free_vram() {
        use super::*;
        const GIB: u64 = 1 << 30;
        let idle = preflight(Vec::new(), Some((4 * GIB, 16 * GIB)));
        assert!(!idle.contended);
        assert_eq!(idle.vram_free_bytes, Some(12 * GIB));
        let busy = preflight(
            vec![BenchServer {
                id: "gw-1".into(),
                name: "Qwen3-4B".into(),
            }],
            Some((14 * GIB, 16 * GIB)),
        );
        assert!(busy.contended);
        assert_eq!(busy.vram_free_bytes, Some(2 * GIB));
        assert!(preflight(Vec::new(), None).vram_total_bytes.is_none());
    }

    use super::*;

    /// Fake llama-bench: one complete JSONL row followed by a hang. Give startup its
    /// own bound, then expire the deadline after the first row is observed.
    #[test]
    fn limited_fake_process_keeps_rows_and_records_timeout_in_history() {
        use std::time::{Duration, Instant};
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("fake.rs");
        let binary = dir
            .path()
            .join(if cfg!(windows) { "fake.exe" } else { "fake" });
        let late = dir.path().join("late.txt");
        std::fs::write(
            &source,
            r#"
            use std::io::Write;
            fn main() {
                if std::env::args().any(|a| a == "--help") { println!("on|off"); return; }
                std::fs::write("started.txt", "1").unwrap();
                println!("{{\"n_prompt\":512,\"avg_ts\":42.0}}");
                std::io::stdout().flush().unwrap();
                while !std::path::Path::new("deadline-armed.txt").exists() {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
                std::fs::write("late.txt", "still running").unwrap();
                std::thread::sleep(std::time::Duration::from_secs(60));
            }
        "#,
        )
        .unwrap();
        let compiled = std::process::Command::new("rustc")
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let model = dir.path().join("fake.gguf");
        std::fs::write(&model, []).unwrap();
        let mgr = BenchmarkManager::default();
        let req = BenchmarkRequest {
            model_path: model,
            ..Default::default()
        };
        let cmd = JobCommand {
            binary: binary.clone(),
            args: build_args(&req, true).unwrap(),
            cwd: Some(dir.path().to_path_buf()),
            outputs: vec![],
        };
        let id = format!("fake-timeout-{}", dir.path().display());
        let started_at = now_secs();
        *mgr.inner.lock().unwrap() = Inner {
            run_id: Some(id.clone()),
            running: true,
            ..Default::default()
        };
        let (tx, rx) = watch::channel(false);
        let ready = mgr.clone();
        let first_row = Arc::new(Mutex::new(None::<Instant>));
        let mark = first_row.clone();
        let armed = dir.path().join("deadline-armed.txt");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let start = Instant::now();
        let outcome = runtime.block_on(async {
            tokio::time::timeout(
                Duration::from_secs(60),
                mgr.run(&cmd, rx, Arc::new(tx), async move {
                    while ready.status().results.is_empty() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    *mark.lock().unwrap() = Some(Instant::now());
                    std::fs::write(armed, "1").unwrap();
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }),
            )
            .await
            .expect("fake did not produce a row or timeout did not stop the job")
        });
        assert_eq!(outcome, JobOutcome::Cancelled);
        mgr.finish(
            id.clone(),
            started_at,
            req.clone(),
            outcome,
            Some(Duration::from_millis(100)),
        );
        let status = mgr.status();
        let run = status.last_run.clone().unwrap();
        assert_eq!(run.status, RunStatus::Error);
        assert!(run.error.as_deref().unwrap().contains("benchmark.timeout"));
        assert_eq!(
            run.results.len(),
            1,
            "after {:?} (fake started: {}): {:?} log {:?}",
            start.elapsed(),
            dir.path().join("started.txt").exists(),
            run,
            status.log_tail
        );
        assert_eq!(run.results[0].avg_ts, 42.0);
        let saved = load_history().into_iter().find(|r| r.id == id).unwrap();
        assert_eq!(saved.results.len(), 1);
        assert_eq!(saved.status, RunStatus::Error);
        assert_eq!(
            bench_processes(&binary),
            0,
            "timed-out process was left running"
        );
        // Time this from the row, not spawn: delayed startup must not shorten the check.
        std::thread::sleep(
            Duration::from_millis(2200)
                .saturating_sub(first_row.lock().unwrap().unwrap().elapsed()),
        );
        assert!(!late.exists(), "timed-out process was left running");

        // Public start_limited still expires without waiting for any output.
        let empty = BenchmarkManager::default();
        empty
            .start_limited(binary, req, Some(Duration::ZERO))
            .unwrap();
        let start = Instant::now();
        while empty.status().running {
            assert!(
                start.elapsed() < Duration::from_secs(60),
                "zero limit did not stop the job"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let run = empty.status().last_run.unwrap();
        assert_eq!(run.status, RunStatus::Error);
        assert!(run.error.unwrap().contains("benchmark.timeout"));
        assert_eq!(MANUAL_TIMEOUT, Duration::from_secs(900));
    }

    #[test]
    fn test_build_args_sweep() {
        let req = BenchmarkRequest {
            model_path: "m.gguf".into(),
            n_prompt: vec![512],
            n_gen: vec![128],
            gpu_layers: vec![0, 99],
            flash_attn: vec!["on".into(), "off".into()],
            cache_type_k: vec!["q8_0".into()],
            repetitions: Some(3),
            ..Default::default()
        };
        let args = build_args(&req, true).unwrap().join(" ");
        assert!(args.contains("-o jsonl --progress"));
        assert!(args.contains("-ngl 0,99"));
        assert!(args.contains("-fa on,off"));
        assert!(args.contains("-ctk q8_0"));
        assert!(args.contains("-r 3"));
        assert!(!args.contains("-ctv"));

        let legacy = build_args(&req, false).unwrap().join(" ");
        assert!(legacy.contains("-fa 1,0"));

        let bad = BenchmarkRequest {
            cache_type_v: vec!["q8_0; rm".into()],
            ..req
        };
        assert!(build_args(&bad, true).is_err());
    }

    #[test]
    fn test_parse_progress_and_result() {
        assert_eq!(
            parse_progress("llama-bench: benchmark 3/8: prompt run 1/5"),
            Some((3, 8, "prompt run 1/5".to_string()))
        );
        assert_eq!(parse_progress("load_backend: loaded CPU backend"), None);

        let line = r#"{"model_type":"llama ?B Q4_0","n_prompt":64,"n_gen":0,"n_gpu_layers":99,"flash_attn":-1,"avg_ts":10756.6,"stddev_ts":1522.8,"type_k":"f16","samples_ts":[1.0]}"#;
        let r: BenchResult = serde_json::from_str(line).unwrap();
        assert_eq!(r.n_prompt, 64);
        assert_eq!(r.n_gpu_layers, 99);
        assert_eq!(r.flash_attn, serde_json::json!(-1));
    }

    #[test]
    fn test_prompt_speed_picks_the_matching_side() {
        let row = |ngl: i32, pp: u32, tg: u32, ts: f64| BenchResult {
            n_gpu_layers: ngl,
            n_prompt: pp,
            n_gen: tg,
            avg_ts: ts,
            ..Default::default()
        };
        let run = |model: &str, finished_at: u64, results: Vec<BenchResult>| BenchmarkRun {
            id: finished_at.to_string(),
            started_at: finished_at,
            finished_at,
            model_name: "m".into(),
            request: BenchmarkRequest {
                model_path: model.into(),
                ..Default::default()
            },
            results,
            status: RunStatus::Completed,
            error: None,
        };
        let runs = vec![
            run(
                "/m/a.gguf",
                20,
                vec![
                    row(0, 128, 0, 40.0),
                    row(0, 512, 0, 34.0),
                    row(0, 0, 32, 9.0),
                    row(99, 128, 0, 80.0),
                    row(99, 0, 32, 5.0),
                ],
            ),
            // Older run of the same model and another model: never used.
            run("/m/a.gguf", 10, vec![row(0, 128, 0, 1.0)]),
            run("/m/b.gguf", 30, vec![row(0, 128, 0, 999.0)]),
        ];
        let a = Path::new("/m/a.gguf");
        // Loaded on the CPU: the CPU row of the longest prompt; on the GPU: the GPU row.
        assert_eq!(prompt_speed(&runs, a, Some(0)), Some(34.0));
        assert_eq!(prompt_speed(&runs, a, Some(99)), Some(80.0));
        // Unknown: the side whose generation was faster (CPU here).
        assert_eq!(prompt_speed(&runs, a, None), Some(34.0));
        // Only one side measured: its rows serve either way.
        let cpu_only = vec![run("/m/c.gguf", 5, vec![row(0, 128, 0, 22.0)])];
        assert_eq!(
            prompt_speed(&cpu_only, Path::new("/m/c.gguf"), Some(99)),
            Some(22.0)
        );
        assert_eq!(prompt_speed(&runs, Path::new("/m/none.gguf"), None), None);
        let no_pp = vec![run("/m/d.gguf", 5, vec![row(0, 0, 32, 9.0)])];
        assert_eq!(prompt_speed(&no_pp, Path::new("/m/d.gguf"), None), None);
    }

    /// Real llama-bench sweep: HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<.gguf> cargo test e2e -- --ignored
    #[test]
    #[ignore]
    fn e2e_benchmark_sweep_and_cancel() {
        use std::time::{Duration, Instant};
        let (Ok(bin), Ok(model)) = (
            std::env::var("HS_LLAMA_SERVER"),
            std::env::var("HS_TEST_MODEL"),
        ) else {
            eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
            return;
        };
        let bench = bench_binary_for(Path::new(&bin)).unwrap();
        let mgr = BenchmarkManager::default();
        let req = BenchmarkRequest {
            model_path: model.into(),
            n_prompt: vec![64],
            n_gen: vec![16],
            gpu_layers: vec![0, 99],
            flash_attn: vec!["auto".into()],
            repetitions: Some(1),
            ..Default::default()
        };
        mgr.start(bench.clone(), req.clone()).unwrap();
        assert!(
            mgr.start(bench.clone(), req.clone()).is_err(),
            "one at a time"
        );
        let t = Instant::now();
        while mgr.status().running {
            assert!(t.elapsed() < Duration::from_secs(120));
            std::thread::sleep(Duration::from_millis(100));
        }
        let status = mgr.status();
        let run = status.last_run.expect("run recorded");
        assert_eq!(run.status, RunStatus::Completed, "{:?}", run.error);
        assert_eq!(run.results.len(), 4, "pp+tg for each -ngl value");
        assert!(run.results.iter().all(|r| r.avg_ts > 0.0));
        assert_eq!(status.total, 4);
        assert!(load_history().iter().any(|r| r.id == run.id));

        // Cancel a long run.
        let long = BenchmarkRequest {
            n_prompt: vec![512],
            n_gen: vec![512],
            repetitions: Some(20),
            ..req
        };
        mgr.start(bench.clone(), long).unwrap();
        std::thread::sleep(Duration::from_millis(1500));
        assert!(bench_processes(&bench) >= 1, "the check must see the run");
        mgr.cancel();
        let t = Instant::now();
        while mgr.status().running {
            assert!(
                t.elapsed() < Duration::from_secs(10),
                "cancel did not stop the run"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(mgr.status().last_run.unwrap().status, RunStatus::Cancelled);
        assert_eq!(
            bench_processes(&bench),
            0,
            "cancel left llama-bench running"
        );

        // A limit stops the run like a cancel and records `benchmark.timeout`.
        let long = BenchmarkRequest {
            n_prompt: vec![512],
            n_gen: vec![512],
            repetitions: Some(20),
            model_path: std::env::var("HS_TEST_MODEL").unwrap().into(),
            ..Default::default()
        };
        mgr.start_limited(bench.clone(), long, Some(Duration::from_secs(2)))
            .unwrap();
        let t = Instant::now();
        while mgr.status().running {
            assert!(t.elapsed() < Duration::from_secs(15), "limit did not stop");
            std::thread::sleep(Duration::from_millis(100));
        }
        let run = mgr.status().last_run.unwrap();
        assert_eq!(run.status, RunStatus::Error);
        assert!(run.error.unwrap().contains("benchmark.timeout"));
        assert_eq!(
            bench_processes(&bench),
            0,
            "timeout left llama-bench running"
        );
    }

    /// Running processes of this exact binary.
    fn bench_processes(bench: &Path) -> usize {
        let mut sys = sysinfo::System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        sys.processes()
            .values()
            .filter(|p| p.exe().is_some_and(|e| e == bench))
            .count()
    }
}
