#[tauri::command]
pub(crate) async fn get_telemetry_snapshot(
    telemetry_state: State<'_, Arc<tokio::sync::Mutex<TelemetryPoller>>>,
    server_id: Option<String>,
) -> Result<TelemetrySnapshot, String> {
    let id = ServerRegistry::resolve_id(server_id)?;
    Ok(TelemetryPoller::fresh_snapshot(&telemetry_state, &id).await)
}

/// What the window's live views show: the telemetry of `server_id` (None = nothing, e.g. the
/// window is hidden). Changes are then pushed as `telemetry` events instead of polled.
/// `memory`: a view shows system RAM/VRAM even while no server runs (Dashboard meters), so it
/// is sampled every 2 s then. `all`: a view shows every running server (Dashboard cards), so
/// their changes are pushed too. `seq`: the UI's order number of the call (an older call that
/// finishes after a newer one is ignored). Returns the current snapshot of `server_id`.
#[tauri::command]
pub(crate) async fn watch_telemetry(
    telemetry_state: State<'_, Arc<tokio::sync::Mutex<TelemetryPoller>>>,
    watch: State<'_, Arc<crate::push::TelemetryWatch>>,
    server_id: Option<String>,
    memory: Option<bool>,
    all: Option<bool>,
    seq: Option<u64>,
) -> Result<Option<TelemetrySnapshot>, String> {
    let id = match server_id {
        Some(id) => Some(ServerRegistry::resolve_id(Some(id))?),
        None => None,
    };
    let current = match &id {
        Some(id) => Some(TelemetryPoller::fresh_snapshot(&telemetry_state, id).await),
        None => None,
    };
    let all = all.unwrap_or(false);
    let others = if all && id.is_some() {
        running_snapshots(&telemetry_state).await
    } else {
        Vec::new()
    };
    watch.set(seq, id, memory.unwrap_or(false), all, current.clone(), others);
    Ok(current)
}

/// One running server's latest telemetry.
#[derive(serde::Serialize)]
pub(crate) struct ServerTelemetry {
    server_id: String,
    snapshot: TelemetrySnapshot,
}

async fn running_snapshots(
    telemetry_state: &Arc<tokio::sync::Mutex<TelemetryPoller>>,
) -> Vec<(String, TelemetrySnapshot)> {
    let p = telemetry_state.lock().await;
    p.live_server_ids()
        .into_iter()
        .map(|id| {
            let snap = p.last(&id);
            (id, snap)
        })
        .collect()
}

/// The latest telemetry of every server the last sampler round reached (the Dashboard cards
/// start from it; changes then arrive as `telemetry` events).
#[tauri::command]
pub(crate) async fn get_telemetry_all(
    telemetry_state: State<'_, Arc<tokio::sync::Mutex<TelemetryPoller>>>,
) -> Result<Vec<ServerTelemetry>, String> {
    Ok(running_snapshots(&telemetry_state)
        .await
        .into_iter()
        .map(|(server_id, snapshot)| ServerTelemetry { server_id, snapshot })
        .collect())
}

/// Explicit, on-demand access to the API key of a running server (Connections snippets,
/// Playground requests, integrations). `list_servers` / `get_active_server` only report
/// `api_key_set`.
#[tauri::command]
pub(crate) fn get_server_api_key(
    registry: State<'_, Arc<ServerRegistry>>,
    server_id: Option<String>,
) -> Option<String> {
    server(&registry, server_id)
        .ok()?
        .active_server()?
        .api_key
        .filter(|k| !k.is_empty())
}

#[tauri::command]
pub(crate) fn get_active_server(
    registry: State<'_, Arc<ServerRegistry>>,
    server_id: Option<String>,
) -> Option<ActiveServerInfo> {
    server(&registry, server_id).ok()?.active_server()
}

#[tauri::command]
pub(crate) fn get_server_logs_since(
    registry: State<'_, Arc<ServerRegistry>>,
    server_id: Option<String>,
    since: u64,
) -> (u64, Vec<TimedLogLine>) {
    server(&registry, server_id)
        .map(|pm| pm.get_timed_logs_since(since))
        .unwrap_or((0, Vec::new()))
}

#[tauri::command]
pub(crate) fn clear_server_logs(registry: State<'_, Arc<ServerRegistry>>, server_id: Option<String>) {
    if let Ok(pm) = server(&registry, server_id) {
        pm.clear_logs();
    }
}

#[tauri::command]
pub(crate) async fn get_telemetry_history(
    telemetry_state: State<'_, Arc<tokio::sync::Mutex<TelemetryPoller>>>,
    server_id: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<TelemetryHistoryPoint>, String> {
    let id = ServerRegistry::resolve_id(server_id)?;
    Ok(telemetry_state.lock().await.get_history(&id, limit))
}





/// CPU, process memory, and GPU load for the Performance view. Sampled on demand (the view
/// asks once a second while visible), never in the background.
#[tauri::command]
pub(crate) async fn get_perf_details(
    registry: State<'_, Arc<ServerRegistry>>,
    telemetry_state: State<'_, Arc<tokio::sync::Mutex<TelemetryPoller>>>,
    sampler: State<'_, Arc<crate::perf::PerfSampler>>,
    server_id: Option<String>,
) -> Result<crate::perf::PerfDetails, String> {
    let pid = server(&registry, server_id).ok().and_then(|pm| pm.pid());
    let (_, gpu) = telemetry_state.lock().await.sampling_context();
    let sampler = sampler.inner().clone();
    tokio::task::spawn_blocking(move || sampler.sample(pid, gpu.as_ref()))
        .await
        .map_err(|e| harness_core::error_key::err("task.join_failed").with("error", e).into())
}
