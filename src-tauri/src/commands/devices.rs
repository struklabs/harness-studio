// Devices page: pairing offers, the approval prompt, the paired-device registry, audit,
// revocation, host suggestions and connecting this computer to another host. Thin adapters over
// harness_core::pairing (the Control API exposes the same).

use harness_core::pairing::{self, service};

fn device_tokens() -> harness_core::control_api::TokenStore {
    harness_core::control_api::TokenStore::global()
}

#[tauri::command]
pub(crate) async fn pairing_create_offer(
    spec: pairing::OfferSpec,
) -> Result<service::OfferCreated, String> {
    blocking(move || service::create_offer(&device_tokens(), &spec)).await?
}

#[tauri::command]
pub(crate) async fn pairing_cancel_offer(id: String) -> Result<bool, String> {
    blocking(move || pairing::hub_for(&device_tokens()).cancel_offer(&id)).await
}

#[tauri::command]
pub(crate) async fn pairing_pending() -> Result<Vec<pairing::PendingClaim>, String> {
    blocking(|| pairing::hub_for(&device_tokens()).pending()).await
}

#[tauri::command]
pub(crate) async fn pairing_approve(id: String) -> Result<pairing::DeviceView, String> {
    blocking(move || {
        let tokens = device_tokens();
        pairing::hub_for(&tokens).approve(&id, &tokens)
    })
    .await?
}

#[tauri::command]
pub(crate) async fn pairing_deny(id: String) -> Result<(), String> {
    blocking(move || pairing::hub_for(&device_tokens()).deny(&id)).await?
}

#[tauri::command]
pub(crate) async fn devices_list() -> Result<Vec<pairing::DeviceView>, String> {
    blocking(|| {
        let tokens = device_tokens();
        pairing::hub_for(&tokens).list(&tokens)
    })
    .await?
}

/// Renames a device, changes its role and/or its access preset (models replace the allowed
/// model patterns when a preset is given).
#[tauri::command]
pub(crate) async fn devices_update(
    id: String,
    name: Option<String>,
    role: Option<pairing::DeviceRole>,
    preset: Option<pairing::AccessPreset>,
    models: Option<Vec<String>>,
    acknowledge_admin: Option<bool>,
) -> Result<pairing::DeviceView, String> {
    blocking(move || {
        let tokens = device_tokens();
        let hub = pairing::hub_for(&tokens);
        let mut view = hub.update(&id, name.as_deref(), role, &tokens)?;
        if let Some(preset) = preset {
            view = hub.set_access(
                &id,
                preset,
                &models.unwrap_or_default(),
                acknowledge_admin.unwrap_or(false),
                &tokens,
            )?;
        }
        Ok(view)
    })
    .await?
}

#[tauri::command]
pub(crate) async fn devices_revoke(id: String) -> Result<(), String> {
    blocking(move || {
        let tokens = device_tokens();
        pairing::hub_for(&tokens).revoke(&id, &tokens)
    })
    .await?
}

/// Lost-phone recovery: unpairs every device; returns how many.
#[tauri::command]
pub(crate) async fn devices_revoke_all() -> Result<usize, String> {
    blocking(|| {
        let tokens = device_tokens();
        pairing::hub_for(&tokens).revoke_all(&tokens)
    })
    .await?
}

#[tauri::command]
pub(crate) async fn devices_audit(limit: Option<usize>) -> Result<Vec<pairing::AuditEntry>, String> {
    blocking(move || pairing::hub_for(&device_tokens()).audit_log(limit.unwrap_or(100))).await
}

#[tauri::command]
pub(crate) async fn devices_suggest(required_bytes: u64) -> Result<service::Suggestion, String> {
    blocking(move || {
        service::suggest(&device_tokens(), required_bytes, service::local_capacity_bytes())
    })
    .await?
}

#[tauri::command]
pub(crate) fn devices_roles() -> Vec<service::RoleInfo> {
    service::roles()
}

/// Starts pairing this computer with another Harness host by its one-time code.
#[tauri::command]
pub(crate) async fn pairing_connect_start(
    url: String,
    code: String,
    device_name: String,
    allow_insecure_http: bool,
) -> Result<pairing::client::ClientClaim, String> {
    pairing::client::start(&url, &code, &device_name, std::env::consts::OS, allow_insecure_http)
        .await
}

/// Polls the other host; once approved the token is saved as a remote link.
#[tauri::command]
pub(crate) async fn pairing_connect_poll(
    link_name: String,
    claim: pairing::client::ClientClaim,
) -> Result<service::ConnectProgress, String> {
    service::connect_progress(&link_name, &claim).await
}

#[tauri::command]
pub(crate) async fn pairing_settings_get() -> Result<pairing::PairingSettings, String> {
    blocking(|| pairing::hub_for(&device_tokens()).settings()).await
}

#[tauri::command]
pub(crate) fn remote_access_available() -> bool {
    cfg!(feature = "remote-access")
}

#[tauri::command]
pub(crate) async fn remote_access_settings_get() -> Result<pairing::remote::RemoteAccessSettingsV1, String> {
    blocking(|| pairing::hub_for(&device_tokens()).remote_settings()).await
}

#[tauri::command]
pub(crate) async fn remote_access_settings_set(enabled: bool) -> Result<(), String> {
    blocking(move || pairing::hub_for(&device_tokens()).set_remote_settings(&pairing::remote::RemoteAccessSettingsV1 { enabled })).await??;
    pairing::remote::runtime::restart(device_tokens());
    Ok(())
}

#[tauri::command]
pub(crate) async fn pairing_settings_set(
    settings: pairing::PairingSettings,
) -> Result<pairing::PairingSettings, String> {
    blocking(move || pairing::hub_for(&device_tokens()).set_settings(&settings)).await?
}
