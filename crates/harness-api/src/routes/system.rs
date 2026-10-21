//! Health, status, hardware, runtime.

use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use axum::Json;
use harness_core::capabilities::parse::{Device, VersionInfo};
use harness_core::capabilities::{self, RuntimeFlag};
use harness_core::config::{AppSettings, RuntimeMode};
use harness_core::hardware::{detect_hardware, SystemHardwareInfo};
use harness_core::instance::InstanceKind;
use harness_core::process::ServerStatus;
use harness_core::runtime_update::{self, ManagedInstall, ManagedRuntimeStatus};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use utoipa::{IntoParams, ToSchema};

use crate::error::{blocking, ApiResult, ErrorBody};
use crate::AppState;

#[derive(Serialize, ToSchema)]
pub struct Health {
    /// Always `ok` when the API answers.
    pub status: String,
    pub version: String,
}

#[utoipa::path(get, operation_id = "get_health", path = "/health", tag = "system", security(()),
    responses((status = 200, body = Health)))]
pub async fn health(State(s): State<AppState>) -> Json<Health> {
    Json(Health {
        status: "ok".into(),
        version: s.ctx.version.clone(),
    })
}

/// The contract versions this build speaks and its optional features (public, like
/// `/health`): API major version, supported module manifest versions, feature flags.
#[utoipa::path(get, operation_id = "get_capabilities", path = "/capabilities", tag = "system",
    security(()),
    responses((status = 200, body = harness_core::control_api::Capabilities)))]
pub async fn capabilities(
    State(s): State<AppState>,
) -> Json<harness_core::control_api::Capabilities> {
    Json(harness_core::control_api::capabilities(&s.ctx.version))
}

#[derive(Serialize, ToSchema)]
pub struct InstanceSummary {
    pub kind: InstanceKind,
    pub pid: u32,
}

#[derive(Serialize, ToSchema)]
pub struct HardwareSummary {
    pub os: String,
    pub arch: String,
    pub cpu_name: String,
    pub cpu_cores: usize,
    pub total_ram_bytes: u64,
    pub gpus: Vec<GpuSummary>,
    pub recommended_backend: String,
}

#[derive(Serialize, ToSchema)]
pub struct GpuSummary {
    pub name: String,
    pub vram_bytes: Option<u64>,
}

#[derive(Serialize, ToSchema)]
pub struct ServerCounts {
    pub total: usize,
    pub running: usize,
}

#[derive(Serialize, ToSchema)]
pub struct StatusResponse {
    pub version: String,
    /// Control API version (`v1`).
    pub api_version: String,
    pub instance: InstanceSummary,
    pub runtime: RuntimeSummary,
    pub hardware: HardwareSummary,
    pub servers: ServerCounts,
    /// The gateway (managed endpoint) of this instance.
    pub gateway: harness_core::gateway::GatewayStatus,
    /// Whether this OS / architecture has an ONNX Runtime build (ONNX embedding, rerank and
    /// chat models need one; Intel Macs have none).
    pub onnx_runtime: harness_core::onnx_runtime::PlatformSupport,
}

/// The llama-server binary the next start uses and what it can do.
#[derive(Serialize, ToSchema)]
pub struct RuntimeSummary {
    /// Configured mode: `managed` (downloaded by Harness) or `custom`.
    pub mode: String,
    /// Where the binary really comes from (`managed`, `custom`, `system` = found on PATH /
    /// WinGet...); `None` when there is none.
    pub source: Option<harness_core::orchestrator::BinarySource>,
    /// The configured mode can't be used, `source` is a fallback.
    pub fallback: bool,
    /// `None` when no llama-server is installed/configured (`error` says why).
    #[schema(value_type = Option<String>)]
    pub binary_path: Option<std::path::PathBuf>,
    pub error: Option<String>,
    pub version: Option<VersionInfo>,
    /// `--help` could be parsed.
    pub help_parsed: bool,
    pub flag_count: usize,
    pub devices: Vec<Device>,
    /// Features verified against a running server of this binary.
    pub endpoint_verified: Vec<String>,
    /// Unix seconds of the probe.
    pub probed_at: Option<u64>,
}

/// `/status` and `/runtime` reuse a device report this young (a probe spawns the binary).
const RUNTIME_DEVICES_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(60);

