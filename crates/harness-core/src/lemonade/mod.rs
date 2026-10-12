//! Lemonade (`lemonade-sdk/lemonade`, Apache-2.0), an optional managed engine.
//!
//! **What it is for.** Harness runs its own llama.cpp, whisper.cpp and ONNX runtimes; Lemonade
//! is the way to models those cannot run: Ryzen AI NPUs through FastFlowLM (`flm`), and the
//! ROCm / Vulkan llama.cpp builds Lemonade manages as an alternative backend. It does not
//! replace the Harness runtimes: it is off until the user installs it.
//!
//! **How it is installed.** The `lemonade` runtime family (`engine::lemonade`): the
//! `lemonade-embeddable-<version>-<os>` archive of a GitHub release (5 MB: `lemond`, the
//! `lemonade` CLI and `resources/`), SHA-256 checked against the digest GitHub publishes, kept
//! in the runtime registry like whisper.cpp builds. A newer release is another entry (the
//! newest is used, an older one stays as the rollback; [`LemonadeSettings::runtime`] pins one).
//!
//! **How it runs.** One `lemond` process per Harness, on a port Harness picks
//! ([`crate::ports::AUTO_RANGE`]), bound to 127.0.0.1 and locked with a per-run API key
//! (`LEMONADE_API_KEY`, every route but `/live` then wants the Bearer key; the key lives in
//! memory and in the child's environment). It starts on demand ([`ensure_running`]), is
//! stopped with Harness, and dies with it (Windows job object / parent-death signal) even when
//! Harness crashes. Its data stays in `<data>/lemonade/`: `cache/` (the backends Lemonade
//! downloads itself, `bin/llamacpp/<backend>/`...), `config/config.json` (the keys Harness
//! owns are rewritten at every start: `models_dir`, `websocket_port`, `llamacpp.backend`) and
//! `models/` (Hugging Face cache layout; kept apart from Harness's own library). Lemonade also
//! starts a WebSocket server (a second port) and its backends (llama-server, whisper-server...)
//! on ports of its own from 8001 up, loopback only.
//!
//! **What is verified.** The embeddable archive is verified by Harness. The model files
//! Lemonade pulls are hash-checked by Lemonade against Hugging Face. The backend binaries
//! Lemonade downloads (llama.cpp, whisper.cpp...) are pinned by version in the archive's
//! `resources/backend_versions.json` and fetched from the projects' GitHub releases, but
//! Lemonade 2026.41 verifies a checksum only for a few third-party backends (the
//! `checksums` table of that file), not for the llama.cpp zips: Harness cannot add that check.
//! FastFlowLM (the NPU backend) is MIT code with proprietary NPU kernels free to use; its
//! terms mention a commercial-use revenue limit, so the NPU path is only offered on machines
//! with an NPU ([`npu`]) and the Settings card names the terms.
//!
//! **How the gateway uses it.** The downloaded, routable models of Lemonade's registry
//! (chat, embeddings, rerank, transcription) are reachable by their Lemonade name through the
//! gateway as transient definitions ([`definitions`]) whose runtime is `lemonade`; a request
//! starts `lemond` if needed and goes to it with the key ([`crate::gateway::scheduler`]).
//! Lemonade loads the model on its first request. Idle models are unloaded after the gateway's
//! default TTL.

pub mod client;
pub mod npu;

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub use client::{BackendInfo, Endpoint, LemonadeModel, ModelKind, PullProgress};
pub use npu::NpuInfo;

use crate::domain::{ModelDefinition, ModelRef};
use crate::error_key::err;
use crate::paths;
use crate::runtime::RuntimeDownloadProgress;
use crate::runtimes::{RuntimeEntry, RuntimeRegistry, RuntimeSource};

/// Family id of Lemonade builds in the runtime registry.
pub const LEMONADE_FAMILY: &str = "lemonade";
/// GitHub repository the managed builds come from.
pub const LEMONADE_REPO: &str = "lemonade-sdk/lemonade";
/// Prefix of the transient definition ids of Lemonade models (`lemonade:Qwen3-0.6B-GGUF`).
pub const DEF_PREFIX: &str = "lemonade:";
/// The `runtime` a Lemonade model's definition names.
pub const RUNTIME_ID: &str = "lemonade";

