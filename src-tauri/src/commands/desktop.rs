#[tauri::command]
pub(crate) async fn check_app_update(
    app: tauri::AppHandle,
    state: State<'_, AppUpdateState>,
) -> Result<Option<AppUpdateInfo>, String> {
    state.check(&app).await
}

#[tauri::command]
pub(crate) async fn install_app_update(
    app: tauri::AppHandle,
    state: State<'_, AppUpdateState>,
    registry: State<'_, Arc<ServerRegistry>>,
) -> Result<(), String> {
    state.install(&app, &registry).await
}

#[tauri::command]
pub(crate) fn get_app_update_progress(state: State<'_, AppUpdateState>) -> Option<AppUpdateProgress> {
    state.progress()
}

#[tauri::command]
pub(crate) fn get_launch_at_login(app: tauri::AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub(crate) fn set_launch_at_login(app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let autolaunch = app.autolaunch();
    let result = if enabled {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    };
    result.map_err(|e| harness_core::error_key::err("desktop.autostart_failed").with("error", e))?;
    #[cfg(target_os = "macos")]
    if enabled {
        desktop::ensure_launch_agent_bundle_id(&app);
    }
    Ok(autolaunch.is_enabled().unwrap_or(enabled))
}

#[tauri::command]
pub(crate) fn save_app_settings(app: tauri::AppHandle, mut settings: AppSettings) -> Result<(), String> {
    // Bind address / CORS origins are validated; "open anyway" confirmations only change
    // through their own commands.
    network::sanitize_settings(&mut settings, &AppSettings::load())?;
    // The tool policy changes only through its own paths (`tools::save_settings`): a settings
    // object from an older frontend that does not know the block must not reset it.
    settings.tools = AppSettings::load().tools;
    desktop::update_tray_language(&app, &settings.language);
    settings.save()
}

/// `backend` optionally overrides the detected backend ("cuda", "vulkan", "cpu", "rocm", ...).

#[tauri::command]
pub(crate) async fn pick_directory_dialog(app: tauri::AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    app.dialog()
        .file()
        .blocking_pick_folder()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().to_string())
}


