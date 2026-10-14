//! Glue between the services: launching servers with the resolved runtime,
//! restoring servers on start, and guards shared by several front ends.

use crate::config::{AppSettings, RuntimeMode};
use crate::error_key::err;
use crate::mcp;
use crate::operation_gate::OperationGate;
use crate::process::{self, LaunchConfig, ProcessManager, ServerStatus};
use crate::runtime::RuntimeManager;
use crate::servers::{self, ServerRegistry};
use std::path::PathBuf;
use std::sync::Arc;

/// Runs blocking work (disk scans, file copies, process probes) off the async threads.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| err("task.join_failed").with("error", e).into())
}

/// Ports a launch must not take automatically: those of the other running instances and of
/// every saved profile except `id`'s own.
async fn taken_ports(registry: &ServerRegistry, id: &str) -> Result<Vec<u16>, String> {
    let own = id.to_string();
    let mut taken: Vec<u16> = blocking(move || {
        crate::domain::store::Domain::global()
            .profiles()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.id != own && p.port != 0)
            .map(|p| p.port)
            .collect::<Vec<u16>>()
    })
    .await?;
    taken.extend(
        registry
            .active_servers()
            .into_iter()
            .filter(|(other, _)| other != id)
            .map(|(_, a)| a.port),
    );
    Ok(taken)
}

/// Settles the port of a launch: an automatic one (`port_auto` or port 0) from
/// `ports::AUTO_RANGE` — the last pick while it is free —, or the user's own, refused when
/// another server of ours uses it (the OS-level check, naming the program, follows in
/// [`launch_with_binary`]).
async fn settle_port(
    registry: &ServerRegistry,
    id: &str,
    config: &mut LaunchConfig,
) -> Result<(), String> {
    if config.wants_auto_port() {
        config.port_auto = true;
        let preferred = (config.port != 0).then_some(config.port);
        let taken = taken_ports(registry, id).await?;
        let host = config.host.clone();
        config.port = blocking(move || crate::ports::pick_auto(&host, preferred, &taken))
            .await?
            .ok_or_else(|| String::from(err("servers.no_free_port")))?;
        return Ok(());
    }
    if let Some(other) = registry.port_conflict(id, config.port) {
        return Err(err("servers.port_in_use_server")
            .num("port", config.port)
            .with("server", other)
            .into());
    }
    Ok(())
}

/// Starts `config` on server instance `id` (created on demand).
pub async fn launch(
    registry: &ServerRegistry,
    runtime_mgr: &RuntimeManager,
    gate: &OperationGate,
    id: String,
    mut config: LaunchConfig,
) -> Result<(), String> {
    let settings = AppSettings::load();
    if config.router_mode && config.models_path.is_none() {
        config.models_path = Some(settings.primary_models_dir());
    }
    // The launch's runtime (definition/profile/form choice), else Settings > Runtime. A
    // registered runtime whose files changed since it was added is refused; so is a model whose
    // tensor types the chosen build certainly can't load (instead of llama-server's
    // "invalid ggml type" after the spawn).
    // Same thread as before (quick: a JSON file, file metadata, a GGUF header; the
    // `llama-quantize --help` probe runs once per build and is cached).
    let reg = crate::runtimes::RuntimeRegistry::global();
    // The engine follows the model file (a whisper ggml file runs on a whisper.cpp runtime).
    let engine = crate::engine::server_for(&config);
    let llama = engine.family() == crate::engine::LLAMA_CPP;
    let binary = if llama {
        // A whisper runtime chosen for a GGUF model is a mismatch, not a launch failure.
        if let Ok(chosen) = crate::runtimes::engine_for_in(&reg, config.runtime.as_deref()) {
            if chosen.family() != engine.family() {
                return Err(format!(
                    "{}: {} ({}, needs {})",
                    crate::runtimes::ERR_FAMILY_MISMATCH,
                    config.runtime.as_deref().unwrap_or_default(),
                    chosen.family(),
                    engine.family()
                ));
            }
        }
        crate::runtimes::resolve_binary_in(&reg, runtime_mgr, &settings, config.runtime.as_deref())?
    } else if engine.family() == crate::engine::ONNX_FAMILY {
        // The sidecar ships with the app; the ONNX Runtime library it loads is a managed
        // download, fetched on the first launch that needs it.
        let variant =
            crate::onnx_runtime::variant_for_provider(&crate::engine::onnx_provider_of(&config));
        if crate::onnx_runtime::installed(variant).is_none() {
            crate::onnx_runtime::install(variant).await?;
        }
        crate::onnx_runtime::sidecar_binary()?
    } else if engine.family() == crate::engine::PYTHON_HF {
        // The interpreter of the managed environment (Settings > Engines installs it).
        crate::engine::python_hf::binary()?
    } else if engine.family() == crate::engine::MLX {
        crate::engine::mlx::binary()?
    } else if engine.family() == crate::engine::MLX_VLM {
        crate::engine::mlx::vlm_binary()?
    } else if engine.family() == crate::engine::MLX_AUDIO {
        crate::engine::mlx::audio_binary()?
    } else {
        crate::runtimes::resolve_family_binary_in(&reg, engine.family(), config.runtime.as_deref())?
    };
    if llama && !config.router_mode {
        crate::runtimes::check_model_support(
            &reg,
            runtime_mgr,
            &settings,
            config.runtime.as_deref(),
            &config.model_path,
        )?;
    }
    launch_with_binary(registry, gate, id, config, binary, &settings).await
}