/// How long a start waits for `lemond` to answer. It scans the devices first (about 10 s on a
/// desktop with two GPUs).
const START_TIMEOUT: Duration = Duration::from_secs(120);
/// Lines of `lemond`'s output kept for error messages.
const LOG_TAIL: usize = 200;

// --- data layout ---

/// `<data>/lemonade`.
pub fn root() -> PathBuf {
    paths::data_root().join("lemonade")
}

fn cache_dir() -> PathBuf {
    root().join("cache")
}

fn config_dir() -> PathBuf {
    root().join("config")
}

/// Where Lemonade keeps the models it pulls.
pub fn models_dir() -> PathBuf {
    root().join("models")
}

/// The output of the running (or last) `lemond`.
pub fn log_file() -> PathBuf {
    root().join("lemond.log")
}

fn settings_file() -> PathBuf {
    root().join("settings.json")
}

fn catalog_file() -> PathBuf {
    root().join("catalog.json")
}

/// A whole-file write that cannot leave half a file behind.
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| {
            String::from(
                err("fs.create_dir_failed")
                    .with("path", dir.display())
                    .with("error", e),
            )
        })?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)
        .and_then(|_| std::fs::rename(&tmp, path))
        .map_err(|e| {
            String::from(
                err("fs.write_failed")
                    .with("path", path.display())
                    .with("error", e),
            )
        })
}

// --- settings ---

/// Backends `llamacpp.backend` accepts in Lemonade.
const LLAMACPP_BACKENDS: &[&str] = &["auto", "vulkan", "rocm", "cpu", "cuda", "metal"];

/// Harness's settings of the Lemonade engine (`<data>/lemonade/settings.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LemonadeSettings {
    /// llama.cpp backend Lemonade uses: `auto` (Lemonade picks), `vulkan`, `rocm`, `cpu`...
    #[serde(default = "default_backend")]
    pub backend: String,
    /// Registry id of the Lemonade build to run (`None` = the newest installed one). Pinning
    /// an older one is the rollback.
    #[serde(default)]
    pub runtime: Option<String>,
}

fn default_backend() -> String {
    "auto".into()
}

impl Default for LemonadeSettings {
    fn default() -> Self {
        Self {
            backend: default_backend(),
            runtime: None,
        }
    }
}

impl LemonadeSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !LLAMACPP_BACKENDS.contains(&self.backend.as_str()) {
            return Err(err("lemonade.bad_backend")
                .with("backend", &self.backend)
                .into());
        }
        Ok(())
    }
}

