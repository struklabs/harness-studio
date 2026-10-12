// Definition preparation uses the same memory budget as the running-server panel.

/// Read-only recommendation for a newly selected Dashboard model, using the core prepare plan.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub(crate) async fn plan_launch_context(
    model_mgr: State<'_, Arc<ModelManager>>,
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    registry: State<'_, Arc<ServerRegistry>>,
    gw: State<'_, GatewayState>,
    telemetry_state: State<'_, Arc<tokio::sync::Mutex<TelemetryPoller>>>,
    config: LaunchConfig,
    server_id: String,
) -> Result<crate::prepare::PreparePlan, String> {
    let mgr = runtime_mgr.inner().clone();
    let id = server_id.clone();
    let config = blocking(move || {
        let mut resolved = Domain::global().resolve_form(&LaunchDefaults::load(), &id, &config)?;
        let settings = AppSettings::load();
        resolved.binary_path = crate::runtimes::resolve_binary_in(
            &crate::runtimes::RuntimeRegistry::global(), &mgr, &settings, resolved.runtime.as_deref(),
        )?;
        Ok::<_, String>(resolved)
    }).await??;
    prepare_launch_plan(model_mgr.inner(), &registry, &gw, &telemetry_state, config,
        vec![server_id], "playground".into(), 0).await
}
#[allow(clippy::too_many_arguments)]
async fn prepare_launch_plan(
    model_mgr: &Arc<ModelManager>,
    registry: &ServerRegistry,
    gw: &GatewayState,
    telemetry_state: &Arc<tokio::sync::Mutex<TelemetryPoller>>,
    config: LaunchConfig,
    stopping: Vec<String>,
    tool_id: String,
    min_ctx: u32,
) -> Result<crate::prepare::PreparePlan, String> {
    let (slots, headroom) = match gw.server.scheduler().await {
        Some(s) => (s.snapshot().await, s.settings().memory_headroom_mib),
        // Gateway off: no idle models can be evicted; only free memory and the measured restart footprint.
        None => (Vec::new(), AppSettings::load().gateway.memory_headroom_mib),
    };
    let target = config.definition_id.clone().unwrap_or_default();
    let mut snapshots = Vec::new();
    for (server_id, pm) in registry.all() {
        if pm.get_status() == ServerStatus::Running
            && (stopping.contains(&server_id)
                || slots.iter().any(|s| {
                    crate::prepare::evictable_model(s, &target)
                        && s.server_id.as_deref() == Some(&server_id)
                }))
        {
            let snapshot = TelemetryPoller::fresh_snapshot(telemetry_state, &server_id).await;
            snapshots.push((server_id, snapshot));
        }
    }
    let mgr = model_mgr.clone();
    blocking(move || {
        let mut base = crate::vram::input_from_launch(&config);
        // Split models: the size of all shards.
        if let Some(m) = mgr
            .scan_models()
            .into_iter()
            .find(|m| m.path == config.model_path)
        {
            base.model_size_bytes = m.size_bytes;
        }
        let need = crate::prepare::ToolNeed::for_tool(&tool_id, min_ctx);
        let report = crate::devices::report(&config.binary_path, std::time::Duration::ZERO);
        let selected =
            crate::prepare::main_device(&report, &crate::devices::os_adapters(), config.main_gpu);
        let available = selected.and_then(|selected| {
            let measured: Vec<_> = snapshots
                .iter()
                .map(|(server, snapshot)| crate::prepare::ServerMemory {
                    server_id: server.clone(),
                    adapter: selected.1,
                    bytes: crate::prepare::server_vram(snapshot, selected.1, server),
                })
                .collect();
            crate::prepare::available_memory(
                &report,
                selected,
                headroom.saturating_mul(1024 * 1024),
                &target,
                &slots,
                &measured,
                &stopping,
            )
        });
        crate::prepare::plan_for_input_with_available_memory(
            base,
            &detect_hardware(),
            need,
            available,
        )
    })
    .await
}

fn definition_stopping(
    definition_id: &str,
    servers: impl IntoIterator<Item = (String, ServerStatus, Option<LaunchConfig>)>,
) -> Vec<String> {
    servers
        .into_iter()
        .filter_map(|(id, status, config)| {
            (matches!(
                status,
                ServerStatus::Running | ServerStatus::Starting | ServerStatus::Stopping
            ) && config.as_ref().and_then(|c| c.definition_id.as_deref()) == Some(definition_id))
            .then_some(id)
        })
        .collect()
}

