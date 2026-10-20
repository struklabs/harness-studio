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
        mgr.process_offline_crash(&settings, None)
    })
    .await?
}

#[tauri::command]
pub(crate) fn list_diagnostic_reports(
    diag_mgr: State<'_, Arc<DiagnosticsManager>>,
) -> Vec<DiagnosticPayload> {
    diag_mgr.cleanup(AppSettings::load().privacy.retention_days);
    diag_mgr.list_reports()
}

#[tauri::command]
pub(crate) fn clear_diagnostic_reports(
    diag_mgr: State<'_, Arc<DiagnosticsManager>>,
) -> Result<(), String> {
    diag_mgr.clear_reports()
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
    let own: Vec<u16> = registry
        .active_servers()
        .into_iter()
        .map(|(_, a)| a.port)
        .collect();
    blocking(move || crate::doctor::run(&paths::data_root(), env!("CARGO_PKG_VERSION"), &own)).await
}

#[tauri::command]
pub(crate) fn delete_diagnostic_report(
    diag_mgr: State<'_, Arc<DiagnosticsManager>>,
    id: String,
) -> Result<(), String> {
    diag_mgr.delete_report(&id)
}

#[tauri::command]
pub(crate) async fn export_diagnostic_report(
    app: tauri::AppHandle,
    diag_mgr: State<'_, Arc<DiagnosticsManager>>,
    id: String,
) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    let mgr = diag_mgr.inner().clone();
    let picked = blocking(move || {
        app.dialog()
            .file()
            .set_file_name("harness-report.zip")
            .add_filter("ZIP", &["zip"])
            .blocking_save_file()
    })
    .await?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(false);
    };
    blocking(move || {
        mgr.cleanup(AppSettings::load().privacy.retention_days);
        mgr.export_zip(&id, &path)
    })
    .await??;
    Ok(true)
}