/// The saved settings (defaults when the file is missing or unreadable).
pub fn settings() -> LemonadeSettings {
    std::fs::read_to_string(settings_file())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Saves the settings; a running `lemond` is stopped so the next use starts it with them.
pub async fn save_settings(new: LemonadeSettings) -> Result<LemonadeSettings, String> {
    new.validate()?;
    if let Some(id) = new.runtime.as_deref() {
        let id = id.to_string();
        tokio::task::spawn_blocking(move || {
            let entry = RuntimeRegistry::global().get(&id)?;
            if entry.family == LEMONADE_FAMILY {
                Ok(())
            } else {
                Err(String::from(
                    err("runtime.family_mismatch").with("runtime", &id),
                ))
            }
        })
        .await
        .map_err(join_failed)??;
    }
    let changed = settings() != new;
    let text = serde_json::to_string_pretty(&new).unwrap_or_default();
    tokio::task::spawn_blocking(move || write_atomic(&settings_file(), &text))
        .await
        .map_err(join_failed)??;
    if changed && is_running() {
        stop().await?;
    }
    Ok(new)
}

fn join_failed(e: tokio::task::JoinError) -> String {
    err("task.join_failed").with("error", e).into()
}

// --- config.json and the command line (pure, unit tested) ---

/// `config.json` for a start: `existing` (what Lemonade wrote) with the keys Harness owns set.
/// The model folder and the WebSocket port are Harness's; the llama.cpp backend is the user's
/// setting. Every other key stays.
pub fn merged_config(
    existing: Value,
    settings: &LemonadeSettings,
    models_dir: &Path,
    websocket_port: u16,
) -> Value {
    let mut cfg = match existing {
        Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    cfg.insert(
        "models_dir".into(),
        Value::String(models_dir.to_string_lossy().to_string()),
    );
    cfg.insert("websocket_port".into(), json!(websocket_port));
    let llama = cfg
        .entry("llamacpp")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if let Value::Object(m) = llama {
        m.insert("backend".into(), Value::String(settings.backend.clone()));
    } else {
        *llama = json!({ "backend": settings.backend });
    }
    Value::Object(cfg)
}

/// Command line of `lemond`: the two directories are positional.
pub fn launch_args(host: &str, port: u16, cache: &Path, config: &Path) -> Vec<String> {
    vec![
        "--host".into(),
        host.to_string(),
        "--port".into(),
        port.to_string(),
        // No UDP discovery beacon on the network.
        "--no-broadcast".into(),
        // Harness keeps lemond's output itself.
        "--log-file".into(),
        "disabled".into(),
        cache.to_string_lossy().to_string(),
        config.to_string_lossy().to_string(),
    ]
}

// --- catalog snapshot ---

/// The registry as of the last time `lemond` ran: lets the gateway list the downloaded models
/// and the card show the choices without starting a server.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Catalog {
    /// Unix seconds of the refresh (0 = never).
    pub updated_at: u64,
    pub models: Vec<LemonadeModel>,
    pub backends: Vec<BackendInfo>,
}

/// The saved catalog (empty before the first run).
pub fn catalog() -> Catalog {
    std::fs::read_to_string(catalog_file())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_catalog(c: &Catalog) {
    let text = serde_json::to_string(c).unwrap_or_default();
    if write_atomic(&catalog_file(), &text).is_ok() {
        *gateway_cache().lock().unwrap() = None;
    }
}

/// Reads the registry and the backends from a running `lemond` and saves them.
pub async fn refresh_catalog(ep: &Endpoint) -> Result<Catalog, String> {
    let models = client::models(ep).await?;
    // Backends are a nice-to-have: a failure keeps the previous list.
    let backends = match client::backends(ep).await {
        Ok(b) => b,
        Err(_) => catalog().backends,
    };
    let c = Catalog {
        updated_at: crate::capabilities::now_secs(),
        models,
        backends,
    };
    let to_save = c.clone();
    tokio::task::spawn_blocking(move || save_catalog(&to_save))
        .await
        .map_err(join_failed)?;
    Ok(c)
}

// --- the process ---

struct Running {
    child: command_group::GroupChild,
    endpoint: Endpoint,
    port: u16,
    websocket_port: u16,
    pid: u32,
    started_at: u64,
    runtime_id: String,
    version: Option<String>,
}

struct Service {
    /// One start / stop at a time.
    lifecycle: tokio::sync::Mutex<()>,
    running: Mutex<Option<Running>>,
    log_tail: Mutex<VecDeque<String>>,
    pull: Mutex<Option<PullProgress>>,
    pulling: AtomicBool,
}

fn service() -> &'static Service {
    static SERVICE: OnceLock<Service> = OnceLock::new();
    SERVICE.get_or_init(|| Service {
        lifecycle: tokio::sync::Mutex::new(()),
        running: Mutex::new(None),
        log_tail: Mutex::new(VecDeque::new()),
        pull: Mutex::new(None),
        pulling: AtomicBool::new(false),
    })
}

/// Drops the record of a `lemond` that exited by itself; whether one runs.
fn is_running() -> bool {
    let mut guard = service().running.lock().unwrap();
    let alive = match guard.as_mut() {
        Some(r) => matches!(r.child.try_wait(), Ok(None)),
        None => return false,
    };
    if !alive {
        guard.take();
    }
    alive
}

/// The endpoint of the running `lemond` (`None` when it is not running).
pub fn endpoint() -> Option<Endpoint> {
    if !is_running() {
        return None;
    }
    service()
        .running
        .lock()
        .unwrap()
        .as_ref()
        .map(|r| r.endpoint.clone())
}

/// The newest installed Lemonade build, or the one the settings pin.
fn pick_install(reg: &RuntimeRegistry, pinned: Option<&str>) -> Result<RuntimeEntry, String> {
    match pinned {
        Some(id) => reg.get(id),
        None => reg.default_for_family(LEMONADE_FAMILY),
    }
}

fn new_key() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn push_tail(line: String) {
    let mut tail = service().log_tail.lock().unwrap();
    if tail.len() >= LOG_TAIL {
        tail.pop_front();
    }
    tail.push_back(line);
}

/// The last lines of `lemond`'s output (newest last).
pub fn log_tail(lines: usize) -> Vec<String> {
    let tail = service().log_tail.lock().unwrap();
    let skip = tail.len().saturating_sub(lines);
    tail.iter().skip(skip).cloned().collect()
}

/// The ports of a start: HTTP and WebSocket, both from Harness's automatic range.
fn pick_ports(host: &str) -> Result<(u16, u16), String> {
    let http = crate::ports::pick_auto(host, None, &[])
        .ok_or_else(|| String::from(err("servers.no_free_port")))?;
    let ws = crate::ports::pick_auto(host, None, &[http])
        .ok_or_else(|| String::from(err("servers.no_free_port")))?;
    Ok((http, ws))
}

/// Spawns `lemond` (blocking): config written, process in its own group / job, output captured.
fn spawn(
    binary: &Path,
    key: &str,
    host: &str,
    port: u16,
    websocket_port: u16,
    settings: &LemonadeSettings,
) -> Result<command_group::GroupChild, String> {
    use command_group::CommandGroup;
    use std::process::{Command, Stdio};

    for dir in [cache_dir(), config_dir(), models_dir()] {
        std::fs::create_dir_all(&dir).map_err(|e| {
            String::from(
                err("fs.create_dir_failed")
                    .with("path", dir.display())
                    .with("error", e),
            )
        })?;
    }
    // Lemonade writes config.json itself (a missing or unreadable file is replaced).
    let config_path = config_dir().join("config.json");
    let existing = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or(Value::Null);
    let cfg = merged_config(existing, settings, &models_dir(), websocket_port);
    write_atomic(
        &config_path,
        &serde_json::to_string_pretty(&cfg).unwrap_or_default(),
    )?;

    let mut cmd = Command::new(binary);
    cmd.args(launch_args(host, port, &cache_dir(), &config_dir()))
        .current_dir(binary.parent().unwrap_or_else(|| Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Harness's choices win over whatever the user's shell exports for the Lemonade CLI.
    for var in [
        "LEMONADE_HOST",
        "LEMONADE_PORT",
        "LEMONADE_ADMIN_API_KEY",
        "LEMONADE_API_KEY",
        "LEMONADE_ALLOWED_ORIGINS",
        "LEMONADE_DEFAULTS_PATH",
    ] {
        cmd.env_remove(var);
    }
    cmd.env("LEMONADE_API_KEY", key);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: only async-signal-safe libc calls are made between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() == 1 {
                    libc::raise(libc::SIGKILL);
                }
                Ok(())
            });
        }
    }
    #[cfg(target_os = "macos")]
    crate::watchdog::install(&mut cmd);

    let mut group = cmd.group();
    // The job object kills lemond and its backends when Harness exits, crash included.
    #[cfg(windows)]
    group.kill_on_drop(true);
    #[cfg(windows)]
    group.creation_flags(0x0800_0000);
    let mut child = group.spawn().map_err(|e| {
        String::from(
            err("process.spawn_failed")
                .with("program", "lemond")
                .with("error", e),
        )
    })?;

    service().log_tail.lock().unwrap().clear();
    let log = std::fs::File::create(log_file())
        .ok()
        .map(|f| std::sync::Arc::new(Mutex::new(f)));
    let mut readers: Vec<Box<dyn FnOnce() + Send>> = Vec::new();
    if let Some(out) = child.inner().stdout.take() {
        readers.push(reader(out, log.clone()));
    }
    if let Some(errs) = child.inner().stderr.take() {
        readers.push(reader(errs, log));
    }
    for r in readers {
        std::thread::spawn(r);
    }
    Ok(child)
}