/// [`launch`] with an already resolved llama-server binary (tests, future CLI `--binary`).
pub async fn launch_with_binary(
    registry: &ServerRegistry,
    gate: &OperationGate,
    id: String,
    mut config: LaunchConfig,
    binary: PathBuf,
    settings: &AppSettings,
) -> Result<(), String> {
    let model_target = config
        .models_path
        .clone()
        .unwrap_or_else(|| config.model_path.clone());
    let _runtime_lease = gate.runtime_read().await;
    let _model_lease = gate.model_read(&model_target).await;
    config.binary_path = binary;
    // From here on `config.api_key` is the resolved key of this launch (typed in the UI or
    // read from the keychain); it only lives in memory and in the child's environment.
    config.api_key = blocking({
        let config = config.clone();
        move || servers::resolve_api_key(&config)
    })
    .await??;
    // Bind address default, CORS and the exposure rule (network bind needs a key or an
    // explicit "open anyway"); fails before anything is saved or started.
    crate::network::prepare_launch(&mut config, settings, &id)?;
    config.mcp_config_path = if config.mcp_enabled {
        mcp::runtime_config(&id)?
    } else {
        None
    };
    settle_port(registry, &id, &mut config).await?;
    // Two running servers never serve the same model id (the gateway and the clients pick a
    // server by it): a later one sharing the id gets a distinct one for this run.
    if let Some(served) = process::served_alias(&config) {
        let taken = registry.served_aliases(&id);
        if taken.iter().any(|t| t.eq_ignore_ascii_case(&served)) {
            if process::custom_alias(&config).is_some() {
                return Err(format!("{}: {}", ERR_ALIAS_IN_USE, served));
            }
            let profile_name = if id == servers::DEFAULT_SERVER_ID {
                None
            } else {
                let pid = id.clone();
                blocking(move || {
                    crate::domain::store::Domain::global()
                        .profile(&pid)
                        .ok()
                        .map(|p| p.name)
                })
                .await?
            };
            config.alias = Some(distinct_served_alias(
                &served,
                profile_name.as_deref(),
                config.port,
                &taken,
            ));
        }
    }
    let pm = registry.get_or_create(&id);
    if matches!(
        pm.get_status(),
        ServerStatus::Stopped | ServerStatus::Error(_)
    ) {
        let (host, port) = (config.host.clone(), config.port);
        blocking(move || servers::check_port_free(&host, port)).await??;
    }
    let used = config.clone();
    let this_start = pm.start_count() + 1;
    pm.start(config).await?;
    // The Dashboard server's saved launch (what every view shows as "Saved · :port") is
    // written only once this launch really runs: a refused start (already running, runtime
    // or model missing, spawn failure) left the old process — and its old port — in place,
    // while the saved launch already named the new one.
    // `start` returns once the process is spawned; a load that then fails (TC-09: 20M tokens,
    // out of memory) must not become the saved launch either, or the next app start puts it
    // back in the form as "1 unsaved change" and the same start fails again.
    if id == servers::DEFAULT_SERVER_ID {
        let config = used.clone();
        tokio::spawn(save_when_running(pm.clone(), config, this_start));
    }
    // Library "last used" (state.db); best effort, never fails the launch.
    let _ = blocking(move || crate::library::ops::record_launch(&used)).await;
    Ok(())
}

/// Saves the Dashboard server's launch once its process is ready. A failed load, a stop or a
/// newer start leaves the saved launch as it was; a retry of this start on a new automatic
/// port (new process) still counts as this start.
async fn save_when_running(pm: Arc<ProcessManager>, mut config: LaunchConfig, start: u64) {
    loop {
        if pm.start_count() != start {
            return;
        }
        match pm.get_status() {
            ServerStatus::Running => break,
            ServerStatus::Starting => {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await
            }
            _ => return,
        }
    }
    // The port it really runs on (the retry may have moved an automatic port).
    if let Some(active) = pm.active_server() {
        config.port = active.port;
    }
    // Checked again where the file is written: a stop and a newer start may come in between.
    let _ = tokio::task::spawn_blocking(move || {
        if pm.start_count() == start && pm.get_status() == ServerStatus::Running {
            servers::save_last_session(&config);
            crate::events::publish(crate::events::CoreEvent::LastSessionSaved {
                server_id: servers::DEFAULT_SERVER_ID.to_string(),
            });
        }
    })
    .await;
}

/// Starts a saved server profile (ServerProfile -> ModelDefinition -> effective
/// LaunchConfig) on its own instance (instance id = profile id).
pub async fn launch_profile(
    registry: &ServerRegistry,
    runtime_mgr: &RuntimeManager,
    gate: &OperationGate,
    profile_id: &str,
) -> Result<(), String> {
    let id = profile_id.to_string();
    let config = blocking(move || {
        crate::domain::store::Domain::global().resolve_profile(
            &crate::domain::launch::LaunchDefaults::load(),
            &id,
            &Default::default(),
        )
    })
    .await??;
    launch(registry, runtime_mgr, gate, profile_id.to_string(), config).await
}

/// Starts a launch form (Dashboard): its definition with the form's differences as a
/// transient layer (nothing is saved; see `Domain::resolve_form`).
pub async fn launch_form(
    registry: &ServerRegistry,
    runtime_mgr: &RuntimeManager,
    gate: &OperationGate,
    id: String,
    config: LaunchConfig,
) -> Result<(), String> {
    let resolved = blocking({
        let id = id.clone();
        move || {
            crate::domain::store::Domain::global().resolve_form(
                &crate::domain::launch::LaunchDefaults::load(),
                &id,
                &config,
            )
        }
    })
    .await??;
    launch(registry, runtime_mgr, gate, id, resolved).await
}

/// Where [`load_definition`] should run a definition.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoadTarget {
    /// An existing instance (restarted with the definition when it runs something else).
    Server { server_id: String },
    /// A saved profile serving this definition (its own settings).
    Profile { profile_id: String },
    /// A new instance on the next free port.
    NewServer,
}

/// Served model id for a launch whose id `alias` another running server already serves:
/// `<alias>-<profile>` when the profile's name says more than the model (the
/// `<alias>-<profile name>` rule, see `profile_alias_base`), else `<alias>-<port>` (auto
/// names like `<model> :8083`, the Dashboard, `m-<alias>` instances). Deterministic for the same
/// profile and port; `-2`, `-3`... only if that is taken too.
pub fn distinct_served_alias(
    alias: &str,
    profile_name: Option<&str>,
    port: u16,
    taken: &[String],
) -> String {
    let named = profile_name
        .map(|n| crate::domain::store::profile_alias_base(alias, n))
        .filter(|b| !b.eq_ignore_ascii_case(alias));
    let base = named.unwrap_or_else(|| format!("{}-{}", alias, port));
    crate::domain::unique_alias(
        &base,
        taken
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(alias)),
    )
}

