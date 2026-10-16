//! Automatic first-load measurement on integrated GPUs.
//!
//! On a GPU that shares system memory the faster way to run a model is not decided by the
//! hardware: a full offload beats the CPU for one model and loses for the next (measured on an
//! Intel HD 6000: gemma-3-1b faster on the GPU, six other models slower). The first time such a
//! model loads, a short `llama-bench` (pp128 + tg32, `-ngl 0` and a full offload, time limited)
//! runs in the background with the build that loaded it and lands in the benchmark history;
//! [`crate::vram::budget_for_model`] then recommends the measured direction.

use crate::benchmark::{BenchmarkManager, BenchmarkRequest};
use crate::vram::MemoryBudget;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Prompt tokens of the measurement (one `pp` row per `-ngl`).
const N_PROMPT: u32 = 128;
/// Generated tokens (one `tg` row per `-ngl`): the rows the recommendation compares.
const N_GEN: u32 = 32;
/// Wall-clock limit of the whole measurement: a stuck driver must not keep llama-bench alive.
pub const TIME_LIMIT: Duration = Duration::from_secs(90);
/// Free memory needed on top of twice the model (the server holds one copy, llama-bench loads
/// another) so the measurement cannot push the machine into swap.
const MEMORY_HEADROOM: u64 = 512 * 1024 * 1024;

/// One measurement at a time, however many servers start.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// The request of the measurement: both `-ngl` values in one sweep, one repetition.
pub fn request_for(model_path: &Path) -> BenchmarkRequest {
    BenchmarkRequest {
        model_path: model_path.to_path_buf(),
        n_prompt: vec![N_PROMPT],
        n_gen: vec![N_GEN],
        gpu_layers: vec![0, 99],
        flash_attn: vec!["auto".into()],
        repetitions: Some(1),
        ..Default::default()
    }
}

/// Whether the history already holds this measurement of the model, finished or not: a run
/// that timed out is not repeated on every load (the user can run one by hand).
pub fn already_measured(model_path: &Path, runs: &[crate::benchmark::BenchmarkRun]) -> bool {
    let want = request_for(model_path);
    let key = crate::library::state_key(model_path);
    runs.iter().any(|r| {
        r.request.n_prompt == want.n_prompt
            && r.request.n_gen == want.n_gen
            && r.request.gpu_layers == want.gpu_layers
            && crate::library::state_key(&r.request.model_path) == key
    })
}

/// Whether to measure now (`None` = no): enabled, an integrated GPU shares the memory, this
/// model was never measured and a second copy of it fits in the free memory.
pub fn plan(
    enabled: bool,
    budget: &MemoryBudget,
    model_path: &Path,
    model_bytes: u64,
    available_ram_bytes: u64,
    runs: &[crate::benchmark::BenchmarkRun],
) -> Option<BenchmarkRequest> {
    if !enabled || !budget.shared_igpu || already_measured(model_path, runs) {
        return None;
    }
    if available_ram_bytes
        < model_bytes
            .saturating_mul(2)
            .saturating_add(MEMORY_HEADROOM)
    {
        return None;
    }
    Some(request_for(model_path))
}