/// The engine of the official runtime (the one /runtime and /status describe).
fn official_engine() -> &'static dyn harness_core::engine::Engine {
    harness_core::runtimes::engine_for(None).unwrap_or(harness_core::engine::llama_cpp())
}

async fn runtime_summary(s: &AppState) -> ApiResult<RuntimeSummary> {
    let runtime = s.ctx.runtime.clone();
    blocking(move || {
        let settings = AppSettings::load();
        let mode = match settings.runtime_mode {
            RuntimeMode::Managed => "managed",
            RuntimeMode::Custom => "custom",
        }
        .to_string();
        match harness_core::orchestrator::resolve_server_binary_detail(&runtime, &settings) {
            Ok(resolved) => {
                let binary = resolved.path;
                let caps = official_engine().detect_capabilities(&binary);
                // Fresh `--list-devices` (free memory corrected with the OS usage).
                let report = harness_core::devices::report(&binary, RUNTIME_DEVICES_MAX_AGE);
                let devices = if report.devices.is_empty() {
                    caps.devices.clone()
                } else {
                    report.as_capability_devices()
                };
                RuntimeSummary {
                    mode,
                    source: Some(resolved.source),
                    fallback: resolved.fallback,
                    binary_path: Some(binary),
                    error: None,
                    version: Some(caps.version.clone()),
                    help_parsed: caps.help_parsed,
                    flag_count: caps.flags.len(),
                    devices,
                    endpoint_verified: caps
                        .endpoint_verified
                        .iter()
                        .filter(|(_, v)| v.ok)
                        .map(|(k, _)| k.clone())
                        .collect(),
                    probed_at: Some(caps.probed_at),
                }
            }
            Err(e) => RuntimeSummary {
                mode,
                source: None,
                fallback: false,
                binary_path: None,
                error: Some(e),
                version: None,
                help_parsed: false,
                flag_count: 0,
                devices: Vec::new(),
                endpoint_verified: Vec::new(),
                probed_at: None,
            },
        }
    })
    .await
}

#[utoipa::path(get, operation_id = "get_status", path = "/status", tag = "system",
    responses((status = 200, body = StatusResponse), (status = 401, body = ErrorBody)))]
pub async fn status(State(s): State<AppState>) -> ApiResult<Json<StatusResponse>> {
    let runtime = runtime_summary(&s).await?;
    let hw = blocking(detect_hardware).await?;
    let summaries = s.ctx.registry.summaries();
    Ok(Json(StatusResponse {
        version: s.ctx.version.clone(),
        api_version: "v1".into(),
        instance: InstanceSummary {
            kind: s.ctx.kind,
            pid: std::process::id(),
        },
        runtime,
        hardware: HardwareSummary {
            os: hw.os.clone(),
            arch: hw.arch.clone(),
            cpu_name: hw.cpu_name.clone(),
            cpu_cores: hw.cpu_cores,
            total_ram_bytes: hw.total_ram_bytes,
            gpus: hw
                .gpus
                .iter()
                .map(|g| GpuSummary {
                    name: g.name.clone(),
                    vram_bytes: g.vram_bytes,
                })
                .collect(),
            recommended_backend: hw.recommended_backend.as_str().to_string(),
        },
        servers: ServerCounts {
            total: summaries.len(),
            running: summaries
                .iter()
                .filter(|v| v.status == ServerStatus::Running)
                .count(),
        },
        gateway: harness_core::gateway::status(),
        onnx_runtime: harness_core::onnx_runtime::platform_support(),
    }))
}

#[utoipa::path(get, operation_id = "get_hardware", path = "/hardware", tag = "system",
    responses((status = 200, body = SystemHardwareInfo), (status = 401, body = ErrorBody)))]