/// Instance id for a new server running `alias` (`m-<alias>`, unique among `taken`).
pub fn new_server_id(alias: &str, taken: &[String]) -> String {
    let mut base: String = alias
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    base = base.trim_matches('-').chars().take(48).collect();
    if base.is_empty() {
        base = "model".into();
    }
    let base = format!("m-{}", base);
    let free = |c: &str| !taken.iter().any(|t| t == c);
    if free(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{}-{}", base, n))
        .find(|c| free(c))
        .expect("unbounded")
}

/// What [`load_definition`] starts; planned first so a start refused by the network rules
/// can be replayed after "open anyway".
#[derive(Debug, Clone)]
pub enum LoadPlan {
    /// Start this saved profile (it serves the definition).
    Profile(String),
    /// Start `config` (the definition with its own settings) on instance `server_id`.
    Config {
        server_id: String,
        config: Box<LaunchConfig>,
    },
}

impl LoadPlan {
    /// Instance the plan starts.
    pub fn server_id(&self) -> &str {
        match self {
            LoadPlan::Profile(id) => id,
            LoadPlan::Config { server_id, .. } => server_id,
        }
    }
}

/// Runs a plan: stops whatever the instance runs, then starts it. Returns the instance id.
pub async fn run_load_plan(
    registry: &ServerRegistry,
    runtime_mgr: &RuntimeManager,
    gate: &OperationGate,
    plan: LoadPlan,
) -> Result<String, String> {
    let id = plan.server_id().to_string();
    if let LoadPlan::Config { config, .. } = &plan {
        check_unified_fit(config).await?;
    }
    stop_if_active(registry, &id).await?;
    match plan {
        LoadPlan::Profile(profile_id) => {
            launch_profile(registry, runtime_mgr, gate, &profile_id).await?
        }
        LoadPlan::Config { server_id, config } => {
            launch(registry, runtime_mgr, gate, server_id, *config).await?
        }
    }
    Ok(id)
}

/// macOS (unified memory): refuses a GPU load whose estimate exceeds the GPU share of the
/// unified memory before anything is stopped or spawned. Metal allocates lazily, so such a
/// load often reports ready and only runs out of memory on the first request. Elsewhere and
/// for CPU-only, router, whisper or `--fit` loads: nothing to check.
async fn check_unified_fit(config: &LaunchConfig) -> Result<(), String> {
    if !cfg!(target_os = "macos")
        || config.gpu_layers == 0
        || config.router_mode
        || process::fit_chooses_layers(config)
        || crate::engine::server_for(config).family() != crate::engine::LLAMA_CPP
        || !config.model_path.is_file()
    {
        return Ok(());
    }
    let config = config.clone();
    let fit =
        blocking(move || crate::vram::fit_launch(&config, &crate::hardware::detect_hardware()))
            .await?;
    if fit.budget.unified && fit.estimate.vram_bytes > fit.budget.vram_bytes {
        const MIB: u64 = 1024 * 1024;
        return Err(err("servers.insufficient_memory")
            .num("need_mib", fit.estimate.vram_bytes.div_ceil(MIB))
            .num("budget_mib", fit.budget.vram_bytes / MIB)
            .into());
    }
    Ok(())
}

/// Loads a definition on an existing server, a profile or a new server.
pub async fn load_definition(
    registry: &ServerRegistry,
    runtime_mgr: &RuntimeManager,
    gate: &OperationGate,
    definition_id: &str,
    target: LoadTarget,
) -> Result<String, String> {
    let plan = plan_load_definition(registry, definition_id, target).await?;
    run_load_plan(registry, runtime_mgr, gate, plan).await
}

/// Resolves where and with which settings a definition is loaded: its own settings
/// (definition `load` plus the Dashboard's serve settings for the file) or a profile.
pub async fn plan_load_definition(
    registry: &ServerRegistry,
    definition_id: &str,
    target: LoadTarget,
) -> Result<LoadPlan, String> {
    plan_load_definition_with(registry, definition_id, target, &Default::default()).await
}

/// [`plan_load_definition`] with one-time load `overrides` (catalog ids -> values, see
/// `domain::launch`) layered over the definition for this load only; the definition itself is
/// not changed. A profile target keeps the profile's own settings (overrides do not apply).
pub async fn plan_load_definition_with(
    registry: &ServerRegistry,
    definition_id: &str,
    target: LoadTarget,
    overrides: &crate::params::ParamValues,
) -> Result<LoadPlan, String> {
    use crate::domain::launch::LaunchDefaults;
    use crate::domain::store::Domain;
    let def_id = definition_id.to_string();
    let overrides = overrides.clone();
    let (def, profiles, mut config) = blocking(move || {
        let domain = Domain::global();
        let def = domain.definition(&def_id)?;
        let config = domain.definition_form_with(&LaunchDefaults::load(), &def_id, &overrides)?;
        Ok::<_, String>((def, domain.profiles()?, config))
    })
    .await??;
    let id = match target {
        LoadTarget::Profile { profile_id } => {
            let p = profiles
                .iter()
                .find(|p| p.id == profile_id && p.id != servers::DEFAULT_SERVER_ID)
                .ok_or_else(|| {
                    String::from(err("servers.profile_not_found").with("id", &profile_id))
                })?;
            if p.definition_id.as_deref() != Some(def.id.as_str()) {
                return Err(err("servers.profile_other_model")
                    .with("profile", &p.name)
                    .with("alias", &def.alias)
                    .into());
            }
            return Ok(LoadPlan::Profile(profile_id));
        }
        LoadTarget::Server { server_id } => {
            servers::validate_id(&server_id)?;
            if profiles
                .iter()
                .any(|p| p.id == server_id && p.id != servers::DEFAULT_SERVER_ID)
            {
                return Err(err("servers.owned_by_profile")
                    .with("id", &server_id)
                    .into());
            }
            // Keep the instance's address: its last launch, the Dashboard session, else the
            // app defaults already in `config`.
            let last = registry.get(&server_id).and_then(|pm| pm.launch_config());
            let session = profiles.iter().find(|p| {
                p.id == servers::DEFAULT_SERVER_ID && server_id == servers::DEFAULT_SERVER_ID
            });
            if let Some(c) = &last {
                config.host = c.host.clone();
                config.port = c.port;
                config.port_auto = c.port_auto;
            } else if let Some(s) = session {
                config.host = s.host.clone();
                config.port = s.port;
                config.port_auto = s.port_auto;
            }
            server_id
        }
        LoadTarget::NewServer => {
            let mut taken: Vec<String> = registry.all().into_iter().map(|(id, _)| id).collect();
            taken.extend(profiles.iter().map(|p| p.id.clone()));
            let id = new_server_id(&def.alias, &taken);
            // A new server never needs a port from the user: the launch picks a free one
            // from Harness's range (`ports::AUTO_RANGE`).
            config.port = 0;
            config.port_auto = true;
            id
        }
    };
    Ok(LoadPlan::Config {
        server_id: id,
        config: Box::new(config),
    })
}

async fn stop_if_active(registry: &ServerRegistry, id: &str) -> Result<(), String> {
    if let Some(pm) = registry.get(id) {
        if matches!(
            pm.get_status(),
            ServerStatus::Starting | ServerStatus::Running
        ) {
            pm.stop().await?;
        }
    }
    Ok(())
}

/// Run state of every definition (Playground model picker).
pub fn definition_states(
    registry: &ServerRegistry,
) -> Result<Vec<crate::domain::status::DefinitionState>, String> {
    let defs = crate::domain::store::Domain::global().definitions()?;
    let servers: Vec<crate::domain::status::ServerView> = registry
        .all()
        .into_iter()
        .map(|(id, pm)| (id, pm.get_status(), pm.launch_config()))
        .collect();
    Ok(crate::domain::status::definition_states(&defs, &servers))
}

/// On app start: relaunch auto-start profiles and (optionally) the last Dashboard session.
/// Failures land in each server's status/log so the UI shows them.
pub async fn restore_servers(
    registry: Arc<ServerRegistry>,
    runtime_mgr: Arc<RuntimeManager>,
    gate: Arc<OperationGate>,
) {
    let restore_session = AppSettings::load().startup.restore_last_session;
    let jobs = blocking(move || {
        let domain = crate::domain::store::Domain::global();
        let defaults = crate::domain::launch::LaunchDefaults::load();
        let mut jobs: Vec<(String, Result<LaunchConfig, String>)> = domain
            .profiles()
            .unwrap_or_else(|e| {
                eprintln!("[Harness Studio] Sunucu profilleri okunamadı: {}", e);
                Vec::new()
            })
            .into_iter()
            .filter(|p| p.auto_start && p.id != servers::DEFAULT_SERVER_ID)
            .map(|p| {
                let config = domain.resolve_profile(&defaults, &p.id, &Default::default());
                (p.id, config)
            })
            .collect();
        if restore_session {
            if let Some(config) = servers::load_last_session() {
                jobs.insert(0, (servers::DEFAULT_SERVER_ID.to_string(), Ok(config)));
            }
        }
        jobs
    })
    .await
    .unwrap_or_default();
    for (id, config) in jobs {
        let result = match config {
            Ok(config) => launch(&registry, &runtime_mgr, &gate, id.clone(), config).await,
            Err(e) => Err(e),
        };
        if let Err(e) = result {
            let pm = registry.get_or_create(&id);
            if !matches!(pm.get_status(), ServerStatus::Error(_)) {
                pm.set_error(err("servers.autostart_failed").with("error", e).into());
            }
        }
    }
}

/// Looks up a server instance (`None` = the default server).
pub fn server(
    registry: &ServerRegistry,
    server_id: Option<String>,
) -> Result<Arc<process::ProcessManager>, String> {
    let id = ServerRegistry::resolve_id(server_id)?;
    registry
        .get(&id)
        .ok_or_else(|| err("servers.not_found").with("id", id).into())
}

pub fn ensure_server_stopped(registry: &ServerRegistry) -> Result<(), String> {
    if registry.any_active() {
        Err(err("runtime.stop_servers_first").into())
    } else {
        Ok(())
    }
}

/// Refuses file operations on the model the running server has loaded.
pub fn ensure_model_not_in_use(registry: &ServerRegistry, model_id: &str) -> Result<(), String> {
    if registry.model_in_use(model_id) {
        Err(err("servers.model_loaded").into())
    } else {
        Ok(())
    }
}

/// Binary a server uses right now (while starting/running), else the one it would use: an
/// explicit runtime choice (the form's selector), else the saved profile's runtime, else
/// Settings > Runtime.
pub fn server_binary(
    registry: &ServerRegistry,
    runtime_mgr: &RuntimeManager,
    server_id: Option<String>,
) -> Result<PathBuf, String> {
    server_binary_for(registry, runtime_mgr, server_id, None)
}

/// [`server_binary`] with the runtime the form currently selects (`Some("official")` = the
/// official one even when the profile names another).
pub fn server_binary_for(
    registry: &ServerRegistry,
    runtime_mgr: &RuntimeManager,
    server_id: Option<String>,
    runtime: Option<String>,
) -> Result<PathBuf, String> {
    let settings = AppSettings::load();
    match server_runtime_choice(registry, server_id, runtime, &settings)? {
        RuntimeChoice::Running(config) => Ok(config.binary_path),
        RuntimeChoice::Id(id) => {
            crate::runtimes::resolve_binary(runtime_mgr, &settings, id.as_deref())
        }
    }
}

/// The engine of the runtime [`server_binary_for`] picks: the family of a registered
/// runtime, llama.cpp for the official and system ones.
pub fn server_engine_for(
    registry: &ServerRegistry,
    server_id: Option<String>,
    runtime: Option<String>,
) -> Result<&'static dyn crate::engine::Engine, String> {
    let settings = AppSettings::load();
    let id = match server_runtime_choice(registry, server_id, runtime, &settings)? {
        RuntimeChoice::Running(config) => config.runtime,
        RuntimeChoice::Id(id) => id,
    };
    crate::runtimes::engine_for(id.as_deref())
}

