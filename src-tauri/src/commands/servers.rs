#[tauri::command]
pub(crate) async fn start_server(
    app: tauri::AppHandle,
    registry: State<'_, Arc<ServerRegistry>>,
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    gate: State<'_, Arc<OperationGate>>,
    pending: State<'_, PendingExposure>,
    config: LaunchConfig,
    server_id: Option<String>,
) -> Result<(), String> {
    let id = ServerRegistry::resolve_id(server_id)?;
    let (host, retry) = (config.host.clone(), config.clone());
    // A config naming its definition runs as definition + transient form changes (the served
    // alias comes from the definition); nothing is saved.
    let result =
        crate::orchestrator::launch_form(&registry, &runtime_mgr, &gate, id.clone(), config).await;
    let start = PendingStart::Config(Box::new(retry));
    note_exposure_block(&app, &pending, &id, &host, start, result)
}

#[tauri::command]
pub(crate) async fn stop_server(
    registry: State<'_, Arc<ServerRegistry>>,
    server_id: Option<String>,
) -> Result<(), String> {
    server(&registry, server_id)?.stop().await
}

/// The models of a router server and whether each is loaded.
#[tauri::command]
pub(crate) async fn list_router_models(
    registry: State<'_, Arc<ServerRegistry>>,
    server_id: Option<String>,
) -> Result<Vec<harness_core::router::RouterModel>, String> {
    let ep = harness_core::router::endpoint(&*server(&registry, server_id)?)?;
    harness_core::router::list(&ep).await
}

/// Loads one model of a router server; returns once the router reports its state.
#[tauri::command]
pub(crate) async fn load_router_model(
    registry: State<'_, Arc<ServerRegistry>>,
    server_id: Option<String>,
    model: String,
) -> Result<harness_core::router::RouterModel, String> {
    let ep = harness_core::router::endpoint(&*server(&registry, server_id)?)?;
    harness_core::router::load(&ep, &model, None).await
}

/// Unloads one model of a router server.
#[tauri::command]
pub(crate) async fn unload_router_model(
    registry: State<'_, Arc<ServerRegistry>>,
    server_id: Option<String>,
    model: String,
) -> Result<harness_core::router::RouterModel, String> {
    let ep = harness_core::router::endpoint(&*server(&registry, server_id)?)?;
    harness_core::router::unload(&ep, &model).await
}

/// Launch config of the server's current (or last) run; the API key is never serialized.
#[tauri::command]
pub(crate) fn get_server_launch_config(
    registry: State<'_, Arc<ServerRegistry>>,
    server_id: Option<String>,
) -> Option<LaunchConfig> {
    let mut config = server(&registry, server_id).ok()?.launch_config()?;
    config.binary_path = PathBuf::new();
    config.mcp_config_path = None;
    Some(config)
}

/// What "prepare the server" would set for a tool (`harness_core::prepare`): the largest
/// context the model supports and the memory holds, with the reason and alternatives.
/// `None` for router-mode servers (no single model) and servers that never started.
#[tauri::command]
pub(crate) async fn plan_server_prepare(
    model_mgr: State<'_, Arc<ModelManager>>,
    registry: State<'_, Arc<ServerRegistry>>,
    gw: State<'_, GatewayState>,
    telemetry_state: State<'_, Arc<tokio::sync::Mutex<TelemetryPoller>>>,
    server_id: Option<String>,
    tool_id: String,
    min_ctx: u32,
) -> Result<Option<crate::prepare::PreparePlan>, String> {
    let id = ServerRegistry::resolve_id(server_id)?;
    let Some(config) = server(&registry, Some(id.clone()))?.launch_config() else {
        return Ok(None);
    };
    if config.router_mode {
        return Ok(None);
    }
    prepare_launch_plan(
        model_mgr.inner(),
        &registry,
        &gw,
        &telemetry_state,
        config,
        vec![id],
        tool_id,
        min_ctx,
    )
    .await
    .map(Some)
}

