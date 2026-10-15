/// Devices of a server's runtime (`--list-devices`, cached ~20 s per binary) with the OS
/// usage folded in and a main-GPU / tensor-split suggestion for several GPUs.
#[tauri::command]
pub(crate) async fn get_runtime_devices(
    registry: State<'_, Arc<ServerRegistry>>,
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    server_id: Option<String>,
    refresh: Option<bool>,
) -> Result<crate::devices::DeviceReport, String> {
    let binary = server_binary(&registry, &runtime_mgr, server_id)?;
    let max_age = if refresh.unwrap_or(false) {
        std::time::Duration::ZERO
    } else {
        crate::devices::FRESH_FOR
    };
    blocking(move || crate::devices::report(&binary, max_age)).await
}

/// Memory estimates for several inputs (`harness_core::model_registry::fit_on_machine`: the
/// llama.cpp estimate of `harness_core::vram` or the whisper.cpp one, by the file format,
/// corrected with measured loads).
#[tauri::command]
pub(crate) async fn estimate_fit(
    requests: Vec<crate::vram::FitInput>,
) -> Result<Vec<crate::vram::FitResult>, String> {
    blocking(move || {
        let hw = detect_hardware();
        requests
            .into_iter()
            .map(|r| harness_core::model_registry::fit_on_machine(r, &hw))
            .collect()
    })
    .await
}

/// Memory estimates of stable-diffusion.cpp image models by their size (main file plus the
/// companions it loads with): the Hub's files are not on disk to be told from an LLM GGUF.
#[tauri::command]
pub(crate) async fn estimate_image_fit(
    sizes: Vec<u64>,
) -> Result<Vec<crate::vram::FitResult>, String> {
    blocking(move || {
        let hw = detect_hardware();
        sizes
            .into_iter()
            .map(|bytes| harness_core::model_registry::image_fit_on_machine(bytes, &hw))
            .collect()
    })
    .await
}

/// Estimate the Dashboard's exact launch form through the same core launch plan as Start.
#[tauri::command]
pub(crate) async fn estimate_launch_fit(
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    config: crate::process::LaunchConfig,
    server_id: Option<String>,
) -> Result<crate::vram::FitResult, String> {
    let id = ServerRegistry::resolve_id(server_id)?;
    let runtime_mgr = runtime_mgr.inner().clone();
    blocking(move || {
        // Mirror `orchestrator::launch_form` up to (but not including) any mutation: resolve
        // the definition/profile layers and selected runtime so capability-dependent slots are
        // estimated from exactly what Start will receive.
        let mut resolved = harness_core::domain::store::Domain::global().resolve_form(
            &harness_core::domain::launch::LaunchDefaults::load(),
            &id,
            &config,
        )?;
        let settings = crate::config::AppSettings::load();
        let runtimes = crate::runtimes::RuntimeRegistry::global();
        resolved.binary_path = crate::runtimes::resolve_binary_in(
            &runtimes,
            &runtime_mgr,
            &settings,
            resolved.runtime.as_deref(),
        )?;
        let hw = detect_hardware();
        // An automatic context is estimated as Start resolves it (small-card full offload).
        harness_core::full_offload::resolve_for_start(&mut resolved, &hw);
        Ok(crate::vram::fit_launch(&resolved, &hw))
    })
    .await?
}