fn stopping_definition(registry: &ServerRegistry, definition_id: &str) -> Vec<String> {
    definition_stopping(
        definition_id,
        registry
            .all()
            .into_iter()
            .map(|(id, pm)| (id, pm.get_status(), pm.launch_config())),
    )
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub(crate) async fn plan_definition_prepare(
    model_mgr: State<'_, Arc<ModelManager>>,
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    registry: State<'_, Arc<ServerRegistry>>,
    gw: State<'_, GatewayState>,
    telemetry_state: State<'_, Arc<tokio::sync::Mutex<TelemetryPoller>>>,
    definition_id: String,
    tool_id: String,
    min_ctx: u32,
) -> Result<crate::prepare::PreparePlan, String> {
    let mgr = runtime_mgr.inner().clone();
    let target = definition_id.clone();
    let config = blocking(move || {
        let settings = AppSettings::load();
        let mut config =
            Domain::global().definition_form(&LaunchDefaults::from_settings(&settings), &target)?;
        config.binary_path = crate::runtimes::resolve_binary_in(
            &crate::runtimes::RuntimeRegistry::global(),
            &mgr,
            &settings,
            config.runtime.as_deref(),
        )?;
        Ok::<_, String>(config)
    })
    .await??;
    let stopping = stopping_definition(&registry, &definition_id);
    prepare_launch_plan(
        model_mgr.inner(),
        &registry,
        &gw,
        &telemetry_state,
        config,
        stopping,
        tool_id,
        min_ctx,
    )
    .await
}

fn prepared_load(
    previous: &params::ParamValues,
    option: &crate::prepare::PrepareOption,
) -> params::ParamValues {
    use params::ParamValue::{Bool, Int, Text};
    let mut load = previous.clone();
    load.insert("context_size".into(), Int(i64::from(option.context_size)));
    load.insert("flash_attn".into(), Bool(true));
    load.insert("cache_type_k".into(), Text(option.cache_type_k.clone()));
    load.insert("cache_type_v".into(), Text(option.cache_type_v.clone()));
    load.remove("parallel_slots");
    // Full offload must clear a previous partial layer count too.
    load.insert(
        "gpu_layers".into(),
        Int(i64::from(option.gpu_layers.unwrap_or(999))),
    );
    load
}

fn restore_prepare_load(error: &crate::gateway::scheduler::SchedError) -> bool {
    use crate::gateway::scheduler::SchedError;
    let (code, message) = match error {
        SchedError::LoadFailed { code, message, .. } => (code.as_str(), message.clone()),
        _ => (error.code(), error.message()),
    };
    error.is_oom()
        || code.contains("insufficient_memory")
        || code.contains("out_of_memory")
        || code == "oom"
        || crate::failure::mentions_out_of_memory(&message)
}

#[derive(serde::Serialize)]
pub(crate) struct DefinitionPrepareResult {
    error: Option<String>,
    restored: bool,
    restore_error: Option<String>,
}

#[tauri::command]
pub(crate) async fn prepare_definition_for_agent(
    registry: State<'_, Arc<ServerRegistry>>,
    gw: State<'_, GatewayState>,
    definition_id: String,
    option: crate::prepare::PrepareOption,
) -> Result<DefinitionPrepareResult, String> {
    let scheduler = gw
        .server
        .scheduler()
        .await
        .ok_or("intg.launch_no_endpoint")?;
    if !(2048..=1_048_576).contains(&option.context_size) {
        return Err(harness_core::error_key::err("servers.invalid_context_size")
            .num("value", option.context_size)
            .into());
    }
    for value in [&option.cache_type_k, &option.cache_type_v] {
        if !matches!(value.as_str(), "f16" | "q8_0" | "q4_0") {
            return Err(harness_core::error_key::err("servers.invalid_cache_type")
                .with("value", value)
                .into());
        }
    }
    let target = definition_id.clone();
    let (definition, previous) = blocking(move || {
        let domain = Domain::global();
        let mut definition = domain.definition(&target)?;
        let previous = definition.load.clone();
        definition.load = prepared_load(&previous, &option);
        Ok::<_, String>((domain.save_definition(definition)?, previous))
    })
    .await??;
    let result = async {
        for id in stopping_definition(&registry, &definition_id) {
            server(&registry, Some(id))
                .map_err(crate::gateway::scheduler::SchedError::Internal)?
                .stop()
                .await
                .map_err(crate::gateway::scheduler::SchedError::Internal)?;
        }
        scheduler.acquire(&definition).await
    }
    .await;
    match result {
        Ok(lease) => {
            drop(lease);
            Ok(DefinitionPrepareResult {
                error: None,
                restored: false,
                restore_error: None,
            })
        }
        Err(error) => {
            let restore_error = if restore_prepare_load(&error) {
                blocking(move || {
                    let domain = Domain::global();
                    let mut definition = domain.definition(&definition_id)?;
                    definition.load = previous;
                    domain.save_definition(definition).map(|_| ())
                })
                .await
                .and_then(|r| r)
                .err()
            } else {
                None
            };
            Ok(DefinitionPrepareResult {
                restored: restore_prepare_load(&error) && restore_error.is_none(),
                error: Some(error.message()),
                restore_error,
            })
        }
    }
}

#[cfg(test)]
mod definition_prepare_tests {
    use super::*;
    use crate::gateway::scheduler::SchedError;
    use crate::prepare::{OptionKind, PrepareOption};
    fn option(layers: Option<u32>) -> PrepareOption {
        PrepareOption {
            context_size: 32768,
            kv_level: harness_core::kv_policy::KvLevel::Balanced,
            cache_type_k: "q8_0".into(),
            cache_type_v: "q4_0".into(),
            gpu_layers: layers,
            vram_bytes: 0,
            ram_bytes: 0,
            percent: 0,
            kind: OptionKind::Chosen,
        }
    }
    #[test]
    fn selected_load_preserves_other_values_and_uses_auto_slots() {
        let previous = serde_json::from_value(
            serde_json::json!({"gpu_layers": 12, "parallel_slots": 4, "seed": 42}),
        )
        .unwrap();
        let load = prepared_load(&previous, &option(None));
        assert_eq!(
            serde_json::to_value(load).unwrap(),
            serde_json::json!({
                "context_size": 32768, "flash_attn": true, "cache_type_k": "q8_0", "cache_type_v": "q4_0", "gpu_layers": 999, "seed": 42
            })
        );
        let partial = prepared_load(&previous, &option(Some(8)));
        assert_eq!(partial["gpu_layers"], params::ParamValue::Int(8));
    }
    #[test]
    fn restores_only_memory_failures() {
        for (code, message, expected) in [
            ("insufficient_memory", "no room", true),
            ("servers.out_of_memory", "failed", true),
            ("oom", "failed", true),
            ("load_failed", "out of memory", true),
            ("load_failed", "invalid model", false),
        ] {
            assert_eq!(
                restore_prepare_load(&SchedError::LoadFailed {
                    alias: "m".into(),
                    code: code.into(),
                    message: message.into()
                }),
                expected
            );
        }
        assert!(!restore_prepare_load(&SchedError::Internal(
            "disk error".into()
        )));
        assert!(restore_prepare_load(&SchedError::InsufficientMemory {
            alias: "m".into(),
            need_bytes: Some(10595 * 1024 * 1024),
            available_bytes: Some(3295 * 1024 * 1024),
            blocking: Vec::new(),
            after_full_eviction: true,
            options: Vec::new(),
            tried: Box::default(),
        }));
    }
    #[test]
    fn stopping_includes_every_active_instance_of_only_this_definition() {
        let cfg = |d: &str| {
            Some(LaunchConfig {
                definition_id: Some(d.into()),
                ..LaunchConfig::default()
            })
        };
        let ids = definition_stopping(
            "a",
            vec![
                ("one".into(), ServerStatus::Running, cfg("a")),
                ("two".into(), ServerStatus::Running, cfg("a")),
                ("starting".into(), ServerStatus::Starting, cfg("a")),
                ("other".into(), ServerStatus::Running, cfg("b")),
                ("stopped".into(), ServerStatus::Stopped, cfg("a")),
                ("unknown".into(), ServerStatus::Running, None),
                ("stopping".into(), ServerStatus::Stopping, cfg("a")),
                (
                    "error".into(),
                    ServerStatus::Error("failed".into()),
                    cfg("a"),
                ),
            ],
        );
        assert_eq!(ids, ["one", "two", "starting", "stopping"]);
    }
}
