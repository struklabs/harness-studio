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
pub(crate) fn get_launch_at_login(app: tauri::AppHandle) -> Result<bool, String> {
    // Linux and Windows write their own entries (see `autostart_linux` / `autostart_windows`).
    // A read error is returned, not shown as "off".
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        Ok(crate::autostart_linux::is_enabled())
    }
    #[cfg(windows)]
    {
        crate::autostart_windows::is_enabled(&app.package_info().name)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        use tauri_plugin_autostart::ManagerExt;
        Ok(app.autolaunch().is_enabled().map_err(|e| {
            harness_core::error_key::err("desktop.autostart_status_failed").with("error", e)
        })?)
    }
}

#[tauri::command]
pub(crate) fn set_launch_at_login(app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    // Linux writes its own entry (quoted path, `$XDG_CONFIG_HOME`, kept current at startup).
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        let result = if enabled {
            crate::autostart_linux::enable()
        } else {
            crate::autostart_linux::disable()
        };
        result.map_err(|e| harness_core::error_key::err("desktop.autostart_failed").with("error", e))?;
        Ok(crate::autostart_linux::is_enabled())
    }
    // Windows writes its own Run value (quoted path, current user only).
    #[cfg(windows)]
    {
        let name = app.package_info().name.clone();
        let result = if enabled {
            crate::autostart_windows::enable(&name)
        } else {
            crate::autostart_windows::disable(&name)
        };
        result.map_err(|e| harness_core::error_key::err("desktop.autostart_failed").with("error", e))?;
        crate::autostart_windows::is_enabled(&name)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    set_launch_at_login_plugin(app, enabled)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn set_launch_at_login_plugin(app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let autolaunch = app.autolaunch();
    // The login entry names this executable; a translocated copy is gone after a restart.
    if enabled {
        if let Ok(exe) = std::env::current_exe() {
            crate::integrations::cli_install::refuse_translocated(&exe)?;
        }
    }
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
    settings.audio_cpp_enabled = AppSettings::load().audio_cpp_enabled;
    desktop::update_tray_language(&app, &settings.language);
    settings.save()
}

#[tauri::command]
pub(crate) fn set_audio_cpp_enabled(enabled: bool, acknowledged: bool) -> Result<AppSettings, String> {
    let mut settings = AppSettings::load();
    settings.set_audio_cpp_enabled(enabled, acknowledged)?;
    settings.save()?;
    Ok(settings)
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