pub async fn hardware() -> ApiResult<Json<SystemHardwareInfo>> {
    Ok(Json(blocking(detect_hardware).await?))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DevicesQuery {
    /// Probe again even when a report younger than 20 s exists.
    #[serde(default)]
    pub refresh: bool,
    /// Use the definition's runtime and main GPU, as gateway admission does.
    pub definition_id: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct HardwareDevicesResponse {
    #[serde(flatten)]
    pub report: harness_core::devices::DeviceReport,
    pub main_device: Option<u32>,
    pub main_adapter: Option<u32>,
}

/// GPU devices as the active runtime reports them (`llama-server --list-devices`: backend,
/// total and free memory; free corrected with the OS usage), plus a main-GPU / tensor-split
/// suggestion when there are several GPUs.
#[utoipa::path(get, operation_id = "list_hardware_devices", path = "/hardware/devices", tag = "system", params(DevicesQuery),
    responses((status = 200, body = HardwareDevicesResponse),
        (status = 409, body = ErrorBody)))]
pub async fn hardware_devices(
    State(s): State<AppState>,
    crate::error::Q(q): crate::error::Q<DevicesQuery>,
) -> ApiResult<Json<HardwareDevicesResponse>> {
    let runtime = s.ctx.runtime.clone();
    let report = blocking(move || {
        let settings = AppSettings::load();
        let config = q
            .definition_id
            .as_deref()
            .map(|id| {
                harness_core::domain::store::Domain::global().definition_form(
                    &harness_core::domain::launch::LaunchDefaults::from_settings(&settings),
                    id,
                )
            })
            .transpose()?;
        let binary = harness_core::runtimes::resolve_binary(
            &runtime,
            &settings,
            config.as_ref().and_then(|c| c.runtime.as_deref()),
        )?;
        let age = if q.refresh {
            std::time::Duration::ZERO
        } else {
            harness_core::devices::FRESH_FOR
        };
        let report = harness_core::devices::report(&binary, age);
        let adapters = harness_core::devices::os_adapters();
        let selected = harness_core::prepare::main_device(
            &report,
            &adapters,
            config.as_ref().and_then(|c| c.main_gpu),
        );
        let main_device = selected.map(|s| s.0);
        let main_adapter = selected.map(|s| s.1);
        Ok::<_, String>(HardwareDevicesResponse {
            report,
            main_device,
            main_adapter,
        })
    })
    .await?
    .map_err(|e| crate::error::ApiError::conflict("runtime.not_available").with("message", e))?;
    Ok(Json(report))
}

#[utoipa::path(get, operation_id = "get_active_runtime", path = "/runtime", tag = "system",
    responses((status = 200, body = RuntimeSummary), (status = 401, body = ErrorBody)))]
pub async fn runtime(State(s): State<AppState>) -> ApiResult<Json<RuntimeSummary>> {
    Ok(Json(runtime_summary(&s).await?))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RuntimeFlagsQuery {
    /// A server whose runtime to read (default: the default server's).
    pub server_id: Option<String>,
    /// A registered runtime id (`official`, `system` or one from `/runtimes`); wins over
    /// the server's own choice.
    pub runtime_id: Option<String>,
}

/// Every flag of a runtime's `--help`, read through the runtime's engine (its family's
/// parameter catalog marks the flags a catalog parameter covers). Without parameters: the
/// runtime the default server uses, else the official one.
#[utoipa::path(get, operation_id = "get_runtime_flags", path = "/runtime/flags", tag = "system", params(RuntimeFlagsQuery),
    responses((status = 200, body = Vec<RuntimeFlag>), (status = 409, body = ErrorBody)))]
pub async fn runtime_flags(
    State(s): State<AppState>,
    crate::error::Q(q): crate::error::Q<RuntimeFlagsQuery>,
) -> ApiResult<Json<Vec<RuntimeFlag>>> {
    let (registry, runtime) = (s.ctx.registry.clone(), s.ctx.runtime.clone());
    let flags = blocking(move || {
        let engine = harness_core::orchestrator::server_engine_for(
            &registry,
            q.server_id.clone(),
            q.runtime_id.clone(),
        )?;
        let binary = harness_core::orchestrator::server_binary_for(
            &registry,
            &runtime,
            q.server_id,
            q.runtime_id,
        )?;
        Ok::<_, String>(capabilities::runtime_flags(
            engine,
            &engine.detect_capabilities(&binary),
        ))
    })
    .await?
    .map_err(|e| crate::error::ApiError::conflict("runtime.not_available").with("message", e))?;
    Ok(Json(flags))
}