enum RuntimeChoice {
    /// The server is starting or running with this launch.
    Running(Box<crate::process::LaunchConfig>),
    /// A runtime id (`None`: the official one).
    Id(Option<String>),
}

/// An explicit runtime, else the running launch's, else the saved profile's.
fn server_runtime_choice(
    registry: &ServerRegistry,
    server_id: Option<String>,
    runtime: Option<String>,
    settings: &AppSettings,
) -> Result<RuntimeChoice, String> {
    let id = ServerRegistry::resolve_id(server_id)?;
    if let Some(runtime) = runtime.filter(|r| !r.trim().is_empty()) {
        return Ok(RuntimeChoice::Id(Some(runtime)));
    }
    if let Some(pm) = registry.get(&id) {
        if matches!(
            pm.get_status(),
            ServerStatus::Starting | ServerStatus::Running
        ) {
            if let Some(config) = pm.launch_config().filter(|c| c.binary_path.is_file()) {
                return Ok(RuntimeChoice::Running(Box::new(config)));
            }
        }
    }
    let saved = crate::domain::store::Domain::global()
        .resolve_profile(
            &crate::domain::launch::LaunchDefaults::from_settings(settings),
            &id,
            &Default::default(),
        )
        .ok()
        .and_then(|c| c.runtime);
    Ok(RuntimeChoice::Id(saved))
}

/// Where the official runtime's binary comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum BinarySource {
    /// Downloaded and activated by Harness (`<data>/bin/managed`).
    Managed,
    /// The binary picked in Settings > Runtime (custom mode).
    Custom,
    /// A llama-server found on PATH or in a known install folder (WinGet, scoop...).
    System,
}

/// The official runtime's binary and how it was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedBinary {
    pub path: PathBuf,
    pub source: BinarySource,
    /// The configured mode couldn't be used (managed build not installed / custom binary
    /// invalid), so `source` is a fallback. The UI says so instead of claiming the mode.
    pub fallback: bool,
}

/// Resolves the llama-server binary on the backend from the configured runtime
/// mode. The frontend never gets to choose which executable is launched.
/// Implements smart fallback: if the active mode is missing/invalid, attempts
/// to resolve from managed or system PATH before failing.
pub fn resolve_server_binary(
    runtime_mgr: &RuntimeManager,
    settings: &AppSettings,
) -> Result<PathBuf, String> {
    resolve_server_binary_detail(runtime_mgr, settings).map(|r| r.path)
}

/// [`resolve_server_binary`] with the source of the binary and whether it is a fallback.
pub fn resolve_server_binary_detail(
    runtime_mgr: &RuntimeManager,
    settings: &AppSettings,
) -> Result<ResolvedBinary, String> {
    let found = |path: PathBuf, source: BinarySource, fallback: bool| {
        Ok(ResolvedBinary {
            path,
            source,
            fallback,
        })
    };
    match settings.runtime_mode {
        RuntimeMode::Managed => {
            if let Some(path) = runtime_mgr.get_managed_binary_path() {
                found(path, BinarySource::Managed, false)
            } else if let Some(path) = RuntimeManager::detect_system_binary_cached() {
                found(path, BinarySource::System, true)
            } else {
                Err(err("runtime.not_installed").into())
            }
        }
        RuntimeMode::Custom => {
            if let Some(ref path) = settings.custom_binary.binary_path {
                if settings.custom_binary.is_valid && path.is_file() {
                    return found(path.clone(), BinarySource::Custom, false);
                }
            }
            if let Some(path) = runtime_mgr.get_managed_binary_path() {
                found(path, BinarySource::Managed, true)
            } else if let Some(path) = RuntimeManager::detect_system_binary_cached() {
                found(path, BinarySource::System, true)
            } else {
                Err(err("runtime.custom_invalid").into())
            }
        }
    }
}