/// Restarts a server with settings suited to agent tools (OpenCode, Open Design): the given
/// context size and KV cache types, flash attention on and the automatic concurrent-request
/// count for agents (`harness_core::slots`: every concurrent request keeps an agent-sized
/// share of the context). `gpu_layers` is set only for a partial-offload choice. The model's
/// preset is updated so the Dashboard shows the same values.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub(crate) async fn tune_server_for_agent(
    app: tauri::AppHandle,
    pending: State<'_, PendingExposure>,
    registry: State<'_, Arc<ServerRegistry>>,
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    gate: State<'_, Arc<OperationGate>>,
    server_id: Option<String>,
    context_size: u32,
    cache_type_k: String,
    cache_type_v: String,
    gpu_layers: Option<u32>,
) -> Result<(), String> {
    for t in [&cache_type_k, &cache_type_v] {
        if !matches!(t.as_str(), "f16" | "q8_0" | "q4_0") {
            return Err(harness_core::error_key::err("servers.invalid_cache_type")
                .with("value", t)
                .into());
        }
    }
    if !(2048..=1_048_576).contains(&context_size) {
        return Err(harness_core::error_key::err("servers.invalid_context_size")
            .num("value", context_size)
            .into());
    }
    let id = ServerRegistry::resolve_id(server_id)?;
    let pm = server(&registry, Some(id.clone()))?;
    let mut config = pm
        .launch_config()
        .ok_or_else(|| String::from(harness_core::error_key::err("servers.never_started")))?;
    config.context_size = context_size;
    config.flash_attn = true;
    config.cache_type_k = Some(cache_type_k);
    config.cache_type_v = Some(cache_type_v);
    config.parallel_slots = None;
    config.launched_by = harness_core::slots::SlotOrigin::Agents;
    if let Some(n) = gpu_layers {
        config.gpu_layers = n;
    }

    if !config.router_mode {
        let model_id = config.model_path.to_string_lossy().to_string();
        let mut preset = ModelManager::get_preset(&model_id).unwrap_or_default();
        preset.context_size = config.context_size;
        preset.flash_attn = true;
        preset.cache_type_k = config.cache_type_k.clone();
        preset.cache_type_v = config.cache_type_v.clone();
        preset.parallel_slots = None;
        preset.gpu_layers = config.gpu_layers;
        blocking(move || ModelManager::save_preset(&model_id, preset)).await??;
    }
    if !matches!(
        pm.get_status(),
        ServerStatus::Stopped | ServerStatus::Error(_)
    ) {
        pm.stop().await?;
    }
    let (host, retry) = (config.host.clone(), config.clone());
    let result = launch(&registry, &runtime_mgr, &gate, id.clone(), config).await;
    let start = PendingStart::Config(Box::new(retry));
    note_exposure_block(&app, &pending, &id, &host, start, result)
}

#[tauri::command]
pub(crate) fn get_server_status(
    registry: State<'_, Arc<ServerRegistry>>,
    server_id: Option<String>,
) -> ServerStatus {
    server(&registry, server_id)
        .map(|pm| pm.get_status())
        .unwrap_or(ServerStatus::Stopped)
}

#[tauri::command]
pub(crate) fn get_server_logs(
    registry: State<'_, Arc<ServerRegistry>>,
    server_id: Option<String>,
    tail: Option<usize>,
) -> Vec<String> {
    server(&registry, server_id)
        .map(|pm| pm.get_logs(tail))
        .unwrap_or_default()
}

#[tauri::command]
pub(crate) fn list_servers(registry: State<'_, Arc<ServerRegistry>>) -> Vec<ServerSummary> {
    registry.summaries()
}

#[tauri::command]
pub(crate) async fn remove_server(
    registry: State<'_, Arc<ServerRegistry>>,
    telemetry_state: State<'_, Arc<tokio::sync::Mutex<TelemetryPoller>>>,
    server_id: String,
) -> Result<(), String> {
    registry.remove(&server_id).await?;
    telemetry_state.lock().await.forget(&server_id);
    Ok(())
}

#[tauri::command]
pub(crate) fn list_server_profiles() -> Vec<ServerProfile> {
    servers::load_profiles()
}

