// Settings > Network & security: bind default, CORS, "open anyway" confirmations, strong keys.

/// A start that was refused only because a network bind has no API key. The UI shows a
/// confirmation ("Yine de aç"); confirming replays it. Kept in memory only.
pub(crate) enum PendingStart {
    Config(Box<LaunchConfig>),
    Profile,
}

#[derive(Default)]
pub(crate) struct PendingExposure(std::sync::Mutex<std::collections::HashMap<String, PendingStart>>);

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ExposureBlocked {
    server_id: String,
    host: String,
}

/// Passes `result` through; when it is the "network bind without key" refusal, remembers the
/// start for a confirmed retry and tells the UI (`network-exposure-blocked`).
pub(crate) fn note_exposure_block(
    app: &tauri::AppHandle,
    pending: &PendingExposure,
    server_id: &str,
    host: &str,
    start: PendingStart,
    result: Result<(), String>,
) -> Result<(), String> {
    if let Err(e) = &result {
        if e == network::ERR_LAN_WITHOUT_KEY {
            pending
                .0
                .lock()
                .unwrap()
                .insert(server_id.to_string(), start);
            let host = if host.trim().is_empty() {
                AppSettings::load().server.host
            } else {
                host.to_string()
            };
            let _ = app.emit(
                "network-exposure-blocked",
                ExposureBlocked {
                    server_id: server_id.to_string(),
                    host,
                },
            );
        }
    }
    result
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct NetworkOverview {
    /// Default bind address (`settings.server.host`) and its scope.
    default_host: String,
    default_scope: network::BindScope,
    /// This computer's LAN IPv4 addresses (for other devices).
    lan_addresses: Vec<network::LanAddress>,
    /// Whether the default server's runtime has `--cors-origins` (`None` = unknown).
    cors_supported: Option<bool>,
    settings: network::NetworkSettings,
}

#[tauri::command]
pub(crate) async fn get_network_overview(
    registry: State<'_, Arc<ServerRegistry>>,
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
) -> Result<NetworkOverview, String> {
    let binary = server_binary(&registry, &runtime_mgr, None).ok();
    blocking(move || {
        let settings = AppSettings::load();
        let cors_supported = binary.map(|b| {
            let caps = capabilities::registry().get_or_probe(&b);
            caps.help_parsed.then(|| caps.has_flag("--cors-origins"))
        });
        NetworkOverview {
            default_scope: network::bind_scope(&settings.server.host)
                .unwrap_or(network::BindScope::Lan),
            default_host: settings.server.host.clone(),
            lan_addresses: network::lan_ipv4_addresses(),
            cors_supported: cors_supported.flatten(),
            settings: settings.network,
        }
    })
    .await
}

/// Records "open anyway" for a server (network bind without key). With `retry`, the start
/// that was refused is replayed.
#[tauri::command]
pub(crate) async fn acknowledge_lan_exposure(
    registry: State<'_, Arc<ServerRegistry>>,
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    gate: State<'_, Arc<OperationGate>>,
    pending: State<'_, PendingExposure>,
    server_id: String,
    host: String,
    retry: Option<bool>,
) -> Result<AppSettings, String> {
    let settings = {
        let (id, host) = (server_id.clone(), host.clone());
        blocking(move || network::acknowledge(&id, &host)).await??
    };
    let start = pending.0.lock().unwrap().remove(&server_id);
    if retry.unwrap_or(false) {
        match start {
            // Same path as `start_server`: a config naming its definition is layered over it
            // (so the served alias stays the definition's); resolved configs pass unchanged.
            Some(PendingStart::Config(config)) => {
                crate::orchestrator::launch_form(&registry, &runtime_mgr, &gate, server_id, *config)
                    .await?
            }
            Some(PendingStart::Profile) => {
                crate::orchestrator::launch_profile(&registry, &runtime_mgr, &gate, &server_id)
                    .await?
            }
            None => {}
        }
    }
    Ok(settings)
}

#[tauri::command]
pub(crate) async fn revoke_lan_exposure(server_id: String) -> Result<AppSettings, String> {
    blocking(move || network::revoke(&server_id)).await?
}

/// Forgets a refused start the user declined to open anyway.
#[tauri::command]
pub(crate) fn discard_pending_exposure(pending: State<'_, PendingExposure>, server_id: String) {
    pending.0.lock().unwrap().remove(&server_id);
}

/// Generates a strong key (32 random bytes, base64url) for a saved profile and stores it in
/// the OS keychain; the profile keeps only the reference. Takes effect on the next start.
#[tauri::command]
pub(crate) async fn generate_profile_api_key(
    profile_id: String,
) -> Result<Vec<ServerProfile>, String> {
    servers::validate_id(&profile_id)?;
    blocking(move || network::generate_profile_key(&profile_id)).await?
}

/// The saved key of a profile, read from the keychain on demand (for copying into clients).
#[tauri::command]
pub(crate) async fn get_profile_api_key(profile_id: String) -> Result<Option<String>, String> {
    servers::validate_id(&profile_id)?;
    blocking(move || {
        servers::load_profiles()
            .into_iter()
            .find(|p| p.id == profile_id)
            .and_then(|p| p.config.api_key_ref)
            .and_then(|r| secrets::api_key_get(&r))
    })
    .await
}
