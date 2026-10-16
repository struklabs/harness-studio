use command_group::{CommandGroup, GroupChild};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::sleep;

use crate::error_key::err;
use crate::failure::{FailureKind, FailurePhase, ServerFailure};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ServerStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LaunchConfig {
    /// Always overwritten by the backend (see `resolve_server_binary`).
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub binary_path: PathBuf,
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub model_path: PathBuf,
    pub gpu_layers: u32,
    pub context_size: u32,
    pub host: String,
    /// The listening port. With `port_auto` (or 0) it is only the last automatic pick.
    pub port: u16,
    /// Harness picks the port (`ports::AUTO_RANGE`, 11440–11499): `port` is kept while it is
    /// free, else another one is taken silently. Off = the user chose `port`; a taken port is
    /// then an error naming the program that holds it. Port 0 means the same as on.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub port_auto: bool,
    pub threads: Option<u32>,

    // Advanced Inference & Performance Controls
    #[serde(default)]
    pub flash_attn: bool,
    #[serde(default)]
    pub cache_type_k: Option<String>,
    #[serde(default)]
    pub cache_type_v: Option<String>,
    #[serde(default)]
    pub parallel_slots: Option<u32>,
    #[serde(default)]
    pub logical_batch: Option<u32>,
    #[serde(default)]
    pub physical_batch: Option<u32>,
    #[serde(default)]
    pub context_shift: Option<bool>,
    #[serde(default)]
    pub split_mode: Option<String>,
    /// Main GPU as an OS adapter index (`gpu::GpuAdapter::index`, what the GPU picker shows),
    /// not the runtime's device index; converted at launch (`devices::pair_adapters`).
    #[serde(default)]
    pub main_gpu: Option<u32>,
    #[serde(default)]
    pub tensor_split: Option<String>,
    #[serde(default)]
    pub rope_scaling: Option<String>,
    #[serde(default)]
    pub rope_freq_base: Option<f32>,
    #[serde(default)]
    pub rope_freq_scale: Option<f32>,
    #[serde(default)]
    pub chat_template: Option<String>,
    /// Input only: a new key typed by the user (or a legacy plaintext value). Never serialized;
    /// the key itself lives in the OS keychain (see `secrets`). `Some("")` clears the saved key.
    #[serde(default, skip_serializing)]
    pub api_key: Option<String>,
    /// Keychain reference of the saved key (safe to store and to send to the UI).
    #[serde(default)]
    pub api_key_ref: Option<String>,
    #[serde(default)]
    pub custom_args: Option<String>,

    // Speculative Decoding & LoRA
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub draft_model_path: Option<PathBuf>,
    #[serde(default)]
    pub draft_gpu_layers: Option<u32>,
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub lora_path: Option<PathBuf>,
    #[serde(default)]
    pub lora_scale: Option<f32>,

    // Multimodal / Vision Projector
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub mmproj_path: Option<PathBuf>,

    // Router / Multi-Model Mode
    #[serde(default)]
    pub router_mode: bool,
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub models_path: Option<PathBuf>,

    // llama-server's own MCP option. (The old `server_tools` / `--tools` field is gone: saved
    // configs that still carry it load, the value is ignored. `--tools` stays a protected flag.)
    #[serde(default)]
    pub mcp_enabled: bool,
    /// Resolved by the backend at launch (never trusted from the frontend).
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub mcp_config_path: Option<PathBuf>,

    /// Catalog parameters (`params.toml`) without a typed field above; emitted only when
    /// the runtime has the flag.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub params: crate::params::ParamValues,

    /// Runtime (llama-server build) of this launch: a `runtimes` registry id; `None` or
    /// `official` = Settings > Runtime. Resolved to `binary_path` by the orchestrator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,

    /// Served model id (`--alias`): the ModelDefinition's alias. `None` = the file stem.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// The ModelDefinition this launch was resolved from (informational: lets the UI and the
    /// Dashboard session know which definition a server runs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_id: Option<String>,
    /// Who starts the server: the gateway's JIT loads serve agents, which decides the
    /// automatic slot count (`slots`). Set by the backend.
    #[serde(default, skip_serializing_if = "crate::slots::SlotOrigin::is_user")]
    pub launched_by: crate::slots::SlotOrigin,
    /// `--cors-origins` value, resolved by the backend at launch from Settings > Network &
    /// security (`network::prepare_launch`); never read from or written to JSON.
    #[serde(skip)]
    pub cors_origins: Option<String>,
    /// The user confirmed "open anyway" for a network bind without a key (set by
    /// `network::prepare_launch` from the saved confirmations; never read from JSON).
    #[serde(skip)]
    pub lan_exposure_ack: bool,
    /// `gpu_layers` is the memory estimate's recommendation (no layer count was set by the
    /// definition, profile or form), so a start with an automatic context may pick context and
    /// layers together (`full_offload`). Set when a definition is resolved; never read from or
    /// written to JSON.
    #[serde(skip)]
    pub gpu_layers_auto: bool,
    /// Settings (catalog ids) that hold the automatic choice, not a value anyone set: the
    /// context, layer count and slot count a form shows from the default-load rule
    /// (`full_offload`), and the layer count of a resolved definition launch. They are left
    /// out of every layer made from this config (`domain::launch::config_layer`), so a
    /// profile, definition or last session never freezes them as explicit values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auto_fields: Vec<String>,
}

impl LaunchConfig {
    /// Harness chooses this launch's port (see [`LaunchConfig::port_auto`]).
    pub fn wants_auto_port(&self) -> bool {
        self.port_auto || self.port == 0
    }
}

/// A command for short helper invocations (`--help`, `--version`, tool probes):
/// no console window on Windows, no stdin.
pub fn quiet_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null());
    crate::appimage::foreign_std(&mut cmd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// Flag dialect of a specific llama-server build, detected from its `--help` output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerCaps {
    /// Newer builds: `--flash-attn on|off|auto`; older builds: bare `--flash-attn`.
    pub flash_attn_takes_value: bool,
    /// Router-mode flag (`--models-dir` on current builds), if supported at all.
    pub models_dir_flag: Option<String>,
    pub supports_metrics: bool,
    /// `--tools` (built-in agent tools)
    pub supports_tools: bool,
    /// `--mcp-servers-config`
    pub supports_mcp: bool,
    /// `--cors-origins`
    pub supports_cors_origins: bool,
}

impl Default for ServerCaps {
    /// Assumes a current llama.cpp build when detection is not possible.
    fn default() -> Self {
        Self {
            flash_attn_takes_value: true,
            models_dir_flag: Some("--models-dir".to_string()),
            supports_metrics: true,
            supports_tools: true,
            supports_mcp: true,
            supports_cors_origins: true,
        }
    }
}

impl ServerCaps {
    pub fn from_help(help: &str) -> Self {
        crate::capabilities::RuntimeCapabilities::from_outputs(Default::default(), "", help, "")
            .server_caps()
    }

    /// Combined stdout+stderr of `<binary> --help` (empty on failure), without a console window.
    pub fn help_text(binary: &std::path::Path) -> String {
        let cwd = binary.parent().unwrap_or_else(|| std::path::Path::new("."));
        quiet_command(binary)
            .arg("--help")
            .current_dir(cwd)
            .output()
            .map(|out| {
                format!(
                    "{}
{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                )
            })
            .unwrap_or_default()
    }

    /// Flag dialect of `binary` from the capability registry (probed once per binary).
    pub fn detect(binary: &std::path::Path) -> Self {
        Self::from_runtime(&crate::capabilities::registry().get_or_probe(binary))
    }

    /// Falls back to the current-build defaults when the help didn't look like llama-server.
    pub fn from_runtime(caps: &crate::capabilities::RuntimeCapabilities) -> Self {
        if caps.looks_like_server() {
            caps.server_caps()
        } else {
            Self::default()
        }
    }
}

/// Splits a command line like a POSIX shell would for simple cases: whitespace
/// separates arguments, single/double quotes group, backslash escapes inside double quotes
/// only before `"` or `\` (so Windows paths keep their backslashes).
pub fn split_args(raw: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut in_arg = false;
    let mut chars = raw.chars().peekable();
    let mut quote: Option<char> = None;

    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else if q == '"' && c == '\\' && matches!(chars.peek(), Some('"') | Some('\\')) {
                    cur.push(chars.next().unwrap());
                } else {
                    cur.push(c);
                }
            }
            None => {
                if c.is_whitespace() {
                    if in_arg {
                        args.push(std::mem::take(&mut cur));
                        in_arg = false;
                    }
                } else if c == '"' || c == '\'' {
                    quote = Some(c);
                    in_arg = true;
                } else {
                    cur.push(c);
                    in_arg = true;
                }
            }
        }
    }
    if quote.is_some() {
        return Err(err("process.unclosed_quote").into());
    }
    if in_arg {
        args.push(cur);
    }
    Ok(args)
}

/// Model id a launch serves (`/v1/models`): a custom `--alias`, else the definition alias
/// (possibly made distinct at launch, see `orchestrator::distinct_served_alias`), else the
/// file stem; `None` in router mode (every file of the directory).
pub fn served_alias(config: &LaunchConfig) -> Option<String> {
    if config.router_mode {
        return None;
    }
    custom_alias(config)
        .or_else(|| {
            config
                .alias
                .as_deref()
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .map(str::to_string)
        })
        .or_else(|| model_alias(&config.model_path))
}

/// `--alias` / `-a` the user put in the custom arguments (it replaces ours, see `build_args`).
pub fn custom_alias(config: &LaunchConfig) -> Option<String> {
    let args = split_args(config.custom_args.as_deref()?).ok()?;
    args.windows(2)
        .find(|w| w[0] == "--alias" || w[0] == "-a")
        .map(|w| w[1].clone())
}

/// Last component of a path, splitting on both `/` and `\\` so a path saved on Windows
/// (`C:\\AI\\model.gguf`) still yields the file name on Linux and macOS.
pub fn file_name_any_sep(path: &std::path::Path) -> Option<String> {
    let full = path.to_string_lossy();
    let name = full.rsplit(['/', '\\']).next().unwrap_or_default();
    (!name.is_empty()).then(|| name.to_string())
}

/// Model id reported to clients: the file name without `.gguf` and without the
/// `-00001-of-00003` suffix of split models.
pub fn model_alias(model_path: &std::path::Path) -> Option<String> {
    // An onnxruntime-genai model is `model.onnx` in a folder named for the model.
    if let Some(alias) = harness_onnx::genai_pkg::alias_of(model_path) {
        return Some(alias);
    }
    // Other ONNX graphs: the repo folder, never the generic `model`.
    if let Some(alias) = harness_onnx::package::alias_of(model_path) {
        return Some(alias);
    }
    let name = file_name_any_sep(model_path)?;
    let stem = name
        .strip_suffix(".gguf")
        .or_else(|| name.strip_suffix(".GGUF"))
        .unwrap_or(&name);
    let stem = match stem.rfind("-of-") {
        Some(i) if stem[i + 4..].chars().all(|c| c.is_ascii_digit()) => stem[..i]
            .rsplit_once('-')
            .filter(|(_, n)| n.chars().all(|c| c.is_ascii_digit()))
            .map_or(stem, |(base, _)| base),
        _ => stem,
    };
    (!stem.is_empty()).then(|| stem.to_string())
}

fn validate_custom_args(args: &[String]) -> Result<(), String> {
    if let Some(flag) = args.iter().find(|arg| crate::params::is_protected(arg)) {
        let flag = flag.split_once('=').map_or(flag.as_str(), |(flag, _)| flag);
        return Err(err("process.protected_flag").with("flag", flag).into());
    }
    Ok(())
}

/// Typed fields of `config` that are set (for catalog `legacy:<field>` requires/conflicts).
fn legacy_fields_set(config: &LaunchConfig) -> std::collections::BTreeSet<String> {
    let path_set = |p: &Option<PathBuf>| p.as_ref().is_some_and(|p| !p.as_os_str().is_empty());
    let text_set = |t: &Option<String>| t.as_deref().is_some_and(|t| !t.is_empty());
    [
        ("lora_path", path_set(&config.lora_path)),
        ("draft_model_path", path_set(&config.draft_model_path)),
        ("mmproj_path", path_set(&config.mmproj_path)),
        ("chat_template", text_set(&config.chat_template)),
        ("split_mode", text_set(&config.split_mode)),
        ("tensor_split", text_set(&config.tensor_split)),
        ("flash_attn", config.flash_attn),
        ("router_mode", config.router_mode),
        ("cors_origins", config.cors_origins.is_some()),
    ]
    .into_iter()
    .filter(|(_, set)| *set)
    .map(|(name, _)| name.to_string())
    .collect()
}

/// `build_args` result with the catalog params that were not emitted.
#[derive(Debug, Clone, Default)]
pub struct BuildOutput {
    pub args: Vec<String>,
    pub skipped: Vec<crate::params::SkippedParam>,
}

/// Environment variable llama-server reads its `--api-key` from (verified against the
/// WinGet build; `LLAMA_ARG_API_KEY` is ignored).
const API_KEY_ENV_VARS: [&str; 1] = ["LLAMA_API_KEY"];

/// Environment spellings of the protected flags. llama-server reads `LLAMA_ARG_*` when the
/// flag is absent, so an inherited variable could otherwise widen the bind, CORS or tools of
/// a run behind Harness Studio's back; they are removed from every child environment.
const PROTECTED_ENV_VARS: &[&str] = &[
    "LLAMA_ARG_HOST",
    "LLAMA_ARG_PORT",
    "LLAMA_ARG_API_KEY",
    "LLAMA_ARG_API_KEY_FILE",
    "LLAMA_ARG_CORS_ORIGINS",
    "LLAMA_ARG_TOOLS",
    "LLAMA_ARG_MCP_SERVERS_CONFIG",
    "LLAMA_ARG_MCP_SERVERS_JSON",
    "LLAMA_ARG_AGENT",
];

/// Proxy variables a Python worker never inherits (Python's HTTP clients read either case).
const PROXY_ENV_VARS: [&str; 8] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
];

/// torch switches a Python worker never inherits: either one set in the user's environment
/// would change what `torch.load` does to a model's PyTorch `.bin` weights (the first turns the
/// `weights_only=True` protection off; neither is Harness's to set).
const TORCH_LOAD_ENV_VARS: [&str; 2] = [
    "TORCH_FORCE_NO_WEIGHTS_ONLY_LOAD",
    "TORCH_FORCE_WEIGHTS_ONLY_LOAD",
];

/// What the UI and telemetry need to talk to the running server.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ActiveServerInfo {
    /// Host to connect to (wildcard bind addresses mapped to loopback).
    pub connect_host: String,
    pub bind_host: String,
    pub port: u16,
    /// Backend-only (telemetry, `get_server_api_key`); never serialized to the frontend.
    #[serde(skip_serializing, default)]
    #[cfg_attr(feature = "openapi", schema(ignore))]
    pub api_key: Option<String>,
    pub api_key_set: bool,
    /// Requests need a key: the user's (`api_key_set`) or one made for this run (a Python
    /// worker always has one). The frontend fetches it with `get_server_api_key`.
    #[serde(default)]
    pub api_key_required: bool,
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub model_path: PathBuf,
    pub router_mode: bool,
    /// Started on a network address without an API key after the user's "open anyway".
    #[serde(default)]
    pub exposure_acknowledged: bool,
    /// `--cors-origins` sent to this run (`None` = llama-server's default).
    #[serde(default)]
    pub cors_origins: Option<String>,
    /// What this run serves: chat, embeddings (`--embeddings`) or reranking (`--rerank`).
    #[serde(default)]
    pub serve_mode: crate::serve_mode::ServeMode,
    /// Runtime family serving this run (`llama.cpp`, `mlx`, `mlx-vlm`, `mlx-audio`, ...): the
    /// routes a client can call depend on it. Empty for runs recorded before it existed.
    #[serde(default)]
    pub engine_family: String,
}

/// The key is redacted: a `{:?}` of this (logs, panics, tracing) must never print it.
impl std::fmt::Debug for ActiveServerInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActiveServerInfo")
            .field("connect_host", &self.connect_host)
            .field("bind_host", &self.bind_host)
            .field("port", &self.port)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("api_key_set", &self.api_key_set)
            .field("api_key_required", &self.api_key_required)
            .field("model_path", &self.model_path)
            .field("router_mode", &self.router_mode)
            .field("exposure_acknowledged", &self.exposure_acknowledged)
            .field("cors_origins", &self.cors_origins)
            .field("serve_mode", &self.serve_mode)
            .field("engine_family", &self.engine_family)
            .finish()
    }
}

impl ActiveServerInfo {
    /// A llama-server (telemetry and capability probes use its `/props`, `/slots`,
    /// `/metrics`); whisper, ONNX and image servers have only `/health`.
    pub fn is_llama(&self) -> bool {
        self.serve_mode != crate::serve_mode::ServeMode::Image
            && crate::engine::is_llama_server(&self.model_path, self.router_mode)
    }
}

/// The launch chooses the host prompt cache size itself (a catalog value or `--cache-ram`).
fn cache_ram_is_set(config: &LaunchConfig) -> bool {
    use crate::params::ParamValue;
    let param = match config.params.get("cache_ram") {
        Some(ParamValue::Text(t)) => !t.trim().is_empty(),
        Some(_) => true,
        None => false,
    };
    param
        || config.custom_args.as_deref().is_some_and(|a| {
            a.split_whitespace()
                .any(|t| matches!(t.split('=').next().unwrap_or(t), "--cache-ram" | "-cram"))
        })
}

/// The launch sets `fit` to `on` explicitly: `-ngl` is left to llama-server's fitting.
pub fn fit_chooses_layers(config: &LaunchConfig) -> bool {
    !config.router_mode
        && matches!(
            config.params.get("fit"),
            Some(crate::params::ParamValue::Text(v)) if v.trim().eq_ignore_ascii_case("on")
        )
}

/// llama-server's `/health`: 200 `{"status":"ok"}` once the model is loaded; 503 with an
/// `error` object while it is loading.
pub fn health_ready(status: u16, body: &str) -> bool {
    (200..300).contains(&status)
        && serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(|s| s == "ok"))
            .unwrap_or(false)
}

/// Maps wildcard bind addresses to a connectable loopback address.
/// The executable that runs [`crate::netns_bridge`] for a server bound to `host`, or why
/// that server cannot be isolated here.
fn bridge_plan(host: &str) -> Result<PathBuf, String> {
    let st = crate::pyenv::isolation::status();
    if !st.available {
        return Err(st.reason.clone().unwrap_or_default());
    }
    // Inside the namespace only the loopback exists: a server bound to a LAN address could not
    // listen there.
    let inside = connect_host(host);
    if !matches!(inside.as_str(), "127.0.0.1" | "localhost" | "[::1]" | "::1") {
        return Err(format!("bound to {host}, not loopback"));
    }
    std::env::current_exe().map_err(|e| format!("current executable: {e}"))
}

pub fn connect_host(bind: &str) -> String {
    match bind {
        "0.0.0.0" | "" => "127.0.0.1".to_string(),
        "::" | "[::]" => "[::1]".to_string(),
        h => h.to_string(),
    }
}

/// Open log file of the current run and when the run started.
/// Open run log file, when the run started, bytes written to this file.
type RunLog = (std::io::LineWriter<std::fs::File>, Instant, u64);

/// A LoRA path llama-server's `--lora-scaled FNAME:SCALE` cannot take (it contains `:`, as
/// every Windows drive path does).
fn lora_needs_post_scale(path: &str) -> bool {
    path.contains(':')
}

/// The scale to set through `POST /lora-adapters` after the start (the adapter was loaded with
/// `--lora-init-without-apply` because its path could not carry the scale), if any.
fn lora_post_scale(config: &LaunchConfig) -> Option<f32> {
    let path = config
        .lora_path
        .as_deref()
        .filter(|p| !p.as_os_str().is_empty())?
        .to_string_lossy();
    let scale = config.lora_scale?;
    (lora_needs_post_scale(&path) && (scale - 1.0).abs() > f32::EPSILON).then_some(scale)
}

/// A log line with the moment it reached the buffer (milliseconds since the Unix epoch).
/// The Dashboard merges the lines of several servers by this time.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TimedLogLine {
    pub at_ms: u64,
    pub text: String,
}

#[derive(Clone)]
pub struct ProcessManager {
    child: Arc<Mutex<Option<GroupChild>>>,
    status: Arc<Mutex<ServerStatus>>,
    ring_buffer: Arc<Mutex<VecDeque<TimedLogLine>>>,
    max_log_lines: usize,
    active_port: Arc<Mutex<u16>>,
    /// Incremented on every start/stop so a stale supervisor never touches a newer run.
    generation: Arc<std::sync::atomic::AtomicU64>,
    /// Incremented once per [`ProcessManager::start`] (a user's start). A retry of the same
    /// start on a new automatic port spawns a new process (new PID and generation) but keeps
    /// this number.
    starts: Arc<std::sync::atomic::AtomicU64>,
    /// Monotonic counter of log lines ever pushed (lets the UI fetch only new lines).
    log_seq: Arc<std::sync::atomic::AtomicU64>,
    /// Connection info of the current run.
    active: Arc<Mutex<Option<ActiveServerInfo>>>,
    /// Server instance id (names the per-run log files).
    id: String,
    /// Log file of the current run (`logs/servers/<id>/`), with run start time.
    log_file: Arc<Mutex<Option<RunLog>>>,
    /// Serializes start, stop, and supervisor cleanup across await points.
    lifecycle: Arc<tokio::sync::Mutex<()>>,
    /// Launch config of the current (or last) run, with the resolved API key; backend-only.
    launch_config: Arc<Mutex<Option<LaunchConfig>>>,
    /// Engine family of the current (or last) run, named in its failure messages.
    engine: Arc<Mutex<&'static str>>,
    /// What the running server actually uses (read from `/props` and the log after start).
    effective: Arc<Mutex<Option<crate::capabilities::verify::EffectiveLaunchValues>>>,
    /// KV cache types the current run was launched with ([`crate::kv_policy::apply`]).
    kv_launched: Arc<Mutex<Option<KvLaunched>>>,
    /// Why the last run failed (cleared when a run starts).
    failure: Arc<Mutex<Option<ServerFailure>>>,
    /// Log sequence number of the current run's first line (classification reads only it).
    run_log_start: Arc<std::sync::atomic::AtomicU64>,
    /// Automatic restart after a crash (attempt counter, pending restart).
    restart: Arc<Mutex<RestartTrack>>,
    /// stdout/stderr reader threads still running.
    readers: Arc<std::sync::atomic::AtomicUsize>,
    /// Automatic-port runs restarted on another port after llama-server could not bind its
    /// port (another program took it between our check and its bind); reset by a user start.
    port_retries: Arc<std::sync::atomic::AtomicU32>,
    /// An ONNX run started again on DirectML after its CUDA provider could not be used (once
    /// per user start).
    dml_retried: Arc<std::sync::atomic::AtomicBool>,
    /// The port-to-socket relay of an isolated Python worker run and its socket folder (the
    /// task is aborted and the folder removed when the run ends).
    relay: Arc<Mutex<Option<WorkerRelay>>>,
    /// Temporary configuration folder of the resident audio.cpp server.
    audio_run: Arc<Mutex<Option<tempfile::TempDir>>>,
    activity: Arc<Mutex<ProcessActivity>>,
}

