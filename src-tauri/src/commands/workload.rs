// Workload consent (heavy jobs, microphone, camera) and the undo of agent file changes
// (harness_core::workload, harness_core::tools::checkpoint). The rules, the storage and the audit
// rows live in the core; these are thin adapters for the Permissions page and the device prompt.

#[tauri::command]
pub(crate) async fn workloads_list() -> Result<Vec<harness_core::workload::WorkloadView>, String> {
    blocking(|| harness_core::workload::Workloads::shared().list()).await
}

#[tauri::command]
pub(crate) async fn workloads_check(
    client: String,
    kind: harness_core::workload::WorkloadKind,
) -> Result<harness_core::workload::Verdict, String> {
    blocking(move || harness_core::workload::Workloads::shared().verdict(&client, kind)).await
}

#[tauri::command]
pub(crate) async fn workloads_remember(
    client: String,
    kind: harness_core::workload::WorkloadKind,
    choice: harness_core::workload::Choice,
) -> Result<harness_core::workload::Consent, String> {
    blocking(move || harness_core::workload::Workloads::shared().remember("ui", &client, kind, choice)).await?
}

#[tauri::command]
pub(crate) async fn workloads_revoke(consent_id: String) -> Result<(), String> {
    blocking(move || harness_core::workload::Workloads::shared().revoke("ui", &consent_id)).await?
}

#[tauri::command]
pub(crate) async fn checkpoints_list() -> Result<Vec<harness_core::tools::checkpoint::Checkpoint>, String> {
    blocking(|| harness_core::tools::checkpoint::Checkpoints::shared().list()).await
}

#[tauri::command]
pub(crate) async fn checkpoints_undo(
    id: String,
    force: bool,
) -> Result<harness_core::tools::checkpoint::Checkpoint, String> {
    blocking(move || harness_core::tools::checkpoint::Checkpoints::shared().undo(&id, force)).await?
}

#[tauri::command]
pub(crate) async fn checkpoints_undo_run(
    group: String,
    force: bool,
) -> Result<Vec<harness_core::tools::checkpoint::Checkpoint>, String> {
    blocking(move || harness_core::tools::checkpoint::Checkpoints::shared().undo_group(&group, force)).await?
}

#[tauri::command]
pub(crate) async fn checkpoints_clear() -> Result<(), String> {
    blocking(|| harness_core::tools::checkpoint::Checkpoints::shared().clear()).await?
}
