#[tauri::command]
pub(crate) async fn check_runtime_update(
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    backend: Option<String>,
) -> Result<ReleaseAssetInfo, String> {
    let hw = detect_hardware();
    // A requested backend (the user chose it), else the recorded one, else the installed
    // build's (updates never switch backends), else the recommendation (first install; the
    // only case that may fall back to another backend).
    let target = crate::runtime::install_query(
        &hw,
        AppSettings::load().managed_runtime.backend.as_deref(),
        runtime_mgr.installed_backend().as_deref(),
        backend.as_deref(),
    );
    runtime_mgr
        .check_latest_release(&target.query, target.mode)
        .await
}

#[tauri::command]
pub(crate) async fn download_and_verify_staging(
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    download_url: String,
    extra_urls: Option<Vec<String>>,
    allow_unverified: Option<bool>,
) -> Result<String, String> {
    let _busy = runtime_mgr.try_busy()?;
    let mut urls = vec![download_url];
    urls.extend(extra_urls.unwrap_or_default());
    // Without the user's explicit confirmation an archive without a SHA-256 digest is refused
    // with `runtime.integrity_unverified` before anything is downloaded.
    runtime_mgr
        .download_and_extract_staging(&urls, allow_unverified.unwrap_or(false))
        .await?;
    let mgr = runtime_mgr.inner().clone();
    tauri::async_runtime::spawn_blocking(move || mgr.verify_stage1_binary())
        .await
        .map_err(|e| {
            String::from(harness_core::error_key::err("task.join_failed").with("error", e))
        })?
}

#[tauri::command]
pub(crate) fn get_runtime_download_progress(
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
) -> Option<RuntimeDownloadProgress> {
    runtime_mgr.get_download_progress()
}

#[tauri::command]
pub(crate) async fn activate_managed_runtime(
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    registry: State<'_, Arc<ServerRegistry>>,
    gate: State<'_, Arc<OperationGate>>,
    version: Option<String>,
) -> Result<AppSettings, String> {
    let _runtime_lease = gate.runtime_write().await;
    ensure_server_stopped(&registry)?;
    let _busy = runtime_mgr.try_busy()?;
    let manifest = runtime_mgr.activate_runtime(version.as_deref())?;
    let mut settings = AppSettings::load();
    crate::runtime::record_activation(&mut settings, &manifest);
    settings.save()?;
    Ok(settings)
}

#[tauri::command]
pub(crate) async fn rollback_managed_runtime(
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    registry: State<'_, Arc<ServerRegistry>>,
    gate: State<'_, Arc<OperationGate>>,
) -> Result<AppSettings, String> {
    let _runtime_lease = gate.runtime_write().await;
    ensure_server_stopped(&registry)?;
    let _busy = runtime_mgr.try_busy()?;
    let restored = runtime_mgr.rollback_runtime()?;
    let mut settings = AppSettings::load();
    crate::runtime::record_rollback(&mut settings, &restored);
    settings.save()?;
    Ok(settings)
}

#[tauri::command]
pub(crate) fn get_runtime_auto_update_status(updater: State<'_, RuntimeAutoUpdater>) -> AutoUpdateStatus {
    updater.status()
}