/// Starts the measurement for a server that just came up, when [`plan`] says so. Returns
/// whether one runs. Blocking (hardware probe, history file); the run itself is in the
/// background and ends on its own at the latest after [`TIME_LIMIT`].
pub fn maybe_start(
    config: &crate::process::LaunchConfig,
    mode: crate::serve_mode::ServeMode,
) -> bool {
    if config.router_mode
        || mode != crate::serve_mode::ServeMode::Chat
        || !config.model_path.is_file()
        || !crate::config::AppSettings::load().server.auto_benchmark
    {
        return false;
    }
    let hw = crate::hardware::detect_hardware();
    let model_bytes = std::fs::metadata(&config.model_path)
        .map(|m| m.len())
        .unwrap_or(0);
    let Some(request) = plan(
        true,
        &crate::vram::budget(&hw),
        &config.model_path,
        model_bytes,
        hw.available_ram_bytes,
        &crate::benchmark::load_history(),
    ) else {
        return false;
    };
    let Ok(bench_bin) = crate::benchmark::bench_binary_for(&config.binary_path) else {
        return false;
    };
    if RUNNING.swap(true, Ordering::SeqCst) {
        return false;
    }
    let manager = BenchmarkManager::default();
    if manager
        .start_limited(bench_bin, request, Some(TIME_LIMIT))
        .is_err()
    {
        RUNNING.store(false, Ordering::SeqCst);
        return false;
    }
    // Free the slot when the run is over (its own threads outlive this call).
    let watcher = manager.clone();
    if crate::resource_io::spawn_thread("harness-auto-bench", move || {
        while watcher.status().running {
            std::thread::sleep(Duration::from_millis(500));
        }
        RUNNING.store(false, Ordering::SeqCst);
    })
    .is_err()
    {
        manager.cancel();
        RUNNING.store(false, Ordering::SeqCst);
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::{BenchResult, BenchmarkRun, RunStatus};

    const GIB: u64 = 1024 * 1024 * 1024;

    fn igpu() -> MemoryBudget {
        MemoryBudget {
            vram_bytes: 2 * GIB,
            ram_bytes: 4 * GIB,
            unified: true,
            shared_igpu: true,
            igpu_offload_faster: Some(false),
            cuda: false,
            rocm: false,
        }
    }

    fn run_of(model: &str, status: RunStatus) -> BenchmarkRun {
        BenchmarkRun {
            id: "r".into(),
            started_at: 1,
            finished_at: 2,
            model_name: "m".into(),
            request: request_for(Path::new(model)),
            results: vec![BenchResult::default()],
            status,
            error: None,
        }
    }

    #[test]
    fn measures_once_on_an_integrated_gpu() {
        let model = Path::new("/models/a.gguf");
        let go = |enabled, b: &MemoryBudget, ram, runs: &[BenchmarkRun]| {
            plan(enabled, b, model, GIB / 2, ram, runs)
        };
        let req = go(true, &igpu(), 3 * GIB, &[]).expect("first load measures");
        assert_eq!(req.gpu_layers, vec![0, 99]);
        assert_eq!(
            (req.n_prompt.clone(), req.n_gen.clone()),
            (vec![128], vec![32])
        );

        assert!(go(false, &igpu(), 3 * GIB, &[]).is_none(), "opted out");
        let discrete = MemoryBudget {
            shared_igpu: false,
            ..igpu()
        };
        assert!(
            go(true, &discrete, 3 * GIB, &[]).is_none(),
            "direction is known"
        );
        assert!(
            go(true, &igpu(), GIB, &[]).is_none(),
            "no room for a second copy"
        );
        // Done (or timed out) before: not again; another model still is measured.
        let done = [run_of("/models/a.gguf", RunStatus::Completed)];
        assert!(go(true, &igpu(), 3 * GIB, &done).is_none());
        let timed_out = [run_of("/models/a.gguf", RunStatus::Error)];
        assert!(go(true, &igpu(), 3 * GIB, &timed_out).is_none());
        let other = [run_of("/models/b.gguf", RunStatus::Completed)];
        assert!(go(true, &igpu(), 3 * GIB, &other).is_some());
        // A hand-made sweep of other sizes does not count as this measurement.
        let mut manual = run_of("/models/a.gguf", RunStatus::Completed);
        manual.request.n_prompt = vec![512];
        assert!(go(true, &igpu(), 3 * GIB, &[manual]).is_some());
    }

    /// Real llama-bench with a real model, as the first load starts it (the hardware gate is
    /// not under test here): the run lands in the history and gives a measured direction, and a
    /// tiny limit stops it as an error. Run with:
    /// HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<.gguf> cargo test -p harness-core auto_bench::tests::e2e -- --ignored --nocapture
    #[test]
    #[ignore]
    fn e2e_auto_measurement_feeds_the_recommendation() {
        let (Ok(bin), Ok(model)) = (
            std::env::var("HS_LLAMA_SERVER"),
            std::env::var("HS_TEST_MODEL"),
        ) else {
            eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
            return;
        };
        let bench = crate::benchmark::bench_binary_for(Path::new(&bin)).unwrap();
        let wait = |m: &BenchmarkManager| {
            let t = std::time::Instant::now();
            while m.status().running {
                assert!(
                    t.elapsed() < Duration::from_secs(180),
                    "limit did not stop it"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
            m.status().last_run.expect("run recorded")
        };

        let m = BenchmarkManager::default();
        m.start_limited(
            bench.clone(),
            request_for(Path::new(&model)),
            Some(TIME_LIMIT),
        )
        .unwrap();
        let run = wait(&m);
        eprintln!(
            "status {:?}, rows {:?}",
            run.status,
            run.results
                .iter()
                .map(|r| (r.n_gpu_layers, r.n_prompt, r.n_gen, r.avg_ts))
                .collect::<Vec<_>>()
        );
        assert_eq!(run.status, RunStatus::Completed, "{:?}", run.error);
        assert_eq!(run.results.len(), 4, "pp128 + tg32 for -ngl 0 and 99");
        assert!(already_measured(
            Path::new(&model),
            &crate::benchmark::load_history()
        ));
        let faster = crate::vram::igpu_offload_faster_measured(&model);
        eprintln!("full offload faster than -ngl 0: {faster:?}");
        assert!(faster.is_some(), "both directions measured");

        let m = BenchmarkManager::default();
        m.start_limited(
            bench,
            request_for(Path::new(&model)),
            Some(Duration::from_millis(300)),
        )
        .unwrap();
        let run = wait(&m);
        assert_eq!(run.status, RunStatus::Error);
        assert!(run.error.unwrap().contains("benchmark.timeout"));
    }
}
