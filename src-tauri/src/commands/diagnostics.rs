#[tauri::command]
pub(crate) async fn detect_system_hardware() -> Result<SystemHardwareInfo, String> {
    blocking(detect_hardware).await
}


#[tauri::command]
pub(crate) async fn check_offline_crash(
    diag_mgr: State<'_, Arc<DiagnosticsManager>>,
) -> Result<Vec<DiagnosticPayload>, String> {
    let mgr = diag_mgr.inner().clone();
    blocking(move || {
        let settings = AppSettings::load();
        let hw = detect_hardware();
        mgr.process_offline_crash(&settings, Some(&hw))
    })
    .await?
}

#[tauri::command]
pub(crate) fn list_diagnostic_reports(diag_mgr: State<'_, Arc<DiagnosticsManager>>) -> Vec<DiagnosticPayload> {
    diag_mgr.list_reports()
}

#[tauri::command]
pub(crate) fn clear_diagnostic_reports(diag_mgr: State<'_, Arc<DiagnosticsManager>>) {
    diag_mgr.clear_reports();
}

#[tauri::command]
pub(crate) fn save_crash_event(
    diag_mgr: State<'_, Arc<DiagnosticsManager>>,
    event: DiagnosticEvent,
) -> Result<(), String> {
    diag_mgr.save_offline_crash(&event)
}

// Storage Management Commands




/// Settings > Diagnostics: the `harness doctor` report (read-only checks).
#[tauri::command]
pub(crate) async fn run_doctor(
    registry: State<'_, Arc<ServerRegistry>>,
) -> Result<crate::doctor::DoctorReport, String> {
    // Ports our own servers hold are not conflicts.
    let own: Vec<u16> = registry.active_servers().into_iter().map(|(_, a)| a.port).collect();
    blocking(move || {
        crate::doctor::run(&paths::data_root(), env!("CARGO_PKG_VERSION"), &own)
    })
    .await
}