struct ProcessActivity {
    last_used: Instant,
    in_flight: usize,
    retiring: bool,
    epoch: u64,
}

impl Default for ProcessActivity {
    fn default() -> Self {
        Self {
            last_used: Instant::now(),
            in_flight: 0,
            retiring: false,
            epoch: 0,
        }
    }
}

/// Keeps a server active until the forwarded request (including its body) completes.
pub struct RequestActivity {
    epoch: u64,
    activity: Arc<Mutex<ProcessActivity>>,
}

impl Drop for RequestActivity {
    fn drop(&mut self) {
        let mut activity = self.activity.lock().unwrap();
        if activity.epoch != self.epoch {
            return;
        }
        activity.in_flight = activity.in_flight.saturating_sub(1);
        activity.last_used = Instant::now();
    }
}

/// The relay task of an isolated Python worker and the folder of its Unix socket.
type WorkerRelay = (tokio::task::JoinHandle<()>, PathBuf);

/// Bind failures on an automatic port retried on the next free one before the start fails.
pub const MAX_PORT_RETRIES: u32 = 3;

/// KV cache settings a run was launched with (the stored config may leave them to the
/// automatic policy).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct KvLaunched {
    pub cache_type_k: Option<String>,
    pub cache_type_v: Option<String>,
    pub flash_attn: bool,
    /// Resolved count actually passed to (or selected by) llama-server.
    #[serde(default)]
    pub parallel_slots: Option<u32>,
    pub applied: crate::kv_policy::Applied,
}

/// Harness-level launch setting: seconds to wait for `/health` (not a llama-server flag).
/// Absent = `AppSettings.server.load_timeout_secs`.
pub const LOAD_TIMEOUT: &str = "load_timeout";
/// Accepted range of a load timeout, seconds.
pub const LOAD_TIMEOUT_RANGE: std::ops::RangeInclusive<u32> = 1..=7200;
/// A run that stayed up this long resets the restart attempt counter.
const RESTART_STABLE: Duration = Duration::from_secs(300);
/// First restart delay; doubles per attempt, capped at [`RESTART_MAX_DELAY`].
const RESTART_BASE_DELAY: Duration = Duration::from_secs(2);
const RESTART_MAX_DELAY: Duration = Duration::from_secs(60);

/// A scheduled automatic restart (shown as "restarting in N s, attempt k/N").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RestartState {
    /// 1-based attempt number of the scheduled restart.
    pub attempt: u32,
    pub max_attempts: u32,
    pub delay_secs: u64,
    /// Unix milliseconds when the restart begins.
    pub restart_at_ms: u64,
}

#[derive(Debug, Default)]
struct RestartTrack {
    /// Restarts done since the last user start (or the last stable run).
    attempts: u32,
    pending: Option<RestartState>,
    /// When the current run became ready.
    ready_at: Option<Instant>,
}

/// Delay before restart attempt `attempt` (1-based): 2 s, 4 s, 8 s... at most 60 s.
pub fn restart_delay(attempt: u32) -> Duration {
    let factor = 1u32 << attempt.saturating_sub(1).min(10);
    (RESTART_BASE_DELAY * factor).min(RESTART_MAX_DELAY)
}

/// Load timeout of a launch: its own `load_timeout` param, else the setting.
pub fn load_timeout_for(config: &LaunchConfig, settings_secs: u32) -> Duration {
    let own = match config.params.get(LOAD_TIMEOUT) {
        Some(crate::params::ParamValue::Int(n)) => u32::try_from(*n).ok(),
        Some(crate::params::ParamValue::Float(f)) if *f > 0.0 => Some(*f as u32),
        Some(crate::params::ParamValue::Text(t)) => t.trim().parse().ok(),
        _ => None,
    };
    let secs = own.unwrap_or(settings_secs);
    Duration::from_secs(u64::from(
        secs.clamp(*LOAD_TIMEOUT_RANGE.start(), *LOAD_TIMEOUT_RANGE.end()),
    ))
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self {
            child: Arc::new(Mutex::new(None)),
            status: Arc::new(Mutex::new(ServerStatus::Stopped)),
            ring_buffer: Arc::new(Mutex::new(VecDeque::with_capacity(1000))),
            max_log_lines: 1000,
            active_port: Arc::new(Mutex::new(8080)),
            generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            starts: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            log_seq: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            active: Arc::new(Mutex::new(None)),
            id: crate::servers::DEFAULT_SERVER_ID.to_string(),
            log_file: Arc::new(Mutex::new(None)),
            lifecycle: Arc::new(tokio::sync::Mutex::new(())),
            launch_config: Arc::new(Mutex::new(None)),
            engine: Arc::new(Mutex::new(crate::engine::LLAMA_CPP)),
            effective: Arc::new(Mutex::new(None)),
            kv_launched: Arc::new(Mutex::new(None)),
            failure: Arc::new(Mutex::new(None)),
            run_log_start: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            restart: Arc::new(Mutex::new(RestartTrack::default())),
            readers: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            port_retries: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            dml_retried: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            relay: Arc::new(Mutex::new(None)),
            audio_run: Arc::new(Mutex::new(None)),
            activity: Arc::new(Mutex::new(ProcessActivity::default())),
        }
    }
}