fn reader<R: std::io::Read + Send + 'static>(
    pipe: R,
    log: Option<std::sync::Arc<Mutex<std::fs::File>>>,
) -> Box<dyn FnOnce() + Send> {
    Box::new(move || {
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            if let Some(f) = &log {
                let _ = writeln!(f.lock().unwrap(), "{line}");
            }
            push_tail(line);
        }
    })
}

/// Starts `lemond` if it is not running and returns its endpoint. A stopped Lemonade starts on
/// the first use (a request for one of its models, the card's model list).
pub async fn ensure_running() -> Result<Endpoint, String> {
    if let Some(ep) = endpoint() {
        return Ok(ep);
    }
    let _lifecycle = service().lifecycle.lock().await;
    // Someone else started it while we waited.
    if let Some(ep) = endpoint() {
        return Ok(ep);
    }
    start_locked().await
}

/// Starts `lemond` (no-op when running); returns its endpoint.
pub async fn start() -> Result<Endpoint, String> {
    ensure_running().await
}

async fn start_locked() -> Result<Endpoint, String> {
    let host = "127.0.0.1";
    let settings = settings();
    let (entry, binary) = {
        let pinned = settings.runtime.clone();
        tokio::task::spawn_blocking(move || {
            let reg = RuntimeRegistry::global();
            let entry = pick_install(&reg, pinned.as_deref())
                .map_err(|_| String::from(err("lemonade.not_installed")))?;
            if entry.family != LEMONADE_FAMILY {
                return Err(String::from(err("lemonade.not_installed")));
            }
            let binary = reg.launch_binary(&entry.id)?;
            Ok((entry, binary))
        })
        .await
        .map_err(join_failed)??
    };
    let (port, websocket_port) = pick_ports(host)?;
    let key = new_key();
    let child = {
        let (b, k, s) = (binary.clone(), key.clone(), settings.clone());
        tokio::task::spawn_blocking(move || spawn(&b, &k, host, port, websocket_port, &s))
            .await
            .map_err(join_failed)??
    };
    let pid = child.id();
    let endpoint = Endpoint {
        base: crate::engine::host_url(host, port),
        key,
    };
    *service().running.lock().unwrap() = Some(Running {
        child,
        endpoint: endpoint.clone(),
        port,
        websocket_port,
        pid,
        started_at: crate::capabilities::now_secs(),
        runtime_id: entry.id.clone(),
        version: entry.version.clone(),
    });

    // Wait until it answers, or exits.
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if !is_running() {
            let tail = log_tail(8).join(" | ");
            return Err(err("lemonade.start_failed").with("detail", tail).into());
        }
        if client::live(&endpoint.base).await && client::health(&endpoint).await.is_ok() {
            break;
        }
        if Instant::now() >= deadline {
            kill_running();
            return Err(err("lemonade.start_timeout")
                .num("seconds", START_TIMEOUT.as_secs())
                .into());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    // The registry as it is now; a failure only means the card shows the older snapshot.
    let _ = refresh_catalog(&endpoint).await;
    Ok(endpoint)
}

/// Kills the process group without asking (blocking, quick).
fn kill_running() {
    if let Some(mut r) = service().running.lock().unwrap().take() {
        let _ = r.child.kill();
        let _ = r.child.wait();
    }
}

/// Stops `lemond`: asks it to shut down (it stops its backends), then ends the process group.
pub async fn stop() -> Result<(), String> {
    let _lifecycle = service().lifecycle.lock().await;
    let Some(ep) = endpoint() else {
        return Ok(());
    };
    let _ = client::shutdown(&ep).await;
    let deadline = Instant::now() + Duration::from_secs(15);
    while is_running() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    tokio::task::spawn_blocking(kill_running)
        .await
        .map_err(join_failed)?;
    Ok(())
}

/// Ends `lemond` at once (app exit): no waiting, the whole process group is killed.
pub fn shutdown_now() {
    kill_running();
}

// --- install ---

/// What `install` was asked for.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct InstallRequest {
    /// Release tag (`v2026.41.1`); default: the newest stable release with a build for this OS.
    #[serde(default)]
    pub tag: Option<String>,
}