/// Public while the API listens on loopback; needs `read` on a network bind.
#[utoipa::path(get, operation_id = "get_openapi", path = "/openapi.json", tag = "system",
    responses((status = 200, description = "OpenAPI 3.1 document", content_type = "application/json")))]
pub async fn openapi_json() -> impl IntoResponse {
    static DOC: OnceLock<String> = OnceLock::new();
    let doc = DOC.get_or_init(|| {
        super::openapi_document()
            .to_pretty_json()
            .unwrap_or_else(|_| "{}".into())
    });
    ([(header::CONTENT_TYPE, "application/json")], doc.as_str())
}

/// Asks a headless owner (`harness serve`) to exit cleanly: it stops its llama-servers and
/// the API, then releases the data directory. The GUI does not accept it (409
/// `api.shutdown_unsupported`).
#[utoipa::path(post, operation_id = "shutdown_app", path = "/shutdown", tag = "system",
    responses((status = 202), (status = 403, body = ErrorBody), (status = 409, body = ErrorBody)))]
pub async fn shutdown(State(s): State<AppState>) -> ApiResult<axum::http::StatusCode> {
    let hook = s
        .shutdown
        .as_ref()
        .ok_or_else(|| crate::error::ApiError::conflict("api.shutdown_unsupported"))?;
    hook.notify_one();
    Ok(axum::http::StatusCode::ACCEPTED)
}

/// Settings > Runtime's managed llama.cpp build: installed and previous version, the backend
/// updates keep, and what "roll back" restores.
#[utoipa::path(get, operation_id = "get_managed_runtime", path = "/runtime/managed", tag = "system",
    responses((status = 200, body = ManagedRuntimeStatus), (status = 401, body = ErrorBody)))]
pub async fn managed_runtime(State(s): State<AppState>) -> ApiResult<Json<ManagedRuntimeStatus>> {
    let runtime = s.ctx.runtime.clone();
    Ok(Json(
        blocking(move || runtime_update::managed_status(&runtime)).await?,
    ))
}

#[derive(Deserialize, ToSchema, Default)]
pub struct ManagedInstallRequest {
    /// `vulkan`, `rocm`, `cuda`, `sycl`, `cpu`, `metal`: exactly this backend (a switch).
    /// Omitted: the installed build's backend (an update), else the hardware recommendation.
    pub backend: Option<String>,
    /// Install archives that have no SHA-256 digest on GitHub.
    #[serde(default)]
    pub allow_unverified: bool,
}

/// Downloads, verifies (SHA-256, `--version`, device check) and activates the newest official
/// build of the managed runtime, like Settings > Runtime. The installed build moves to the
/// backup slot. 409 `runtime.stop_servers_first` while a server runs, 409 `runtime.busy` while
/// another install runs; 422 `runtime.no_matching_asset` when no recent release has the backend.
#[utoipa::path(post, path = "/runtime/managed/install", tag = "system",
    request_body(content = Option<ManagedInstallRequest>),
    responses((status = 200, body = ManagedInstall), (status = 409, body = ErrorBody),
        (status = 422, body = ErrorBody)))]
pub async fn install_managed_runtime(
    State(s): State<AppState>,
    crate::error::OptBody(req): crate::error::OptBody<ManagedInstallRequest>,
) -> ApiResult<Json<ManagedInstall>> {
    let req = req.unwrap_or_default();
    Ok(Json(
        runtime_update::install_managed(
            &s.ctx.runtime,
            &s.ctx.registry,
            &s.ctx.gate,
            req.backend.as_deref(),
            req.allow_unverified,
        )
        .await?,
    ))
}

/// "Roll back" (Settings > Runtime): the build in the backup slot becomes the installed one;
/// the replaced build takes its place. 409 `runtime.no_backup` when the slot holds no build.
#[utoipa::path(post, path = "/runtime/managed/rollback", tag = "system",
    responses((status = 200, body = ManagedRuntimeStatus), (status = 409, body = ErrorBody)))]
pub async fn rollback_managed_runtime(
    State(s): State<AppState>,
) -> ApiResult<Json<ManagedRuntimeStatus>> {
    Ok(Json(
        runtime_update::rollback_managed(&s.ctx.runtime, &s.ctx.registry, &s.ctx.gate).await?,
    ))
}