impl ProcessManager {
    /// Owned per-worker artifact directory, kept alive until the worker stops.
    pub fn audio_output_dir(&self) -> Option<PathBuf> {
        self.audio_run
            .lock()
            .unwrap()
            .as_ref()
            .map(|d| d.path().join("outputs"))
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_id(id: &str) -> Self {
        Self {
            id: id.to_string(),
            ..Self::default()
        }
    }

    pub fn request_activity(&self) -> Option<RequestActivity> {
        let mut activity = self.activity.lock().unwrap();
        if activity.retiring {
            return None;
        }
        activity.in_flight += 1;
        activity.last_used = Instant::now();
        Some(RequestActivity {
            epoch: activity.epoch,
            activity: self.activity.clone(),
        })
    }

    async fn stop_audio_if_idle(&self, generation: u64, idle: Duration) -> bool {
        let _lifecycle = self.lifecycle.lock().await;
        if !self.is_current(generation) || self.get_status() != ServerStatus::Running {
            return false;
        }
        {
            let mut activity = self.activity.lock().unwrap();
            if activity.in_flight != 0 || activity.last_used.elapsed() < idle {
                return false;
            }
            activity.retiring = true;
        }
        let _ = self
            .stop_locked_until(Instant::now() + Duration::from_secs(7))
            .await;
        true
    }

    /// Test seam: this instance looks Running with `active`, without a process. Only the Control
    /// API's integration tests call it (a server key needs a running server).
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn simulate_running_for_tests(&self, active: ActiveServerInfo) {
        *self.active.lock().unwrap() = Some(active);
        *self.status.lock().unwrap() = ServerStatus::Running;
    }

    /// Config the current (or last) run was started with.
    pub fn launch_config(&self) -> Option<LaunchConfig> {
        self.launch_config.lock().unwrap().clone()
    }

    /// Effective values of the running server (`None` until checked after start, and
    /// after it stopped).
    pub fn effective_values(&self) -> Option<crate::capabilities::verify::EffectiveLaunchValues> {
        match self.get_status() {
            ServerStatus::Running => self.effective.lock().unwrap().clone(),
            _ => None,
        }
    }

    /// The CUDA build converts the K/V `pair` to f16 on every token: a log note, and the pair
    /// in the effective values. The warning comes with the first generation, usually after
    /// the effective values were read at start.
    fn note_kv_fa_fallback(&self, pair: &str, lang: &str) {
        if let Some(values) = self.effective.lock().unwrap().as_mut() {
            values.kv_fa_fallback = Some(pair.to_string());
        }
        self.push_log(crate::capabilities::verify::kv_fa_fallback_note(pair, lang));
    }

    /// After the server reported healthy: checks endpoints, records them in the capability
    /// registry and stores the effective launch values. Best effort; never fails the run.
    async fn verify_after_start(&self, generation: u64) {
        let (Some(config), Some(active)) = (self.launch_config(), self.active_server()) else {
            return;
        };
        // The probes are llama-server's endpoints (`/props`, `/slots`, `/metrics`...).
        if !active.is_llama() {
            return;
        }
        let target = crate::capabilities::verify::ProbeTarget {
            base_url: format!("http://{}:{}", active.connect_host, active.port),
            api_key: active.api_key.clone(),
            router_mode: config.router_mode,
            has_mmproj: config
                .mmproj_path
                .as_ref()
                .is_some_and(|p| !p.as_os_str().is_empty()),
            serve_mode: active.serve_mode,
        };
        let report = crate::capabilities::verify::probe_server(&target).await;
        if !self.is_current(generation) {
            return;
        }
        let lang = crate::config::AppSettings::load().language;
        for line in crate::capabilities::verify::probe_log_notes(&report, &lang) {
            self.push_log(line);
        }
        let log = crate::capabilities::verify::parse_log(&self.get_logs(None));
        if log.fit_failed {
            self.push_log(crate::capabilities::verify::fit_failed_note(&lang));
        }
        let mut values = crate::capabilities::verify::EffectiveLaunchValues::combine(
            config.context_size,
            config.gpu_layers,
            config.parallel_slots.filter(|&n| n > 0),
            &report.props,
            &log,
        );
        {
            use crate::capabilities::verify::GpuLayerInput;
            let input = if config.router_mode
                || crate::capabilities::verify::custom_args_set_gpu_layers(
                    config.custom_args.as_deref(),
                ) {
                GpuLayerInput::Unknown
            } else if fit_chooses_layers(&config) {
                GpuLayerInput::Fit
            } else {
                GpuLayerInput::Explicit(config.gpu_layers)
            };
            let (model, binary) = (config.model_path.clone(), config.binary_path.clone());
            let (block_count, gpu_devices) = tokio::task::spawn_blocking(move || {
                let blocks = crate::gguf::read_gguf_metadata(&model)
                    .ok()
                    .and_then(|m| m.block_count);
                let devices = crate::capabilities::registry()
                    .cached(&binary)
                    .filter(|c| c.help_parsed)
                    .map(|c| c.devices.len());
                (blocks, devices)
            })
            .await
            .unwrap_or((None, None));
            if !self.is_current(generation) {
                return;
            }
            values.apply_gpu_layer_rule(input, block_count, gpu_devices);
        }
        if values.ctx_adjusted {
            if let Some(n) = values.n_ctx {
                self.push_log(format!(
                    "[Harness Studio] Etkin bağlam: istenen {} -> etkin {} (slot başına)",
                    config.context_size, n
                ));
            }
        }
        // `fit_failed` from the log alone misses a spill: llama-server skips its fit check when
        // `-ngl` is given (always, from the Harness). The OS counters see the moved memory.
        if !values.fit_failed && config.gpu_layers > 0 {
            if let Some(pid) = self.pid() {
                let adapter = config.main_gpu;
                let spilled = tokio::task::spawn_blocking(move || {
                    // On an integrated GPU shared memory is where the model belongs (an APU
                    // with a 512 MiB+ carve-out would otherwise look like a spill).
                    let target = adapter.or_else(crate::gpu::primary_gpu_index)?;
                    crate::gpu::list_adapters()
                        .iter()
                        .find(|a| a.index == target)
                        .filter(|a| a.is_discrete)?;
                    let shared = crate::gpu::SharedProbe::open()
                        .and_then(|mut p| p.sample())
                        .and_then(|m| m.get(&pid).copied())?;
                    let dedicated = crate::gpu::process_vram_bytes(pid)?;
                    Some((dedicated, shared))
                })
                .await
                .ok()
                .flatten();
                // Stopped or started again while the counters were read: not this run's values.
                if !self.is_current(generation) {
                    return;
                }
                if let Some((dedicated, shared)) = spilled {
                    values.shared_memory_bytes = Some(shared);
                    if crate::capabilities::verify::spilled_to_shared(dedicated, shared) {
                        values.fit_failed = true;
                        self.push_log(crate::capabilities::verify::spilled_note(shared, &lang));
                    } else if crate::capabilities::verify::shared_memory_slowdown(dedicated, shared)
                    {
                        values.shared_memory_slowdown = true;
                        self.push_log(crate::capabilities::verify::shared_slowdown_note(
                            shared, &lang,
                        ));
                    }
                }
            }
        }
        *self.effective.lock().unwrap() = Some(values);
        let binary = config.binary_path.clone();
        // The measurement is keyed by the cache types the run really used.
        let mut config = config;
        if let Some(kv) = self.kv_launched() {
            config.cache_type_k = kv.cache_type_k;
            config.cache_type_v = kv.cache_type_v;
            config.flash_attn = kv.flash_attn;
            // The saved form intentionally keeps "Auto", but a measurement must be keyed by
            // what this process actually launched. Do not re-run the slot rule after KV auto
            // has changed the cache types.
            config.parallel_slots = kv.parallel_slots;
        }
        let (lines, pid) = (self.get_logs(None), self.pid());
        let serve_mode = active.serve_mode;
        let measuring = tokio::task::spawn_blocking(move || {
            let registry = crate::capabilities::registry();
            let caps = registry.cached(&binary);
            if let Some(caps) = &caps {
                registry.record_probe(&caps.key.id(), &report);
            }
            // Measured memory footprint of this load (corrects later VRAM estimates).
            crate::vram::record_after_load(
                &config,
                &lines,
                pid,
                caps.and_then(|c| c.version.build),
            );
            // First load of this model on an integrated GPU: a short background measurement
            // (CPU vs full offload) so the `-ngl` recommendation follows the real direction.
            crate::auto_bench::maybe_start(&config, serve_mode)
        })
        .await;
        if measuring.unwrap_or(false) && self.is_current(generation) {
            self.push_log(
                if lang == "en" {
                    "[Harness Studio] First load of this model on an integrated GPU: a short background measurement (CPU vs GPU, up to 90 s) records which is faster; turn it off in Settings > Server defaults."
                } else {
                    "[Harness Studio] Bu model tümleşik GPU'da ilk kez yüklendi: CPU ve GPU hızını karşılaştıran kısa bir arka plan ölçümü (en çok 90 sn) başladı; Ayarlar > Sunucu varsayılanları'ndan kapatılabilir."
                }
                .to_string(),
            );
        }
    }

    /// KV cache settings of the current (or last) run.
    pub fn kv_launched(&self) -> Option<KvLaunched> {
        self.kv_launched.lock().unwrap().clone()
    }

    pub fn get_status(&self) -> ServerStatus {
        self.status.lock().unwrap().clone()
    }

    pub fn active_model(&self) -> Option<PathBuf> {
        self.active
            .lock()
            .unwrap()
            .as_ref()
            .map(|a| a.model_path.clone())
    }

    /// Connection info while the server is starting or running.
    pub fn active_server(&self) -> Option<ActiveServerInfo> {
        match self.get_status() {
            ServerStatus::Starting | ServerStatus::Running => self.active.lock().unwrap().clone(),
            _ => None,
        }
    }

    pub fn get_active_port(&self) -> u16 {
        *self.active_port.lock().unwrap()
    }

    /// OS process id of the running llama-server (the group leader).
    pub fn pid(&self) -> Option<u32> {
        self.child.lock().unwrap().as_ref().map(|c| c.id())
    }

    pub fn has_child(&self) -> bool {
        self.child.lock().unwrap().is_some()
    }

    pub fn get_logs(&self, tail: Option<usize>) -> Vec<String> {
        let buffer = self.ring_buffer.lock().unwrap();
        let count = tail.unwrap_or(buffer.len()).min(buffer.len());
        buffer
            .iter()
            .skip(buffer.len() - count)
            .map(|l| l.text.clone())
            .collect()
    }

    /// Returns `(next_seq, lines)` for all lines with sequence number >= `since`
    /// (limited to what is still in the ring buffer).
    pub fn get_logs_since(&self, since: u64) -> (u64, Vec<String>) {
        let (next, lines) = self.get_timed_logs_since(since);
        (next, lines.into_iter().map(|l| l.text).collect())
    }

    /// CLI/API tail: exclude earlier runs while keeping sequence cursors monotonic.
    pub fn get_run_logs_since(&self, since: u64) -> (u64, Vec<String>) {
        let start = self.run_log_start.load(std::sync::atomic::Ordering::SeqCst);
        self.get_logs_since(since.max(start))
    }

    /// [`Self::get_logs_since`] with the time each line was written.
    pub fn get_timed_logs_since(&self, since: u64) -> (u64, Vec<TimedLogLine>) {
        let buffer = self.ring_buffer.lock().unwrap();
        let next = self.log_seq.load(std::sync::atomic::Ordering::SeqCst);
        let first_in_buffer = next - buffer.len() as u64;
        let skip = since
            .saturating_sub(first_in_buffer)
            .min(buffer.len() as u64) as usize;
        (next, buffer.iter().skip(skip).cloned().collect())
    }

    pub fn clear_logs(&self) {
        self.ring_buffer.lock().unwrap().clear();
    }

    /// Adds a line to the server log (a note from outside the process, e.g. the gateway).
    pub fn append_log(&self, line: String) {
        self.push_log(line);
    }

    fn push_log(&self, line: String) {
        let mut buffer = self.ring_buffer.lock().unwrap();
        if buffer.len() >= self.max_log_lines {
            buffer.pop_front();
        }
        // Never earlier than the line before it (the clock can step back).
        let at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
            .max(buffer.back().map_or(0, |l| l.at_ms));
        buffer.push_back(TimedLogLine {
            at_ms,
            text: line.clone(),
        });
        self.log_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        drop(buffer);
        crate::changes::log_appended(&self.id);
        let mut log_file = self.log_file.lock().unwrap();
        if let Some((file, started, written)) = log_file.as_mut() {
            use std::io::Write;
            let entry = format!(
                "[+{:.3}s] {}
",
                started.elapsed().as_secs_f64(),
                line
            );
            let _ = file.write_all(entry.as_bytes());
            *written += entry.len() as u64;
            if *written > crate::server_logs::MAX_RUN_LOG_BYTES {
                // Continue in a new file; the old one counts towards the pruning limits.
                let started = *started;
                *log_file = crate::server_logs::open_run_log(&self.id).map(|f| (f, started, 0));
            }
        }
    }

    /// Why the last run failed (`None` while starting/running and after a clean stop).
    pub fn last_failure(&self) -> Option<ServerFailure> {
        self.failure.lock().unwrap().clone()
    }

    /// The scheduled automatic restart, if any.
    pub fn restart_state(&self) -> Option<RestartState> {
        self.restart.lock().unwrap().pending.clone()
    }

    /// Records a classified failure: kept for the UI/API and published as an event.
    fn record_failure(&self, mut failure: ServerFailure) {
        // The messages name the engine that failed (mlx, mistral.rs...), not always llama.cpp.
        if !failure.params.contains_key("engine") {
            failure = failure.with(
                "engine",
                crate::engine::display_name(*self.engine.lock().unwrap()),
            );
        }
        self.push_log(format!(
            "[Harness Studio Error] {} {}",
            failure.code,
            failure
                .params
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(" ")
        ));
        *self.failure.lock().unwrap() = Some(failure.clone());
        crate::changes::servers_changed();
        crate::events::publish(crate::events::CoreEvent::ServerFailed {
            server_id: self.id.clone(),
            failure: Box::new(failure),
        });
    }

    /// Log lines of the current (or last) run.
    fn run_lines(&self) -> Vec<String> {
        let since = self.run_log_start.load(std::sync::atomic::Ordering::SeqCst);
        self.get_logs_since(since).1
    }

    pub fn set_error(&self, msg: String) -> String {
        self.push_log(format!("[Harness Studio Error] {}", msg));
        self.set_status(ServerStatus::Error(msg.clone()));
        msg
    }

    /// Makes the instance look running (no process), for tests of what may stop it.
    #[cfg(test)]
    pub(crate) fn mark_running_for_tests(&self) {
        *self.status.lock().unwrap() = ServerStatus::Running;
    }

    /// Sets the status and publishes it on the event bus.
    fn set_status(&self, status: ServerStatus) {
        *self.status.lock().unwrap() = status.clone();
        self.publish_status(status);
    }

    fn publish_status(&self, status: ServerStatus) {
        crate::changes::servers_changed();
        crate::events::publish(crate::events::CoreEvent::ServerStatus {
            server_id: self.id.clone(),
            status,
        });
    }

    /// Number of [`start`](Self::start) calls so far (see the `starts` field).
    pub fn start_count(&self) -> u64 {
        self.starts.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Starts a run requested by the user (or a front end): resets the automatic restart
    /// counter and cancels a pending restart.
    pub async fn start(&self, config: LaunchConfig) -> Result<(), String> {
        self.starts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        {
            let mut track = self.restart.lock().unwrap();
            track.attempts = 0;
            track.pending = None;
        }
        self.port_retries
            .store(0, std::sync::atomic::Ordering::SeqCst);
        self.dml_retried
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.start_run(config, None).await
    }

    /// Starts a run. `restart_of` is the generation of the failed run an automatic restart
    /// replaces: under the lifecycle lock such a start goes ahead only while that run is still the
    /// current one and still in Error, so a stop (or a start by hand) that came first wins.
    async fn start_run(
        &self,
        mut config: LaunchConfig,
        restart_of: Option<u64>,
    ) -> Result<(), String> {
        if crate::engine::server_for(&config).family() == crate::engine::AUDIO_CPP {
            crate::engine::audio_cpp::require_enabled()?;
            config.host = "127.0.0.1".into();
            config.port = crate::engine::audio_cpp::random_port()?;
            config.port_auto = false;
        }
        let _lifecycle = self.lifecycle.lock().await;
        // 1. Atomically claim the "Starting" state so two concurrent starts can't both proceed.
        {
            let mut status = self.status.lock().unwrap();
            if restart_of
                .is_some_and(|g| !self.is_current(g) || !matches!(*status, ServerStatus::Error(_)))
            {
                return Ok(()); // stopped or started by hand while the restart waited
            }
            if matches!(
                *status,
                ServerStatus::Starting | ServerStatus::Running | ServerStatus::Stopping
            ) {
                return Err(err("servers.already_active").into());
            }
            *status = ServerStatus::Starting;
        }
        self.publish_status(ServerStatus::Starting);
        // A stop that ran out of time kept its process (status Error, child still tracked). End it
        // before a new one takes its place: overwriting the handle would leave it running,
        // untracked, with its memory and port. (Starting, Running and Stopping were refused above,
        // so a child here is always such a leftover.)
        if self.has_child() {
            self.push_log(
                "[Harness Studio] The previous process is still running; stopping it first."
                    .to_string(),
            );
            if let Err(error) = self
                .terminate_child_until(Instant::now() + Duration::from_secs(7))
                .await
            {
                self.push_log(format!("[Harness Studio Error] {}", error));
                return Err(self.set_error(err("servers.previous_still_running").into()));
            }
            // What a clean stop would have cleared.
            self.active.lock().unwrap().take();
            self.audio_run.lock().unwrap().take();
            self.stop_relay();
            crate::mcp::remove_runtime_config(&self.id);
        }
        self.run_claimed(config).await
    }

    /// The start after the "Starting" claim (the caller holds `lifecycle`): checks, arguments,
    /// spawn and the supervisor. A bind-failure retry on a new automatic port comes here
    /// directly, so the server stays "Starting" throughout.
    async fn run_claimed(&self, config: LaunchConfig) -> Result<(), String> {
        *self.engine.lock().unwrap() = crate::engine::server_for(&config).family();
        self.failure.lock().unwrap().take();
        self.restart.lock().unwrap().ready_at = None;
        self.run_log_start.store(
            self.log_seq.load(std::sync::atomic::Ordering::SeqCst),
            std::sync::atomic::Ordering::SeqCst,
        );
        *self.log_file.lock().unwrap() =
            crate::server_logs::open_run_log(&self.id).map(|f| (f, Instant::now(), 0));

        if !config.binary_path.exists() {
            self.record_failure(
                ServerFailure::new(FailureKind::StartFailed, FailurePhase::Loading)
                    .with("detail", "runtime_missing"),
            );
            return Err(self.set_error(
                err("process.binary_not_found")
                    .with("path", config.binary_path.display())
                    .with(
                        "engine",
                        crate::engine::display_name(crate::engine::server_for(&config).family()),
                    )
                    .into(),
            ));
        }
        let model_target = if config.router_mode {
            config.models_path.as_ref().unwrap_or(&config.model_path)
        } else {
            &config.model_path
        };
        if !model_target.exists() {
            self.record_failure(ServerFailure::new(
                FailureKind::ModelNotFound,
                FailurePhase::Loading,
            ));
            return Err(self.set_error(
                err("process.model_not_found")
                    .with("path", model_target.display())
                    .into(),
            ));
        }

        let engine = crate::engine::server_for(&config);
        // llama-server gets the llama.cpp launch preparation (serving mode, slots, KV policy,
        // GPU index); another family (whisper-server) starts with what its engine builds.
        let is_llama = engine.family() == crate::engine::LLAMA_CPP;
        let whisper_cuda = engine.family() == crate::engine::WHISPER_CPP
            && config.binary_path.parent().is_some_and(|dir| {
                crate::runtime::detect_backend_in_dir(dir).as_deref() == Some("cuda")
            });
        let program = engine.release_source().binary;
        let runtime = {
            let bin = config.binary_path.clone();
            tokio::task::spawn_blocking(move || engine.detect_capabilities(&bin))
                .await
                .ok()
        };
        let caps = runtime
            .as_deref()
            .filter(|_| is_llama)
            .map(ServerCaps::from_runtime)
            .unwrap_or_default();
        let runtime_for_params = runtime.as_deref().filter(|r| {
            if is_llama {
                r.looks_like_server()
            } else {
                r.help_parsed
            }
        });
        // Serving mode (embeddings / rerank from the file kind) and launch-time fixes; the
        // stored config keeps the user's settings, only the arguments get the result.
        let meta = if config.router_mode || !is_llama {
            None
        } else {
            let path = config.model_path.clone();
            tokio::task::spawn_blocking(move || crate::models::cached_metadata(&path))
                .await
                .ok()
                .flatten()
        };
        let mut launch = config.clone();
        let prepared = if is_llama {
            crate::serve_mode::prepare(&mut launch, meta.as_ref(), runtime_for_params)
        } else if engine.family() == crate::engine::ONNX_FAMILY {
            // The sidecar reads its task from the repo files; the log and the UI show it.
            let task = match launch.params.get("task") {
                Some(crate::params::ParamValue::Text(value)) => {
                    harness_onnx::package::Task::parse(value)
                }
                _ => harness_onnx::package::library_task(&launch.model_path),
            };
            crate::serve_mode::Prepared {
                mode: if harness_onnx::genai_pkg::is_chat_model(&launch.model_path) {
                    // An onnxruntime-genai folder (genai_config.json) is a chat model.
                    crate::serve_mode::ServeMode::Chat
                } else {
                    match task {
                        Some(harness_onnx::package::Task::AsrCtc) => {
                            crate::serve_mode::ServeMode::Transcription
                        }
                        Some(harness_onnx::package::Task::Rerank) => {
                            crate::serve_mode::ServeMode::Rerank
                        }
                        Some(
                            harness_onnx::package::Task::Classify
                            | harness_onnx::package::Task::TokenClassify
                            | harness_onnx::package::Task::FillMask
                            | harness_onnx::package::Task::QuestionAnswering,
                        ) => crate::serve_mode::ServeMode::Pipeline,
                        // A vision or audio model (no tokenizer, a processor config).
                        None if harness_onnx::vision_pkg::VisionPackage::discover(
                            &launch.model_path,
                        )
                        .is_some()
                            || harness_onnx::audio_pkg::AudioPackage::discover(
                                &launch.model_path,
                            )
                            .is_some()
                            || harness_onnx::seq2seq_pkg::Seq2SeqPackage::discover(
                                &launch.model_path,
                            )
                            .is_some() =>
                        {
                            crate::serve_mode::ServeMode::Pipeline
                        }
                        _ => crate::serve_mode::ServeMode::Embedding,
                    }
                },
                notes: Vec::new(),
            }
        } else if engine.family() == crate::engine::PYTHON_HF {
            // Embedding and ranking models keep their OpenAI routes; other tasks are pipelines.
            crate::serve_mode::Prepared {
                mode: crate::engine::python_hf::serve_mode(&launch.model_path),
                notes: Vec::new(),
            }
        } else {
            crate::serve_mode::Prepared {
                mode: if crate::engine::crispasr_tts::is_model(&launch.model_path) {
                    crate::serve_mode::ServeMode::Pipeline
                } else {
                    crate::engine::serve_mode_of(engine.family())
                },
                notes: Vec::new(),
            }
        };
        // Automatic context (`context_size` 0): resolved here for every launch path.
        if launch.context_size == 0 {
            // A mistral.rs chat model's trained context is in its `config.json`.
            let mistralrs = engine.family() == crate::engine::MISTRALRS;
            let trained = if mistralrs {
                crate::engine::mistralrs_trained_context(&launch.model_path)
            } else {
                meta.as_ref().and_then(|m| m.context_length)
            };
            let chat = prepared.mode == crate::serve_mode::ServeMode::Chat
                && (is_llama || mistralrs)
                && !launch.router_mode;
            launch.context_size =
                crate::prepare::resolve_auto_context(chat, launch.custom_args.as_deref(), trained);
            // Context, `-ngl` and cache all automatic: the same rule as the gateway's load on
            // request keeps the whole model on a small card's GPU (`full_offload`).
            let own_context =
                crate::prepare::custom_args_set_context(launch.custom_args.as_deref());
            if is_llama && crate::full_offload::applies_to_start(&launch, chat && !own_context) {
                let (probe, requested) = (launch.clone(), launch.context_size);
                let choice = tokio::task::spawn_blocking(move || {
                    let mut probe = probe;
                    let hw = crate::hardware::detect_hardware();
                    let limit = crate::full_offload::os_need_limit(&hw, 0);
                    crate::full_offload::apply_to_launch_with(
                        &mut probe,
                        requested,
                        |c| crate::vram::fit_launch(c, &hw),
                        limit,
                    )
                    .map(|f| (f, probe))
                })
                .await
                .ok()
                .flatten();
                if let Some((f, chosen)) = choice {
                    launch = chosen;
                    let lang = crate::config::AppSettings::load().language;
                    if f.context != requested {
                        self.push_log(crate::full_offload::full_offload_log_line(
                            f.context, requested, &lang,
                        ));
                    }
                    if f.free_memory_cut {
                        self.push_log(crate::full_offload::free_memory_log_line(f.layers, &lang));
                    }
                }
            }
        }
        // Concurrent requests: the automatic count (or the set value) and `--kv-unified`.
        let slot_plan = if is_llama {
            let c = launch.clone();
            tokio::task::spawn_blocking(move || {
                crate::slots::plan_for_launch(&c, &crate::vram::base_input_from_launch(&c))
            })
            .await
            .ok()
            .flatten()
        } else {
            None
        };
        if let Some(plan) = &slot_plan {
            launch.parallel_slots = plan.np;
            if let Some(unified) = plan.kv_unified {
                launch.params.insert(
                    "kv_unified".to_string(),
                    crate::params::ParamValue::Bool(unified),
                );
            }
            let lang = crate::config::AppSettings::load().language;
            self.push_log(plan.advice.log_line(&lang));
        }
        // KV cache precision: automatic types when none are set, flash attention for a
        // quantized V cache (see `kv_policy`). Chat runs only; embeddings keep tiny caches.
        let kv_applied = if is_llama && prepared.mode == crate::serve_mode::ServeMode::Chat {
            let fa = runtime_for_params.is_none_or(|r| r.has_flag("--flash-attn"));
            let (mut l, m) = (launch.clone(), meta.clone());
            let res = tokio::task::spawn_blocking(move || {
                let input = crate::vram::input_from_launch(&l);
                let budget = crate::vram::budget_for_model(
                    &crate::hardware::detect_hardware(),
                    input.model_path.as_deref(),
                );
                let measured = input
                    .model_path
                    .as_deref()
                    .map(crate::vram::measurements_for)
                    .unwrap_or_default();
                let applied = crate::kv_policy::apply(&mut l, m.as_ref(), &budget, &measured, fa);
                (l, applied)
            })
            .await;
            match res {
                Ok((l, applied)) => {
                    launch = l;
                    Some(applied)
                }
                Err(_) => None,
            }
        } else {
            None
        };
        // `main_gpu` is an OS adapter index; `--main-gpu` takes the runtime's (see
        // `devices::pair_adapters`). Converted here, for this launch only.
        if is_llama && launch.gpu_layers > 0 {
            if let Some(adapter) = launch.main_gpu.or_else(crate::gpu::primary_gpu_index) {
                let explicit = launch.main_gpu.is_some();
                let binary = launch.binary_path.clone();
                let index = tokio::task::spawn_blocking(move || {
                    crate::devices::runtime_index_for_adapter(&binary, adapter)
                })
                .await
                .map_err(|e| self.set_error(e.to_string()))?;
                launch.main_gpu = match index {
                    Ok(index) => Some(index),
                    Err(error) if explicit => return Err(self.set_error(error)),
                    // No verified mapping: let the backend choose its default instead of
                    // passing a DXGI index that might name a different backend device.
                    Err(_) => Some(0),
                };
            }
        }
        // Several slots in one unified KV pool: the host prompt cache stalls every wave of
        // concurrent requests on Vulkan, but on CUDA it is what clears idle slots from the pool
        // (`slots::host_cache_off_for_unified_pool` has the measurements). Judged by the device
        // the server runs on (`--main-gpu` is the runtime's index by now).
        if let Some(plan) = &slot_plan {
            if crate::slots::runs_unified_pool(plan, &launch)
                && !cache_ram_is_set(&launch)
                && runtime_for_params.is_some_and(|r| {
                    r.has_flag("--cache-ram")
                        && crate::slots::host_cache_off_for_unified_pool(
                            &crate::slots::served_backends(
                                r,
                                launch.main_gpu,
                                launch.split_mode.as_deref(),
                            ),
                        )
                })
            {
                launch
                    .params
                    .insert("cache_ram".to_string(), crate::params::ParamValue::Int(0));
                let lang = crate::config::AppSettings::load().language;
                self.push_log(if lang == "tr" {
                    "[Harness Studio] Ortak bağlamda birden çok eşzamanlı istek: llama.cpp'nin ana bellek istem önbelleği kapatıldı (--cache-ram 0); açıkken her istek dalgası ~3,7 sn takılıyor. Açmak için Önbellek RAM değerini elle girin.".to_string()
                } else {
                    "[Harness Studio] Several concurrent requests share one context pool: llama.cpp's host prompt cache is off (--cache-ram 0); with it on every wave of requests stalls for ~3.7 s. Set the cache RAM yourself to turn it back on.".to_string()
                });
            }
        }
        *self.kv_launched.lock().unwrap() = Some(KvLaunched {
            cache_type_k: launch.cache_type_k.clone(),
            cache_type_v: launch.cache_type_v.clone(),
            flash_attn: launch.flash_attn,
            parallel_slots: slot_plan.as_ref().map(|p| p.advice.slots),
            applied: kv_applied.clone().unwrap_or_default(),
        });
        if engine.family() == crate::engine::AUDIO_CPP {
            let dir = tempfile::Builder::new()
                .prefix("harness-audio-")
                .tempdir()
                .map_err(|e| self.set_error(e.to_string()))?;
            // Configure the worker's cwd/request base inside its owned output
            // directory. v0.9.1 returns inline audio; any file artifact is only
            // readable by the gateway inside this directory.
            let outputs = dir.path().join("outputs");
            std::fs::create_dir_all(&outputs).map_err(|e| self.set_error(e.to_string()))?;
            crate::engine::audio_cpp::write_config(&launch, &outputs)
                .map_err(|e| self.set_error(e))?;
            *self.audio_run.lock().unwrap() = Some(dir);
        }
        let mut args = match engine.build_launch(&launch, &caps, runtime_for_params) {
            Ok(out) => {
                for s in &out.skipped {
                    self.push_log(format!(
                        "[Harness Studio Warn] Parametre gönderilmedi: {} ({})",
                        s.id, s.reason
                    ));
                }
                out.args
            }
            Err(e) => return Err(self.set_error(e)),
        };
        // Router mode: every model definition in the served directory loads with its own
        // settings (`--models-preset`), as a single-model server would start it.
        if is_llama
            && launch.router_mode
            && runtime_for_params.is_some_and(|r| r.has_flag("--models-preset"))
            && !args.iter().any(|a| a.starts_with("--models-preset"))
        {
            let (id, c, rt) = (self.id.clone(), launch.clone(), runtime.clone());
            let written = tokio::task::spawn_blocking(move || {
                crate::router::write_preset_for_launch(&id, &c, rt.as_deref())
            })
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
            match written {
                Ok(Some(path)) => {
                    self.push_log(format!(
                        "[Harness Studio] Router preset: {}",
                        path.display()
                    ));
                    args.push("--models-preset".to_string());
                    args.push(path.to_string_lossy().to_string());
                }
                Ok(None) => {}
                Err(e) => self.push_log(format!("[Harness Studio Warn] Router preset: {e}")),
            }
        }
        self.effective.lock().unwrap().take();

        *self.active_port.lock().unwrap() = config.port;
        {
            // The resolved automatic context is what the run really asked for (the effective
            // value check compares against it).
            let mut stored = config.clone();
            if stored.context_size == 0 {
                stored.context_size = launch.context_size;
                // The default load chose context and layers together.
                if stored.gpu_layers_auto {
                    stored.gpu_layers = launch.gpu_layers;
                }
            }
            *self.launch_config.lock().unwrap() = Some(stored);
        }
        // A Python worker always has a key (the user's, else one for this run): its port is a
        // relay any local process could reach.
        let is_worker = engine.worker_env("").is_some();
        let worker_key = if engine.family() == crate::engine::CRISPASR {
            Some(crate::network::generate_api_key().map_err(|e| self.set_error(e))?)
        } else if is_worker {
            match config.api_key.clone().filter(|k| !k.is_empty()) {
                Some(key) => Some(key),
                None => Some(crate::network::generate_api_key().map_err(|e| self.set_error(e))?),
            }
        } else {
            None
        };
        *self.active.lock().unwrap() = Some(ActiveServerInfo {
            connect_host: connect_host(&config.host),
            bind_host: config.host.clone(),
            port: config.port,
            api_key: worker_key
                .clone()
                .or_else(|| config.api_key.clone().filter(|k| !k.is_empty())),
            api_key_set: config.api_key.as_deref().is_some_and(|k| !k.is_empty()),
            api_key_required: worker_key.is_some()
                || config.api_key.as_deref().is_some_and(|k| !k.is_empty()),
            model_path: model_target.clone(),
            router_mode: config.router_mode,
            exposure_acknowledged: crate::network::check_launch(&config)
                == Ok(crate::network::Exposure::LanAcknowledged),
            cors_origins: config.cors_origins.clone(),
            serve_mode: prepared.mode,
            engine_family: engine.family().to_string(),
        });
        self.push_log(format!(
            "[Harness Studio] Starting {}: {:?}",
            program, model_target
        ));
        let kv_notes = kv_applied.as_ref().map(|a| !a.notes("en").is_empty());
        if !prepared.notes.is_empty() || kv_notes == Some(true) {
            let lang = crate::config::AppSettings::load().language;
            for note in &prepared.notes {
                self.push_log(note.message(&lang));
            }
            for line in kv_applied.iter().flat_map(|a| a.notes(&lang)) {
                self.push_log(line);
            }
        }
        // The launch's layers: the full-offload rule may have moved all of them to the CPU.
        if is_llama && launch.gpu_layers > 0 {
            if let Some(target) = config.main_gpu.or_else(crate::gpu::primary_gpu_index) {
                self.push_log(format!(
                    "[Harness Studio] Target GPU: adapter index {} → --main-gpu {} (split-mode: {})",
                    target,
                    launch.main_gpu.unwrap_or(target),
                    config.split_mode.as_deref().unwrap_or("none")
                ));
            }
        }

        // Working directory = binary directory so Windows finds ggml/cublas/vulkan DLLs.
        let cwd = config
            .binary_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));

        // An isolated Python worker (no network) listens on a Unix socket; the port is relayed
        // to it.
        self.stop_relay();
        let isolated = is_worker && crate::pyenv::isolation::available() && engine.uses_socket();
        // On Linux a worker that can run without network must: no isolation (unprivileged user
        // namespaces turned off) is a refusal with the reason, not a silent fallback.
        if cfg!(target_os = "linux") && is_worker && engine.uses_socket() && !isolated {
            let reason = crate::pyenv::isolation::status()
                .reason
                .clone()
                .unwrap_or_default();
            return Err(self.set_error(
                err("pyenv.isolation_unavailable")
                    .with("reason", reason)
                    .into(),
            ));
        }
        // A native server that needs no network but only listens on TCP runs behind the
        // namespace bridge (crate::netns_bridge); without isolation it runs as before.
        let bridge = if cfg!(target_os = "linux") && engine.netns_bridge() && !isolated {
            match bridge_plan(&config.host) {
                Ok(exe) => Some(exe),
                Err(reason) => {
                    self.push_log(format!(
                        "[Harness Studio] Network isolation unavailable, the server keeps network access: {reason}"
                    ));
                    None
                }
            }
        } else {
            None
        };
        let bridged = bridge.is_some() && !isolated;
        let mut cmd = if isolated {
            #[cfg(unix)]
            {
                match self.start_relay(&config).await {
                    Ok(socket) => engine.listen_on_socket(&mut args, &socket),
                    Err(e) => return Err(self.set_error(e)),
                }
            }
            let mut cmd = Command::new("unshare");
            cmd.args(["--user", "--map-root-user", "--net", "--"])
                .arg(&config.binary_path);
            cmd
        } else if let Some(exe) = bridge {
            #[cfg(unix)]
            let socket = match self.start_relay(&config).await {
                Ok(socket) => socket,
                Err(e) => return Err(self.set_error(e)),
            };
            #[cfg(not(unix))]
            let socket = PathBuf::new();
            self.push_log("[Harness Studio] Server: isolated (no network)".to_string());
            let mut cmd = Command::new("unshare");
            cmd.args(["--user", "--map-root-user", "--net", "--"])
                .arg(exe)
                .args(crate::netns_bridge::bridge_args(
                    &socket,
                    &connect_host(&config.host),
                    config.port,
                ))
                .arg(&config.binary_path);
            cmd
        } else {
            Command::new(&config.binary_path)
        };
        // The bridge is this executable (it needs the AppImage's libraries) and cleans the
        // environment for the server it starts.
        if !bridged {
            crate::appimage::foreign_std(&mut cmd);
        }
        if let Some(env) = worker_key.as_deref().and_then(|key| engine.worker_env(key)) {
            // Models are local: no proxy is ever needed, and an inherited one is a way out.
            for var in PROXY_ENV_VARS.iter().chain(&TORCH_LOAD_ENV_VARS) {
                cmd.env_remove(var);
            }
            self.push_log(format!(
                "[Harness Studio] Worker: {}",
                if isolated {
                    "isolated (no network)"
                } else {
                    "not isolated"
                }
            ));
            for (var, value) in env {
                cmd.env(var, value);
            }
            // The Harness window (and the user's CORS origins) may call it from the browser.
            cmd.env(
                crate::engine::python_hf::ORIGINS_ENV,
                crate::network::worker_origins(config.cors_origins.as_deref()),
            );
        }
        let audio_cwd = self
            .audio_run
            .lock()
            .unwrap()
            .as_ref()
            .map(|d| d.path().join("outputs"));
        let cwd = if engine.family() == crate::engine::AUDIO_CPP {
            audio_cwd.as_deref().unwrap_or(cwd)
        } else {
            cwd
        };
        cmd.args(&args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // The API key travels in the environment, not on the command line (which any local
        // process can read from the process list).
        for var in API_KEY_ENV_VARS.iter().chain(PROTECTED_ENV_VARS) {
            cmd.env_remove(var);
        }
        // whisper-server has no authentication: it is bound to loopback and the gateway
        // guards it.
        if let Some(key) = config
            .api_key
            .as_deref()
            .filter(|k| !k.is_empty() && is_llama)
        {
            for var in API_KEY_ENV_VARS {
                cmd.env(var, key);
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
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
                    // The parent may have died between fork and PR_SET_PDEATHSIG.
                    if libc::getppid() == 1 {
                        libc::raise(libc::SIGKILL);
                    }
                    Ok(())
                });
            }
        }