/// Context, `-ngl` and slot count the default load (Start with an automatic context and layer
/// count) would choose for the Dashboard's form on this machine, or `None` when the rule does
/// not apply (user cache types, not a chat server). Whether the layer count is the user's is the
/// caller's to know.
/// The same function the launch runs (`harness_core::full_offload`), so the form shows what Start
/// does.
#[tauri::command]
pub(crate) async fn automatic_launch_choice(
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    config: crate::process::LaunchConfig,
    server_id: Option<String>,
) -> Result<Option<harness_core::full_offload::AutomaticLaunch>, String> {
    let id = ServerRegistry::resolve_id(server_id)?;
    let runtime_mgr = runtime_mgr.inner().clone();
    blocking(move || {
        // The context shown in the form is not the user's: resolve as if it were automatic.
        let mut form = config;
        form.context_size = 0;
        // Settings the form filled in itself are automatic, not the user's.
        for field in form.auto_fields.clone() {
            match field.as_str() {
                "parallel_slots" => form.parallel_slots = None,
                "cache_type_k" => form.cache_type_k = None,
                "cache_type_v" => form.cache_type_v = None,
                _ => {}
            }
        }
        let mut resolved = harness_core::domain::store::Domain::global().resolve_form(
            &harness_core::domain::launch::LaunchDefaults::load(),
            &id,
            &form,
        )?;
        // Whether the layer count is the user's is the caller's to know (the form shows it).
        resolved.context_size = 0;
        resolved.gpu_layers = 999;
        let settings = crate::config::AppSettings::load();
        let runtimes = crate::runtimes::RuntimeRegistry::global();
        resolved.binary_path = crate::runtimes::resolve_binary_in(
            &runtimes,
            &runtime_mgr,
            &settings,
            resolved.runtime.as_deref(),
        )?;
        Ok(harness_core::full_offload::automatic_choice(
            &resolved,
            &detect_hardware(),
        ))
    })
    .await?
}

/// Estimate a definition load with one-time `overrides` (Playground "Load Settings"), through
/// the same launch plan the load uses (a new server; automatic values resolve as at load).
#[tauri::command]
pub(crate) async fn estimate_definition_load_fit(
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
    definition_id: String,
    overrides: harness_core::params::ParamValues,
) -> Result<crate::vram::FitResult, String> {
    let runtime_mgr = runtime_mgr.inner().clone();
    blocking(move || {
        let mut resolved = harness_core::domain::store::Domain::global().definition_form_with(
            &harness_core::domain::launch::LaunchDefaults::load(),
            &definition_id,
            &overrides,
        )?;
        let settings = crate::config::AppSettings::load();
        let runtimes = crate::runtimes::RuntimeRegistry::global();
        resolved.binary_path = crate::runtimes::resolve_binary_in(
            &runtimes,
            &runtime_mgr,
            &settings,
            resolved.runtime.as_deref(),
        )?;
        let hw = detect_hardware();
        // An automatic context is estimated as Start resolves it (small-card full offload).
        harness_core::full_offload::resolve_for_start(&mut resolved, &hw);
        Ok(crate::vram::fit_launch(&resolved, &hw))
    })
    .await?
}

/// Backend of the installed managed runtime next to the recommendation for this hardware.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BackendStatus {
    /// Backend of the managed build (from its files); `None` when none is installed.
    pub installed: Option<String>,
    pub rocm_unsupported_gpu: Option<String>,
    pub advice: crate::hardware::BackendAdvice,
    /// Backends the official releases publish for this OS/arch.
    pub official: Vec<crate::hardware::RecommendedBackend>,
}

#[tauri::command]
pub(crate) async fn get_backend_status(
    runtime_mgr: State<'_, Arc<RuntimeManager>>,
) -> Result<BackendStatus, String> {
    let mgr = runtime_mgr.inner().clone();
    blocking(move || {
        let hw = detect_hardware();
        let installed = mgr.installed_backend();
        // Device names of the installed runtime are more precise than the OS names on Linux.
        let names: Vec<String> = mgr
            .get_managed_binary_path()
            .and_then(|b| capabilities::registry().cached(&b))
            .map(|c| c.devices.iter().map(|d| d.name.clone()).collect())
            .unwrap_or_default();
        let advice =
            crate::hardware::advise_backend(&hw.os, &hw.arch, &hw.gpus, &hw.driver_info, &names);
        BackendStatus {
            installed,
            rocm_unsupported_gpu: harness_core::backend_support::rocm_unsupported_gpu(
                &hw.gpus, &names,
            ),
            advice,
            official: crate::hardware::official_backends(&hw.os, &hw.arch).to_vec(),
        }
    })
    .await
}
