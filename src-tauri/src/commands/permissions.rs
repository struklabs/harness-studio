// Trust levels and capability grants (harness_core::permissions). The rules, the storage and the
// audit rows live in the core; these are thin adapters for the Permissions page.

#[tauri::command]
pub(crate) async fn permissions_list() -> Result<Vec<harness_core::permissions::SubjectView>, String> {
    blocking(|| harness_core::permissions::Permissions::shared().list()).await
}

#[tauri::command]
pub(crate) async fn permissions_grant(
    kind: harness_core::permissions::SubjectKind,
    id: String,
    capability: harness_core::permissions::Capability,
    paths: Vec<String>,
) -> Result<harness_core::permissions::Grant, String> {
    blocking(move || harness_core::permissions::Permissions::shared().grant("ui", kind, &id, capability, paths)).await?
}

#[tauri::command]
pub(crate) async fn permissions_revoke_grant(
    kind: harness_core::permissions::SubjectKind,
    id: String,
    grant_id: String,
) -> Result<(), String> {
    blocking(move || harness_core::permissions::Permissions::shared().revoke_grant("ui", kind, &id, &grant_id)).await?
}

#[tauri::command]
pub(crate) async fn permissions_revoke_subject(
    kind: harness_core::permissions::SubjectKind,
    id: String,
) -> Result<u32, String> {
    blocking(move || harness_core::permissions::Permissions::shared().revoke_subject("ui", kind, &id)).await?
}

#[tauri::command]
pub(crate) async fn permissions_set_trust(
    kind: harness_core::permissions::SubjectKind,
    id: String,
    trust: Option<harness_core::tools::PermissionClass>,
) -> Result<(), String> {
    blocking(move || harness_core::permissions::Permissions::shared().set_trust("ui", kind, &id, trust)).await?
}
