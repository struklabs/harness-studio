// Settings > Gateway and the managed endpoint (harness-gateway): status, settings, models.

/// The gateway listener and the services it routes to (same instances as the GUI's).
pub(crate) struct GatewayState {
    pub(crate) server: Arc<harness_gateway::GatewayServer>,
    pub(crate) ctx: harness_gateway::GatewayContext,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct GatewayOverview {
    settings: crate::gateway::GatewaySettings,
    status: crate::gateway::GatewayStatus,
    models: Vec<crate::gateway::scheduler::ModelSlot>,
    /// Token of Harness's own clients (Playground) while the gateway runs; the webview is
    /// ours, so it gets the value.
    client_token: Option<String>,
    /// Only the owner of the data directory runs the gateway.
    owner: bool,
    /// Named tokens with the `inference` (or `admin`) scope.
    inference_tokens: usize,
    /// Stored Responses in state.db (`None` when it cannot be read).
    responses_stats: Option<crate::state::StoredResponsesStats>,
    /// Queue history of the last week: which definitions keep making requests wait for a slot
    /// and how many slots to try at the next load.
    queue_hints: Vec<crate::analytics::QueueHint>,
}

async fn gateway_overview(
    instance: &InstanceState,
    gw: &GatewayState,
) -> Result<GatewayOverview, String> {
    let models = match gw.server.scheduler().await {
        Some(s) => s.snapshot().await,
        None => Vec::new(),
    };
    let (inference_tokens, responses_stats, queue_hints) = blocking(|| {
        (
            crate::gateway::inference_token_count(&crate::control_api::TokenStore::global()),
            crate::state::shared()
                .ok()
                .and_then(|db| db.responses_stats().ok()),
            crate::analytics::queue_hints().unwrap_or_default(),
        )
    })
    .await?;
    Ok(GatewayOverview {
        settings: AppSettings::load().gateway,
        status: gw.server.status().await,
        models,
        client_token: gw.server.client_token().await,
        owner: instance.lock.is_some(),
        inference_tokens,
        responses_stats,
        queue_hints,
    })
}

#[tauri::command]
pub(crate) async fn get_gateway_status(
    instance: State<'_, InstanceState>,
    gw: State<'_, GatewayState>,
) -> Result<GatewayOverview, String> {
    gateway_overview(&instance, &gw).await
}

#[tauri::command]
pub(crate) async fn model_picker_memory(
    gw: State<'_, GatewayState>,
) -> Result<Vec<crate::gateway::scheduler::ModelMemoryHint>, String> {
    match gw.server.scheduler().await {
        Some(s) => s.model_memory_hints().await.map_err(|e| e.message()),
        None => Ok(Vec::new()),
    }
}

/// Validates and saves the gateway settings, then starts, updates or stops it. Errors are
/// i18n keys (`gateway.*`, `network.invalid_host`).
#[tauri::command]
pub(crate) async fn set_gateway_settings(
    instance: State<'_, InstanceState>,
    gw: State<'_, GatewayState>,
    settings: crate::gateway::GatewaySettings,
) -> Result<GatewayOverview, String> {
    let validated = blocking(move || {
        let tokens = crate::control_api::TokenStore::global();
        crate::gateway::validate_settings(
            &settings,
            crate::gateway::inference_token_count(&tokens),
            Some(AppSettings::load().control_api.port),
        )
    })
    .await??;
    if validated.enabled && instance.lock.is_none() {
        return Err(crate::gateway::ERR_NOT_OWNER.to_string());
    }
    {
        let validated = validated.clone();
        blocking(move || {
            let mut all = AppSettings::load();
            all.gateway = validated;
            all.save()
        })
        .await??;
    }
    if validated.enabled {
        gw.server.start(gw.ctx.clone(), &validated).await?;
    } else {
        gw.server.stop().await;
    }
    gateway_overview(&instance, &gw).await
}

/// Deletes every stored gateway Response (all tokens) from state.db.
#[tauri::command]
pub(crate) async fn clear_gateway_responses(
    instance: State<'_, InstanceState>,
    gw: State<'_, GatewayState>,
) -> Result<GatewayOverview, String> {
    blocking(|| -> Result<usize, String> { Ok(crate::state::shared()?.clear_responses()?) })
        .await??;
    gateway_overview(&instance, &gw).await
}

/// Playground split view: can these models answer at the same time? `None` while the
/// gateway is off.
#[tauri::command]
pub(crate) async fn gateway_co_residency(
    gw: State<'_, GatewayState>,
    models: Vec<String>,
) -> Result<Option<crate::gateway::scheduler::CoResidency>, String> {
    let Some(s) = gw.server.scheduler().await else {
        return Ok(None);
    };
    s.co_residency(&models)
        .await
        .map(Some)
        .map_err(|e| e.message())
}

/// Load settings for the gateway's next JIT load of a definition only (empty = clear); the
/// definition is not changed. No-op while the gateway is off.
#[tauri::command]
pub(crate) async fn queue_gateway_load_override(
    gw: State<'_, GatewayState>,
    definition_id: String,
    overrides: harness_core::params::ParamValues,
) -> Result<(), String> {
    if let Some(s) = gw.server.scheduler().await {
        s.queue_load_override(&definition_id, overrides);
    }
    Ok(())
}

/// Unloads a definition the gateway serves (its servers stop).
#[tauri::command]
pub(crate) async fn unload_gateway_model(
    instance: State<'_, InstanceState>,
    gw: State<'_, GatewayState>,
    definition_id: String,
) -> Result<GatewayOverview, String> {
    if let Some(s) = gw.server.scheduler().await {
        s.unload(&definition_id).await?;
    }
    gateway_overview(&instance, &gw).await
}

#[tauri::command]
pub(crate) fn get_jobs() -> Vec<harness_core::jobs::JobStatus> {
    harness_core::jobs::global().snapshot()
}

#[tauri::command]
pub(crate) fn cancel_job(id: u64) -> bool {
    harness_core::jobs::global().cancel(id)
}
