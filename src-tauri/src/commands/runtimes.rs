// Runtime registry (P2 alternative runtimes): Settings > Runtime list, add/verify/remove, and
// the per-model advice the Dashboard shows.

#[tauri::command]
pub(crate) async fn list_runtimes(
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
) -> Result<Vec<crate::runtimes::RuntimeView>, String> {
    let mgr = runtime_mgr.inner().clone();
    blocking(move || {
        crate::runtimes::list_in(
            &crate::runtimes::RuntimeRegistry::global(),
            &mgr,
            &AppSettings::load(),
        )
    })
    .await?
}

/// The archive(s) a GitHub install would download (shown before asking the user).
#[tauri::command]
pub(crate) async fn preview_runtime_release(
    query: crate::runtimes::github::ReleaseQuery,
) -> Result<crate::runtimes::github::ReleasePick, String> {
    crate::runtimes::github::find_release(&query).await
}

#[tauri::command]
pub(crate) async fn install_runtime_github(
    request: crate::runtimes::github::GithubInstall,
) -> Result<crate::runtimes::RuntimeEntry, String> {
    crate::runtimes::github::install(&crate::runtimes::RuntimeRegistry::global(), &request).await
}

#[tauri::command]
pub(crate) fn get_runtime_install_progress() -> Option<RuntimeDownloadProgress> {
    crate::runtimes::github::progress()
}

#[tauri::command]
pub(crate) async fn add_runtime_local(
    request: crate::runtimes::AddLocal,
) -> Result<crate::runtimes::RuntimeEntry, String> {
    blocking(move || crate::runtimes::RuntimeRegistry::global().add_local(&request)).await?
}

#[tauri::command]
pub(crate) async fn verify_runtime(
    id: String,
    accept_changes: Option<bool>,
) -> Result<crate::runtimes::VerifyReport, String> {
    blocking(move || {
        crate::runtimes::RuntimeRegistry::global().verify(&id, accept_changes.unwrap_or(false))
    })
    .await?
}

#[tauri::command]
pub(crate) async fn get_runtime_users(
    registry: State<'_, Arc<ServerRegistry>>,
    id: String,
) -> Result<crate::orchestrator::RuntimeUsers, String> {
    let registry = registry.inner().clone();
    blocking(move || crate::orchestrator::runtime_users(&registry, &id)).await?
}

#[tauri::command]
pub(crate) async fn remove_runtime(
    registry: State<'_, Arc<ServerRegistry>>,
    gate: State<'_, Arc<OperationGate>>,
    id: String,
    force: Option<bool>,
) -> Result<(), String> {
    crate::orchestrator::remove_runtime(&registry, &gate, id, force.unwrap_or(false))
        .await
        .map(|_| ())
}

/// Which runtimes can load a model (GGUF tensor types) and what to suggest.
#[tauri::command]
pub(crate) async fn get_model_runtime_advice(
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    model_path: String,
    runtime_id: Option<String>,
) -> Result<crate::runtimes::RuntimeAdvice, String> {
    let mgr = runtime_mgr.inner().clone();
    blocking(move || {
        crate::runtimes::advise(
            &mgr,
            &AppSettings::load(),
            Path::new(&model_path),
            runtime_id.as_deref(),
        )
    })
    .await?
}

// Managed ONNX Runtime (the library `harness-onnx` loads): Settings > Runtime card.

#[tauri::command]
pub(crate) async fn list_onnx_runtime() -> Result<Vec<crate::onnx_runtime::VariantView>, String> {
    blocking(crate::onnx_runtime::view).await
}

/// Whether this OS / architecture has any ONNX Runtime build (none on Intel Macs).
#[tauri::command]
pub(crate) fn get_onnx_runtime_support() -> crate::onnx_runtime::PlatformSupport {
    crate::onnx_runtime::platform_support()
}

#[tauri::command]
pub(crate) fn get_onnx_runtime_progress() -> Option<crate::onnx_runtime::InstallProgress> {
    crate::onnx_runtime::progress()
}

#[tauri::command]
pub(crate) async fn install_onnx_runtime(
    variant: String,
) -> Result<crate::onnx_runtime::Installed, String> {
    let v = crate::onnx_runtime::OrtVariant::parse(&variant)
        .ok_or_else(|| format!("onnx.unsupported_platform: {variant}"))?;
    crate::onnx_runtime::install(v).await
}

/// The files that differ from the pinned digests (empty = verified).
#[tauri::command]
pub(crate) async fn verify_onnx_runtime(variant: String) -> Result<Vec<String>, String> {
    let v = crate::onnx_runtime::OrtVariant::parse(&variant)
        .ok_or_else(|| format!("onnx.unsupported_platform: {variant}"))?;
    blocking(move || crate::onnx_runtime::verify_in(&crate::onnx_runtime::root(), v)).await?
}

#[tauri::command]
pub(crate) async fn remove_onnx_runtime(variant: String) -> Result<(), String> {
    let v = crate::onnx_runtime::OrtVariant::parse(&variant)
        .ok_or_else(|| format!("onnx.unsupported_platform: {variant}"))?;
    blocking(move || crate::onnx_runtime::remove_in(&crate::onnx_runtime::root(), v)).await?
}
