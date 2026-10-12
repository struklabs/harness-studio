/// Capabilities of the server binary a server uses (or would use), read by its runtime's
/// engine. Probes the binary once (`--version`, `--help`, `--list-devices`); later calls
/// are cached.
#[tauri::command]
pub(crate) async fn get_runtime_capabilities(
    registry: State<'_, Arc<ServerRegistry>>,
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    server_id: Option<String>,
    runtime_id: Option<String>,
) -> Result<Arc<RuntimeCapabilities>, String> {
    let engine = harness_core::orchestrator::server_engine_for(
        &registry,
        server_id.clone(),
        runtime_id.clone(),
    )?;
    let binary = server_binary_for(&registry, &runtime_mgr, server_id, runtime_id)?;
    blocking(move || engine.detect_capabilities(&binary)).await
}

/// Parameter catalog of a server's runtime family; without a server or runtime, llama.cpp's.
#[tauri::command]
pub(crate) fn get_param_catalog(
    registry: State<'_, Arc<ServerRegistry>>,
    server_id: Option<String>,
    runtime_id: Option<String>,
) -> Result<ParamCatalog, String> {
    if server_id.is_none() && runtime_id.is_none() {
        return Ok(params::catalog().clone());
    }
    let engine = harness_core::orchestrator::server_engine_for(&registry, server_id, runtime_id)?;
    Ok(engine.param_catalog().clone())
}

/// Per-feature capability level for the server's runtime and (optionally) a model.
#[tauri::command]
pub(crate) async fn get_feature_status(
    registry: State<'_, Arc<ServerRegistry>>,
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    server_id: Option<String>,
    model_path: Option<String>,
    has_mmproj: Option<bool>,
    runtime_id: Option<String>,
) -> Result<Vec<FeatureStatus>, String> {
    let engine = harness_core::orchestrator::server_engine_for(
        &registry,
        server_id.clone(),
        runtime_id.clone(),
    )?;
    let binary = server_binary_for(&registry, &runtime_mgr, server_id, runtime_id)?;
    blocking(move || {
        let caps = engine.detect_capabilities(&binary);
        let model_path = model_path.filter(|p| !p.trim().is_empty());
        let meta = model_path
            .as_deref()
            .map(Path::new)
            .filter(|p| p.is_file())
            .and_then(|p| read_gguf_metadata(p).ok());
        capabilities::features::feature_status(
            &caps,
            capabilities::features::FeatureContext {
                model: meta.as_ref(),
                model_path: model_path.as_deref(),
                has_mmproj: has_mmproj.unwrap_or(false),
            },
        )
    })
    .await
}

/// What the running server actually uses (`None` until it has been checked after start).
#[tauri::command]
pub(crate) fn get_effective_launch_values(
    registry: State<'_, Arc<ServerRegistry>>,
    server_id: Option<String>,
) -> Option<EffectiveLaunchValues> {
    server(&registry, server_id).ok()?.effective_values()
}

/// Every flag of the runtime's `--help` (for the "all flags" explorer).
#[tauri::command]
pub(crate) async fn list_runtime_flags(
    registry: State<'_, Arc<ServerRegistry>>,
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    server_id: Option<String>,
    runtime_id: Option<String>,
) -> Result<Vec<RuntimeFlag>, String> {
    let engine = harness_core::orchestrator::server_engine_for(
        &registry,
        server_id.clone(),
        runtime_id.clone(),
    )?;
    let binary = server_binary_for(&registry, &runtime_mgr, server_id, runtime_id)?;
    blocking(move || capabilities::runtime_flags(engine, &engine.detect_capabilities(&binary))).await
}

/// Hub task picker: how far the installed runtime runs each Hugging Face task. The baseline is
/// what Harness can do with llama-server; the runtime's capability registry lowers it where the
/// binary lacks the flag a task needs (`--embeddings`, `--rerank`, `--mmproj`) and marks tasks a
/// real run verified. `runtime_id` picks another registered runtime (default: the active one).
#[tauri::command]
pub(crate) async fn get_task_support(
    registry: State<'_, Arc<ServerRegistry>>,
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    runtime_id: Option<String>,
) -> Result<Vec<harness_core::hub_tasks::TaskSupport>, String> {
    // No usable runtime yet (fresh install): the baseline table, unchecked.
    let binary = server_binary_for(&registry, &runtime_mgr, None, runtime_id).ok();
    blocking(move || {
        let caps = binary.map(|b| capabilities::registry().get_or_probe(&b));
        let features = caps
            .as_deref()
            .map(|c| {
                capabilities::features::feature_status(
                    c,
                    capabilities::features::FeatureContext::default(),
                )
            })
            .unwrap_or_default();
        let installed = harness_core::model_registry::installed_families();
        harness_core::hub_tasks::task_support(caps.as_deref(), &features, &installed)
    })
    .await
}