/// The release an install would take, shown before anything is downloaded.
pub async fn preview_release(
    tag: Option<String>,
) -> Result<crate::runtimes::github::ReleasePick, String> {
    crate::runtimes::github::find_release(&release_query(tag)).await
}

fn release_query(tag: Option<String>) -> crate::runtimes::github::ReleaseQuery {
    crate::runtimes::github::ReleaseQuery {
        family: LEMONADE_FAMILY.into(),
        repo: LEMONADE_REPO.into(),
        tag: tag.filter(|t| !t.trim().is_empty()),
        asset: None,
        backend: None,
    }
}

/// Downloads, verifies and registers a Lemonade release.
pub async fn install(req: InstallRequest) -> Result<RuntimeEntry, String> {
    let query = release_query(req.tag);
    let pick = crate::runtimes::github::find_release(&query).await?;
    let request = crate::runtimes::github::GithubInstall {
        query,
        name: Some(format!("Lemonade {}", pick.tag)),
        id: None,
        allow_unverified: false,
    };
    crate::runtimes::github::install(&RuntimeRegistry::global(), &request).await
}

// --- models ---

/// The registry as the card shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelList {
    /// `lemond` is running (the list is current); otherwise it is the last snapshot.
    pub live: bool,
    #[serde(flatten)]
    pub catalog: Catalog,
}