// --- Shared by the GUI commands and the Control API ---

/// What references a registered runtime.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RuntimeUsers {
    /// Servers starting/running with it (always blocks removal).
    pub running: Vec<String>,
    /// Aliases of model definitions that choose it.
    pub definitions: Vec<String>,
    /// Names of server profiles that choose it (override).
    pub profiles: Vec<String>,
}

impl RuntimeUsers {
    pub fn is_empty(&self) -> bool {
        self.running.is_empty() && self.definitions.is_empty() && self.profiles.is_empty()
    }
}

/// Servers, definitions and profiles that use runtime `id`. Blocking.
pub fn runtime_users(registry: &ServerRegistry, id: &str) -> Result<RuntimeUsers, String> {
    use crate::domain::launch::RUNTIME;
    use crate::params::ParamValue;
    let names = |v: Option<&ParamValue>| matches!(v, Some(ParamValue::Text(t)) if t.trim() == id);
    let domain = crate::domain::store::Domain::global();
    let mut users = RuntimeUsers {
        running: registry
            .all()
            .into_iter()
            .filter(|(_, pm)| {
                matches!(
                    pm.get_status(),
                    ServerStatus::Starting | ServerStatus::Running
                ) && pm
                    .launch_config()
                    .is_some_and(|c| c.runtime.as_deref() == Some(id))
            })
            .map(|(sid, _)| sid)
            .collect(),
        ..Default::default()
    };
    for d in domain.definitions()? {
        if names(d.load.get(RUNTIME)) {
            users.definitions.push(d.alias);
        }
    }
    for p in domain.profiles()? {
        if names(p.serve_overrides.get(RUNTIME)) {
            users.profiles.push(p.name);
        }
    }
    Ok(users)
}

/// Unregisters a runtime (see `runtimes::RuntimeRegistry::remove`). Refused while a server
/// runs it; refused while definitions/profiles choose it unless `force` (their launches then
/// fail with `runtime.not_found` until another runtime is picked). Holds the runtime lease.
pub async fn remove_runtime(
    registry: &ServerRegistry,
    gate: &OperationGate,
    id: String,
    force: bool,
) -> Result<crate::runtimes::RuntimeEntry, String> {
    let _lease = gate.runtime_write().await;
    let users = runtime_users(registry, &id)?;
    if !users.running.is_empty() || (!force && !users.is_empty()) {
        let mut who: Vec<String> = users.running.clone();
        who.extend(users.definitions.iter().cloned());
        who.extend(users.profiles.iter().cloned());
        return Err(format!(
            "{}: {}",
            crate::runtimes::ERR_IN_USE,
            who.join(", ")
        ));
    }
    blocking(move || crate::runtimes::RuntimeRegistry::global().remove(&id)).await?
}

/// Model is not in the library (scan).
pub const ERR_MODEL_NOT_FOUND: &str = "models.not_found";
/// A server uses one of the model's files.
pub const ERR_MODEL_IN_USE: &str = "models.in_use";
/// No model definition has this alias.
pub const ERR_ALIAS_NOT_FOUND: &str = "definitions.alias_not_found";

/// Deletes a library model (all parts; see `library::ops::delete_model`). Refused while any
/// server uses one of its files (`models.in_use: <servers>`), for linked models and for files
/// outside the library directories. Holds the model's write lease.
pub async fn delete_library_model(
    registry: &ServerRegistry,
    gate: &OperationGate,
    model_id: String,
    delete_definitions: bool,
) -> Result<crate::library::ops::DeleteReport, String> {
    let _model_lease = gate.model_write(std::path::Path::new(&model_id)).await;
    let model = find_model(model_id).await?;
    let running = crate::library::ops::servers_using(registry, &model.shard_paths);
    if !running.is_empty() {
        return Err(format!("{}: {}", ERR_MODEL_IN_USE, running.join(", ")));
    }
    blocking(move || {
        let db = crate::state::shared().ok();
        crate::library::ops::delete_model(
            &model,
            &AppSettings::load().all_models_dirs(),
            &crate::domain::store::Domain::global(),
            delete_definitions,
            db.as_deref(),
        )
    })
    .await?
}

/// A library model by id (= launch path), from a fresh scan.
pub async fn find_model(model_id: String) -> Result<crate::models::LocalModel, String> {
    blocking(move || {
        crate::models::ModelManager
            .find_model(&model_id)
            .ok_or_else(|| ERR_MODEL_NOT_FOUND.to_string())
    })
    .await?
}

/// Starts a llama-bench sweep with the `llama-bench` next to the configured llama-server.
/// Returns the run id.
pub async fn start_benchmark(
    bench: &crate::benchmark::BenchmarkManager,
    runtime_mgr: &RuntimeManager,
    request: crate::benchmark::BenchmarkRequest,
) -> Result<String, String> {
    let server_bin = resolve_server_binary(runtime_mgr, &AppSettings::load())?;
    let bench_bin = crate::benchmark::bench_binary_for(&server_bin)?;
    let bench = bench.clone();
    blocking(move || bench.start(bench_bin, request)).await?
}

/// The id of the definition with this alias (case-insensitive), or the id itself when it
/// names a definition.
pub fn resolve_definition(alias_or_id: &str) -> Result<String, String> {
    let defs = crate::domain::store::Domain::global().definitions()?;
    defs.iter()
        .find(|d| d.id == alias_or_id)
        .or_else(|| {
            defs.iter()
                .find(|d| d.alias.eq_ignore_ascii_case(alias_or_id))
        })
        .map(|d| d.id.clone())
        .ok_or_else(|| ERR_ALIAS_NOT_FOUND.to_string())
}

/// Stops every running (or starting) server that serves this definition. Returns their ids.
pub async fn unload_definition(
    registry: &ServerRegistry,
    definition_id: &str,
) -> Result<Vec<String>, String> {
    let states = definition_states(registry)?;
    let ids: Vec<String> = states
        .into_iter()
        .filter(|s| s.definition_id == definition_id)
        .flat_map(|s| s.servers.into_iter().map(|v| v.server_id))
        .collect();
    for id in &ids {
        stop_if_active(registry, id).await?;
    }
    Ok(ids)
}

/// What the views of a front end need from the telemetry sampler (desktop app: set by the
/// window, all false while it is hidden).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SamplerWatch {
    /// A visible view shows a server's live telemetry: sample more often than nobody looking,
    /// every second while a server works, every [`SAMPLE_LIVE_IDLE`] while all are idle.
    pub live: bool,
    /// A visible view shows system RAM/VRAM: sample it (every 2 s) even with no server.
    pub memory: bool,
}