        // macOS has no parent-death signal: a kqueue watchdog stops the server if we die.
        #[cfg(target_os = "macos")]
        crate::watchdog::install(&mut cmd);

        // Process group / Windows Job Object. Windows uses KILL_ON_JOB_CLOSE and Linux uses
        // PR_SET_PDEATHSIG above, so llama-server also dies if Harness Studio crashes.
        let mut group = cmd.group();
        // Only Windows has kill-on-drop for a std command group (the Job Object); on Unix the
        // process group is stopped explicitly and Linux also uses PR_SET_PDEATHSIG.
        #[cfg(windows)]
        group.kill_on_drop(true);
        // command-group replaces the inner command's creation flags with its own on spawn, so
        // CREATE_NO_WINDOW must be set on the group too or a console window opens.
        #[cfg(windows)]
        group.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let mut child_group = match group.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.stop_relay();
                self.record_failure(crate::failure::spawn_failure(&e));
                return Err(self.set_error(
                    err("process.spawn_failed")
                        .with("program", program)
                        .with("error", e)
                        .into(),
                ));
            }
        };

        use std::sync::atomic::Ordering::SeqCst;
        let readers_started = (|| -> std::io::Result<()> {
            if let Some(stdout) = child_group.inner().stdout.take() {
                let this = self.clone();
                this.readers.fetch_add(1, SeqCst);
                let spawned = crate::resource_io::spawn_thread("harness-process-log", move || {
                    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                        this.push_log(format!("[stdout] {}", line));
                    }
                    this.readers.fetch_sub(1, SeqCst);
                });
                if let Err(e) = spawned {
                    self.readers.fetch_sub(1, SeqCst);
                    return Err(e);
                }
            }
            if let Some(stderr) = child_group.inner().stderr.take() {
                let this = self.clone();
                this.readers.fetch_add(1, SeqCst);
                let spawned = crate::resource_io::spawn_thread("harness-process-log", move || {
                    let mut whisper_fallback_noted = false;
                    let mut fallback_noted = false;
                    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                        if whisper_cuda
                            && !whisper_fallback_noted
                            && line.to_ascii_lowercase().contains("no gpu found")
                        {
                            let lang = crate::config::AppSettings::load().language;
                            if let Some(note) =
                                crate::engine::whisper_gpu_fallback_note(&line, &lang)
                            {
                                whisper_fallback_noted = true;
                                this.push_log(note.into());
                            }
                        }
                        let fallback = crate::capabilities::verify::kv_fa_fallback(&line);
                        this.push_log(format!("[stderr] {}", line));
                        // llama.cpp repeats the warning (every graph build); note it once per run.
                        if let Some(pair) = fallback.filter(|_| !fallback_noted) {
                            fallback_noted = true;
                            let lang = crate::config::AppSettings::load().language;
                            this.note_kv_fa_fallback(&pair, &lang);
                        }
                    }
                    this.readers.fetch_sub(1, SeqCst);
                });
                if let Err(e) = spawned {
                    self.readers.fetch_sub(1, SeqCst);
                    return Err(e);
                }
            }

            Ok(())
        })();
        if let Err(e) = readers_started {
            // A child with undrained pipes can deadlock. Kill and reap even when
            // only the first reader started, then publish a normal startup error.
            let _ = child_group.kill();
            let _ = child_group.wait();
            self.stop_relay();
            self.record_failure(crate::failure::spawn_failure(&e));
            return Err(self.set_error(err("process.log_reader_failed").with("error", e).into()));
        }

        {
            let mut activity = self.activity.lock().unwrap();
            *activity = ProcessActivity {
                epoch: activity.epoch + 1,
                ..Default::default()
            };
        }
        *self.child.lock().unwrap() = Some(child_group);
        let generation = self
            .generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;

        // 2. Supervisor: waits for /health, then keeps watching for unexpected exits.
        let this = self.clone();
        let health_host = connect_host(&config.host);
        let port = config.port;
        let load_timeout = load_timeout_for(
            &config,
            crate::config::AppSettings::load().server.load_timeout_secs,
        );
        tokio::spawn(async move {
            this.supervise_with_timeout(
                generation,
                engine,
                format!("http://{}:{}{}", health_host, port, engine.health_path()),
                load_timeout,
            )
            .await;
        });

        Ok(())
    }

    /// Aborts the relay of the last isolated Python worker run, if any, and removes its socket.
    fn stop_relay(&self) {
        if let Some((task, dir)) = self.relay.lock().unwrap().take() {
            task.abort();
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// Binds the run's port and relays it to a Unix socket in a private folder; returns the
    /// socket path the worker should listen on.
    #[cfg(unix)]
    async fn start_relay(&self, config: &LaunchConfig) -> Result<PathBuf, String> {
        let parent = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .unwrap_or_else(crate::paths::config_dir)
            .join("harness-studio");
        // A short, stable folder name (server ids can be long): `sun_path` holds 107 bytes.
        use sha2::Digest;
        let digest = crate::hex::lower(&sha2::Sha256::digest(self.id.as_bytes()));
        let dir = crate::pyenv::isolation::socket_dir(&parent, &format!("w-{}", &digest[..12]))?;
        let socket = dir.join("w.sock");
        if socket.as_os_str().len() > 107 {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(err("pyenv.socket_path_too_long")
                .with("path", socket.display())
                .into());
        }
        let _ = std::fs::remove_file(&socket);
        let listener = tokio::net::TcpListener::bind((config.host.as_str(), config.port))
            .await
            .map_err(|e| {
                String::from(
                    err("process.spawn_failed")
                        .with("program", format!("relay :{}", config.port))
                        .with("error", e),
                )
            })?;
        *self.relay.lock().unwrap() = Some((
            crate::pyenv::isolation::relay(listener, socket.clone()),
            dir,
        ));
        Ok(socket)
    }

    fn is_current(&self, generation: u64) -> bool {
        self.generation.load(std::sync::atomic::Ordering::SeqCst) == generation
    }

    async fn supervise_with_timeout(
        &self,
        generation: u64,
        engine: &'static dyn crate::engine::ServerEngine,
        health_url: String,
        load_timeout: Duration,
    ) {
        let start_time = Instant::now();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(800))
            .build()
            .unwrap_or_default();
        #[cfg(target_os = "macos")]
        let mut log_cursor = self.run_log_start.load(std::sync::atomic::Ordering::SeqCst);

        loop {
            if !self.is_current(generation) {
                return; // stopped or restarted
            }

            if engine.family() == crate::engine::AUDIO_CPP
                && crate::engine::audio_cpp::require_enabled().is_err()
            {
                let _ = self.stop().await;
                return;
            }

            if engine.family() == crate::engine::AUDIO_CPP
                && self
                    .stop_audio_if_idle(
                        generation,
                        Duration::from_secs(crate::engine::audio_cpp::IDLE_SECONDS),
                    )
                    .await
            {
                return;
            }

            // Did the process exit?
            let exit = {
                let mut lock = self.child.lock().unwrap();
                match lock.as_mut() {
                    Some(child) => child.try_wait(),
                    None => return,
                }
            };
            match exit {
                Ok(Some(status)) => {
                    let _lifecycle = self.lifecycle.lock().await;
                    if !self.is_current(generation) {
                        return;
                    }
                    let pid = self.child.lock().unwrap().take().map(|c| c.id());
                    self.active.lock().unwrap().take();
                    self.audio_run.lock().unwrap().take();
                    self.stop_relay();
                    crate::mcp::remove_runtime_config(&self.id);
                    let was_running = self.get_status() == ServerStatus::Running;
                    // The reader threads may still be pushing the last lines of the log.
                    self.wait_for_log_readers().await;
                    let phase = if was_running {
                        FailurePhase::Running
                    } else {
                        FailurePhase::Loading
                    };
                    let mut failure =
                        crate::failure::classify(&self.run_lines(), status.code(), phase);
                    crate::failure::note_signal(&mut failure, &status);
                    // Killed from outside: did the system end it for memory?
                    if let Some(pid) = pid.filter(|_| failure.message_key == "failure.killed") {
                        let mut checked = failure.clone();
                        if let Ok(f) = tokio::task::spawn_blocking(move || {
                            crate::failure::confirm_memory_kill(&mut checked, pid);
                            checked
                        })
                        .await
                        {
                            failure = f;
                        }
                    }
                    // The ONNX sidecar could not use CUDA (`--cuda-fallback-exit`): DirectML.
                    if !was_running
                        && status.code() == Some(harness_onnx::cli::EXIT_CUDA_UNUSABLE)
                        && self.retry_on_directml(generation)
                    {
                        return;
                    }
                    if failure.kind == FailureKind::PortInUse && !was_running {
                        // An automatic port taken between our check and llama-server's bind:
                        // start again on the next free one (bounded), still "Starting".
                        if self.retry_on_new_port(generation) {
                            return;
                        }
                        self.log_port_holder().await;
                    }
                    // The failure code stays in the text (`[{code}]` in the message).
                    let msg: String = err(if was_running {
                        "process.exited_unexpectedly"
                    } else {
                        "process.exited_while_loading"
                    })
                    .with("status", status)
                    .with("code", &failure.code)
                    .with("engine", crate::engine::display_name(engine.family()))
                    .into();
                    self.record_failure(failure.clone());
                    self.set_error(msg.clone());
                    // Keep the tail of the log with the crash report (sanitized on processing).
                    let tail = self.get_logs(Some(40)).join("\n");
                    crate::diagnostics::DiagnosticsManager::record("llama_error", msg, Some(tail));
                    self.schedule_restart(generation, &failure);
                    return;
                }
                Ok(None) => {}
                Err(e) => {
                    self.fail_run(
                        generation,
                        err("process.status_failed").with("error", e).into(),
                    )
                    .await;
                    return;
                }
            }

            // A Metal out-of-memory leaves llama-server running (the request only fails with
            // "Compute error."), so the exit-time classification never sees it: stop the
            // server and report it as such. No restart: the same load runs out again.
            #[cfg(target_os = "macos")]
            if let Some(phase) = self.metal_oom_since(&mut log_cursor) {
                let failure = crate::failure::classify(&self.run_lines(), None, phase);
                let msg: String = err("process.gpu_out_of_memory")
                    .with("code", &failure.code)
                    .into();
                self.record_failure(failure);
                self.fail_run(generation, msg).await;
                return;
            }

            if self.get_status() == ServerStatus::Starting {
                if start_time.elapsed() > load_timeout {
                    self.record_failure(
                        ServerFailure::new(FailureKind::LoadTimeout, FailurePhase::Loading)
                            .with("seconds", load_timeout.as_secs()),
                    );
                    self.fail_run(
                        generation,
                        err("process.health_timeout")
                            .num("seconds", load_timeout.as_secs())
                            .with("code", FailureKind::LoadTimeout.code())
                            .with("engine", crate::engine::display_name(engine.family()))
                            .into(),
                    )
                    .await;
                    return;
                }
                if let Ok(res) = client.get(&health_url).send().await {
                    if res.status().is_success() {
                        let ok = engine
                            .health(res.status().as_u16(), &res.text().await.unwrap_or_default());
                        if ok && self.is_current(generation) {
                            self.apply_lora_scale(&client).await;
                            let mut status = self.status.lock().unwrap();
                            if *status == ServerStatus::Starting {
                                *status = ServerStatus::Running;
                                drop(status);
                                self.activity.lock().unwrap().last_used = Instant::now();
                                self.restart.lock().unwrap().ready_at = Some(Instant::now());
                                self.publish_status(ServerStatus::Running);
                                let secs = start_time.elapsed().as_secs_f32();
                                self.push_log(
                                    if crate::config::AppSettings::load().language == "tr" {
                                        format!("[Harness Studio] Sunucu hazır ({secs:.1} sn).")
                                    } else {
                                        format!("[Harness Studio] Server ready ({secs:.1} s).")
                                    },
                                );
                                let this = self.clone();
                                tokio::spawn(async move {
                                    this.verify_after_start(generation).await;
                                });
                            }
                        }
                    }
                }
                sleep(Duration::from_millis(250)).await;
            } else {
                sleep(Duration::from_millis(500)).await;
            }
        }
    }

    /// macOS: the failure phase when a log line since `cursor` reports a Metal out-of-memory
    /// (see [`crate::failure::metal_out_of_memory`]); advances `cursor`. Only while loading
    /// and within [`RESTART_STABLE`] after ready: a server that has been serving longer is not
    /// stopped for a later line.
    #[cfg(target_os = "macos")]
    fn metal_oom_since(&self, cursor: &mut u64) -> Option<FailurePhase> {
        let phase = match self.get_status() {
            ServerStatus::Starting => FailurePhase::Loading,
            ServerStatus::Running
                if self
                    .restart
                    .lock()
                    .unwrap()
                    .ready_at
                    .is_some_and(|t| t.elapsed() < RESTART_STABLE) =>
            {
                FailurePhase::Running
            }
            _ => return None,
        };
        let (next, lines) = self.get_logs_since(*cursor);
        *cursor = next;
        lines
            .iter()
            .any(|l| crate::failure::metal_out_of_memory(l))
            .then_some(phase)
    }

    /// Sets the scale of a LoRA adapter whose path could not carry it on the command line
    /// (see [`lora_post_scale`]): `POST /lora-adapters` before the server is reported ready.
    async fn apply_lora_scale(&self, client: &reqwest::Client) {
        let Some(config) = self.launch_config.lock().unwrap().clone() else {
            return;
        };
        let Some(scale) = lora_post_scale(&config) else {
            return;
        };
        let url = format!(
            "http://{}:{}/lora-adapters",
            connect_host(&config.host),
            config.port
        );
        let mut req = client
            .post(&url)
            .json(&serde_json::json!([{ "id": 0, "scale": scale }]));
        if let Some(key) = config.api_key.as_deref().filter(|k| !k.is_empty()) {
            req = req.bearer_auth(key);
        }
        match req.send().await {
            Ok(res) if res.status().is_success() => {
                self.push_log(format!("[Harness Studio] LoRA ölçeği {scale} uygulandı."));
            }
            Ok(res) => self.push_log(format!(
                "[Harness Studio Error] LoRA ölçeği uygulanamadı (HTTP {}); adaptör uygulanmadan çalışıyor.",
                res.status()
            )),
            Err(e) => self.push_log(format!(
                "[Harness Studio Error] LoRA ölçeği uygulanamadı ({e}); adaptör uygulanmadan çalışıyor."
            )),
        }
    }

    /// Waits (at most 1 s) until the stdout/stderr reader threads have seen EOF, so the last
    /// lines of an exited process are in the ring buffer before it is classified.
    async fn wait_for_log_readers(&self) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while self.readers.load(std::sync::atomic::Ordering::SeqCst) > 0
            && Instant::now() < deadline
        {
            sleep(Duration::from_millis(20)).await;
        }
    }

    /// After a crash: schedules an automatic restart when the setting allows it and attempts
    /// are left (exponential back-off). Failures a restart cannot fix (bad argument, broken
    /// file, missing library, port taken) never restart; nor does a lost GPU (a driver that
    /// resets under load can freeze the machine on the next try), and neither does a run that failed
    /// while loading unless it was itself an automatic restart. A stop or a new start cancels
    /// the pending restart.
    fn schedule_restart(&self, generation: u64, failure: &ServerFailure) {
        let settings = crate::config::AppSettings::load().server;
        if !settings.auto_restart || settings.restart_max_attempts == 0 {
            return;
        }
        if matches!(
            failure.kind,
            FailureKind::InvalidArgument
                | FailureKind::CorruptModel
                | FailureKind::UnsupportedArchitecture
                | FailureKind::ModelNotFound
                | FailureKind::MissingLibrary
                | FailureKind::PortInUse
                | FailureKind::DeviceLost
                | FailureKind::DraftLoadFailed
                // Loading the same model again would run out of memory again (and on macOS
                // push the whole system into swap); the user picks a recovery option instead.
                | FailureKind::OutOfMemory
        ) {
            return;
        }
        let state = {
            let mut track = self.restart.lock().unwrap();
            let auto_run = track.attempts > 0;
            if failure.phase == FailurePhase::Loading && !auto_run {
                return;
            }
            if track
                .ready_at
                .is_some_and(|t| t.elapsed() >= RESTART_STABLE)
            {
                track.attempts = 0;
            }
            if track.attempts >= settings.restart_max_attempts {
                track.pending = None;
                drop(track);
                self.push_log(format!(
                    "[Harness Studio] Otomatik yeniden başlatma durdu: {} deneme kullanıldı.",
                    settings.restart_max_attempts
                ));
                return;
            }
            track.attempts += 1;
            let delay = restart_delay(track.attempts);
            let state = RestartState {
                attempt: track.attempts,
                max_attempts: settings.restart_max_attempts,
                delay_secs: delay.as_secs(),
                restart_at_ms: unix_ms() + delay.as_millis() as u64,
            };
            track.pending = Some(state.clone());
            state
        };
        self.push_log(format!(
            "[Harness Studio] {} sn sonra yeniden başlatılacak (deneme {}/{}).",
            state.delay_secs, state.attempt, state.max_attempts
        ));
        crate::changes::servers_changed();
        crate::events::publish(crate::events::CoreEvent::ServerRestart {
            server_id: self.id.clone(),
            restart: state.clone(),
        });
        let this = self.clone();
        tokio::spawn(async move {
            sleep(Duration::from_secs(state.delay_secs)).await;
            {
                let mut track = this.restart.lock().unwrap();
                if track.pending.as_ref() != Some(&state) || !this.is_current(generation) {
                    return; // stopped, started by hand, or superseded
                }
                track.pending = None;
            }
            if !matches!(this.get_status(), ServerStatus::Error(_)) {
                return;
            }
            let Some(mut config) = this.launch_config() else {
                return;
            };
            // The MCP config file was removed with the crashed run.
            if config.mcp_enabled {
                match crate::mcp::runtime_config(&this.id) {
                    Ok(path) => config.mcp_config_path = path,
                    Err(e) => {
                        this.set_error(e);
                        return;
                    }
                }
            }
            this.push_log(format!(
                "[Harness Studio] Otomatik yeniden başlatma (deneme {}/{}).",
                state.attempt, state.max_attempts
            ));
            let _ = this.start_run(config, Some(generation)).await;
        });
    }

    /// After a bind failure on an automatic port: picks the next free automatic port and
    /// starts the same launch there (at most [`MAX_PORT_RETRIES`] times per user start).
    /// False when the port was chosen by the user or the retries are used up.
    /// An ONNX run whose CUDA provider could not be used: the same launch with the provider set to
    /// DirectML, once per user start (the launch config the user saved keeps CUDA).
    fn retry_on_directml(&self, generation: u64) -> bool {
        use std::sync::atomic::Ordering::SeqCst;
        let Some(mut config) = self.launch_config() else {
            return false;
        };
        if crate::engine::server_for(&config).family() != crate::engine::ONNX_FAMILY
            || self.dml_retried.swap(true, SeqCst)
        {
            return false;
        }
        config.params.insert(
            crate::engine::ONNX_PROVIDER_PARAM.to_string(),
            crate::params::ParamValue::Text("directml".into()),
        );
        let this = self.clone();
        tokio::spawn(async move {
            let _lifecycle = this.lifecycle.lock().await;
            if !this.is_current(generation) || this.get_status() != ServerStatus::Starting {
                return;
            }
            let tr = crate::config::AppSettings::load().language == "tr";
            this.push_log(if tr {
                "[Harness Studio] CUDA kullanılamadı; model DirectML ile yeniden başlatılıyor."
                    .to_string()
            } else {
                "[Harness Studio] CUDA could not be used; starting the model again on DirectML."
                    .to_string()
            });
            let _ = this.run_claimed(config).await;
        });
        true
    }

    fn retry_on_new_port(&self, generation: u64) -> bool {
        use std::sync::atomic::Ordering::SeqCst;
        let Some(config) = self.launch_config().filter(|c| c.port_auto) else {
            return false;
        };
        if self.port_retries.fetch_add(1, SeqCst) >= MAX_PORT_RETRIES {
            return false;
        }
        let this = self.clone();
        tokio::spawn(async move {
            this.respawn_on_new_port(generation, config).await;
        });
        true
    }

    async fn respawn_on_new_port(&self, generation: u64, mut config: LaunchConfig) {
        let old = config.port;
        let host = config.host.clone();
        let next =
            tokio::task::spawn_blocking(move || crate::ports::pick_auto(&host, None, &[old]))
                .await
                .ok()
                .flatten();
        let _lifecycle = self.lifecycle.lock().await;
        // Stopped (or started again) meanwhile.
        if !self.is_current(generation) || self.get_status() != ServerStatus::Starting {
            return;
        }
        let Some(next) = next else {
            self.record_failure(
                ServerFailure::new(FailureKind::PortInUse, FailurePhase::Loading).with("port", old),
            );
            self.set_error(err("servers.no_free_port").into());
            return;
        };
        self.push_log(format!(
            "[Harness Studio] {} portu başka bir program tarafından alındı; otomatik port {} ile yeniden deneniyor.",
            old, next
        ));
        config.port = next;
        // The MCP config file was removed with the failed run.
        if config.mcp_enabled {
            match crate::mcp::runtime_config(&self.id) {
                Ok(path) => config.mcp_config_path = path,
                Err(e) => {
                    self.set_error(e);
                    return;
                }
            }
        }
        let _ = self.run_claimed(config).await;
    }

    /// Names the program holding the port of a run that failed to bind it (in the log).
    async fn log_port_holder(&self) {
        let Some(port) = self.launch_config().map(|c| c.port) else {
            return;
        };
        let holder = tokio::task::spawn_blocking(move || crate::ports::holder(port))
            .await
            .ok()
            .flatten();
        if let Some(h) = holder {
            self.push_log(format!(
                "[Harness Studio Error] {} portunu '{}' (PID {}) kullanıyor.",
                port,
                h.label(),
                h.pid
            ));
        }
    }

    async fn fail_run(&self, generation: u64, msg: String) {
        let _lifecycle = self.lifecycle.lock().await;
        if !self.is_current(generation) {
            return;
        }
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let family = self
            .active
            .lock()
            .unwrap()
            .as_ref()
            .map(|a| a.engine_family.clone())
            .filter(|f| !f.is_empty());
        let tr = crate::config::AppSettings::load().language != "en";
        self.push_log(match (family, tr) {
            (Some(f), true) => {
                format!("[Harness Studio] Hatalı {f} sunucu süreci sonlandırılıyor...")
            }
            (Some(f), false) => {
                format!("[Harness Studio] Stopping the failed {f} server process...")
            }
            (None, true) => "[Harness Studio] Hatalı sunucu süreci sonlandırılıyor...".to_string(),
            (None, false) => "[Harness Studio] Stopping the failed server process...".to_string(),
        });
        let cleanup = self
            .terminate_child_until(Instant::now() + Duration::from_secs(7))
            .await;
        self.active.lock().unwrap().take();
        crate::mcp::remove_runtime_config(&self.id);
        let msg = match cleanup {
            Ok(()) => msg,
            Err(error) => err("process.cleanup_failed")
                .with("error", msg)
                .with("cause", error)
                .into(),
        };
        self.set_error(msg);
    }

    async fn terminate_child_until(&self, deadline: Instant) -> Result<(), String> {
        if !self.has_child() {
            return Ok(());
        }

        #[cfg(unix)]
        let first_kill_error = {
            use command_group::{Signal, UnixChildExt};
            self.child
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|child| child.signal(Signal::SIGTERM).err())
        };
        #[cfg(windows)]
        let first_kill_error = self
            .child
            .lock()
            .unwrap()
            .as_mut()
            .and_then(|child| child.kill().err());
        let escalation_at = (Instant::now() + Duration::from_secs(5)).min(deadline);
        let mut escalated = false;
        loop {
            let exit = self
                .child
                .lock()
                .unwrap()
                .as_mut()
                .map(GroupChild::try_wait);
            match exit {
                Some(Ok(Some(_))) => {
                    self.child.lock().unwrap().take();
                    return Ok(());
                }
                Some(Err(error)) => {
                    return Err(err("process.wait_failed").with("error", error).into());
                }
                None => return Ok(()),
                Some(Ok(None)) => {}
            }

            let now = Instant::now();
            if !escalated && now >= escalation_at {
                escalated = true;
                let escalation_error = self
                    .child
                    .lock()
                    .unwrap()
                    .as_mut()
                    .and_then(|child| child.kill().err());
                if let Some(error) = escalation_error {
                    self.push_log(format!(
                        "[Harness Studio Warn] Zorunlu sonlandırma başarısız: {}",
                        error
                    ));
                }
            }
            if now >= deadline {
                return Err(match first_kill_error {
                    Some(error) => err("process.stop_timeout_kill_failed").with("error", error),
                    None => err("process.stop_timeout"),
                }
                .into());
            }
            sleep(Duration::from_millis(100).min(deadline.saturating_duration_since(now))).await;
        }
    }

    pub(crate) async fn stop_until(&self, deadline: Instant) -> Result<(), String> {
        let _lifecycle = self.lifecycle.lock().await;
        self.stop_locked_until(deadline).await
    }

    async fn stop_locked_until(&self, deadline: Instant) -> Result<(), String> {
        // A stop also cancels a scheduled automatic restart.
        self.restart.lock().unwrap().pending = None;
        {
            let mut status = self.status.lock().unwrap();
            if *status == ServerStatus::Stopped && !self.has_child() {
                self.active.lock().unwrap().take();
                return Ok(());
            }
            *status = ServerStatus::Stopping;
        }
        self.publish_status(ServerStatus::Stopping);
        // Invalidate the supervisor before killing so it doesn't report a "crash".
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let family = self
            .active
            .lock()
            .unwrap()
            .as_ref()
            .map(|a| a.engine_family.clone())
            .filter(|f| !f.is_empty());
        self.push_log(match family {
            Some(f) => format!("[Harness Studio] Stopping the {f} server process..."),
            None => "[Harness Studio] Stopping the server process...".to_string(),
        });

        if let Err(error) = self.terminate_child_until(deadline).await {
            self.set_status(ServerStatus::Error(error.clone()));
            self.push_log(format!("[Harness Studio Error] {}", error));
            return Err(error);
        }

        self.active.lock().unwrap().take();
        self.audio_run.lock().unwrap().take();
        self.stop_relay();
        crate::mcp::remove_runtime_config(&self.id);
        self.set_status(ServerStatus::Stopped);
        self.push_log("[Harness Studio] Server stopped cleanly.".to_string());
        self.log_file.lock().unwrap().take();
        Ok(())
    }

    pub async fn stop(&self) -> Result<(), String> {
        self.stop_until(Instant::now() + Duration::from_secs(7))
            .await
    }

    pub fn build_args(config: &LaunchConfig, caps: &ServerCaps) -> Result<Vec<String>, String> {
        Self::build_args_with(config, caps, None).map(|out| out.args)
    }

    /// Typed fields keep their fixed spellings (see the golden tests); catalog params
    /// (`config.params`) are rendered for `runtime` and skipped when it lacks the flag.
    pub fn build_args_with(
        config: &LaunchConfig,
        caps: &ServerCaps,
        runtime: Option<&crate::capabilities::RuntimeCapabilities>,
    ) -> Result<BuildOutput, String> {
        if crate::engine::audio_cpp::is_model(&config.model_path) {
            return Err(err("audio_cpp.llama_rejected").into());
        }
        let mut args: Vec<String> = Vec::new();

        if config.router_mode {
            let flag = caps
                .models_dir_flag
                .as_deref()
                .ok_or_else(|| String::from(err("process.router_unsupported")))?;
            let p = config.models_path.as_ref().unwrap_or(&config.model_path);
            args.push(flag.to_string());
            args.push(p.to_string_lossy().to_string());
        } else {
            args.push("-m".to_string());
            args.push(config.model_path.to_string_lossy().to_string());
            // Report a readable model id ("qwen2.5-coder-14b-instruct-q4_k_m") instead of the
            // file path, unless the user picked their own alias.
            let user_alias = config
                .custom_args
                .as_deref()
                .is_some_and(|a| a.split_whitespace().any(|t| t == "--alias" || t == "-a"));
            let alias = config
                .alias
                .as_deref()
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .map(str::to_string)
                .or_else(|| model_alias(&config.model_path));
            if let (false, Some(alias)) = (user_alias, alias) {
                args.push("--alias".to_string());
                args.push(alias);
            }
        }

        // `--fit on` only adjusts what is not set: with it chosen explicitly, llama-server
        // picks the GPU layers that fit instead of `-ngl` (the context stays as set).
        if !fit_chooses_layers(config) {
            args.push("-ngl".to_string());
            args.push(config.gpu_layers.to_string());
        }
        args.push("-c".to_string());
        args.push(config.context_size.to_string());
        args.push("--host".to_string());
        args.push(config.host.clone());
        args.push("--port".to_string());
        args.push(config.port.to_string());
        if caps.supports_metrics {
            args.push("--metrics".to_string());
        }

        if let Some(threads) = config.threads {
            args.push("-t".to_string());
            args.push(threads.to_string());
        }

        // Flash Attention
        if config.flash_attn {
            args.push("--flash-attn".to_string());
            if caps.flash_attn_takes_value {
                args.push("on".to_string());
            }
        }

        // KV Cache Quantization
        if let Some(ctk) = config.cache_type_k.as_deref().filter(|s| !s.is_empty()) {
            args.push("-ctk".to_string());
            args.push(ctk.to_string());
        }
        if let Some(ctv) = config.cache_type_v.as_deref().filter(|s| !s.is_empty()) {
            args.push("-ctv".to_string());
            args.push(ctv.to_string());
        }

        // `-np` in the custom arguments replaces ours (they come last; llama-server keeps the
        // last value, but one flag is clearer in the log).
        if let Some(np) = config
            .parallel_slots
            .filter(|&n| n > 0)
            .filter(|_| crate::slots::custom_slots(config.custom_args.as_deref()).is_none())
        {
            args.push("-np".to_string());
            args.push(np.to_string());
        }

        if let Some(b) = config.logical_batch.filter(|&n| n > 0) {
            args.push("-b".to_string());
            args.push(b.to_string());
        }
        if let Some(ub) = config.physical_batch.filter(|&n| n > 0) {
            args.push("-ub".to_string());
            args.push(ub.to_string());
        }

        if let Some(cs) = config.context_shift {
            args.push(
                if cs {
                    "--context-shift"
                } else {
                    "--no-context-shift"
                }
                .to_string(),
            );
        }

        // Multi-GPU / Primary GPU Selection / Tensor Split
        let target_gpu = if config.gpu_layers > 0 {
            config.main_gpu.or_else(crate::gpu::primary_gpu_index)
        } else {
            None
        };

        if let Some(sm) = config.split_mode.as_deref().filter(|s| !s.is_empty()) {
            args.push("--split-mode".to_string());
            args.push(sm.to_string());
        } else if target_gpu.is_some() {
            // Pin to single GPU when no explicit multi-GPU split mode is specified
            // to prevent llama-server from spreading layers to low-VRAM integrated GPUs.
            args.push("--split-mode".to_string());
            args.push("none".to_string());
        }

        if let Some(mg) = target_gpu {
            args.push("--main-gpu".to_string());
            args.push(mg.to_string());
        }
        if let Some(ts) = config.tensor_split.as_deref().filter(|s| !s.is_empty()) {
            args.push("-ts".to_string());
            args.push(ts.to_string());
        }

        // RoPE Scaling
        if let Some(rs) = config
            .rope_scaling
            .as_deref()
            .filter(|s| !s.is_empty() && *s != "none")
        {
            args.push("--rope-scaling".to_string());
            args.push(rs.to_string());
        }
        if let Some(rfb) = config.rope_freq_base {
            args.push("--rope-freq-base".to_string());
            args.push(rfb.to_string());
        }
        if let Some(rfs) = config.rope_freq_scale {
            args.push("--rope-freq-scale".to_string());
            args.push(rfs.to_string());
        }

        if let Some(ct) = config.chat_template.as_deref().filter(|s| !s.is_empty()) {
            args.push("--chat-template".to_string());
            args.push(ct.to_string());
        }

        // Speculative Decoding
        if let Some(dmp) = config
            .draft_model_path
            .as_deref()
            .filter(|p| !p.as_os_str().is_empty())
        {
            args.push("-md".to_string());
            args.push(dmp.to_string_lossy().to_string());
            if let Some(ngld) = config.draft_gpu_layers {
                args.push("-ngld".to_string());
                args.push(ngld.to_string());
            }
        }

        // LoRA Adapters (current syntax: --lora-scaled FNAME:SCALE)
        if let Some(lora) = config
            .lora_path
            .as_deref()
            .filter(|p| !p.as_os_str().is_empty())
        {
            let lora = lora.to_string_lossy().to_string();
            match config.lora_scale {
                // llama-server splits `--lora-scaled FNAME:SCALE` at every `:`, so a Windows
                // drive path (`C:\...`) cannot be passed with a scale. The adapter loads
                // without being applied and the scale is set through `/lora-adapters` once the
                // server is up (`ProcessManager::apply_lora_scale`).
                Some(scale) if lora_needs_post_scale(&lora) => {
                    args.push("--lora".to_string());
                    args.push(lora);
                    if (scale - 1.0).abs() > f32::EPSILON {
                        args.push("--lora-init-without-apply".to_string());
                    }
                }
                Some(scale) => {
                    args.push("--lora-scaled".to_string());
                    args.push(format!("{}:{}", lora, scale));
                }
                None => {
                    args.push("--lora".to_string());
                    args.push(lora);
                }
            }
        }

        // A live LoRA scale change (`lora_live`) drops the slots' prompt caches, which
        // llama-server only allows with a slot save path: without it the next request reuses
        // a prompt computed with the old scale. llama-server refuses a folder that does not
        // exist, so it is made here; when that fails the flag stays off (the load must not).
        let has_lora = config
            .lora_path
            .as_deref()
            .is_some_and(|p| !p.as_os_str().is_empty())
            || ["lora_adapters", "lora_scaled"]
                .iter()
                .any(|k| config.params.contains_key(*k));
        if has_lora
            && runtime.is_some_and(|r| r.has_flag("--slot-save-path"))
            && !config
                .custom_args
                .as_deref()
                .is_some_and(|c| c.contains("--slot-save-path"))
        {
            let dir = crate::paths::data_root().join("slot-cache");
            if std::fs::create_dir_all(&dir).is_ok() {
                args.push("--slot-save-path".to_string());
                args.push(dir.to_string_lossy().to_string());
            }
        }

        // Multimodal / Vision Projector
        if let Some(mm) = config
            .mmproj_path
            .as_deref()
            .filter(|p| !p.as_os_str().is_empty())
        {
            args.push("--mmproj".to_string());
            args.push(mm.to_string_lossy().to_string());
        }

        // llama-server's own MCP option (experimental). Harness's tools run in Harness,
        // not through `--tools`. Same rule as `network::prepare_launch` (every start path
        // passes through here).
        crate::network::check_launch(config)?;
        if config.mcp_enabled {
            if !caps.supports_mcp {
                return Err(err("process.mcp_unsupported").into());
            }
            let path = config
                .mcp_config_path
                .as_ref()
                .ok_or_else(|| String::from(err("process.no_mcp_servers")))?;
            args.push("--mcp-servers-config".to_string());
            args.push(path.to_string_lossy().to_string());
        }

        // CORS origins (Settings > Network & security, resolved at launch)
        let mut skipped = Vec::new();
        if let Some(origins) = config.cors_origins.as_deref() {
            if caps.supports_cors_origins {
                args.push("--cors-origins".to_string());
                args.push(origins.to_string());
            } else {
                skipped.push(crate::params::SkippedParam {
                    id: "cors_origins".to_string(),
                    reason: "unsupported_by_runtime".to_string(),
                });
            }
        }

        // Catalog parameters (only flags this runtime has). The Harness-level serving mode is
        // not a flag (`serve_mode::prepare` has already turned it into `embeddings`/`rerank`).
        let custom_args = match config
            .custom_args
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            Some(raw) => {
                let parsed = split_args(raw)?;
                validate_custom_args(&parsed)?;
                parsed
            }
            None => Vec::new(),
        };
        let mut values = config.params.clone();
        values.remove(crate::serve_mode::SERVE_MODE);
        values.remove(LOAD_TIMEOUT);
        // Lower memory ceilings than llama-server's own (prompt cache 8 GiB, 32 context
        // checkpoints per slot) unless the user chose them; docs/en/PERFORMANCE.md has the numbers.
        crate::params::with_harness_defaults(
            crate::params::catalog(),
            runtime,
            &mut values,
            &custom_args,
        );
        if !values.is_empty() {
            let rendered = crate::params::render_params(
                crate::params::catalog(),
                runtime,
                &values,
                &crate::params::RenderContext {
                    host: config.host.clone(),
                    router_mode: config.router_mode,
                    legacy_set: legacy_fields_set(config),
                },
            )?;
            args.extend(rendered.args);
            skipped.extend(rendered.skipped);
        }

        // Raw CLI Arguments (quote-aware)
        args.extend(custom_args);

        if !config.router_mode {
            if let Ok(meta) = crate::gguf::read_gguf_metadata(&config.model_path) {
                apply_metadata_fixes(&mut args, &meta, &config.params);
            }
        }

        Ok(BuildOutput { args, skipped })
    }
}