/// Runs one auto-update cycle now (still only activates when no server is running).
#[tauri::command]
pub(crate) async fn run_runtime_auto_update(
    app: tauri::AppHandle,
    updater: State<'_, RuntimeAutoUpdater>,
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    registry: State<'_, Arc<ServerRegistry>>,
    gate: State<'_, Arc<OperationGate>>,
) -> Result<AutoUpdateStatus, String> {
    let (updater, runtime, registry, gate) = (
        updater.inner().clone(),
        runtime_mgr.inner().clone(),
        registry.inner().clone(),
        gate.inner().clone(),
    );
    // Waiting for activation may take long: run in the background, report the phase.
    let u = updater.clone();
    tauri::async_runtime::spawn(async move {
        u.run_once(&runtime, &registry, &gate, &|s: &AppSettings| {
            let _ = app.emit("runtime-updated", s);
        })
        .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    Ok(updater.status())
}

#[tauri::command]
pub(crate) fn has_runtime_backup(runtime_mgr: State<'_, Arc<RuntimeManager>>) -> bool {
    runtime_mgr.has_backup()
}

#[tauri::command]
pub(crate) async fn validate_custom_binary(path: String) -> Result<CustomBinaryValidation, String> {
    blocking(move || RuntimeManager::validate_custom_binary(&PathBuf::from(path))).await
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDetectionInfo {
    pub managed_installed: bool,
    pub managed_version: Option<String>,
    pub managed_path: Option<String>,
    pub system_detected: bool,
    pub system_path: Option<String>,
    pub system_version: Option<String>,
    pub system_supports_metrics: bool,
    pub active_mode: RuntimeMode,
    pub resolved_binary_path: Option<String>,
}

#[tauri::command]
pub(crate) async fn get_runtime_detection_info(
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
) -> Result<RuntimeDetectionInfo, String> {
    let settings = AppSettings::load();
    let managed_path_buf = runtime_mgr.get_managed_binary_path();
    let managed_installed = managed_path_buf.is_some();
    let managed_path = managed_path_buf
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());
    let managed_version = settings.managed_runtime.installed_version.clone();

    let (system_detected, system_path, system_version, system_supports_metrics) =
        if let Some(sys_path) = RuntimeManager::detect_system_binary() {
            let val = RuntimeManager::validate_custom_binary(&sys_path);
            (
                true,
                Some(sys_path.to_string_lossy().to_string()),
                val.version,
                val.supports_metrics,
            )
        } else {
            (false, None, None, false)
        };

    let resolved = resolve_server_binary(&runtime_mgr, &settings)
        .ok()
        .map(|p| p.to_string_lossy().to_string());

    Ok(RuntimeDetectionInfo {
        managed_installed,
        managed_version,
        managed_path,
        system_detected,
        system_path,
        system_version,
        system_supports_metrics,
        active_mode: settings.runtime_mode,
        resolved_binary_path: resolved,
    })
}

#[tauri::command]
pub(crate) async fn auto_configure_system_runtime() -> Result<AppSettings, String> {
    let mut settings = AppSettings::load();
    if let Some(sys_path) = RuntimeManager::detect_system_binary() {
        let val = RuntimeManager::validate_custom_binary(&sys_path);
        if val.is_valid {
            settings.runtime_mode = RuntimeMode::Custom;
            settings.custom_binary.binary_path = Some(sys_path);
            settings.custom_binary.is_valid = true;
            settings.custom_binary.version = val.version;
            settings.custom_binary.supports_metrics = val.supports_metrics;
            settings.save()?;
            return Ok(settings);
        }
    }
    Err(harness_core::error_key::err("runtime.system_binary_not_found").into())
}



/// Activates a managed build that was downloaded (digest-checked) into staging but never
/// activated: runs stage 1 again (`--version`; staging is removed if it fails), moves it to
/// `managed` (the previous one to `backup`) and records its build as the installed version.
/// Does not change the runtime mode (the user switches to "managed" in the form).
#[tauri::command]
pub(crate) async fn activate_staged_runtime(
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    registry: State<'_, Arc<ServerRegistry>>,
    gate: State<'_, Arc<OperationGate>>,
) -> Result<AppSettings, String> {
    let _runtime_lease = gate.runtime_write().await;
    ensure_server_stopped(&registry)?;
    let _busy = runtime_mgr.try_busy()?;
    let mgr = runtime_mgr.inner().clone();
    let text = tauri::async_runtime::spawn_blocking(move || mgr.verify_stage1_binary())
        .await
        .map_err(|e| {
            String::from(harness_core::error_key::err("task.join_failed").with("error", e))
        })??;
    let version = crate::capabilities::parse::parse_version(&text)
        .build
        .map(|b| format!("b{}", b));
    let manifest = runtime_mgr.activate_runtime(version.as_deref())?;
    let mut settings = AppSettings::load();
    crate::runtime::record_activation(&mut settings, &manifest);
    settings.save()?;
    Ok(settings)
}