/// Sampling period while a server works on a request (tokens per second moves every second).
const SAMPLE_BUSY: std::time::Duration = std::time::Duration::from_secs(1);
/// Sampling period while a view shows a server live but nothing works: the numbers do not
/// move, so a slower tick costs the llama-server less (`/metrics` + `/slots` every round). A
/// request is noticed within this time, then sampling goes back to every second.
const SAMPLE_LIVE_IDLE: std::time::Duration = std::time::Duration::from_secs(3);
/// Sampling period of running servers with nothing to do and nobody looking. A request is
/// noticed within this time (then every second); gateway requests also trigger a sample
/// right when they finish, so request analytics keep their memory peak.
const SAMPLE_IDLE: std::time::Duration = std::time::Duration::from_secs(5);
/// System memory only (no server runs), for a view's meters.
const SAMPLE_MEMORY: std::time::Duration = std::time::Duration::from_secs(2);

/// Background telemetry sampler: polls `/metrics` + `/slots` of every active server so the
/// history is complete whichever front end looks: every second while a server is busy, every
/// 3 s while all are idle and a view shows them live, every 5 s with nobody looking. `on_count` is called with the number of active servers whenever it
/// changes (tray tooltip). Runs until the task is dropped.
///
/// While no server is active it does nothing at all: it sleeps until one starts. Front ends
/// that read system memory then get it on demand ([`TelemetryPoller::fresh_snapshot`]).
///
/// [`TelemetryPoller::fresh_snapshot`]: crate::telemetry::TelemetryPoller::fresh_snapshot
pub async fn run_telemetry_sampler(
    poller: Arc<tokio::sync::Mutex<crate::telemetry::TelemetryPoller>>,
    registry: Arc<ServerRegistry>,
    on_count: impl FnMut(usize) + Send,
) {
    let (_keep, watch) = tokio::sync::watch::channel(SamplerWatch::default());
    run_telemetry_sampler_watched(poller, registry, watch, on_count, |_| {}).await;
}