/// The registry: current when `lemond` runs, the last snapshot otherwise.
pub async fn list_models() -> Result<ModelList, String> {
    match endpoint() {
        Some(ep) => Ok(ModelList {
            live: true,
            catalog: refresh_catalog(&ep).await?,
        }),
        None => Ok(ModelList {
            live: false,
            catalog: tokio::task::spawn_blocking(catalog)
                .await
                .map_err(join_failed)?,
        }),
    }
}

/// [`list_models`] after starting `lemond` when it is not running (the current registry).
pub async fn list_models_started() -> Result<ModelList, String> {
    ensure_running().await?;
    list_models().await
}

/// Progress of the running pull or backend install (`None` when idle).
pub fn pull_progress() -> Option<PullProgress> {
    service().pull.lock().unwrap().clone()
}

/// One download at a time (a pull or a backend install).
struct PullGuard;

impl PullGuard {
    fn take() -> Result<Self, String> {
        if service()
            .pulling
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(err("lemonade.busy").into());
        }
        Ok(Self)
    }
}

impl Drop for PullGuard {
    fn drop(&mut self) {
        *service().pull.lock().unwrap() = None;
        service().pulling.store(false, Ordering::SeqCst);
    }
}

fn set_progress(p: PullProgress) {
    *service().pull.lock().unwrap() = Some(p);
}

/// A model reference Lemonade accepts: a registry name or a checkpoint (`owner/repo[:variant]`).
pub fn validate_model_ref(model: &str) -> Result<(), String> {
    let ok = !model.trim().is_empty()
        && model.len() <= 256
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':' | '/' | '+'))
        && !model.contains("..");
    if ok {
        Ok(())
    } else {
        Err(err("lemonade.bad_model").with("model", model).into())
    }
}

/// Downloads a model into Lemonade (starts `lemond` first). Progress: [`pull_progress`].
pub async fn pull_model(model: &str) -> Result<(), String> {
    validate_model_ref(model)?;
    let _busy = PullGuard::take()?;
    let ep = ensure_running().await?;
    client::pull(&ep, model, set_progress).await?;
    let _ = refresh_catalog(&ep).await;
    Ok(())
}

/// Deletes a downloaded model (starts `lemond` first).
pub async fn delete_model(model: &str) -> Result<(), String> {
    validate_model_ref(model)?;
    let ep = ensure_running().await?;
    client::delete(&ep, model).await?;
    let _ = refresh_catalog(&ep).await;
    Ok(())
}

/// Installs a backend (`llamacpp` + `rocm`, `flm` + `npu`...). Progress: [`pull_progress`].
pub async fn install_backend(recipe: &str, backend: &str) -> Result<(), String> {
    let name_ok = |s: &str| {
        !s.is_empty()
            && s.len() <= 40
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    if !name_ok(recipe) || !name_ok(backend) {
        return Err(err("lemonade.bad_model")
            .with("model", format!("{recipe}:{backend}"))
            .into());
    }
    let _busy = PullGuard::take()?;
    let ep = ensure_running().await?;
    client::install_backend(&ep, recipe, backend, set_progress).await?;
    let _ = refresh_catalog(&ep).await;
    Ok(())
}

/// Unloads every model Lemonade holds (the gateway's idle TTL); no-op when it is not running.
pub async fn unload_all() {
    if let Some(ep) = endpoint() {
        let _ = client::unload(&ep, None).await;
    }
}

// --- status ---

/// A registered Lemonade build.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct InstallView {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    /// Release tag it was installed from.
    pub tag: Option<String>,
    pub added_at: u64,
    /// The build the next start runs.
    pub active: bool,
}