fn apply_metadata_fixes(
    args: &mut Vec<String>,
    meta: &crate::gguf::GgufMetadata,
    params: &crate::params::ParamValues,
) {
    let Some(fix) = meta.known_metadata_fixes().override_kv else {
        return;
    };
    let key = fix.split_once('=').unwrap().0;
    let has_key = |value: &str| {
        value.split([',', '\n']).any(|item| {
            item.trim()
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == key)
        })
    };
    // Respect load settings even when capability filtering did not emit their flag.
    let user_setting = match params.get("override_kv") {
        Some(crate::params::ParamValue::List(values)) => values.iter().any(|v| has_key(v)),
        Some(crate::params::ParamValue::Text(value)) => has_key(value),
        _ => false,
    };
    let override_value = |i: usize| {
        if args[i] == "--override-kv" {
            args.get(i + 1).map(String::as_str)
        } else {
            args[i].strip_prefix("--override-kv=")
        }
    };
    if user_setting || (0..args.len()).any(|i| override_value(i).is_some_and(has_key)) {
        return;
    }
    // Catalog lists use commas; extend an existing override list in the same form.
    if let Some(i) = (0..args.len()).rfind(|&i| override_value(i).is_some()) {
        let value = if args[i] == "--override-kv" {
            &mut args[i + 1]
        } else {
            &mut args[i]
        };
        value.push(',');
        value.push_str(fix);
    } else {
        args.extend(["--override-kv".into(), fix.into()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_loopback_servers_are_bridged() {
        let available = crate::pyenv::isolation::available();
        for host in ["127.0.0.1", "0.0.0.0", "localhost", "::1"] {
            assert_eq!(bridge_plan(host).is_ok(), available, "{host}");
        }
        // Inside the namespace a LAN address does not exist: such a server keeps its network.
        let lan = bridge_plan("192.168.1.20");
        assert!(lan.is_err());
        if available {
            assert!(lan.unwrap_err().contains("not loopback"));
        }
    }

    #[test]
    fn audio_and_onnx_servers_are_bridged() {
        use crate::engine::{CrispAsrEngine, ServerEngine};
        for engine in [
            &CrispAsrEngine as &dyn ServerEngine,
            crate::engine::audio_cpp(),
            crate::engine::onnx(),
        ] {
            assert!(engine.netns_bridge(), "{}", engine.family());
        }
        assert!(!crate::engine::llama_cpp().netns_bridge());
        assert!(!crate::engine::python_hf().netns_bridge());
    }

    fn gemma3_config(embedding: u32, window: u32) -> (tempfile::TempDir, LaunchConfig) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gemma.gguf");
        let mut bytes = b"GGUF".to_vec();
        bytes.extend(3u32.to_le_bytes());
        bytes.extend(0u64.to_le_bytes());
        bytes.extend(4u64.to_le_bytes());
        let key = b"general.architecture";
        bytes.extend((key.len() as u64).to_le_bytes());
        bytes.extend(key);
        bytes.extend(8u32.to_le_bytes()); // GGUF string
        bytes.extend(6u64.to_le_bytes());
        bytes.extend(b"gemma3");
        for (key, value) in [
            ("gemma3.embedding_length", embedding),
            ("gemma3.block_count", 26),
            ("gemma3.attention.sliding_window", window),
        ] {
            bytes.extend((key.len() as u64).to_le_bytes());
            bytes.extend(key.as_bytes());
            bytes.extend(4u32.to_le_bytes()); // GGUF uint32
            bytes.extend(value.to_le_bytes());
        }
        std::fs::write(&path, bytes).unwrap();
        let config = LaunchConfig {
            model_path: path,
            host: "127.0.0.1".into(),
            ..Default::default()
        };
        (dir, config)
    }

    #[test]
    fn gemma3_1b_sliding_window_override_added() {
        let (_dir, config) = gemma3_config(1152, 1024);
        let args = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap();
        assert_eq!(
            &args[args.len() - 2..],
            ["--override-kv", "gemma3.attention.sliding_window=int:512"]
        );
    }

    #[test]
    fn gemma3_other_sizes_and_correct_window_need_no_override() {
        for (embedding, window) in [(2560, 1024), (1152, 512)] {
            let (_dir, config) = gemma3_config(embedding, window);
            let args = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap();
            assert!(!args.iter().any(|a| a == "--override-kv"));
        }
    }

    #[test]
    fn gemma3_catalog_override_emitted_once_with_auto_fix() {
        let (_dir, mut config) = gemma3_config(1152, 1024);
        let metadata = crate::gguf::read_gguf_metadata(&config.model_path).unwrap();
        let fix = metadata.known_metadata_fixes().override_kv.unwrap();
        let runtime = crate::capabilities::testdata::caps("b11236");
        for values in [vec!["a=int:1", "b=str:x"], vec!["a=int:1", "b=str:x", fix]] {
            config.params.insert(
                "override_kv".into(),
                crate::params::ParamValue::List(values.into_iter().map(str::to_string).collect()),
            );
            let output =
                ProcessManager::build_args_with(&config, &ServerCaps::default(), Some(&runtime))
                    .unwrap();
            assert!(!output.skipped.iter().any(|p| p.id == "override_kv"));
            assert_eq!(
                output.args.iter().filter(|a| *a == "--override-kv").count(),
                1
            );
            assert!(output.args.contains(&format!("a=int:1,b=str:x,{fix}")));
            assert_eq!(
                output
                    .args
                    .iter()
                    .map(|a| a.matches(fix).count())
                    .sum::<usize>(),
                1
            );
        }
    }

    #[test]
    fn gemma3_user_sliding_window_override_not_duplicated() {
        for raw in [
            "--override-kv gemma3.attention.sliding_window=int:256",
            "--override-kv other=int:1,gemma3.attention.sliding_window=int:256",
            "--override-kv other=int:1 --override-kv gemma3.attention.sliding_window=int:256",
            "--override-kv=gemma3.attention.sliding_window=int:256",
        ] {
            let (_dir, mut config) = gemma3_config(1152, 1024);
            config.custom_args = Some(raw.into());
            let args = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap();
            assert_eq!(
                args.iter()
                    .filter(|a| a.contains("gemma3.attention.sliding_window"))
                    .count(),
                1,
                "{args:?}"
            );
            assert!(!args.iter().any(|a| a.contains("int:512")));
        }
    }

    #[test]
    fn lora_launch_gets_a_slot_save_path_when_the_runtime_has_it() {
        let runtime = crate::capabilities::RuntimeCapabilities::from_outputs(
            Default::default(),
            "",
            "--slot-save-path PATH\n--lora FNAME",
            "",
        );
        let mut config = LaunchConfig {
            model_path: "m.gguf".into(),
            host: "127.0.0.1".into(),
            port: 8080,
            lora_path: Some(r"C:\lora\a.gguf".into()),
            ..Default::default()
        };
        let has = |c: &LaunchConfig, r| {
            ProcessManager::build_args_with(c, &ServerCaps::default(), r)
                .unwrap()
                .args
                .iter()
                .any(|a| a == "--slot-save-path")
        };
        assert!(has(&config, Some(&runtime)));
        // llama-server refuses a folder that does not exist ("not a directory").
        assert!(crate::paths::data_root().join("slot-cache").is_dir());
        // An older build without the flag, or no LoRA at all: nothing added.
        assert!(!has(&config, None));
        config.lora_path = None;
        assert!(!has(&config, Some(&runtime)));
    }

    #[test]
    fn gemma3_load_setting_override_not_duplicated() {
        use crate::params::ParamValue;
        let runtime = crate::capabilities::RuntimeCapabilities::from_outputs(
            Default::default(),
            "",
            "--override-kv KEY=TYPE:VALUE",
            "",
        );
        for value in [
            ParamValue::List(vec![
                "other=int:1".into(),
                "gemma3.attention.sliding_window=int:256".into(),
            ]),
            ParamValue::Text("other=int:1\ngemma3.attention.sliding_window=int:256".into()),
        ] {
            let (_dir, mut config) = gemma3_config(1152, 1024);
            config.params.insert("override_kv".into(), value);
            for runtime in [Some(&runtime), None] {
                let args =
                    ProcessManager::build_args_with(&config, &ServerCaps::default(), runtime)
                        .unwrap()
                        .args;
                assert!(!args.iter().any(|a| a.contains("int:512")));
            }
        }
    }

    #[test]
    fn gemma3_fix_extends_other_override_keys() {
        for raw in ["--override-kv other=int:1", "--override-kv=other=int:1"] {
            let (_dir, mut config) = gemma3_config(1152, 1024);
            config.custom_args = Some(raw.into());
            let args = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap();
            assert!(args
                .iter()
                .any(|a| a.ends_with("other=int:1,gemma3.attention.sliding_window=int:512")));
        }
    }

    fn long_running_child() -> GroupChild {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args(["/C", "ping", "-n", "31", "127.0.0.1"]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 30"]);
            command
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut group = command.group();
        #[cfg(windows)]
        group.kill_on_drop(true);
        group.spawn().unwrap()
    }

    #[test]
    fn test_process_manager_initial_state() {
        let mgr = ProcessManager::new();
        assert_eq!(mgr.get_status(), ServerStatus::Stopped);
        assert_eq!(mgr.get_active_port(), 8080);
        assert!(mgr.get_logs(None).is_empty());
    }

    #[test]
    fn kv_fa_fallback_after_start_reaches_effective_values() {
        // Real order (b11327, RTX 3050 Ti Laptop, -ctk q8_0 -ctv f16): the effective values
        // are read right after start, the warning comes with the first generated token.
        let mgr = ProcessManager::new();
        *mgr.effective.lock().unwrap() = Some(Default::default());
        mgr.note_kv_fa_fallback("q8_0-f16", "en");
        let values = mgr.effective.lock().unwrap().clone().unwrap();
        assert_eq!(values.kv_fa_fallback.as_deref(), Some("q8_0-f16"));
        assert!(mgr.get_logs(None).last().unwrap().contains("q8_0-f16"));
    }

    #[test]
    fn test_log_ring_buffer_capacity() {
        let mgr = ProcessManager::new();
        for i in 0..1200 {
            mgr.push_log(format!("Log line {}", i));
        }
        let logs = mgr.get_logs(None);
        assert_eq!(logs.len(), 1000);
        assert_eq!(logs[0], "Log line 200");
        assert_eq!(logs[999], "Log line 1199");

        let tail = mgr.get_logs(Some(5));
        assert_eq!(tail.len(), 5);
        assert_eq!(tail[4], "Log line 1199");
    }

    #[test]
    fn test_run_log_continues_in_new_file_past_size_limit() {
        let id = format!("rotate-test-{}", std::process::id());
        let dir = crate::server_logs::server_log_dir(&id);
        let _ = std::fs::remove_dir_all(&dir);
        let mgr = ProcessManager::with_id(&id);
        *mgr.log_file.lock().unwrap() =
            crate::server_logs::open_run_log(&id).map(|f| (f, Instant::now(), 0));
        let line = "x".repeat(4096);
        let lines = crate::server_logs::MAX_RUN_LOG_BYTES as usize / 4096 + 10;
        for _ in 0..lines {
            mgr.push_log(line.clone());
        }
        mgr.log_file.lock().unwrap().take();
        let files = crate::server_logs::list(&id);
        assert_eq!(files.len(), 2);
        assert!(files
            .iter()
            .all(|f| f.size_bytes <= crate::server_logs::MAX_RUN_LOG_BYTES + 4200));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_build_args_mandatory_metrics() {
        let config = LaunchConfig {
            binary_path: PathBuf::from("llama-server.exe"),
            model_path: PathBuf::from("models/test.gguf"),
            gpu_layers: 24,
            context_size: 4096,
            host: "127.0.0.1".to_string(),
            port: 8080,
            threads: Some(8),
            ..Default::default()
        };

        let args = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap();
        assert!(args.contains(&"--metrics".to_string()));
        assert!(args.contains(&"-ngl".to_string()));
        assert!(args.contains(&"24".to_string()));
        assert!(args.contains(&"-c".to_string()));
        assert!(args.contains(&"4096".to_string()));
        assert!(args.contains(&"-t".to_string()));
        assert!(args.contains(&"8".to_string()));
    }

    /// llama-server's default port is moving (8080 → 9931, announced in its log): every launch
    /// passes `--port` (and `--host`) itself, whatever the mode, so its default never matters.
    #[test]
    fn test_build_args_always_pass_host_and_port() {
        let port_of = |args: &[String]| {
            let i = args.iter().position(|a| a == "--port").expect("--port");
            assert_eq!(
                args.iter().filter(|a| *a == "--port").count(),
                1,
                "{args:?}"
            );
            assert!(args.iter().any(|a| a == "--host"), "{args:?}");
            args[i + 1].clone()
        };
        let model = LaunchConfig {
            binary_path: PathBuf::from("llama-server.exe"),
            model_path: PathBuf::from("models/test.gguf"),
            host: "127.0.0.1".to_string(),
            port: 11440,
            port_auto: true,
            ..Default::default()
        };
        let caps = ServerCaps::default();
        assert_eq!(
            port_of(&ProcessManager::build_args(&model, &caps).unwrap()),
            "11440"
        );
        let router = LaunchConfig {
            router_mode: true,
            models_path: Some(PathBuf::from("models")),
            port: 8080,
            port_auto: false,
            ..model.clone()
        };
        assert_eq!(
            port_of(&ProcessManager::build_args(&router, &caps).unwrap()),
            "8080"
        );
        // Embedding / rerank launches go through the same builder.
        let embed = LaunchConfig {
            params: [(
                "embeddings".to_string(),
                crate::params::ParamValue::Bool(true),
            )]
            .into_iter()
            .collect(),
            ..model
        };
        assert_eq!(
            port_of(&ProcessManager::build_args(&embed, &caps).unwrap()),
            "11440"
        );
    }

    #[test]
    fn test_build_args_advanced_flags() {
        let config = LaunchConfig {
            binary_path: PathBuf::from("llama-server.exe"),
            model_path: PathBuf::from("models/test.gguf"),
            gpu_layers: 33,
            context_size: 8192,
            host: "127.0.0.1".to_string(),
            port: 8080,
            threads: Some(4),
            flash_attn: true,
            cache_type_k: Some("q8_0".to_string()),
            cache_type_v: Some("q4_0".to_string()),
            parallel_slots: Some(4),
            logical_batch: Some(2048),
            physical_batch: Some(512),
            context_shift: Some(true),
            split_mode: Some("layer".to_string()),
            main_gpu: Some(0),
            tensor_split: Some("3,1".to_string()),
            rope_scaling: Some("yarn".to_string()),
            rope_freq_base: Some(10000.0),
            rope_freq_scale: Some(0.5),
            chat_template: Some("chatml".to_string()),
            api_key: Some("secret123".to_string()),
            custom_args: Some("--no-warmup --verbose".to_string()),
            ..Default::default()
        };

        let args = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap();
        let fa = args.iter().position(|a| a == "--flash-attn").unwrap();
        assert_eq!(args[fa + 1], "on");
        assert!(args.contains(&"-ctk".to_string()));
        assert!(args.contains(&"q8_0".to_string()));
        assert!(args.contains(&"-ctv".to_string()));
        assert!(args.contains(&"q4_0".to_string()));
        assert!(args.contains(&"-np".to_string()));
        assert!(args.contains(&"4".to_string()));
        assert!(args.contains(&"-b".to_string()));
        assert!(args.contains(&"2048".to_string()));
        assert!(args.contains(&"-ub".to_string()));
        assert!(args.contains(&"512".to_string()));
        assert!(args.contains(&"--context-shift".to_string()));
        assert!(args.contains(&"--split-mode".to_string()));
        assert!(args.contains(&"layer".to_string()));
        assert!(args.contains(&"--main-gpu".to_string()));
        assert!(args.contains(&"0".to_string()));
        assert!(args.contains(&"-ts".to_string()));
        assert!(args.contains(&"3,1".to_string()));
        assert!(args.contains(&"--rope-scaling".to_string()));
        assert!(args.contains(&"yarn".to_string()));
        assert!(args.contains(&"--chat-template".to_string()));
        assert!(args.contains(&"chatml".to_string()));
        // The API key must never appear on the command line (it goes through the environment).
        assert!(!args
            .iter()
            .any(|a| a == "--api-key" || a.contains("secret123")));
        assert!(args.contains(&"--no-warmup".to_string()));
        assert!(args.contains(&"--verbose".to_string()));
    }

    #[test]
    fn test_active_server_info_never_serializes_key() {
        let info = ActiveServerInfo {
            connect_host: "127.0.0.1".to_string(),
            bind_host: "127.0.0.1".to_string(),
            port: 8080,
            api_key: Some("secret123".to_string()),
            api_key_set: true,
            api_key_required: true,
            model_path: PathBuf::from("m.gguf"),
            router_mode: false,
            exposure_acknowledged: false,
            cors_origins: None,
            serve_mode: Default::default(),
            engine_family: String::new(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(!json.contains("secret123"));
        assert!(json.contains("\"api_key_set\":true"));
    }

    #[test]
    fn test_launch_config_never_serializes_key() {
        let config = LaunchConfig {
            api_key: Some("secret123".to_string()),
            api_key_ref: Some("session".to_string()),
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("secret123"));
        assert!(json.contains("\"api_key_ref\":\"session\""));
        // ...but a plaintext key from an old file / the UI is still readable.
        let old: LaunchConfig = serde_json::from_str(
            &json.replace("\"api_key_ref\"", "\"api_key\":\"legacy\",\"api_key_ref\""),
        )
        .unwrap();
        assert_eq!(old.api_key.as_deref(), Some("legacy"));
    }

    #[test]
    fn test_build_args_gpu_pinning() {
        // When GPU layers are offloaded and user didn't specify split-mode,
        // it must pin to split-mode none and set main-gpu.
        let config = LaunchConfig {
            binary_path: PathBuf::from("llama-server.exe"),
            model_path: PathBuf::from("models/test.gguf"),
            gpu_layers: 32,
            context_size: 4096,
            host: "127.0.0.1".to_string(),
            port: 8080,
            main_gpu: Some(1),
            ..Default::default()
        };

        let args = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap();
        assert!(args.contains(&"--split-mode".to_string()));
        let sm_idx = args.iter().position(|a| a == "--split-mode").unwrap();
        assert_eq!(args[sm_idx + 1], "none");
        let mg_idx = args.iter().position(|a| a == "--main-gpu").unwrap();
        assert_eq!(args[mg_idx + 1], "1");

        // When gpu_layers is 0 (pure CPU), no main-gpu or split-mode should be passed
        let cpu_config = LaunchConfig {
            binary_path: PathBuf::from("llama-server.exe"),
            model_path: PathBuf::from("models/test.gguf"),
            gpu_layers: 0,
            context_size: 4096,
            host: "127.0.0.1".to_string(),
            port: 8080,
            ..Default::default()
        };
        let cpu_args = ProcessManager::build_args(&cpu_config, &ServerCaps::default()).unwrap();
        assert!(!cpu_args.contains(&"--main-gpu".to_string()));
    }

    #[test]
    fn lora_with_a_drive_path_is_scaled_after_the_start() {
        let mut config = LaunchConfig {
            binary_path: PathBuf::from("llama-server.exe"),
            model_path: PathBuf::from("C:/m/main.gguf"),
            gpu_layers: 33,
            context_size: 4096,
            host: "127.0.0.1".to_string(),
            port: 8080,
            lora_path: Some(PathBuf::from("C:/m/adapter.gguf")),
            lora_scale: Some(0.5),
            ..Default::default()
        };
        // `--lora-scaled C:/m/adapter.gguf:0.5` is split at every `:` by llama-server.
        let args = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap();
        assert!(!args.contains(&"--lora-scaled".to_string()), "{args:?}");
        assert!(args.contains(&"--lora-init-without-apply".to_string()));
        let i = args.iter().position(|a| a == "--lora").unwrap();
        assert_eq!(args[i + 1], "C:/m/adapter.gguf");
        assert_eq!(lora_post_scale(&config), Some(0.5));
        // Scale 1 is llama-server's default: nothing to apply later.
        config.lora_scale = Some(1.0);
        let args = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap();
        assert!(!args.contains(&"--lora-init-without-apply".to_string()));
        assert_eq!(lora_post_scale(&config), None);
    }

    #[test]
    fn test_build_args_speculative_and_lora() {
        let config = LaunchConfig {
            binary_path: PathBuf::from("llama-server.exe"),
            model_path: PathBuf::from("models/main-model.gguf"),
            gpu_layers: 33,
            context_size: 4096,
            host: "127.0.0.1".to_string(),
            port: 8080,
            draft_model_path: Some(PathBuf::from("models/draft-model.gguf")),
            draft_gpu_layers: Some(16),
            lora_path: Some(PathBuf::from("models/adapter.gguf")),
            lora_scale: Some(0.8),
            ..Default::default()
        };

        let args = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap();
        assert!(args.contains(&"-md".to_string()));
        assert!(
            args.contains(&"models/draft-model.gguf".to_string())
                || args.contains(&"models\\draft-model.gguf".to_string())
        );
        assert!(args.contains(&"-ngld".to_string()));
        assert!(args.contains(&"16".to_string()));
        assert!(args.contains(&"--lora-scaled".to_string()));
        assert!(args.iter().any(|a| a.ends_with("adapter.gguf:0.8")));
    }

    #[test]
    fn test_build_args_router_mode() {
        let config = LaunchConfig {
            binary_path: PathBuf::from("llama-server.exe"),
            model_path: PathBuf::from("models"),
            router_mode: true,
            models_path: Some(PathBuf::from("models/directory")),
            gpu_layers: 24,
            context_size: 4096,
            host: "127.0.0.1".to_string(),
            port: 8080,
            ..Default::default()
        };

        let args = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap();
        assert!(args.contains(&"--models-dir".to_string()));
        assert!(
            args.contains(&"models/directory".to_string())
                || args.contains(&"models\\directory".to_string())
        );
        assert!(!args.contains(&"-m".to_string()));
    }

    #[test]
    fn test_split_args_quotes() {
        assert_eq!(
            split_args(r#"--no-warmup --alias "my model" --lora 'C:\x y\a.gguf'"#).unwrap(),
            vec![
                "--no-warmup",
                "--alias",
                "my model",
                "--lora",
                r"C:\x y\a.gguf"
            ]
        );
        assert_eq!(split_args(r#"--x "a\"b""#).unwrap(), vec!["--x", "a\"b"]);
        assert!(split_args(r#"--x "open"#).is_err());
        assert!(split_args("   ").unwrap().is_empty());
    }

    #[test]
    fn test_custom_args_reject_reserved_security_flags() {
        let model = tempfile::NamedTempFile::new().unwrap();
        let reserved = [
            "--host 0.0.0.0",
            "--host=0.0.0.0",
            "--port 9999",
            "--port=9999",
            "--api-key stolen",
            "--api-key=stolen",
            "--api-key-file keys.txt",
            "--api-key-file=keys.txt",
            "--mcp-servers-config evil.json",
            "--mcp-servers-config=evil.json",
            "--mcp_servers_config=evil.json",
            "--mcp-servers-json={}",
            "--tools all",
            "--tools=all",
            "--agent",
            "-ag",
        ];

        for custom_args in reserved {
            let config = LaunchConfig {
                model_path: model.path().to_path_buf(),
                host: "127.0.0.1".to_string(),
                port: 8080,
                custom_args: Some(custom_args.to_string()),
                ..Default::default()
            };
            let error = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap_err();
            let (key, params) = crate::error_key::parse(&error).unwrap();
            assert_eq!(key, "process.protected_flag", "{custom_args}: {error}");
            assert!(params["flag"].is_string(), "{custom_args}: {error}");
        }
    }

    #[test]
    fn test_model_alias_is_readable_file_stem() {
        use std::path::Path;
        assert_eq!(
            model_alias(Path::new(
                r"D:\Models\Qwen\qwen2.5-coder-14b-instruct-q4_k_m.gguf"
            ))
            .as_deref(),
            Some("qwen2.5-coder-14b-instruct-q4_k_m")
        );
        assert_eq!(
            model_alias(Path::new("/m/Big-Model-Q4_K_M-00001-of-00003.gguf")).as_deref(),
            Some("Big-Model-Q4_K_M")
        );
        assert_eq!(model_alias(Path::new("/m/plain")).as_deref(), Some("plain"));
        assert_eq!(
            model_alias(Path::new("/m/st/all-MiniLM-L6-v2/onnx/model.onnx")).as_deref(),
            Some("all-MiniLM-L6-v2")
        );
    }

    #[test]
    fn test_custom_args_allow_unreserved_flags() {
        let config = LaunchConfig {
            model_path: PathBuf::from("model.gguf"),
            host: "127.0.0.1".to_string(),
            port: 8080,
            custom_args: Some(r#"--alias "yerel model" --verbose"#.to_string()),
            ..Default::default()
        };
        let args = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap();
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--alias", "yerel model"]));
        assert!(args.contains(&"--verbose".to_string()));
    }

    /// The served id follows what `build_args` sends: custom `--alias`, else the definition
    /// alias, else the file stem.
    #[test]
    fn test_served_alias_matches_the_sent_alias() {
        let mut config = LaunchConfig {
            model_path: PathBuf::from("Qwen3-4B-Q4_K_M.gguf"),
            alias: Some("qwen-coder".into()),
            ..Default::default()
        };
        assert_eq!(served_alias(&config).as_deref(), Some("qwen-coder"));
        config.alias = Some("  ".into());
        assert_eq!(served_alias(&config).as_deref(), Some("Qwen3-4B-Q4_K_M"));
        config.custom_args = Some(r#"--verbose -a "yerel model""#.into());
        assert_eq!(custom_alias(&config).as_deref(), Some("yerel model"));
        assert_eq!(served_alias(&config).as_deref(), Some("yerel model"));
        config.custom_args = Some("--verbose".into());
        assert_eq!(custom_alias(&config), None);
        config.router_mode = true;
        assert_eq!(served_alias(&config), None);
    }

    /// A definition's alias is the served model id; custom `--alias` still wins.
    #[test]
    fn test_definition_alias_is_sent_as_alias() {
        let mut config = LaunchConfig {
            model_path: PathBuf::from("Qwen3-4B-Q4_K_M.gguf"),
            host: "127.0.0.1".to_string(),
            port: 8080,
            alias: Some("qwen-coder".into()),
            ..Default::default()
        };
        let args = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap();
        assert!(args.windows(2).any(|p| p == ["--alias", "qwen-coder"]));
        assert_eq!(args.iter().filter(|a| *a == "--alias").count(), 1);
        config.alias = Some("  ".into());
        let args = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap();
        assert!(args.windows(2).any(|p| p == ["--alias", "Qwen3-4B-Q4_K_M"]));
        config.alias = Some("qwen-coder".into());
        config.custom_args = Some("--alias mine".into());
        let args = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap();
        assert!(args.windows(2).any(|p| p == ["--alias", "mine"]));
        assert_eq!(args.iter().filter(|a| *a == "--alias").count(), 1);
    }

    #[tokio::test]
    async fn audio_idle_stop_without_gateway_respects_activity_and_generation() {
        let mgr = ProcessManager::new();
        *mgr.child.lock().unwrap() = Some(long_running_child());
        *mgr.status.lock().unwrap() = ServerStatus::Running;
        // Match start(): the idle clock starts after the OS has spawned the child.
        *mgr.activity.lock().unwrap() = ProcessActivity::default();
        let idle = Duration::from_millis(100);
        let request = mgr.request_activity().unwrap();
        sleep(idle * 2).await;
        assert!(
            !mgr.stop_audio_if_idle(0, idle).await,
            "active speech must survive"
        );
        drop(request);
        assert!(
            !mgr.stop_audio_if_idle(0, idle).await,
            "completion resets idle time"
        );
        sleep(idle * 2).await;
        assert!(
            !mgr.stop_audio_if_idle(1, idle).await,
            "stale supervisor must not stop a run"
        );
        assert!(mgr.stop_audio_if_idle(0, idle).await);
        assert_eq!(mgr.get_status(), ServerStatus::Stopped);
        assert!(!mgr.has_child());
        assert!(
            mgr.request_activity().is_none(),
            "retiring run refuses requests"
        );
    }

    #[tokio::test]
    async fn audio_idle_stop_without_requests_uses_launch_time() {
        let mgr = ProcessManager::new();
        *mgr.child.lock().unwrap() = Some(long_running_child());
        *mgr.status.lock().unwrap() = ServerStatus::Running;
        // Match start(): the idle clock starts after the OS has spawned the child.
        *mgr.activity.lock().unwrap() = ProcessActivity::default();
        let idle = Duration::from_millis(100);
        assert!(!mgr.stop_audio_if_idle(0, idle).await);
        sleep(idle * 2).await;
        assert!(mgr.stop_audio_if_idle(0, idle).await);
        assert_eq!(mgr.get_status(), ServerStatus::Stopped);
        assert!(!mgr.has_child());
    }

    #[tokio::test]
    async fn test_stop_reaps_child_from_error_state() {
        let mgr = ProcessManager::new();
        *mgr.child.lock().unwrap() = Some(long_running_child());
        *mgr.status.lock().unwrap() = ServerStatus::Error("yükleme hatası".to_string());

        mgr.stop().await.unwrap();

        assert_eq!(mgr.get_status(), ServerStatus::Stopped);
        assert!(!mgr.has_child());
    }

    /// An automatic restart that waited for the lifecycle lock while the user stopped the server
    /// does not start it again; one whose run is still current and failed does go ahead.
    #[tokio::test]
    async fn test_restart_yields_to_a_stop_that_came_first() {
        let mgr = Arc::new(ProcessManager::new());
        *mgr.status.lock().unwrap() = ServerStatus::Error("crashed".to_string());
        let failed = mgr.generation.load(std::sync::atomic::Ordering::SeqCst);
        let config = || LaunchConfig {
            binary_path: PathBuf::from("does-not-exist/llama-server"),
            model_path: PathBuf::from("m.gguf"),
            ..Default::default()
        };

        // The user's stop queues for the lock before the restart's timer fires (the lock is fair:
        // whoever waits first gets it first).
        let busy = mgr.lifecycle.lock().await;
        let stop = {
            let mgr = Arc::clone(&mgr);
            tokio::spawn(async move { mgr.stop().await })
        };
        tokio::task::yield_now().await;
        let restart = {
            let mgr = Arc::clone(&mgr);
            let config = config();
            tokio::spawn(async move { mgr.start_run(config, Some(failed)).await })
        };
        tokio::task::yield_now().await;
        drop(busy);
        stop.await.unwrap().unwrap();
        restart.await.unwrap().unwrap();
        assert_eq!(mgr.get_status(), ServerStatus::Stopped);
        assert_eq!(mgr.start_count(), 0);

        // Without the stop, the restart goes ahead (and fails here: no runtime).
        *mgr.status.lock().unwrap() = ServerStatus::Error("crashed".to_string());
        let failed = mgr.generation.load(std::sync::atomic::Ordering::SeqCst);
        let _ = mgr.start_run(config(), Some(failed)).await;
        assert_ne!(
            mgr.get_status(),
            ServerStatus::Error("crashed".to_string()),
            "the restart did not run"
        );
    }

    /// A stop that timed out leaves its process tracked with an Error status; the next start ends
    /// that process before anything else instead of overwriting (and losing) its handle.
    #[tokio::test]
    async fn test_start_ends_the_process_a_timed_out_stop_left() {
        let mgr = ProcessManager::new();
        *mgr.child.lock().unwrap() = Some(long_running_child());
        *mgr.status.lock().unwrap() = ServerStatus::Error("process.stop_timeout".to_string());
        *mgr.active.lock().unwrap() = Some(ActiveServerInfo {
            connect_host: "127.0.0.1".to_string(),
            bind_host: "127.0.0.1".to_string(),
            port: 1,
            api_key: None,
            api_key_set: false,
            api_key_required: false,
            model_path: PathBuf::from("model.gguf"),
            router_mode: false,
            exposure_acknowledged: false,
            cors_origins: None,
            serve_mode: Default::default(),
            engine_family: String::new(),
        });

        // The runtime is missing, so the start itself fails after the leftover is gone.
        let config = LaunchConfig {
            binary_path: PathBuf::from("does-not-exist/llama-server"),
            model_path: PathBuf::from("m.gguf"),
            ..Default::default()
        };
        let _ = mgr.start(config).await;

        assert!(!mgr.has_child(), "the leftover process was not ended");
        assert!(
            mgr.active.lock().unwrap().is_none(),
            "its run info was kept"
        );
        assert!(matches!(mgr.get_status(), ServerStatus::Error(_)));
    }

    /// A start sent to a running (or starting) server is refused and leaves its process alone.
    #[tokio::test]
    async fn test_start_never_ends_a_running_server() {
        for status in [ServerStatus::Running, ServerStatus::Starting] {
            let mgr = ProcessManager::new();
            *mgr.child.lock().unwrap() = Some(long_running_child());
            *mgr.status.lock().unwrap() = status.clone();
            let config = LaunchConfig {
                binary_path: PathBuf::from("does-not-exist/llama-server"),
                model_path: PathBuf::from("m.gguf"),
                ..Default::default()
            };
            let e = mgr.start(config).await.unwrap_err();
            assert!(e.starts_with("servers.already_active"), "{e}");
            assert_eq!(mgr.get_status(), status);
            let alive = mgr
                .child
                .lock()
                .unwrap()
                .as_mut()
                .map(|c| matches!(c.try_wait(), Ok(None)));
            assert_eq!(
                alive,
                Some(true),
                "{status:?}: the running process was ended"
            );
            mgr.stop().await.unwrap();
        }
    }

    /// `{:?}` of a server's connection info never prints its key.
    #[test]
    fn active_server_debug_redacts_the_key() {
        let info = ActiveServerInfo {
            connect_host: "127.0.0.1".into(),
            bind_host: "127.0.0.1".into(),
            port: 1,
            api_key: Some("k-secret-value".into()),
            api_key_set: true,
            api_key_required: true,
            model_path: PathBuf::from("m"),
            router_mode: false,
            exposure_acknowledged: false,
            cors_origins: None,
            serve_mode: Default::default(),
            engine_family: String::new(),
        };
        let text = format!("{info:?}");
        assert!(!text.contains("k-secret-value"), "{text}");
        assert!(text.contains("api_key: Some(\"<redacted>\")"), "{text}");
    }

    /// A failure names the engine of its run (its messages say "the mlx server", not always
    /// llama-server); one that already names an engine keeps it.
    #[test]
    fn test_failures_name_their_engine() {
        let mgr = ProcessManager::new();
        mgr.record_failure(ServerFailure::new(
            FailureKind::Crashed,
            FailurePhase::Running,
        ));
        let engine = |m: &ProcessManager| {
            m.failure
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .params
                .get("engine")
                .cloned()
        };
        assert_eq!(engine(&mgr).as_deref(), Some("llama.cpp"));
        *mgr.engine.lock().unwrap() = crate::engine::mlx::MLX;
        mgr.record_failure(ServerFailure::new(
            FailureKind::Crashed,
            FailurePhase::Running,
        ));
        assert_eq!(engine(&mgr).as_deref(), Some("MLX"));
        *mgr.engine.lock().unwrap() = crate::engine::PYTHON_HF;
        mgr.record_failure(ServerFailure::new(
            FailureKind::Crashed,
            FailurePhase::Running,
        ));
        assert_eq!(engine(&mgr).as_deref(), Some("Python (HF)"));
        mgr.record_failure(
            ServerFailure::new(FailureKind::Crashed, FailurePhase::Running).with("engine", "x"),
        );
        assert_eq!(engine(&mgr).as_deref(), Some("x"));
    }

    #[tokio::test]
    async fn test_stop_waits_for_in_progress_start_section_then_reaps_child() {
        let mgr = Arc::new(ProcessManager::new());
        let start_section = mgr.lifecycle.lock().await;
        *mgr.status.lock().unwrap() = ServerStatus::Starting;
        *mgr.child.lock().unwrap() = Some(long_running_child());

        let stopping = {
            let mgr = Arc::clone(&mgr);
            tokio::spawn(async move { mgr.stop().await })
        };
        tokio::task::yield_now().await;
        assert_eq!(mgr.get_status(), ServerStatus::Starting);
        assert!(mgr.has_child());

        drop(start_section);
        stopping.await.unwrap().unwrap();
        assert_eq!(mgr.get_status(), ServerStatus::Stopped);
        assert!(!mgr.has_child());
    }

    #[tokio::test]
    async fn test_health_timeout_kills_and_clears_active_process() {
        let mgr = ProcessManager::new();
        *mgr.child.lock().unwrap() = Some(long_running_child());
        *mgr.status.lock().unwrap() = ServerStatus::Starting;
        *mgr.active.lock().unwrap() = Some(ActiveServerInfo {
            connect_host: "127.0.0.1".to_string(),
            bind_host: "127.0.0.1".to_string(),
            port: 1,
            api_key: None,
            api_key_set: false,
            api_key_required: false,
            model_path: PathBuf::from("model.gguf"),
            router_mode: false,
            exposure_acknowledged: false,
            cors_origins: None,
            serve_mode: Default::default(),
            engine_family: String::new(),
        });
        mgr.generation.store(1, std::sync::atomic::Ordering::SeqCst);

        mgr.supervise_with_timeout(
            1,
            crate::engine::llama_cpp(),
            "http://127.0.0.1:1/health".to_string(),
            Duration::ZERO,
        )
        .await;

        assert!(matches!(mgr.get_status(), ServerStatus::Error(_)));
        assert!(!mgr.has_child());
        assert!(mgr.active.lock().unwrap().is_none());
    }

    #[test]
    fn test_server_caps_from_help() {
        let new_help = "-fa,   --flash-attn [on|off|auto]       set Flash Attention use\n--models-dir PATH   dir\n--metrics   enable";
        let caps = ServerCaps::from_help(new_help);
        assert!(caps.flash_attn_takes_value);
        assert_eq!(caps.models_dir_flag.as_deref(), Some("--models-dir"));
        assert!(caps.supports_metrics);

        let old_help =
            "-fa,   --flash-attn   enable Flash Attention (default: disabled)\n--metrics";
        let caps = ServerCaps::from_help(old_help);
        assert!(!caps.flash_attn_takes_value);
        assert_eq!(caps.models_dir_flag, None);

        let config = LaunchConfig {
            model_path: PathBuf::from("m.gguf"),
            router_mode: true,
            ..Default::default()
        };
        assert!(ProcessManager::build_args(&config, &caps).is_err());
    }

    #[test]
    fn test_logs_since() {
        let mgr = ProcessManager::new();
        for i in 0..5 {
            mgr.push_log(format!("l{}", i));
        }
        let (next, lines) = mgr.get_logs_since(3);
        assert_eq!(next, 5);
        assert_eq!(lines, vec!["l3", "l4"]);
        let (_, all) = mgr.get_logs_since(0);
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn test_run_logs_since_excludes_previous_run_and_preserves_cursor() {
        let mgr = ProcessManager::new();
        mgr.push_log("old run".into());
        mgr.run_log_start
            .store(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(mgr.get_run_logs_since(0), (1, vec![]));
        mgr.push_log("new run".into());
        assert_eq!(mgr.get_run_logs_since(0), (2, vec!["new run".into()]));
        assert_eq!(mgr.get_run_logs_since(2), (2, vec![]));
        assert_eq!(mgr.get_logs_since(0).1.len(), 2);
    }

    #[test]
    fn test_timed_logs_are_in_order_and_current() {
        let mgr = ProcessManager::new();
        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        for i in 0..4 {
            mgr.push_log(format!("t{}", i));
        }
        let (next, lines) = mgr.get_timed_logs_since(1);
        assert_eq!(next, 4);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].text, "t1");
        assert!(lines.iter().all(|l| l.at_ms >= before));
        assert!(lines.windows(2).all(|w| w[0].at_ms <= w[1].at_ms));
    }

    #[test]
    fn test_load_timeout_and_restart_delay() {
        let mut c = LaunchConfig::default();
        assert_eq!(load_timeout_for(&c, 300), Duration::from_secs(300));
        c.params
            .insert(LOAD_TIMEOUT.into(), crate::params::ParamValue::Int(900));
        assert_eq!(load_timeout_for(&c, 300), Duration::from_secs(900));
        c.params.insert(
            LOAD_TIMEOUT.into(),
            crate::params::ParamValue::Text("45".into()),
        );
        assert_eq!(load_timeout_for(&c, 300), Duration::from_secs(45));
        c.params
            .insert(LOAD_TIMEOUT.into(), crate::params::ParamValue::Int(999_999));
        assert_eq!(load_timeout_for(&c, 300), Duration::from_secs(7200));
        assert_eq!(restart_delay(1), Duration::from_secs(2));
        assert_eq!(restart_delay(2), Duration::from_secs(4));
        assert_eq!(restart_delay(3), Duration::from_secs(8));
        assert_eq!(restart_delay(9), Duration::from_secs(60));
        // Neither the load timeout nor an explicit `fit on` reach the command line as such.
        c.params
            .insert("fit".into(), crate::params::ParamValue::Text("on".into()));
        c.gpu_layers = 99;
        c.model_path = PathBuf::from("m.gguf");
        c.host = "127.0.0.1".into();
        let caps = crate::capabilities::testdata::caps("b11236");
        let args =
            ProcessManager::build_args_with(&c, &ServerCaps::from_runtime(&caps), Some(&caps))
                .unwrap()
                .args;
        assert!(
            !args
                .iter()
                .any(|a| a == "-ngl" || a.contains("load_timeout")),
            "{args:?}"
        );
        let fit = args
            .iter()
            .position(|a| a == "--fit" || a == "-fit")
            .expect("--fit");
        assert_eq!(args[fit + 1], "on");
    }

    #[tokio::test]
    async fn test_concurrent_start_rejected_and_stop_idempotent() {
        let mgr = ProcessManager::new();
        *mgr.status.lock().unwrap() = ServerStatus::Starting;
        let err = mgr.start(LaunchConfig::default()).await.unwrap_err();
        assert_eq!(err, crate::orchestrator::ERR_ALREADY_ACTIVE);
        *mgr.status.lock().unwrap() = ServerStatus::Stopped;
        assert!(mgr.stop().await.is_ok());
        assert_eq!(mgr.get_status(), ServerStatus::Stopped);
    }
}

#[cfg(test)]
mod e2e {
    use super::*;

    /// Real llama-server smoke test. Run with:
    /// HS_LLAMA_SERVER=<path to llama-server> HS_TEST_MODEL=<small .gguf> cargo test e2e -- --ignored
    #[tokio::test]
    #[ignore]
    async fn test_real_server_lifecycle_and_crash_detection() {
        let (Ok(bin), Ok(model)) = (
            std::env::var("HS_LLAMA_SERVER"),
            std::env::var("HS_TEST_MODEL"),
        ) else {
            eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
            return;
        };
        let mgr = ProcessManager::new();
        let config = LaunchConfig {
            binary_path: PathBuf::from(bin),
            model_path: PathBuf::from(model),
            gpu_layers: 0,
            context_size: 512,
            host: "127.0.0.1".to_string(),
            port: 18089,
            flash_attn: true,
            custom_args: Some(r#"--alias "smoke test""#.to_string()),
            ..Default::default()
        };
        mgr.start(config.clone()).await.unwrap();
        assert!(
            mgr.start(config).await.is_err(),
            "second start must be rejected"
        );

        let t = Instant::now();
        while mgr.get_status() != ServerStatus::Running {
            assert!(
                t.elapsed() < Duration::from_secs(60),
                "not ready: {:?} {:?}",
                mgr.get_status(),
                mgr.get_logs(Some(20))
            );
            if let ServerStatus::Error(e) = mgr.get_status() {
                panic!("start failed: {} {:?}", e, mgr.get_logs(Some(30)));
            }
            sleep(Duration::from_millis(200)).await;
        }

        // Simulate a crash: kill the process behind the manager's back.
        mgr.child.lock().unwrap().as_mut().unwrap().kill().unwrap();
        let t = Instant::now();
        loop {
            if let ServerStatus::Error(e) = mgr.get_status() {
                let (key, params) = crate::error_key::parse(&e).unwrap();
                assert_eq!(key, "process.exited_unexpectedly", "{}", e);
                assert_eq!(params["code"], "servers.crashed", "{}", e);
                let f = mgr.last_failure().unwrap();
                assert_eq!(f.kind, FailureKind::Crashed, "{f:?}");
                assert_eq!(f.phase, FailurePhase::Running);
                break;
            }
            assert!(t.elapsed() < Duration::from_secs(5), "crash not detected");
            sleep(Duration::from_millis(100)).await;
        }

        // Restart then stop cleanly: must end Stopped, not Error.
        let config = LaunchConfig {
            binary_path: PathBuf::from(std::env::var("HS_LLAMA_SERVER").unwrap()),
            model_path: PathBuf::from(std::env::var("HS_TEST_MODEL").unwrap()),
            context_size: 512,
            host: "127.0.0.1".to_string(),
            port: 18089,
            ..Default::default()
        };
        mgr.start(config).await.unwrap();
        sleep(Duration::from_millis(1500)).await;
        mgr.stop().await.unwrap();
        sleep(Duration::from_millis(800)).await;
        assert_eq!(mgr.get_status(), ServerStatus::Stopped);
    }

    /// Waits until the run leaves `Starting`; returns the final status.
    async fn settle(mgr: &ProcessManager, secs: u64) -> ServerStatus {
        let t = Instant::now();
        loop {
            let s = mgr.get_status();
            if s != ServerStatus::Starting {
                return s;
            }
            assert!(
                t.elapsed() < Duration::from_secs(secs),
                "still starting: {:?}",
                mgr.get_logs(Some(20))
            );
            sleep(Duration::from_millis(100)).await;
        }
    }

    /// A GGUF header with an architecture llama.cpp does not know (no tensors).
    fn fake_arch_gguf(path: &std::path::Path) {
        let s = |x: &str| {
            let mut v = (x.len() as u64).to_le_bytes().to_vec();
            v.extend_from_slice(x.as_bytes());
            v
        };
        let mut b = b"GGUF".to_vec();
        b.extend_from_slice(&3u32.to_le_bytes());
        b.extend_from_slice(&0u64.to_le_bytes());
        b.extend_from_slice(&2u64.to_le_bytes());
        b.extend(s("general.architecture"));
        b.extend_from_slice(&8u32.to_le_bytes());
        b.extend(s("fakearch"));
        b.extend(s("general.alignment"));
        b.extend_from_slice(&4u32.to_le_bytes());
        b.extend_from_slice(&32u32.to_le_bytes());
        std::fs::write(path, b).unwrap();
    }

    /// Failure classification against the real binary (ports 18700+). Run with:
    /// HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<small .gguf> cargo test -p harness-core
    ///   process::e2e::test_real_failure_classes -- --ignored --nocapture
    /// HS_TEST_OOM=1 also asks the GPU for far more KV cache than 16 GB (short, ~15 s).
    /// Bind race on an automatic port (item: another program takes the port between Harness's
    /// check and llama-server's bind): the run moves to another free port, still "Starting",
    /// and comes up. Needs HS_LLAMA_SERVER and HS_TEST_MODEL.
    #[tokio::test]
    #[ignore]
    async fn test_real_auto_port_moves_after_bind_failure() {
        let (Ok(bin), Ok(model)) = (
            std::env::var("HS_LLAMA_SERVER"),
            std::env::var("HS_TEST_MODEL"),
        ) else {
            eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
            return;
        };
        // The "other program": holds a port of the automatic range (the orchestrator's check is
        // skipped here, as if the port had been taken right after it).
        let listener = crate::ports::AUTO_RANGE
            .clone()
            .find_map(|p| std::net::TcpListener::bind(("127.0.0.1", p)).ok())
            .expect("a free port in the automatic range");
        let taken = listener.local_addr().unwrap().port();
        let config = LaunchConfig {
            binary_path: PathBuf::from(&bin),
            model_path: PathBuf::from(&model),
            gpu_layers: 0,
            context_size: 512,
            host: "127.0.0.1".to_string(),
            port: taken,
            port_auto: true,
            ..Default::default()
        };
        let mgr = ProcessManager::with_id("e2e-port-retry");
        mgr.start(config.clone()).await.unwrap();
        let status = settle(&mgr, 180).await;
        let logs = mgr.get_logs(Some(40));
        assert_eq!(status, ServerStatus::Running, "{logs:#?}");
        let port = mgr.active_server().unwrap().port;
        assert_ne!(port, taken);
        assert!(crate::ports::AUTO_RANGE.contains(&port), "{port}");
        assert!(
            logs.iter().any(|l| l.contains("yeniden deneniyor")),
            "{logs:#?}"
        );
        // The server really answers on the new port.
        let health = reqwest::get(format!("http://127.0.0.1:{port}/health"))
            .await
            .unwrap();
        assert!(health.status().is_success());
        mgr.stop().await.unwrap();

        // A port the user chose is not moved: the start fails as before.
        let mgr = ProcessManager::with_id("e2e-port-explicit");
        mgr.start(LaunchConfig {
            port_auto: false,
            ..config
        })
        .await
        .unwrap();
        let status = settle(&mgr, 120).await;
        assert!(matches!(status, ServerStatus::Error(_)), "{status:?}");
        assert_eq!(mgr.last_failure().unwrap().kind, FailureKind::PortInUse);
        let logs = mgr.get_logs(Some(40));
        assert!(
            logs.iter().any(|l| l.contains(&format!("{taken} portunu"))),
            "holder named: {logs:#?}"
        );
        drop(listener);
    }

    #[tokio::test]
    #[ignore]
    async fn test_real_failure_classes() {
        let (Ok(bin), Ok(model)) = (
            std::env::var("HS_LLAMA_SERVER"),
            std::env::var("HS_TEST_MODEL"),
        ) else {
            eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
            return;
        };
        let base = LaunchConfig {
            binary_path: PathBuf::from(&bin),
            model_path: PathBuf::from(&model),
            gpu_layers: 0,
            context_size: 512,
            host: "127.0.0.1".to_string(),
            port: 18711,
            ..Default::default()
        };
        let run = |config: LaunchConfig| async move {
            let mgr = ProcessManager::with_id("e2e-failure");
            mgr.start(config).await.unwrap();
            let status = settle(&mgr, 120).await;
            assert!(matches!(status, ServerStatus::Error(_)), "{status:?}");
            let f = mgr.last_failure().expect("classified failure");
            eprintln!("{} {:?} {:?}", f.code, f.params, f.evidence);
            f
        };

        // Unknown flag.
        let f = run(LaunchConfig {
            custom_args: Some("--no-such-flag-xyz".into()),
            ..base.clone()
        })
        .await;
        assert_eq!(f.kind, FailureKind::InvalidArgument);
        assert_eq!(f.params["flag"], "--no-such-flag-xyz");

        // Port taken by another program (the orchestrator's pre-check is skipped here).
        let listener = std::net::TcpListener::bind(("127.0.0.1", 18712)).unwrap();
        let f = run(LaunchConfig {
            port: 18712,
            ..base.clone()
        })
        .await;
        drop(listener);
        assert_eq!(f.kind, FailureKind::PortInUse);
        assert_eq!(f.params["port"], "18712");

        // Truncated copy and a GGUF with an unknown architecture (temp dir, removed after).
        let dir = tempfile::tempdir().unwrap();
        let truncated = dir.path().join("truncated.gguf");
        {
            use std::io::{Read, Write};
            let mut src = std::fs::File::open(&model).unwrap();
            let mut buf = vec![0u8; 64 << 20];
            src.read_exact(&mut buf).unwrap();
            std::fs::File::create(&truncated)
                .unwrap()
                .write_all(&buf)
                .unwrap();
        }
        let f = run(LaunchConfig {
            model_path: truncated,
            ..base.clone()
        })
        .await;
        assert_eq!(f.kind, FailureKind::CorruptModel);
        let fake = dir.path().join("fake.gguf");
        fake_arch_gguf(&fake);
        let f = run(LaunchConfig {
            model_path: fake,
            ..base.clone()
        })
        .await;
        assert_eq!(f.kind, FailureKind::UnsupportedArchitecture);
        assert_eq!(f.params["arch"], "fakearch");
        drop(dir);

        // Load timeout (1 s is less than any real load).
        let mut config = base.clone();
        config
            .params
            .insert(LOAD_TIMEOUT.into(), crate::params::ParamValue::Int(1));
        let f = run(config).await;
        assert_eq!(f.kind, FailureKind::LoadTimeout);
        assert_eq!(f.params["seconds"], "1");

        if std::env::var("HS_TEST_OOM").as_deref() == Ok("1") {
            let f = run(LaunchConfig {
                gpu_layers: 99,
                context_size: 262_144,
                parallel_slots: Some(1),
                ..base.clone()
            })
            .await;
            assert_eq!(f.kind, FailureKind::OutOfMemory);
            assert_eq!(f.params.get("stage").map(String::as_str), Some("kv_cache"));
        }
    }

    /// Endpoint verification + effective values against a real server. Run with:
    /// HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<small .gguf> cargo test -p harness-core
    ///   e2e::test_real_server_capabilities -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn test_real_server_capabilities() {
        use crate::capabilities::features::{feature_status, CapabilityLevel, FeatureContext};
        use crate::params::ParamValue;
        let (Ok(bin), Ok(model)) = (
            std::env::var("HS_LLAMA_SERVER"),
            std::env::var("HS_TEST_MODEL"),
        ) else {
            eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
            return;
        };
        let mgr = ProcessManager::with_id("caps-e2e");
        let mut config = LaunchConfig {
            binary_path: PathBuf::from(&bin),
            model_path: PathBuf::from(&model),
            gpu_layers: 99,
            context_size: 4096,
            parallel_slots: Some(2),
            host: "127.0.0.1".to_string(),
            port: 18091,
            ..Default::default()
        };
        config.params.insert("jinja".into(), ParamValue::Bool(true));
        config
            .params
            .insert("cache_ram".into(), ParamValue::Int(512));
        config
            .params
            .insert("kv_unified".into(), ParamValue::Bool(false));
        config.params.insert(
            "reasoning_format".into(),
            ParamValue::Text("deepseek".into()),
        );
        mgr.start(config).await.unwrap();

        let t = Instant::now();
        while mgr.get_status() != ServerStatus::Running {
            if let ServerStatus::Error(e) = mgr.get_status() {
                panic!("start failed: {} {:?}", e, mgr.get_logs(Some(30)));
            }
            assert!(t.elapsed() < Duration::from_secs(120), "not ready");
            sleep(Duration::from_millis(200)).await;
        }
        let t = Instant::now();
        let eff = loop {
            if let Some(v) = mgr.effective_values() {
                break v;
            }
            assert!(t.elapsed() < Duration::from_secs(30), "no effective values");
            sleep(Duration::from_millis(200)).await;
        };
        eprintln!("effective: {:?}", eff);
        assert_eq!(eff.total_slots, Some(2));
        assert_eq!(eff.n_ctx, Some(2048), "4096 split over 2 slots");
        assert_eq!(eff.kv_unified, Some(false));
        assert!(!eff.ctx_adjusted);
        // Catalog params reached the real command line (read back from the OS).
        let pid = mgr.pid().unwrap();
        let mut sys = sysinfo::System::new();
        sys.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
            true,
            sysinfo::ProcessRefreshKind::nothing().with_cmd(sysinfo::UpdateKind::Always),
        );
        let cmd: Vec<String> = sys
            .process(sysinfo::Pid::from_u32(pid))
            .unwrap()
            .cmd()
            .iter()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        eprintln!("command line: {:?}", cmd);
        for pair in [["--cache-ram", "512"], ["--reasoning-format", "deepseek"]] {
            assert!(cmd.windows(2).any(|w| w == pair), "{:?} missing", pair);
        }
        assert!(cmd.contains(&"--jinja".to_string()));
        assert!(cmd.contains(&"--no-kv-unified".to_string()));
        assert!(eff.build_info.as_deref().unwrap_or("").starts_with('b'));
        let logs = mgr.get_logs(None).join("\n");
        assert!(!logs.contains("Parametre gönderilmedi"), "{}", logs);

        // The probe is recorded on the registry entry of this binary.
        let registry = crate::capabilities::registry();
        let t = Instant::now();
        let caps = loop {
            if let Some(c) = registry
                .cached(std::path::Path::new(&bin))
                .filter(|c| c.verification("props").is_some())
            {
                break c;
            }
            assert!(t.elapsed() < Duration::from_secs(10), "probe not recorded");
            sleep(Duration::from_millis(100)).await;
        };
        for feature in [
            "props",
            "metrics",
            "slots",
            "models_endpoint",
            "apply_template",
        ] {
            let v = caps
                .verification(feature)
                .unwrap_or_else(|| panic!("{feature}"));
            assert!(v.ok, "{feature}: {:?}", v);
        }
        eprintln!("verified: {:?}", caps.endpoint_verified);
        assert!(caps
            .metric_names
            .iter()
            .any(|m| m == "llamacpp:prompt_tokens_total"));

        let list = feature_status(
            &caps,
            FeatureContext {
                model: None,
                model_path: Some(&model),
                has_mmproj: false,
            },
        );
        let get = |id: &str| list.iter().find(|f| f.id == id).unwrap().clone();
        assert_eq!(get("metrics").level, CapabilityLevel::EndpointVerified);
        assert_eq!(get("props").level, CapabilityLevel::EndpointVerified);
        let tools = get("tool_calling");
        assert_eq!(tools.level, CapabilityLevel::ModelDependent);
        assert!(
            tools.available.is_some(),
            "observed from /props: {:?}",
            tools
        );
        let vision = get("vision");
        assert_eq!(vision.available, Some(false), "{:?}", vision);

        mgr.stop().await.unwrap();
        assert_eq!(mgr.get_status(), ServerStatus::Stopped);
        assert!(mgr.effective_values().is_none());
        assert!(!mgr.has_child());
    }
}

/// Golden tests: exact `build_args` output for every existing `LaunchConfig` field.
/// Written against the pre-catalog implementation; the catalog refactor must keep them green.
#[cfg(test)]
pub(crate) mod golden {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    pub(crate) fn real_help(build: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(format!("llama-server-{}.help.txt", build));
        std::fs::read_to_string(path).unwrap()
    }

    pub(crate) fn base() -> LaunchConfig {
        LaunchConfig {
            binary_path: PathBuf::from("llama-server.exe"),
            model_path: PathBuf::from("models/Qwen3-4B-Q4_K_M.gguf"),
            gpu_layers: 0,
            context_size: 4096,
            host: "127.0.0.1".to_string(),
            port: 8080,
            ..Default::default()
        }
    }

    pub(crate) fn full() -> LaunchConfig {
        LaunchConfig {
            gpu_layers: 33,
            context_size: 8192,
            threads: Some(6),
            flash_attn: true,
            cache_type_k: Some("q8_0".to_string()),
            cache_type_v: Some("q4_0".to_string()),
            parallel_slots: Some(2),
            logical_batch: Some(1024),
            physical_batch: Some(256),
            context_shift: Some(false),
            split_mode: Some("layer".to_string()),
            main_gpu: Some(1),
            tensor_split: Some("3,1".to_string()),
            rope_scaling: Some("yarn".to_string()),
            rope_freq_base: Some(10000.0),
            rope_freq_scale: Some(0.5),
            chat_template: Some("chatml".to_string()),
            api_key: Some("secret".to_string()),
            draft_model_path: Some(PathBuf::from("models/draft.gguf")),
            draft_gpu_layers: Some(99),
            lora_path: Some(PathBuf::from("models/a.gguf")),
            lora_scale: Some(0.75),
            mmproj_path: Some(PathBuf::from("models/mmproj.gguf")),
            custom_args: Some("--no-warmup -lv 3".to_string()),
            ..base()
        }
    }

    const FULL_EXPECTED: &[&str] = &[
        "-m",
        "models/Qwen3-4B-Q4_K_M.gguf",
        "--alias",
        "Qwen3-4B-Q4_K_M",
        "-ngl",
        "33",
        "-c",
        "8192",
        "--host",
        "127.0.0.1",
        "--port",
        "8080",
        "--metrics",
        "-t",
        "6",
        "--flash-attn",
        "on",
        "-ctk",
        "q8_0",
        "-ctv",
        "q4_0",
        "-np",
        "2",
        "-b",
        "1024",
        "-ub",
        "256",
        "--no-context-shift",
        "--split-mode",
        "layer",
        "--main-gpu",
        "1",
        "-ts",
        "3,1",
        "--rope-scaling",
        "yarn",
        "--rope-freq-base",
        "10000",
        "--rope-freq-scale",
        "0.5",
        "--chat-template",
        "chatml",
        "-md",
        "models/draft.gguf",
        "-ngld",
        "99",
        "--lora-scaled",
        "models/a.gguf:0.75",
        "--mmproj",
        "models/mmproj.gguf",
        "--no-warmup",
        "-lv",
        "3",
    ];

    #[test]
    fn golden_minimal_cpu() {
        let args = ProcessManager::build_args(&base(), &ServerCaps::default()).unwrap();
        assert_eq!(
            args,
            s(&[
                "-m",
                "models/Qwen3-4B-Q4_K_M.gguf",
                "--alias",
                "Qwen3-4B-Q4_K_M",
                "-ngl",
                "0",
                "-c",
                "4096",
                "--host",
                "127.0.0.1",
                "--port",
                "8080",
                "--metrics",
            ])
        );
    }

    #[test]
    fn golden_every_field_default_caps() {
        let args = ProcessManager::build_args(&full(), &ServerCaps::default()).unwrap();
        assert_eq!(args, s(FULL_EXPECTED));
    }

    #[test]
    fn golden_every_field_real_help_both_builds() {
        for build in ["b11236", "b11026"] {
            let caps = ServerCaps::from_help(&real_help(build));
            let args = ProcessManager::build_args(&full(), &caps).unwrap();
            assert_eq!(args, s(FULL_EXPECTED), "{build}");
        }
    }

    #[test]
    fn golden_variants() {
        // Bare flash-attn (old dialect), no metrics, lora without scale, context shift on,
        // rope "none" is dropped, empty strings are ignored, user alias suppresses ours.
        let caps = ServerCaps {
            flash_attn_takes_value: false,
            models_dir_flag: None,
            supports_metrics: false,
            supports_tools: false,
            supports_mcp: false,
            supports_cors_origins: false,
        };
        let config = LaunchConfig {
            flash_attn: true,
            context_shift: Some(true),
            rope_scaling: Some("none".to_string()),
            cache_type_k: Some(String::new()),
            split_mode: Some(String::new()),
            tensor_split: Some(String::new()),
            chat_template: Some(String::new()),
            parallel_slots: Some(0),
            logical_batch: Some(0),
            physical_batch: Some(0),
            lora_path: Some(PathBuf::from("models/a.gguf")),
            draft_model_path: Some(PathBuf::new()),
            draft_gpu_layers: Some(5),
            mmproj_path: Some(PathBuf::new()),
            custom_args: Some("--alias mine".to_string()),
            ..base()
        };
        let args = ProcessManager::build_args(&config, &caps).unwrap();
        assert_eq!(
            args,
            s(&[
                "-m",
                "models/Qwen3-4B-Q4_K_M.gguf",
                "-ngl",
                "0",
                "-c",
                "4096",
                "--host",
                "127.0.0.1",
                "--port",
                "8080",
                "--flash-attn",
                "--context-shift",
                "--lora",
                "models/a.gguf",
                "--alias",
                "mine",
            ])
        );
    }

    #[test]
    fn golden_gpu_pinning_uses_primary_gpu() {
        let config = LaunchConfig {
            gpu_layers: 10,
            ..base()
        };
        let args = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap();
        let mut expected = s(&[
            "-m",
            "models/Qwen3-4B-Q4_K_M.gguf",
            "--alias",
            "Qwen3-4B-Q4_K_M",
            "-ngl",
            "10",
            "-c",
            "4096",
            "--host",
            "127.0.0.1",
            "--port",
            "8080",
            "--metrics",
        ]);
        if let Some(gpu) = crate::gpu::primary_gpu_index() {
            expected.extend(s(&["--split-mode", "none", "--main-gpu"]));
            expected.push(gpu.to_string());
        }
        assert_eq!(args, expected);
    }

    #[test]
    fn catalog_params_go_before_custom_args_and_leave_typed_fields_unchanged() {
        use crate::params::ParamValue;
        let runtime = crate::capabilities::testdata::caps("b11236");
        let caps = ServerCaps::from_runtime(&runtime);
        // No catalog params: the golden output plus Harness' memory defaults (the runtime
        // has both flags), before the custom "--no-warmup -lv 3".
        let defaults = s(&["--cache-ram", "1024", "--ctx-checkpoints", "4"]);
        // The LoRA of `full()` gets the slot save path when the runtime has the flag (a live
        // scale change drops the slots' caches), right before the projector.
        let with_slot_path = |mut v: Vec<String>| {
            let at = v.iter().position(|a| a == "--mmproj").unwrap();
            let dir = crate::paths::data_root().join("slot-cache");
            v.splice(
                at..at,
                [
                    "--slot-save-path".to_string(),
                    dir.to_string_lossy().to_string(),
                ],
            );
            v
        };
        let out = ProcessManager::build_args_with(&full(), &caps, Some(&runtime)).unwrap();
        let mut expected = with_slot_path(s(FULL_EXPECTED));
        let at = expected.len() - 3;
        expected.splice(at..at, defaults.clone());
        assert_eq!(out.args, expected);

        let mut config = full();
        config.params.insert("jinja".into(), ParamValue::Bool(true));
        config
            .params
            .insert("swa_full".into(), ParamValue::Bool(true));
        config
            .params
            .insert("not_in_catalog".into(), ParamValue::Int(3));
        let out = ProcessManager::build_args_with(&config, &caps, Some(&runtime)).unwrap();
        let mut expected = with_slot_path(s(FULL_EXPECTED));
        let at = expected.len() - 3; // before the custom "--no-warmup -lv 3"
        expected.splice(
            at..at,
            s(&[
                "--swa-full",
                "--cache-ram",
                "1024",
                "--ctx-checkpoints",
                "4",
                "--jinja",
            ]),
        );
        assert_eq!(out.args, expected);
        assert_eq!(out.skipped.len(), 1);
        assert_eq!(out.skipped[0].reason, "unknown_param");
        // Plain build_args has no runtime: catalog params are never guessed.
        let plain = ProcessManager::build_args(&config, &caps).unwrap();
        assert_eq!(plain, s(FULL_EXPECTED));
    }

    #[test]
    fn the_host_prompt_cache_choice_is_the_users_when_made() {
        use crate::params::ParamValue;
        let mut c = full();
        c.custom_args = None;
        assert!(!cache_ram_is_set(&c));
        c.params
            .insert("cache_ram".into(), ParamValue::Text(" ".into()));
        assert!(!cache_ram_is_set(&c)); // the "unset" marker of a definition layer
        c.params.insert("cache_ram".into(), ParamValue::Int(0));
        assert!(cache_ram_is_set(&c));
        c.params.clear();
        for custom in ["-cram 512", "--cache-ram=0", "--no-warmup --cache-ram 1024"] {
            c.custom_args = Some(custom.into());
            assert!(cache_ram_is_set(&c), "{custom}");
        }
        c.custom_args = Some("--no-warmup".into());
        assert!(!cache_ram_is_set(&c));
    }

    #[test]
    fn memory_defaults_yield_to_the_user_and_to_old_runtimes() {
        use crate::params::ParamValue;
        let runtime = crate::capabilities::testdata::caps("b11236");
        let caps = ServerCaps::from_runtime(&runtime);
        let flag_value = |args: &[String], flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .map(|i| args[i + 1].clone())
        };
        let mut c = full();
        c.custom_args = None;
        let args = ProcessManager::build_args_with(&c, &caps, Some(&runtime))
            .unwrap()
            .args;
        assert_eq!(flag_value(&args, "--cache-ram").as_deref(), Some("1024"));
        assert_eq!(flag_value(&args, "--ctx-checkpoints").as_deref(), Some("4"));

        // A value of the user's own (even llama-server's 8192 or -1) is sent as is.
        c.params.insert("cache_ram".into(), ParamValue::Int(-1));
        c.params
            .insert("ctx_checkpoints".into(), ParamValue::Int(32));
        let args = ProcessManager::build_args_with(&c, &caps, Some(&runtime))
            .unwrap()
            .args;
        assert_eq!(flag_value(&args, "--cache-ram").as_deref(), Some("-1"));
        assert_eq!(
            flag_value(&args, "--ctx-checkpoints").as_deref(),
            Some("32")
        );
        assert_eq!(args.iter().filter(|a| *a == "--cache-ram").count(), 1);

        // Any spelling in the custom arguments wins too (no second flag from Harness).
        c.params.clear();
        c.custom_args = Some("-cram 0 --swa-checkpoints=8".into());
        let args = ProcessManager::build_args_with(&c, &caps, Some(&runtime))
            .unwrap()
            .args;
        assert!(!args
            .iter()
            .any(|a| a == "--cache-ram" || a == "--ctx-checkpoints"));
        assert!(args.ends_with(&s(&["-cram", "0", "--swa-checkpoints=8"])));

        // A runtime without the flags (older builds) gets nothing and no warning.
        let mut old = crate::capabilities::testdata::caps("b11236");
        old.flags.retain(|k, _| {
            ![
                "--cache-ram",
                "-cram",
                "--ctx-checkpoints",
                "-ctxcp",
                "--swa-checkpoints",
            ]
            .contains(&k.as_str())
        });
        let out =
            ProcessManager::build_args_with(&full(), &ServerCaps::from_runtime(&old), Some(&old))
                .unwrap();
        assert!(!out
            .args
            .iter()
            .any(|a| a == "--cache-ram" || a == "--ctx-checkpoints"));
        assert!(out.skipped.is_empty(), "{:?}", out.skipped);
    }

    #[test]
    fn launch_config_json_without_params_is_compatible() {
        let old = r#"{"model_path":"m.gguf","gpu_layers":1,"context_size":2048,"host":"127.0.0.1","port":8080,"threads":null}"#;
        let config: LaunchConfig = serde_json::from_str(old).unwrap();
        assert!(config.params.is_empty());
        // Empty params are not written, so older app versions read the file unchanged.
        assert!(!serde_json::to_string(&config)
            .unwrap()
            .contains("\"params\""));
        let with = r#"{"model_path":"m.gguf","gpu_layers":1,"context_size":2048,"host":"127.0.0.1","port":8080,"threads":null,"params":{"jinja":false,"cache_ram":2048}}"#;
        let config: LaunchConfig = serde_json::from_str(with).unwrap();
        assert_eq!(config.params.len(), 2);
        let preset: crate::models::ModelPreset = serde_json::from_str(
            r#"{"gpu_layers":1,"context_size":2048,"threads":null,"params":{"fit":"off"}}"#,
        )
        .unwrap();
        assert_eq!(
            preset.params.get("fit"),
            Some(&crate::params::ParamValue::Text("off".into()))
        );
    }

    #[test]
    fn golden_router_and_mcp() {
        let config = LaunchConfig {
            router_mode: true,
            models_path: Some(PathBuf::from("models/dir")),
            mcp_enabled: true,
            mcp_config_path: Some(PathBuf::from("cfg/mcp.json")),
            ..base()
        };
        let caps = ServerCaps::from_help(&real_help("b11236"));
        let args = ProcessManager::build_args(&config, &caps).unwrap();
        assert_eq!(
            args,
            s(&[
                "--models-dir",
                "models/dir",
                "-ngl",
                "0",
                "-c",
                "4096",
                "--host",
                "127.0.0.1",
                "--port",
                "8080",
                "--metrics",
                "--mcp-servers-config",
                "cfg/mcp.json",
            ])
        );
    }

    /// Configs and presets saved while `server_tools` (llama-server `--tools`) existed still
    /// load; the value is ignored and `--tools` is never passed.
    #[test]
    fn old_saved_server_tools_load_and_are_ignored() {
        let old = r#"{"model_path":"m.gguf","gpu_layers":1,"context_size":2048,"host":"127.0.0.1","port":8080,"threads":null,"server_tools":["read_file","all"]}"#;
        let config: LaunchConfig = serde_json::from_str(old).unwrap();
        let args = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap();
        assert!(!args.iter().any(|a| a.contains("tools")), "{args:?}");
        assert!(!serde_json::to_string(&config)
            .unwrap()
            .contains("server_tools"));
        let preset: crate::models::ModelPreset = serde_json::from_str(
            r#"{"gpu_layers":1,"context_size":2048,"threads":null,"server_tools":["all"]}"#,
        )
        .unwrap();
        assert!(!serde_json::to_string(&preset)
            .unwrap()
            .contains("server_tools"));
    }
}