/// [`run_telemetry_sampler`] for a front end with live views (see [`SamplerWatch`]);
/// `on_round` runs after every round with the updated poller (the desktop app pushes changed
/// telemetry to its window).
pub async fn run_telemetry_sampler_watched(
    poller: Arc<tokio::sync::Mutex<crate::telemetry::TelemetryPoller>>,
    registry: Arc<ServerRegistry>,
    mut watch: tokio::sync::watch::Receiver<SamplerWatch>,
    mut on_count: impl FnMut(usize) + Send,
    mut on_round: impl FnMut(&crate::telemetry::TelemetryPoller) + Send,
) {
    use tokio::sync::broadcast::error::RecvError;
    use tokio::time::Instant;
    let mut servers_rx = crate::changes::subscribe_servers();
    let mut events = crate::events::subscribe();
    let mut last_count = usize::MAX;
    let mut last_round: Option<Instant> = None;
    let mut busy = false;
    // A closed watch channel never wakes again (instead of waking in a loop).
    let mut watch_open = true;
    loop {
        servers_rx.borrow_and_update();
        let servers = registry.active_servers();
        if servers.len() != last_count {
            last_count = servers.len();
            on_count(last_count);
        }
        let want = *watch.borrow_and_update();
        let stale_live = servers.is_empty() && poller.lock().await.any_live();
        let period = if !servers.is_empty() {
            if busy {
                Some(SAMPLE_BUSY)
            } else if want.live {
                Some(SAMPLE_LIVE_IDLE)
            } else {
                Some(SAMPLE_IDLE)
            }
        } else if want.memory {
            Some(SAMPLE_MEMORY)
        } else if stale_live {
            Some(std::time::Duration::ZERO) // one round clears the stopped server's values
        } else {
            None
        };
        let due = match (period, last_round) {
            (Some(p), Some(at)) => Some(at + p),
            (Some(_), None) => Some(Instant::now()),
            (None, _) => None,
        };
        // Wait for the next round, or for something that changes the plan: a server started
        // or stopped, a view opened or closed, a gateway request finished (sample right away,
        // its analytics record wants the memory of that moment).
        let sample_now = tokio::select! {
            _ = async {
                match due {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            } => true,
            _ = servers_rx.changed() => false,
            r = async {
                if watch_open { watch.changed().await } else { std::future::pending().await }
            } => {
                watch_open = r.is_ok();
                false
            }
            e = events.recv(), if !servers.is_empty() => match e {
                Ok(crate::events::CoreEvent::GatewayRequest { .. }) | Err(RecvError::Lagged(_)) => true,
                Ok(_) => false,
                Err(RecvError::Closed) => false,
            },
        };
        if !sample_now {
            continue;
        }
        let (client, probe) = poller.lock().await.round_context();
        let processes = registry.server_processes();
        let labels: std::collections::HashMap<String, String> = servers
            .iter()
            .map(|(id, info)| {
                let name = info.model_path.file_stem().unwrap_or_default();
                (id.clone(), name.to_string_lossy().into_owned())
            })
            .collect();
        crate::analytics::direct::register_servers(&servers);
        let round =
            crate::telemetry::TelemetryPoller::gather(client, probe, servers, processes).await;
        last_round = Some(Instant::now());
        let mut p = poller.lock().await;
        p.apply_round(round);
        // Servers' own counters minus what the gateway recorded = use that bypasses it.
        {
            let raw = p.raw_metrics();
            let samples: Vec<crate::analytics::direct::Sample<'_>> = raw
                .iter()
                .map(|(id, metrics)| crate::analytics::direct::Sample {
                    server_id: id,
                    label: labels.get(id).map(String::as_str).unwrap_or_default(),
                    metrics,
                })
                .collect();
            let active: Vec<String> = labels.keys().cloned().collect();
            crate::analytics::direct::observe(&samples, &active);
        }
        busy = p.any_busy();
        on_round(&p);
    }
}

/// The instance is starting or running already.
pub const ERR_ALREADY_ACTIVE: &str = "servers.already_active";
/// The port is taken (another program, or another server of ours). Every variant starts with
/// this key: `servers.port_in_use_by {port, program, pid}` (the program holding it is known),
/// `servers.port_in_use_at {port}`, `servers.port_in_use_server {port, server}`.
pub const ERR_PORT_IN_USE: &str = "servers.port_in_use";
/// A custom `--alias` another running server already serves: `servers.alias_in_use: <alias>`.
pub const ERR_ALIAS_IN_USE: &str = "servers.alias_in_use";
/// The server has no failure to recover from (it never failed, or it runs).
pub const ERR_NO_FAILURE: &str = "servers.no_failure";
/// Nothing to start: no definition given and the instance has never run.
pub const ERR_NOTHING_TO_START: &str = "servers.definition_required";
/// The profile serves another definition.
pub const ERR_PROFILE_DEFINITION: &str = "servers.profile_definition_mismatch";

/// What `start` of an instance runs (Control API `POST /servers/{id}/start`, CLI):
///
/// * a saved profile: its own settings (`overrides` as a transient layer); a `definition_id`
///   must be the profile's;
/// * another instance with `definition_id`: that definition with the instance's address
///   (see [`plan_load_definition`]), `overrides` layered over the definition;
/// * another instance without one: its last launch (the Dashboard session for `default`).
pub async fn plan_start(
    registry: &ServerRegistry,
    server_id: &str,
    definition_id: Option<String>,
    overrides: crate::params::ParamValues,
) -> Result<LoadPlan, String> {
    use crate::domain::launch::LaunchDefaults;
    use crate::domain::store::Domain;
    servers::validate_id(server_id)?;
    let id = server_id.to_string();
    let profile = blocking({
        let id = id.clone();
        move || {
            Domain::global().profiles().map(|p| {
                p.into_iter()
                    .find(|p| p.id == id && p.id != servers::DEFAULT_SERVER_ID)
            })
        }
    })
    .await??;
    if let Some(profile) = profile {
        if definition_id
            .as_deref()
            .is_some_and(|d| profile.definition_id.as_deref() != Some(d))
        {
            return Err(ERR_PROFILE_DEFINITION.to_string());
        }
        if overrides.is_empty() {
            return Ok(LoadPlan::Profile(id));
        }
        let config = blocking(move || {
            Domain::global().resolve_profile(&LaunchDefaults::load(), &id, &overrides)
        })
        .await??;
        return Ok(LoadPlan::Config {
            server_id: server_id.to_string(),
            config: Box::new(config),
        });
    }
    match definition_id {
        Some(def) => {
            let plan =
                plan_load_definition(registry, &def, LoadTarget::Server { server_id: id }).await?;
            if overrides.is_empty() {
                return Ok(plan);
            }
            let LoadPlan::Config { server_id, config } = plan else {
                return Ok(plan);
            };
            let (host, port, port_auto) = (config.host.clone(), config.port, config.port_auto);
            let mut layered = blocking(move || {
                Domain::global().resolve_definition(&LaunchDefaults::load(), &def, &overrides)
            })
            .await??;
            layered.host = host;
            layered.port = port;
            layered.port_auto = port_auto;
            Ok(LoadPlan::Config {
                server_id,
                config: Box::new(layered),
            })
        }
        None => {
            let last = registry.get(&id).and_then(|pm| pm.launch_config());
            let last = match last {
                Some(c) => Some(c),
                None if id == servers::DEFAULT_SERVER_ID => {
                    blocking(servers::load_last_session).await?
                }
                None => None,
            };
            let mut config = last.ok_or_else(|| ERR_NOTHING_TO_START.to_string())?;
            // Overrides go over the last launch (a retry with other values).
            crate::domain::launch::apply_layer(&mut config, &overrides)?;
            Ok(LoadPlan::Config {
                server_id: id,
                config: Box::new(config),
            })
        }
    }
}

/// Starts a plan on an instance that is not active (see [`plan_start`]).
pub async fn start_plan(
    registry: &ServerRegistry,
    runtime_mgr: &RuntimeManager,
    gate: &OperationGate,
    plan: LoadPlan,
) -> Result<String, String> {
    if let Some(pm) = registry.get(plan.server_id()) {
        if matches!(
            pm.get_status(),
            ServerStatus::Starting | ServerStatus::Running | ServerStatus::Stopping
        ) {
            return Err(ERR_ALREADY_ACTIVE.to_string());
        }
    }
    run_load_plan(registry, runtime_mgr, gate, plan).await
}

// --- failure recovery ---

/// The last failure of a server and, for out-of-memory, the ways to retry
/// (`recovery::oom_options`). `servers.no_failure` when the server has none.
pub async fn recovery_plan(
    registry: &ServerRegistry,
    server_id: &str,
) -> Result<crate::recovery::RecoveryPlan, String> {
    use crate::recovery::{OtherServer, RecoveryInput, RecoveryPlan};
    servers::validate_id(server_id)?;
    let pm = registry
        .get(server_id)
        .ok_or_else(|| String::from(err("servers.not_found").with("id", server_id)))?;
    let failure = pm
        .last_failure()
        .filter(|_| matches!(pm.get_status(), ServerStatus::Error(_)))
        .ok_or_else(|| ERR_NO_FAILURE.to_string())?;
    let Some(config) = pm.launch_config() else {
        return Ok(RecoveryPlan {
            server_id: server_id.to_string(),
            failure,
            options: Vec::new(),
            free_vram_bytes: None,
            estimated_need_bytes: None,
        });
    };
    let others: Vec<OtherServer> = registry
        .all()
        .into_iter()
        .filter(|(id, pm)| {
            id != server_id
                && matches!(
                    pm.get_status(),
                    ServerStatus::Starting | ServerStatus::Running
                )
        })
        .filter_map(|(id, pm)| {
            let c = pm.launch_config()?;
            let label = c
                .alias
                .clone()
                .or_else(|| process::model_alias(&c.model_path))
                .unwrap_or_else(|| id.clone());
            Some(OtherServer {
                id,
                label,
                gpu_layers: c.gpu_layers,
            })
        })
        .collect();
    blocking(move || {
        let meta = (!config.router_mode)
            .then(|| crate::models::cached_metadata(&config.model_path))
            .flatten();
        let model_bytes = crate::recovery::model_bytes(&config.model_path);
        let runtime = crate::capabilities::registry().cached(&config.binary_path);
        let free = crate::recovery::free_vram_bytes(&config);
        let input = RecoveryInput {
            config: &config,
            failure: &failure,
            meta: meta.as_ref(),
            model_bytes,
            runtime: runtime.as_deref(),
            free_vram_bytes: free,
            free_ram_bytes: crate::recovery::free_ram_bytes(),
            others,
        };
        let options = if failure.is_out_of_memory() {
            crate::recovery::oom_options(&input)
        } else {
            Vec::new()
        };
        RecoveryPlan {
            server_id: String::new(),
            estimated_need_bytes: crate::recovery::estimated_need(&input),
            failure: failure.clone(),
            options,
            free_vram_bytes: free,
        }
    })
    .await
    .map(|mut plan| {
        plan.server_id = server_id.to_string();
        plan
    })
}

/// Retries a failed (or stopped) server with a transient layer over its last launch: stops
/// the servers in `unload` first, then a real start. With `save`, the layer is also written
/// to the served model definition (and to the profile's own overrides of the same ids), so
/// later starts use it too.
pub async fn retry_with_overrides(
    registry: &ServerRegistry,
    runtime_mgr: &RuntimeManager,
    gate: &OperationGate,
    server_id: &str,
    overrides: crate::params::ParamValues,
    unload: Vec<String>,
    save: bool,
) -> Result<(), String> {
    servers::validate_id(server_id)?;
    let pm = registry
        .get(server_id)
        .ok_or_else(|| String::from(err("servers.not_found").with("id", server_id)))?;
    if matches!(
        pm.get_status(),
        ServerStatus::Starting | ServerStatus::Running | ServerStatus::Stopping
    ) {
        return Err(ERR_ALREADY_ACTIVE.to_string());
    }
    let mut config = pm
        .launch_config()
        .ok_or_else(|| ERR_NOTHING_TO_START.to_string())?;
    crate::domain::launch::apply_layer(&mut config, &overrides)?;
    for id in unload.iter().filter(|id| id.as_str() != server_id) {
        stop_if_active(registry, id).await?;
    }
    if save {
        let (config, overrides, id) = (config.clone(), overrides.clone(), server_id.to_string());
        blocking(move || save_overrides(&id, &config, &overrides)).await??;
    }
    launch(registry, runtime_mgr, gate, server_id.to_string(), config).await
}

/// Writes a recovery layer where the server's settings come from: the model definition's
/// `load`, and a profile's `serve_overrides` for ids the profile sets itself.
fn save_overrides(
    server_id: &str,
    config: &LaunchConfig,
    overrides: &crate::params::ParamValues,
) -> Result<(), String> {
    use crate::domain::store::Domain;
    let domain = Domain::global();
    let model_layer = crate::domain::launch::model_part(overrides);
    if let Some(def_id) = config.definition_id.as_deref() {
        let mut def = domain.definition(def_id)?;
        for (k, v) in &model_layer {
            def.load.insert(k.clone(), v.clone());
        }
        domain.save_definition(def)?;
    }
    domain.update_profile_overrides(server_id, overrides)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sampler_rests_without_servers_or_watchers() {
        use crate::telemetry::TelemetryPoller;
        use std::time::Duration;
        let poller = Arc::new(tokio::sync::Mutex::new(TelemetryPoller::new(None)));
        let registry = Arc::new(ServerRegistry::default());
        let (demand, watch) = tokio::sync::watch::channel(SamplerWatch::default());
        let counts = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = counts.clone();
        let task = tokio::spawn(run_telemetry_sampler_watched(
            poller.clone(),
            registry,
            watch,
            move |n| seen.lock().unwrap().push(n),
            |_| {},
        ));
        tokio::time::sleep(Duration::from_millis(1500)).await;
        // No server, nobody watching: not a single system sample.
        assert!(poller.lock().await.system_sampled_at().is_none());
        assert_eq!(*counts.lock().unwrap(), vec![0]);

        // A view asks for memory: sampled right away.
        demand
            .send(SamplerWatch {
                live: false,
                memory: true,
            })
            .unwrap();
        let t = std::time::Instant::now();
        while poller.lock().await.system_sampled_at().is_none() {
            assert!(
                t.elapsed() < Duration::from_secs(5),
                "no sample after demand"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        task.abort();
    }

    /// The Dashboard server's saved launch is what every view shows ("Saved · :port") and
    /// what the next start without a form reuses. A start that never ran (here: the runtime
    /// is missing) must leave it alone, or the views show a port nothing listens on.
    #[tokio::test]
    async fn refused_dashboard_start_keeps_the_saved_launch() {
        let model = crate::paths::data_root().join("models").join("saved.gguf");
        let saved = LaunchConfig {
            model_path: model.clone(),
            host: "127.0.0.1".into(),
            port: 8080,
            context_size: 4096,
            ..Default::default()
        };
        servers::save_last_session(&saved);
        assert_eq!(servers::load_last_session().map(|c| c.port), Some(8080));

        let registry = ServerRegistry::default();
        let port = registry.suggest_port("127.0.0.1", 18_431, &[]).unwrap();
        let form = LaunchConfig {
            port,
            ..saved.clone()
        };
        let missing = crate::paths::data_root()
            .join("no-runtime")
            .join("llama-server");
        let result = launch_with_binary(
            &registry,
            &OperationGate::default(),
            servers::DEFAULT_SERVER_ID.into(),
            form,
            missing,
            &AppSettings::default(),
        )
        .await;
        assert!(result.is_err(), "the start is refused");
        let session = servers::load_last_session().expect("still saved");
        assert_eq!(
            session.port, 8080,
            "the saved port is the one that last ran"
        );
        assert_eq!(session.model_path, model);

        // A start that is spawned but never runs (TC-09: out of memory while loading) does not
        // become the saved launch either: the save waits for this start to run, and a task of an
        // older start returns at once. Here the start ends in an error (no runtime).
        let pm = Arc::new(ProcessManager::with_id(servers::DEFAULT_SERVER_ID));
        let failing = LaunchConfig {
            binary_path: crate::paths::data_root()
                .join("no-runtime")
                .join("llama-server"),
            context_size: 20_000_000,
            ..saved.clone()
        };
        assert_eq!(pm.start_count(), 0);
        assert!(pm.start(failing.clone()).await.is_err());
        assert_eq!(pm.start_count(), 1);
        save_when_running(pm.clone(), failing.clone(), 1).await;
        save_when_running(pm.clone(), failing, 0).await;
        let session = servers::load_last_session().expect("still saved");
        assert_eq!((session.context_size, session.port), (4096, 8080));
    }

    /// Two running servers never share a served id: the later one gets `<alias>-<profile>` or,
    /// for an auto-named server (`<model> :<port>`), `<alias>-<port>`.
    #[test]
    fn distinct_served_alias_uses_profile_then_port() {
        let stem = "nomic-embed-text-v1.5.Q8_0";
        let taken = vec![stem.to_string()];
        // "+ New server" from a selected server: the auto name carries only model and port.
        assert_eq!(
            distinct_served_alias(stem, Some("nomic-embed-text-v1.5.Q8_0 :8083"), 8083, &taken),
            "nomic-embed-text-v1.5.Q8_0-8083"
        );
        // Dashboard / `m-<alias>` instances have no profile name.
        assert_eq!(
            distinct_served_alias(stem, None, 8084, &taken),
            "nomic-embed-text-v1.5.Q8_0-8084"
        );
        // A name that says more than the model.
        assert_eq!(
            distinct_served_alias("qwen3", Some("Kod asistanı"), 8090, &["qwen3".into()]),
            "qwen3-Kod-asistan"
        );
        assert_eq!(
            distinct_served_alias("qwen3", Some("qwen3 hızlı :8091"), 8091, &["QWEN3".into()]),
            "qwen3-h-zl"
        );
        // Deterministic, and still unique when that one is taken as well.
        let taken = vec!["qwen3".to_string(), "qwen3-work".to_string()];
        assert_eq!(
            distinct_served_alias("qwen3", Some("work"), 8092, &taken),
            "qwen3-work-2"
        );
        assert_eq!(
            distinct_served_alias("qwen3", Some("work"), 8092, &taken),
            "qwen3-work-2"
        );
        // Never the original id, never a colon from the port.
        let out = distinct_served_alias("m", Some(" :8081"), 8081, &["m".into()]);
        assert_eq!(out, "m-8081");
    }
}