/// Everything the Settings card shows.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LemonadeStatus {
    /// Lemonade publishes a build for this OS and CPU.
    pub supported: bool,
    /// Installed builds, newest first.
    pub installs: Vec<InstallView>,
    pub settings: LemonadeSettings,
    pub running: bool,
    pub port: Option<u16>,
    pub websocket_port: Option<u16>,
    pub pid: Option<u32>,
    /// `http://127.0.0.1:<port>` (the key is never shown).
    pub base_url: Option<String>,
    /// Registry id of the build that is running.
    pub running_runtime: Option<String>,
    /// Version of the running `lemond`.
    pub running_version: Option<String>,
    pub started_at: Option<u64>,
    /// An AMD Ryzen AI NPU is present (FastFlowLM / Ryzen AI backends need one).
    pub npu: NpuInfo,
    /// Backends as of the last run.
    pub backends: Vec<BackendInfo>,
    pub models_dir: String,
    pub log_file: String,
    /// Progress of the running release download.
    #[cfg_attr(feature = "openapi", schema(value_type = Option<Object>))]
    pub install_progress: Option<RuntimeDownloadProgress>,
    /// Progress of the running model pull / backend install.
    pub pull: Option<PullProgress>,
}

/// Whether Lemonade publishes an embeddable build for this OS and CPU.
pub fn platform_supported() -> bool {
    cfg!(any(
        all(windows, target_arch = "x86_64"),
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ),
        all(target_os = "macos", target_arch = "aarch64")
    ))
}

/// The installed builds, newest first, with the active one marked.
pub fn installs(reg: &RuntimeRegistry, pinned: Option<&str>) -> Vec<InstallView> {
    let mut entries: Vec<RuntimeEntry> = reg
        .entries()
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.family == LEMONADE_FAMILY)
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.added_at));
    let active = pick_install(reg, pinned).ok().map(|e| e.id);
    entries
        .into_iter()
        .map(|e| InstallView {
            active: active.as_deref() == Some(e.id.as_str()),
            tag: match &e.source {
                RuntimeSource::Github { tag, .. } => Some(tag.clone()),
                _ => None,
            },
            id: e.id,
            name: e.name,
            version: e.version,
            added_at: e.added_at,
        })
        .collect()
}

/// The card's state. Blocking (registry read, device enumeration).
pub fn status() -> LemonadeStatus {
    let settings = settings();
    let running = is_running();
    let (port, ws, pid, base, runtime, version, started) = if running {
        match service().running.lock().unwrap().as_ref() {
            Some(r) => (
                Some(r.port),
                Some(r.websocket_port),
                Some(r.pid),
                Some(r.endpoint.base.clone()),
                Some(r.runtime_id.clone()),
                r.version.clone(),
                Some(r.started_at),
            ),
            None => (None, None, None, None, None, None, None),
        }
    } else {
        (None, None, None, None, None, None, None)
    };
    LemonadeStatus {
        supported: platform_supported(),
        installs: installs(&RuntimeRegistry::global(), settings.runtime.as_deref()),
        settings,
        running,
        port,
        websocket_port: ws,
        pid,
        base_url: base,
        running_runtime: runtime,
        running_version: version,
        started_at: started,
        npu: npu_cached(),
        backends: catalog().backends,
        models_dir: models_dir().display().to_string(),
        log_file: log_file().display().to_string(),
        install_progress: crate::runtimes::github::progress(),
        pull: pull_progress(),
    }
}

/// The NPU of this machine, looked up at most once a minute (the card polls the status while a
/// download runs; hardware does not change under it).
fn npu_cached() -> NpuInfo {
    static CACHE: OnceLock<Mutex<Option<(Instant, NpuInfo)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().unwrap();
    if let Some((at, info)) = guard.as_ref() {
        if at.elapsed() < Duration::from_secs(60) {
            return info.clone();
        }
    }
    let info = npu::detect();
    *guard = Some((Instant::now(), info.clone()));
    info
}