/// The Dashboard's own server while it is not running: its saved `"default"` profile (which
/// `list_server_profiles` leaves out because it is not user-editable), else the settings
/// defaults. Every view (Dashboard form, Servers list, title bar) reads it from here.
#[tauri::command]
pub(crate) async fn get_default_server() -> Result<DefaultServer, String> {
    blocking(|| {
        let mut server = servers::default_server();
        server.config.binary_path = PathBuf::new();
        server.config.mcp_config_path = None;
        server
    })
    .await
}

#[tauri::command]
pub(crate) fn save_server_profile(profile: ServerProfile) -> Result<Vec<ServerProfile>, String> {
    servers::save_profile(profile)
}

#[tauri::command]
pub(crate) fn delete_server_profile(profile_id: String) -> Result<Vec<ServerProfile>, String> {
    servers::delete_profile(&profile_id)
}

/// Starts a saved profile on its own server instance (instance id = profile id).
#[tauri::command]
pub(crate) async fn start_server_profile(
    app: tauri::AppHandle,
    registry: State<'_, Arc<ServerRegistry>>,
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    gate: State<'_, Arc<OperationGate>>,
    pending: State<'_, PendingExposure>,
    profile_id: String,
) -> Result<(), String> {
    servers::validate_id(&profile_id)?;
    if profile_id == servers::DEFAULT_SERVER_ID {
        return Err(harness_core::error_key::err("servers.profile_not_found")
            .with("id", &profile_id)
            .into());
    }
    let result =
        crate::orchestrator::launch_profile(&registry, &runtime_mgr, &gate, &profile_id).await;
    let host = servers::load_profiles()
        .into_iter()
        .find(|p| p.id == profile_id)
        .map(|p| p.config.host)
        .unwrap_or_default();
    note_exposure_block(
        &app,
        &pending,
        &profile_id,
        &host,
        PendingStart::Profile,
        result,
    )
}

/// Last failure of a server and, after an out-of-memory failure, the recovery options.
#[tauri::command]
pub(crate) async fn get_recovery_plan(
    registry: State<'_, Arc<ServerRegistry>>,
    server_id: Option<String>,
) -> Result<crate::recovery::RecoveryPlan, String> {
    let id = ServerRegistry::resolve_id(server_id)?;
    crate::orchestrator::recovery_plan(&registry, &id).await
}

/// Starts a failed server again with a transient layer (a recovery option's overrides),
/// stopping `unload` first; `save` also writes the values to the model definition.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub(crate) async fn retry_server(
    registry: State<'_, Arc<ServerRegistry>>,
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    gate: State<'_, Arc<OperationGate>>,
    server_id: Option<String>,
    overrides: crate::params::ParamValues,
    unload: Vec<String>,
    save: bool,
) -> Result<(), String> {
    let id = ServerRegistry::resolve_id(server_id)?;
    crate::orchestrator::retry_with_overrides(
        &registry,
        &runtime_mgr,
        &gate,
        &id,
        overrides,
        unload,
        save,
    )
    .await
}

/// LoRA adapters of a running server with their current scales (`GET /lora-adapters`).
#[tauri::command]
pub(crate) async fn list_lora_adapters(
    registry: State<'_, Arc<ServerRegistry>>,
    server_id: Option<String>,
) -> Result<Vec<crate::lora_live::LoraAdapter>, String> {
    let id = ServerRegistry::resolve_id(server_id)?;
    crate::lora_live::list_for(&registry, &id).await
}

/// Sets adapter scales on a running server without a restart; returns the adapters as the
/// server reports them afterwards.
#[tauri::command]
pub(crate) async fn set_lora_scales(
    registry: State<'_, Arc<ServerRegistry>>,
    server_id: Option<String>,
    scales: Vec<crate::lora_live::LoraScale>,
) -> Result<Vec<crate::lora_live::LoraAdapter>, String> {
    let id = ServerRegistry::resolve_id(server_id)?;
    crate::lora_live::set_scales_for(&registry, &id, &scales).await
}
