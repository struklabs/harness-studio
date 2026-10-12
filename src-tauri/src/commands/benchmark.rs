// --- Benchmark (llama-bench) ---

fn running_bench_servers(registry: &ServerRegistry) -> Vec<harness_core::benchmark::BenchServer> {
    registry
        .active_servers()
        .into_iter()
        .map(|(id, active)| harness_core::benchmark::BenchServer {
            name: active
                .model_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| id.clone()),
            id,
        })
        .collect()
}

/// Servers and free VRAM a benchmark would share the GPU with (Performance > Benchmark asks
/// before it starts).
#[tauri::command]
pub(crate) async fn benchmark_preflight(
    registry: State<'_, Arc<ServerRegistry>>,
) -> Result<harness_core::benchmark::BenchPreflight, String> {
    let servers = running_bench_servers(&registry);
    let usage = tokio::task::spawn_blocking(|| {
        let adapters = harness_core::gpu::list_adapters();
        adapters
            .first()
            .and_then(harness_core::gpu::vram_usage)
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(harness_core::benchmark::preflight(servers, usage))
}

#[tauri::command]
pub(crate) async fn start_benchmark(
    bench_mgr: State<'_, Arc<BenchmarkManager>>,
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    registry: State<'_, Arc<ServerRegistry>>,
    mut request: BenchmarkRequest,
    stop_servers: Option<bool>,
) -> Result<(), String> {
    if stop_servers.unwrap_or(false) {
        for (id, _) in registry.active_servers() {
            if let Some(pm) = registry.get(&id) {
                pm.stop().await?;
            }
        }
    }
    // Whatever still runs now shares the GPU with the measurement: the run records it.
    request.concurrent_servers = running_bench_servers(&registry).len() as u32;
    crate::orchestrator::start_benchmark(&bench_mgr, &runtime_mgr, request)
        .await
        .map(|_run_id| ())
}

#[tauri::command]
pub(crate) fn get_benchmark_status(bench_mgr: State<'_, Arc<BenchmarkManager>>) -> BenchmarkStatus {
    bench_mgr.status()
}

#[tauri::command]
pub(crate) fn cancel_benchmark(bench_mgr: State<'_, Arc<BenchmarkManager>>) {
    bench_mgr.cancel();
}

#[tauri::command]
pub(crate) async fn list_benchmark_runs() -> Result<Vec<BenchmarkRun>, String> {
    blocking(benchmark::load_history).await
}

#[tauri::command]
pub(crate) fn delete_benchmark_run(run_id: String) -> Result<Vec<BenchmarkRun>, String> {
    benchmark::delete_run(&run_id)
}

#[tauri::command]
pub(crate) fn clear_benchmark_runs() -> Result<(), String> {
    benchmark::clear_history()
}

// --- Model & Downloader Commands ---