/// Removes an installed build (not the one that is running). A build the settings pinned is
/// unpinned with it.
pub fn remove_install(id: &str) -> Result<(), String> {
    let reg = RuntimeRegistry::global();
    let entry = reg.get(id)?;
    if entry.family != LEMONADE_FAMILY {
        return Err(err("runtime.family_mismatch").with("runtime", id).into());
    }
    if is_running()
        && service()
            .running
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|r| r.runtime_id == id)
    {
        return Err(err("lemonade.in_use").with("runtime", id).into());
    }
    reg.remove(id)?;
    let current = settings();
    if current.runtime.as_deref() == Some(id) {
        let unpinned = LemonadeSettings {
            runtime: None,
            ..current
        };
        write_atomic(
            &settings_file(),
            &serde_json::to_string_pretty(&unpinned).unwrap_or_default(),
        )?;
    }
    Ok(())
}

// --- the gateway's view ---

type GatewayCache = Mutex<Option<(Instant, Vec<LemonadeModel>)>>;

fn gateway_cache() -> &'static GatewayCache {
    static CACHE: OnceLock<GatewayCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// The downloaded models the gateway can serve (chat, embeddings, rerank, transcription),
/// from the last snapshot; empty while no Lemonade build is installed. Cheap (cached for 2 s).
pub fn gateway_models() -> Vec<LemonadeModel> {
    {
        let cache = gateway_cache().lock().unwrap();
        if let Some((at, models)) = cache.as_ref() {
            if at.elapsed() < Duration::from_secs(2) {
                return models.clone();
            }
        }
    }
    let installed = RuntimeRegistry::global()
        .default_for_family(LEMONADE_FAMILY)
        .is_ok();
    let models = if installed {
        servable(catalog().models)
    } else {
        Vec::new()
    };
    *gateway_cache().lock().unwrap() = Some((Instant::now(), models.clone()));
    models
}

/// The models of a registry the gateway can serve: downloaded, of a routable kind, and named
/// in a way a model alias can be.
pub fn servable(models: Vec<LemonadeModel>) -> Vec<LemonadeModel> {
    models
        .into_iter()
        .filter(|m| m.downloaded && m.kind.routable())
        .filter(|m| crate::domain::validate_alias(&m.id).is_ok())
        .collect()
}

/// The Lemonade model a request's `model` names (case-insensitive).
pub fn find_model(name: &str) -> Option<LemonadeModel> {
    gateway_models()
        .into_iter()
        .find(|m| m.id.eq_ignore_ascii_case(name.trim()))
}

/// Id of the transient definition of a Lemonade model.
pub fn def_id(model: &str) -> String {
    format!("{DEF_PREFIX}{model}")
}

/// The Lemonade model a definition id stands for (`None`: a regular definition).
pub fn model_of_def(def_id: &str) -> Option<&str> {
    def_id.strip_prefix(DEF_PREFIX).filter(|m| !m.is_empty())
}

/// The Lemonade models as transient definitions (runtime `lemonade`), named by their Lemonade
/// id. A stored definition with the same alias wins.
pub fn definitions(existing: &[ModelDefinition]) -> Vec<ModelDefinition> {
    definitions_of(gateway_models(), existing)
}

/// [`definitions`] of a given model list.
pub fn definitions_of(
    models: Vec<LemonadeModel>,
    existing: &[ModelDefinition],
) -> Vec<ModelDefinition> {
    models
        .into_iter()
        .filter(|m| !existing.iter().any(|d| d.alias.eq_ignore_ascii_case(&m.id)))
        .map(|m| {
            let id = def_id(&m.id);
            let mut def = ModelDefinition::new(
                m.id.clone(),
                ModelRef::Local {
                    path: PathBuf::from(&id),
                },
            );
            def.id = id;
            def.description = format!("Lemonade ({})", m.recipe);
            def.load.insert(
                crate::domain::launch::RUNTIME.to_string(),
                crate::params::ParamValue::Text(RUNTIME_ID.to_string()),
            );
            def
        })
        .collect()
}

#[cfg(test)]
mod tests;
