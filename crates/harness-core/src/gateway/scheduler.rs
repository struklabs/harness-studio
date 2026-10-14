//! Gateway scheduler: which llama-server answers a `model`, and when models load and unload.
//!
//! * **Resolution**: `model` → [`ModelDefinition`] by alias (case-insensitive) or id. A
//!   server that already runs the definition (started from the Dashboard, a profile, the
//!   Library or by the gateway) answers it; nothing is loaded twice.
//! * **JIT load**: otherwise the definition is started on a new loopback instance
//!   (`gw-<alias>`, port after the gateway's) with its own load settings (the same plan as
//!   "Load" in the Library). The first request waits for `/health`.
//! * **Limits before a load** (only models the gateway loaded itself count and are evicted;
//!   servers the user started are left alone, their memory is simply not free):
//!   * `max_loaded_models`: the least recently used idle model is unloaded, else `503
//!     max_loaded` with `Retry-After`.
//!   * memory: estimate (or measurement) of `vram.rs` + headroom against the free VRAM of
//!     `--list-devices`; least recently used idle models are evicted until it fits, else
//!     `503 insufficient_memory`.
//!   * pinned definitions are never evicted and are loaded when the gateway starts (warm).
//! * **OOM recovery** (hook): a JIT load that fails with `servers.out_of_memory` evicts
//!   the least recently used idle model and retries once (`orchestrator::retry_with_overrides`),
//!   else `503 insufficient_memory` with the recovery options of `orchestrator::recovery_plan`.
//! * **Queue**: per definition, at most as many requests run as the server has slots
//!   (`/props` `total_slots`); up to `max_queue` more wait (FIFO), the rest get `503
//!   queue_full` + `Retry-After`.
//! * **Idle TTL**: a definition's `ttl_seconds` (else the gateway default; off by default)
//!   unloads a gateway-loaded model that had no request for that long. The `keep_alive` of
//!   the last Ollama-API request ([`KeepAlive`], [`Scheduler::acquire_keep_alive`]) overrides it
//!   until the model unloads or a later Ollama request sets another one.

use crate::domain::ModelDefinition;
use crate::full_offload::{
    default_context_for, full_offload_context_with, full_offload_log_line, FullOffload,
};
use crate::gateway::admission::Demand;
use crate::gateway::GatewaySettings;
use crate::operation_gate::OperationGate;
use crate::prepare::FIT_MIN_CONTEXT;
use crate::process::ServerStatus;
use crate::runtime::RuntimeManager;
use crate::servers::ServerRegistry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const MIB: u64 = 1024 * 1024;
/// Prefix of the instances the gateway starts.
pub const SERVER_PREFIX: &str = "gw-";
/// Extra wait on top of the load timeout before a JIT load is given up.
const LOAD_GRACE: Duration = Duration::from_secs(30);

/// Why a request could not get a server.
#[derive(Debug, Clone, PartialEq)]
pub enum SchedError {
    /// No `model` in the request and not exactly one model loaded.
    ModelRequired {
        available: Vec<String>,
    },
    ModelNotFound {
        model: String,
        available: Vec<String>,
    },
    /// The definition's base file is not on disk (e.g. an `hf:` reference not downloaded).
    ModelNotDownloaded {
        alias: String,
    },
    QueueFull {
        alias: String,
        retry_after: u64,
    },
    MaxLoaded {
        alias: String,
        limit: u32,
        retry_after: u64,
    },
    /// Unpinned gateway models can leave once their active requests finish.
    ModelBusy {
        alias: String,
        busy: Vec<String>,
        reason: String,
        retry_after: u64,
    },
    InsufficientMemory {
        alias: String,
        need_bytes: Option<u64>,
        available_bytes: Option<u64>,
        /// Loaded models that could not be evicted (busy, pinned or started by the user).
        blocking: Vec<String>,
        /// The pre-check proved it does not fit even with every idle model unloaded (false: a
        /// load that failed for memory after the room was made).
        after_full_eviction: bool,
        /// i18n keys of what could make it fit (`recovery.lower_context`...).
        options: Vec<String>,
        /// What the gateway already tried to fit the load (cache types, context range);
        /// empty when the definition's own cache settings were kept.
        tried: Box<Vec<String>>,
    },
    LoadFailed {
        alias: String,
        code: String,
        message: String,
    },
    /// Waited longer than the request timeout for a slot.
    QueueTimeout {
        alias: String,
        retry_after: u64,
    },
    AudioCppSpeechOnly,
    Internal(String),
}

impl SchedError {
    /// Stable machine code (`error.code` of the response).
    pub fn code(&self) -> &'static str {
        match self {
            SchedError::ModelRequired { .. } => "model_required",
            SchedError::ModelNotFound { .. } => "model_not_found",
            SchedError::ModelNotDownloaded { .. } => "model_not_downloaded",
            SchedError::QueueFull { .. } => "queue_full",
            SchedError::MaxLoaded { .. } => "max_loaded_models",
            SchedError::ModelBusy { .. } => "model_busy",
            SchedError::InsufficientMemory { .. } => "insufficient_memory",
            SchedError::LoadFailed { .. } => "model_load_failed",
            SchedError::QueueTimeout { .. } => "queue_timeout",
            SchedError::AudioCppSpeechOnly => "unsupported_route",
            SchedError::Internal(_) => "gateway_error",
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            SchedError::ModelRequired { .. } | SchedError::AudioCppSpeechOnly => 400,
            SchedError::ModelNotFound { .. } | SchedError::ModelNotDownloaded { .. } => 404,
            SchedError::QueueFull { .. }
            | SchedError::MaxLoaded { .. }
            | SchedError::ModelBusy { .. }
            | SchedError::InsufficientMemory { .. }
            | SchedError::QueueTimeout { .. } => 503,
            SchedError::LoadFailed { .. } => 502,
            SchedError::Internal(_) => 500,
        }
    }

    /// The request failed for lack of memory (no room, or the load ran out of memory).
    pub fn is_oom(&self) -> bool {
        match self {
            SchedError::InsufficientMemory { .. } => true,
            SchedError::LoadFailed { code, .. } => code.contains("out_of_memory"),
            _ => false,
        }
    }

    /// Seconds for `Retry-After`.
    pub fn retry_after(&self) -> Option<u64> {
        match self {
            SchedError::QueueFull { retry_after, .. }
            | SchedError::MaxLoaded { retry_after, .. }
            | SchedError::ModelBusy { retry_after, .. }
            | SchedError::QueueTimeout { retry_after, .. } => Some(*retry_after),
            SchedError::InsufficientMemory { .. } => Some(30),
            _ => None,
        }
    }

    /// English message for API clients (the UI localizes by code).
    pub fn message(&self) -> String {
        fn mib(b: Option<u64>) -> String {
            b.map(|b| format!("{} MiB", b / MIB))
                .unwrap_or_else(|| "unknown".into())
        }
        match self {
            SchedError::AudioCppSpeechOnly => "audio.cpp only accepts POST /v1/audio/speech".into(),
            SchedError::ModelRequired { available } => format!(
                "The request has no \"model\". Available models: {}.",
                list(available)
            ),
            SchedError::ModelNotFound { model, available } => format!(
                "Model \"{}\" is not a Harness model definition. Available models: {}.",
                model,
                list(available)
            ),
            SchedError::ModelNotDownloaded { alias } => format!(
                "The file of model \"{}\" is not on this computer; download it in Harness first.",
                alias
            ),
            SchedError::QueueFull { alias, retry_after } => format!(
                "Too many requests are waiting for \"{}\"; retry in {} s.",
                alias, retry_after
            ),
            SchedError::MaxLoaded { alias, limit, .. } => format!(
                "\"{}\" needs a load but {} model(s) are loaded (the limit) and all are busy or pinned.",
                alias, limit
            ),
            SchedError::ModelBusy { alias, busy, .. } => format!(
                "Cannot load \"{}\": model(s) {} are busy; retry after their requests finish.",
                alias, list(busy)
            ),
            SchedError::InsufficientMemory {
                alias,
                need_bytes,
                available_bytes,
                blocking,
                after_full_eviction,
                tried,
                ..
            } => format!(
                "Not enough GPU memory to load \"{}\" (needs about {}, {} free){}{}.",
                alias,
                mib(*need_bytes),
                mib(*available_bytes),
                match (*after_full_eviction, blocking.is_empty()) {
                    (true, true) => "; it does not fit even with the idle models unloaded".to_string(),
                    (true, false) => format!(
                        "; it does not fit even with the idle models unloaded; loaded and not evictable: {}",
                        blocking.join(", ")
                    ),
                    (false, true) => String::new(),
                    (false, false) => format!("; loaded and not evictable: {}", blocking.join(", ")),
                },
                if tried.is_empty() {
                    String::new()
                } else {
                    format!("; tried to fit it: {}", tried.join("; "))
                }
            ),
            SchedError::LoadFailed {
                alias,
                code,
                message,
            } => format!("Loading \"{}\" failed ({}): {}", alias, code, message),
            SchedError::QueueTimeout { alias, .. } => format!(
                "Timed out waiting for a free slot of \"{}\".",
                alias
            ),
            SchedError::Internal(e) => e.clone(),
        }
    }
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        "none".into()
    } else {
        items.join(", ")
    }
}

/// Prompts with less text than this are estimated from their size (a few thousand tokens at
/// most); longer ones are counted by the server's `/tokenize` before admission.
const TOKENIZE_MIN_BYTES: usize = 8 * 1024;
const TOKENIZE_TIMEOUT: Duration = Duration::from_secs(5);

/// `Lease::waited`: every slot of the server was busy.
pub const WAIT_SLOTS: &str = "slots";
/// `Lease::waited`: a slot was free but the unified KV pool had no room for the request yet.
pub const WAIT_TOKENS: &str = "tokens";

/// Where a definition stands in the scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SlotState {
    Unloaded,
    Loading,
    Ready,
    Draining,
}

/// Can several models answer at the same time ([`Scheduler::co_residency`])?
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CoResidency {
    /// All of them can be loaded together (or they are one model).
    pub concurrent: bool,
    /// Why not: `max_loaded_models` or `insufficient_memory`.
    pub reason: Option<String>,
    /// `max_loaded_models` when that is the reason.
    pub limit: Option<u32>,
    /// Memory the pending loads need (estimate + headroom) and what is free or evictable.
    pub need_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    /// Aliases that are not loaded yet.
    pub to_load: Vec<String>,
    /// One model: its parallel slots, when it is loaded (1 = requests take turns).
    pub slots: Option<u32>,
}

/// Read-only picker hint, computed with the same plan and memory picture as JIT admission.
#[derive(Debug, Clone, Serialize)]
pub struct ModelMemoryHint {
    pub alias: String,
    pub status: String,
}

/// One definition as the gateway sees it (GUI table, Control API `/gateway`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelSlot {
    pub definition_id: String,
    pub alias: String,
    pub state: SlotState,
    pub server_id: Option<String>,
    pub port: Option<u16>,
    /// Loaded by the gateway (counts against the limits, may be evicted / unloaded by TTL).
    /// `false` = a server the user started; the gateway only routes to it.
    pub managed: bool,
    pub pinned: bool,
    /// Effective idle TTL (Ollama `keep_alive` override, else the definition's, else the
    /// gateway default).
    pub ttl_secs: Option<u64>,
    /// `keep_alive` of the last Ollama-API request, while it overrides the TTL.
    #[serde(default)]
    pub keep_alive: Option<KeepAlive>,
    pub in_flight: u32,
    pub queued: u32,
    /// Requests that may run at once (llama-server slots).
    pub concurrency: u32,
    /// Requests that had to wait for a free slot since the model was loaded (all slots were
    /// busy): a hint to give the model more concurrent requests at its next load.
    #[serde(default)]
    pub waited: u64,
    /// Token budget of the shared KV pool (`-c` with `--kv-unified` and several slots; 0 = no
    /// budget) and the part the running requests hold (their estimates).
    #[serde(default)]
    pub token_pool: u64,
    #[serde(default)]
    pub tokens_in_use: u64,
    /// Requests that had a free slot but waited for room in the shared KV pool since the model
    /// was loaded (more slots do not help these; a larger context does).
    #[serde(default)]
    pub waited_tokens: u64,
    /// Seconds since the last request finished.
    pub idle_secs: Option<u64>,
    /// The current load was fitted into the free memory with other cache types / context than
    /// the definition's.
    #[serde(default)]
    pub adapted: Option<LoadAdaptation>,
}

/// Ollama `keep_alive` of the last Ollama-API request for a definition (overrides the idle
/// TTL of a gateway-loaded model; pinned definitions are never unloaded).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum KeepAlive {
    /// Negative `keep_alive`: never unloaded for idleness.
    Forever,
    /// Unload after this many idle seconds (`0` = as soon as the last request ends).
    Secs(u64),
}

/// What the load of a request did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LoadReport {
    pub ms: u64,
    /// Aliases unloaded to make room.
    pub evicted: Vec<String>,
    /// The first attempt ran out of memory and the retry after an eviction worked.
    pub recovered: bool,
    /// The definition's cache / context did not fit the free memory: what the load used
    /// instead (this load only, the definition is unchanged).
    #[serde(default)]
    pub adapted: Option<LoadAdaptation>,
}

/// Cache types and context the gateway loaded a model with to fit it into the free memory
/// (the definition asked for more; it stays as the user left it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LoadAdaptation {
    pub cache_type_k: String,
    pub cache_type_v: String,
    pub context_size: u32,
    /// The context the definition asked for.
    pub requested_context: u32,
    /// Layers left on the GPU when even the smallest context did not fit the whole model
    /// (automatic `-ngl` only); `None` = the recommended count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_layers: Option<u32>,
}

impl LoadAdaptation {
    /// `K q8_0 / V q4_0 cache, 32768 context` (server log, request record).
    pub fn summary(&self) -> String {
        let layers = self
            .gpu_layers
            .map(|n| format!(", {n} GPU layers"))
            .unwrap_or_default();
        format!(
            "K {} / V {} cache, context {}{layers}",
            self.cache_type_k, self.cache_type_v, self.context_size
        )
    }

    /// Server-log line in `lang` (`tr`, else English).
    pub fn log_line(&self, lang: &str) -> String {
        if lang == "tr" {
            let layers = self
                .gpu_layers
                .map(|n| format!(", ekran kartında {n} katman"))
                .unwrap_or_default();
            format!(
                "[Harness Studio] Belleğe sığdırmak için K {} / V {} önbellek ve {} bağlamla yüklendi{layers} (tanım {} istiyordu; tanım değişmedi).",
                self.cache_type_k, self.cache_type_v, self.context_size, self.requested_context
            )
        } else {
            let layers = self
                .gpu_layers
                .map(|n| format!(", {n} layers on the GPU"))
                .unwrap_or_default();
            format!(
                "[Harness Studio] Fitted into free memory: loaded with K {} / V {} cache and context {}{layers} (the definition asks for {}; it is unchanged).",
                self.cache_type_k, self.cache_type_v, self.context_size, self.requested_context
            )
        }
    }
}

#[derive(Debug, Default)]
struct EntryState {
    alias: String,
    server_id: Option<String>,
    managed: bool,
    /// Permits handed to the semaphore so far (= slots of the server).
    concurrency: u32,
    in_flight: u32,
    waiting: u32,
    /// Requests that found every slot busy since the server was adopted.
    waited: u64,
    /// Requests that had a slot but waited for room in the shared KV pool.
    waited_tokens: u64,
    /// The current load's memory adaptation (see [`LoadAdaptation`]).
    adapted: Option<LoadAdaptation>,
    /// Token budget handed to `Entry::tokens` (the unified KV pool, `-c`); 0 = no budget
    /// (separate per-slot caches, one slot, or unknown).
    pool: u64,
    last_used: Option<Instant>,
    draining: bool,
    /// Moving average of request durations (Retry-After estimate).
    avg_ms: f64,
    /// Context window of one slot (`/props` `n_ctx`) of the adopted server (analytics).
    ctx_size: Option<u64>,
    /// `/props` `chat_template_caps.supports_tools` of the adopted server (tool-call diagnosis).
    template_tools: Option<bool>,
    /// ggml backend of the adopted server's build (`vulkan`, `cuda`...; analytics).
    backend: Option<String>,
    /// Ollama `keep_alive` override of the idle TTL (cleared when the model unloads).
    keep_alive: Option<KeepAlive>,
}

/// What the scheduler reads from a server's `/props`.
#[derive(Debug, Default)]
struct Props {
    slots: Option<u32>,
    ctx_size: Option<u64>,
    template_tools: Option<bool>,
}

struct Entry {
    def_id: String,
    permits: Arc<Semaphore>,
    /// Token budget of a unified KV pool: a request holds its estimated prompt + generation
    /// tokens while it runs (see [`admission`]).
    tokens: Arc<Semaphore>,
    /// One JIT load / readiness check per definition at a time.
    ready: tokio::sync::Mutex<()>,
    st: Mutex<EntryState>,
}

impl Entry {
    fn state(&self) -> std::sync::MutexGuard<'_, EntryState> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn idle(&self) -> bool {
        let s = self.state();
        s.in_flight == 0 && s.waiting == 0
    }
}

/// A slot on a server for one request. Dropping it frees the slot and marks the model used.
pub struct Lease {
    pub definition_id: String,
    pub alias: String,
    pub server_id: String,
    /// `http://127.0.0.1:<port>` of the llama-server.
    pub upstream: String,
    /// API key of the server (sent upstream as Bearer); never shown to clients.
    pub api_key: Option<String>,
    pub queue_wait: Duration,
    pub load: Option<LoadReport>,
    /// Context window of one slot of the server (`/props`), when known.
    pub ctx_size: Option<u64>,
    /// Whether the server's chat template handles tools (`/props` `chat_template_caps`), when
    /// known; `Some(false)`: tools sent to this model never reach it.
    pub template_tools: Option<bool>,
    /// ggml backend of the server's llama.cpp build, when known.
    pub backend: Option<String>,
    entry: Arc<Entry>,
    started: Instant,
    /// Slots of the server (the semaphore size when the request was admitted).
    pub slots: u32,
    /// Why the request had to wait after its load: `slots` (every slot was busy) or `tokens`
    /// (a slot was free but the KV pool budget was not); `None` = it ran at once.
    pub waited: Option<&'static str>,
    /// Tokens held in the unified KV pool (estimate; 0 = no budget applies).
    pub reserved_tokens: u64,
    _activity: Option<crate::process::RequestActivity>,
    _permit: OwnedSemaphorePermit,
    _tokens: Option<OwnedSemaphorePermit>,
}

impl std::fmt::Debug for Lease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lease")
            .field("alias", &self.alias)
            .field("server_id", &self.server_id)
            .field("upstream", &self.upstream)
            .finish()
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let ms = self.started.elapsed().as_secs_f64() * 1000.0;
        let mut s = self.entry.state();
        s.in_flight = s.in_flight.saturating_sub(1);
        s.last_used = Some(Instant::now());
        s.avg_ms = if s.avg_ms == 0.0 {
            ms
        } else {
            s.avg_ms * 0.8 + ms * 0.2
        };
    }
}

/// Decrements the waiting count when a request stops waiting (also when it is cancelled).
struct Waiting(Arc<Entry>);

impl Drop for Waiting {
    fn drop(&mut self) {
        let mut s = self.0.state();
        s.waiting = s.waiting.saturating_sub(1);
    }
}

/// The services the scheduler drives (same instances as the GUI / `harness serve`).
#[derive(Clone)]
pub struct SchedulerContext {
    pub registry: Arc<ServerRegistry>,
    pub runtime: Arc<RuntimeManager>,
    pub gate: Arc<OperationGate>,
}

pub struct Scheduler {
    ctx: SchedulerContext,
    settings: RwLock<GatewaySettings>,
    entries: Mutex<HashMap<String, Arc<Entry>>>,
    /// Serializes load decisions (memory accounting, max loaded) and TTL unloads.
    load_lock: tokio::sync::Mutex<()>,
    defs: Mutex<Option<(Instant, Vec<ModelDefinition>)>>,
    /// One-time load settings per definition id, taken by that definition's next JIT load
    /// (`queue_load_override`).
    load_overrides: Mutex<HashMap<String, crate::params::ParamValues>>,
    http: reqwest::Client,
}

/// The Lemonade models as transient definitions (a snapshot read, off the async threads).
async fn lemonade_definitions(defs: &[ModelDefinition]) -> Vec<ModelDefinition> {
    let defs = defs.to_vec();
    blocking(move || crate::lemonade::definitions(&defs))
        .await
        .unwrap_or_default()
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, SchedError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| SchedError::Internal(e.to_string()))
}

fn gw_server_id(alias: &str) -> String {
    let base: String = alias
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let base: String = base.trim_matches('-').chars().take(48).collect();
    format!(
        "{}{}",
        SERVER_PREFIX,
        if base.is_empty() { "model" } else { &base }
    )
}

impl Scheduler {
    pub fn new(ctx: SchedulerContext, settings: GatewaySettings) -> Arc<Self> {
        Arc::new(Self {
            ctx,
            settings: RwLock::new(settings),
            entries: Mutex::new(HashMap::new()),
            load_lock: tokio::sync::Mutex::new(()),
            defs: Mutex::new(None),
            load_overrides: Mutex::new(HashMap::new()),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
        })
    }

    pub fn settings(&self) -> GatewaySettings {
        self.settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// New limits apply to the next decision (running requests are not touched).
    pub fn set_settings(&self, settings: GatewaySettings) {
        *self.settings.write().unwrap_or_else(|e| e.into_inner()) = settings;
    }

    /// Load settings for the next JIT load of `definition_id` only (the definition is not
    /// changed); an empty set clears a queued one. Taken when that load starts.
    pub fn queue_load_override(&self, definition_id: &str, overrides: crate::params::ParamValues) {
        let mut queued = self.load_overrides.lock().unwrap();
        if overrides.is_empty() {
            queued.remove(definition_id);
        } else {
            queued.insert(definition_id.to_string(), overrides);
        }
    }

    fn take_load_override(&self, definition_id: &str) -> crate::params::ParamValues {
        self.load_overrides
            .lock()
            .unwrap()
            .remove(definition_id)
            .unwrap_or_default()
    }

    pub fn registry(&self) -> &Arc<ServerRegistry> {
        &self.ctx.registry
    }

    /// Model definitions (re-read at most once a second).
    pub async fn definitions(&self) -> Result<Vec<ModelDefinition>, SchedError> {
        if let Some((at, defs)) = self.defs.lock().unwrap().as_ref() {
            if at.elapsed() < Duration::from_secs(1) {
                return Ok(defs.clone());
            }
        }
        let defs = blocking(|| crate::domain::store::Domain::global().definitions())
            .await?
            .map_err(SchedError::Internal)?;
        *self.defs.lock().unwrap() = Some((Instant::now(), defs.clone()));
        Ok(defs)
    }

    /// Aliases clients can ask for (`/v1/models`): definitions whose file is on disk.
    pub async fn servable(&self) -> Result<Vec<ModelDefinition>, SchedError> {
        let defs = self.definitions().await?;
        Ok(defs
            .into_iter()
            .filter(|d| {
                d.base
                    .local_path()
                    .is_some_and(crate::engine::model_on_disk)
            })
            .collect())
    }

    /// The definition a request's `model` names.
    pub async fn resolve(&self, model: Option<&str>) -> Result<ModelDefinition, SchedError> {
        let mut defs = self.definitions().await?;
        // A definition created within the cache window (e.g. `harness launch <file.gguf>` right
        // before its first request) is not in the cached list yet: read once more.
        if let Some(m) = model.map(str::trim).filter(|m| !m.is_empty()) {
            let known = defs
                .iter()
                .any(|d| d.alias.eq_ignore_ascii_case(m) || d.id == m);
            if !known {
                *self.defs.lock().unwrap() = None;
                defs = self.definitions().await?;
            }
        }
        let available = || -> Vec<String> {
            defs.iter()
                .filter(|d| {
                    d.base
                        .local_path()
                        .is_some_and(crate::engine::model_on_disk)
                })
                .map(|d| d.alias.clone())
                .collect()
        };
        let model = model.map(str::trim).filter(|m| !m.is_empty());
        let Some(model) = model else {
            // Clients that omit `model` get the only model that is loaded, if exactly one is.
            let loaded: Vec<&ModelDefinition> = defs
                .iter()
                .filter(|d| self.serving_server(d).is_some())
                .collect();
            if let [only] = loaded.as_slice() {
                return Ok((*only).clone());
            }
            return Err(SchedError::ModelRequired {
                available: available(),
            });
        };
        let def = match defs
            .iter()
            .find(|d| d.alias.eq_ignore_ascii_case(model))
            .or_else(|| defs.iter().find(|d| d.id == model))
        {
            Some(def) => def,
            None => {
                // A server the user started without a definition (e.g. an `--embeddings` server
                // from "New server") is reachable by the id it serves.
                let mut external = external_models(&defs, &self.server_views());
                external.extend(self.router_externals(&defs).await);
                // Models of the Lemonade engine (downloaded in its registry), by their name.
                external.extend(lemonade_definitions(&defs).await);
                if let Some(d) = external
                    .iter()
                    .find(|d| d.alias.eq_ignore_ascii_case(model))
                {
                    return Ok(d.clone());
                }
                let mut names = available();
                names.extend(external.into_iter().map(|d| d.alias));
                return Err(SchedError::ModelNotFound {
                    model: model.to_string(),
                    available: names,
                });
            }
        };
        if !def
            .base
            .local_path()
            .is_some_and(crate::engine::model_on_disk)
        {
            return Err(SchedError::ModelNotDownloaded {
                alias: def.alias.clone(),
            });
        }
        Ok(def.clone())
    }

    /// Context window (per slot) a request to `alias` runs with: the loaded server's own
    /// (`/props`), else the definition's `context_size`; `None` when neither is known.
    pub async fn context_window(&self, alias: &str) -> Result<Option<u64>, SchedError> {
        let def = self.resolve(Some(alias)).await?;
        let loaded = self
            .entries
            .lock()
            .unwrap()
            .get(&def.id)
            .and_then(|e| e.state().ctx_size);
        if loaded.is_some() {
            return Ok(loaded);
        }
        Ok(match def.load.get("context_size") {
            Some(crate::params::ParamValue::Int(n)) if *n > 0 => Some(*n as u64),
            _ => None,
        })
    }

    fn server_views(&self) -> Vec<crate::domain::status::ServerView> {
        self.ctx
            .registry
            .all()
            .into_iter()
            .map(|(id, pm)| (id, pm.get_status(), pm.launch_config()))
            .collect()
    }

    /// Running servers no definition describes, as transient definitions (`/v1/models`).
    pub async fn external(&self) -> Result<Vec<ModelDefinition>, SchedError> {
        let defs = self.definitions().await?;
        let mut external = external_models(&defs, &self.server_views());
        external.extend(self.router_externals(&defs).await);
        external.extend(lemonade_definitions(&defs).await);
        Ok(external)
    }

    /// The models of the running router servers (llama-server `--models-dir`) as transient
    /// definitions: a request for one goes to the router, which loads it on demand. A router
    /// that does not answer contributes nothing.
    async fn router_externals(&self, defs: &[ModelDefinition]) -> Vec<ModelDefinition> {
        let mut routers = Vec::new();
        for (id, pm) in self.ctx.registry.all() {
            let Some(config) = pm.launch_config().filter(|c| c.router_mode) else {
                continue;
            };
            let Ok(ep) = crate::router::endpoint(&pm) else {
                continue;
            };
            let Ok(models) = crate::router::list(&ep).await else {
                continue;
            };
            let dir = config.models_path.unwrap_or(config.model_path);
            routers.push((id, dir, models));
        }
        router_external_models(defs, &routers)
    }

    fn entry(&self, def: &ModelDefinition) -> Arc<Entry> {
        let mut map = self.entries.lock().unwrap();
        let e = map.entry(def.id.clone()).or_insert_with(|| {
            Arc::new(Entry {
                def_id: def.id.clone(),
                permits: Arc::new(Semaphore::new(0)),
                tokens: Arc::new(Semaphore::new(0)),
                ready: tokio::sync::Mutex::new(()),
                st: Mutex::new(EntryState::default()),
            })
        });
        e.state().alias = def.alias.clone();
        e.clone()
    }

    /// A server of the registry running (or starting, or stopping) this definition:
    /// `(id, status)`, running ones first.
    fn serving_server(&self, def: &ModelDefinition) -> Option<(String, ServerStatus)> {
        let views: Vec<crate::domain::status::ServerView> = self
            .ctx
            .registry
            .all()
            .into_iter()
            .map(|(id, pm)| (id, pm.get_status(), pm.launch_config()))
            .collect();
        let state = crate::domain::status::definition_states(std::slice::from_ref(def), &views)
            .into_iter()
            .next()?;
        let mut servers = state.servers;
        servers.sort_by_key(|s| s.status != ServerStatus::Running);
        servers.into_iter().next().map(|s| (s.server_id, s.status))
    }

    /// Waits a request's turn on `def`: loads it when needed, then takes a slot (no token
    /// budget, see [`Scheduler::acquire_tokens`]).
    pub async fn acquire(self: &Arc<Self>, def: &ModelDefinition) -> Result<Lease, SchedError> {
        self.acquire_tokens(def, None).await
    }

    /// Like [`Scheduler::acquire`], and when the server shares one KV pool between its slots
    /// (`--kv-unified`) also waits until the request's `demand` (prompt + output limit, see
    /// [`Demand`]) fits next to the requests that already run:
    /// llama-server does not check this itself and fails *all* running requests with "Context
    /// size has been exceeded" when they outgrow the pool. Waiters are served in arrival
    /// order; a request larger than the whole pool is admitted when it runs alone (the server
    /// then answers it itself). `None` skips the budget.
    pub async fn acquire_tokens(
        self: &Arc<Self>,
        def: &ModelDefinition,
        demand: Option<&Demand>,
    ) -> Result<Lease, SchedError> {
        self.acquire_keep_alive(def, demand, None).await
    }

    /// Like [`Scheduler::acquire_tokens`] for an Ollama-API request: its `keep_alive` (`None`
    /// = the request sent none) sets the idle TTL override of the definition, like Ollama
    /// resets a model's expiry on each request. Every other acquire clears the override, so
    /// it never outlives the request that sent it.
    pub async fn acquire_keep_alive(
        self: &Arc<Self>,
        def: &ModelDefinition,
        demand: Option<&Demand>,
        keep_alive: Option<KeepAlive>,
    ) -> Result<Lease, SchedError> {
        self.acquire_for(def, demand, keep_alive, false).await
    }

    /// Only the speech handler may opt in, after validating and rebuilding its body.
    pub async fn acquire_speech(
        self: &Arc<Self>,
        def: &ModelDefinition,
    ) -> Result<Lease, SchedError> {
        self.acquire_for(def, None, None, true).await
    }

    async fn acquire_for(
        self: &Arc<Self>,
        def: &ModelDefinition,
        demand: Option<&Demand>,
        keep_alive: Option<KeepAlive>,
        speech: bool,
    ) -> Result<Lease, SchedError> {
        if is_audio_cpp(def) {
            if !speech {
                return Err(SchedError::AudioCppSpeechOnly);
            }
            crate::engine::audio_cpp::require_enabled().map_err(|message| {
                SchedError::LoadFailed {
                    alias: def.alias.clone(),
                    code: "engine_disabled".into(),
                    message,
                }
            })?;
        }
        if crate::lemonade::model_of_def(&def.id).is_some() {
            return self.acquire_lemonade(def).await;
        }
        let settings = self.settings();
        let entry = self.entry(def);
        entry.state().keep_alive = keep_alive;
        let arrived = Instant::now();
        {
            let mut s = entry.state();
            // Waiting requests beyond the free slots are the queue.
            let free = entry.permits.available_permits() as u32;
            if s.waiting.saturating_sub(free) >= settings.max_queue {
                return Err(SchedError::QueueFull {
                    alias: def.alias.clone(),
                    retry_after: retry_after(&s),
                });
            }
            s.waiting += 1;
        }
        let waiting = Waiting(entry.clone());
        let load = self.ensure_ready(&entry, def).await?;
        let wait_limit = match settings.request_timeout_secs {
            0 => None,
            n => Some(Duration::from_secs(u64::from(n))),
        };
        let pool = entry.state().pool;
        let want = match demand {
            Some(d) if pool > 0 => self.demand_tokens(&entry, d).await.min(pool),
            _ => 0,
        };
        let waited = if entry.permits.available_permits() == 0 {
            Some(WAIT_SLOTS)
        } else if want > 0 && (entry.tokens.available_permits() as u64) < want {
            Some(WAIT_TOKENS)
        } else {
            None
        };
        match waited {
            Some(WAIT_SLOTS) => entry.state().waited += 1,
            Some(_) => entry.state().waited_tokens += 1,
            None => {}
        }
        let admit = {
            let entry = entry.clone();
            async move {
                let slot = entry.permits.clone().acquire_owned().await?;
                let budget = if want > 0 {
                    Some(entry.tokens.clone().acquire_many_owned(want as u32).await?)
                } else {
                    None
                };
                Ok::<_, tokio::sync::AcquireError>((slot, budget))
            }
        };
        let (permit, budget) =
            match wait_limit {
                Some(limit) => tokio::time::timeout(limit, admit).await.map_err(|_| {
                    SchedError::QueueTimeout {
                        alias: def.alias.clone(),
                        retry_after: retry_after(&entry.state()),
                    }
                })?,
                None => admit.await,
            }
            .map_err(|e| SchedError::Internal(e.to_string()))?;
        drop(waiting);
        // The server may have gone away while we waited for the slot (stopped by the user):
        // check again.
        let (server_id, upstream, api_key) = match self.upstream_of(&entry) {
            Some(u) => u,
            None => {
                drop(budget);
                drop(permit);
                return Box::pin(self.acquire_for(def, demand, keep_alive, speech)).await;
            }
        };
        let activity = match self.ctx.registry.get(&server_id) {
            Some(pm) => match pm.request_activity() {
                Some(activity) => Some(activity),
                None => {
                    drop(budget);
                    drop(permit);
                    return Box::pin(self.acquire_for(def, demand, keep_alive, speech)).await;
                }
            },
            None => None,
        };
        let (ctx_size, template_tools, backend, slots) = {
            let mut s = entry.state();
            s.in_flight += 1;
            (
                s.ctx_size,
                s.template_tools,
                s.backend.clone(),
                s.concurrency,
            )
        };
        Ok(Lease {
            _activity: activity,
            definition_id: def.id.clone(),
            alias: def.alias.clone(),
            server_id,
            upstream,
            api_key,
            queue_wait: arrived.elapsed(),
            load,
            ctx_size,
            template_tools,
            backend,
            entry,
            started: Instant::now(),
            slots,
            waited,
            reserved_tokens: want,
            _permit: permit,
            _tokens: budget,
        })
    }

    /// A slot on the Lemonade engine for a model of its registry (a transient definition,
    /// see [`crate::lemonade::definitions`]). `lemond` is started when it is not running;
    /// Lemonade itself loads the model on the first request. One request at a time per model
    /// (one llama.cpp slot), the rest wait in the gateway's queue like for any other model.
    async fn acquire_lemonade(
        self: &Arc<Self>,
        def: &ModelDefinition,
    ) -> Result<Lease, SchedError> {
        let settings = self.settings();
        let entry = self.entry(def);
        let arrived = Instant::now();
        {
            let mut s = entry.state();
            if s.concurrency == 0 {
                s.concurrency = 1;
                entry.permits.add_permits(1);
            }
            let free = entry.permits.available_permits() as u32;
            if s.waiting.saturating_sub(free) >= settings.max_queue {
                return Err(SchedError::QueueFull {
                    alias: def.alias.clone(),
                    retry_after: retry_after(&s),
                });
            }
            s.waiting += 1;
        }
        let waiting = Waiting(entry.clone());
        let model = crate::lemonade::model_of_def(&def.id).unwrap_or_default();
        if crate::lemonade::find_model(model).is_none() {
            // Deleted from Lemonade's registry since the definition was listed.
            return Err(SchedError::ModelNotDownloaded {
                alias: def.alias.clone(),
            });
        }
        let loaded_before = crate::lemonade::endpoint().is_some();
        let started = Instant::now();
        let ep = crate::lemonade::ensure_running()
            .await
            .map_err(|e| SchedError::LoadFailed {
                alias: def.alias.clone(),
                code: crate::error_key::parse(&e)
                    .map(|(k, _)| k.to_string())
                    .unwrap_or_else(|| "lemonade_failed".into()),
                message: e,
            })?;
        let load = (!loaded_before).then(|| LoadReport {
            ms: started.elapsed().as_millis() as u64,
            ..Default::default()
        });
        let waited = (entry.permits.available_permits() == 0).then_some(WAIT_SLOTS);
        if waited.is_some() {
            entry.state().waited += 1;
        }
        let wait_limit = match settings.request_timeout_secs {
            0 => None,
            n => Some(Duration::from_secs(u64::from(n))),
        };
        let admit = entry.permits.clone().acquire_owned();
        let permit =
            match wait_limit {
                Some(limit) => tokio::time::timeout(limit, admit).await.map_err(|_| {
                    SchedError::QueueTimeout {
                        alias: def.alias.clone(),
                        retry_after: retry_after(&entry.state()),
                    }
                })?,
                None => admit.await,
            }
            .map_err(|e| SchedError::Internal(e.to_string()))?;
        drop(waiting);
        entry.state().in_flight += 1;
        Ok(Lease {
            definition_id: def.id.clone(),
            alias: def.alias.clone(),
            _activity: None,
            server_id: crate::lemonade::RUNTIME_ID.to_string(),
            upstream: ep.base,
            api_key: Some(ep.key),
            queue_wait: arrived.elapsed(),
            load,
            ctx_size: None,
            template_tools: None,
            backend: None,
            entry,
            started: Instant::now(),
            slots: 1,
            waited,
            reserved_tokens: 0,
            _permit: permit,
            _tokens: None,
        })
    }

    /// Unloads Lemonade's models once every request for them has been idle for the gateway's
    /// default TTL (Lemonade keeps a model loaded until told otherwise, and its memory is not
    /// part of the gateway's accounting).
    async fn expire_lemonade(&self) {
        let Some(ttl) = self.settings().default_ttl_secs.filter(|&t| t > 0) else {
            return;
        };
        let entries: Vec<Arc<Entry>> = self
            .entries
            .lock()
            .unwrap()
            .values()
            .filter(|e| crate::lemonade::model_of_def(&e.def_id).is_some())
            .cloned()
            .collect();
        if entries.is_empty() || crate::lemonade::endpoint().is_none() {
            return;
        }
        let mut newest: Option<Instant> = None;
        for e in &entries {
            if !e.idle() {
                return;
            }
            if let Some(last) = e.state().last_used {
                newest = Some(newest.map_or(last, |n| n.max(last)));
            }
        }
        let Some(last) = newest else { return };
        if last.elapsed() < Duration::from_secs(ttl) {
            return;
        }
        for e in &entries {
            e.state().last_used = None;
        }
        crate::lemonade::unload_all().await;
    }

    /// Tokens a request holds in the pool: the server's own count of its text
    /// (`/tokenize`) when the text is long enough to matter, else the byte estimate. A failed
    /// count falls back to the estimate.
    async fn demand_tokens(&self, entry: &Entry, demand: &Demand) -> u64 {
        if demand.text.len() < TOKENIZE_MIN_BYTES {
            return demand.estimate();
        }
        let Some((id, upstream, key)) = self.upstream_of(entry) else {
            return demand.estimate();
        };
        let mut req = self
            .http
            .post(format!("{upstream}/tokenize"))
            .timeout(TOKENIZE_TIMEOUT)
            .json(&serde_json::json!({"content": demand.text}));
        if let Some(key) = key {
            req = req.bearer_auth(key);
        }
        let counted = async {
            let v: serde_json::Value = req.send().await.ok()?.json().await.ok()?;
            Some(v.get("tokens")?.as_array()?.len() as u64)
        }
        .await;
        match counted {
            Some(n) => demand.with_text_tokens(n),
            None => {
                eprintln!("[Harness Gateway] {id}: /tokenize failed, using the byte estimate");
                demand.estimate()
            }
        }
    }

    fn upstream_of(&self, entry: &Entry) -> Option<(String, String, Option<String>)> {
        let id = entry.state().server_id.clone()?;
        let pm = self.ctx.registry.get(&id)?;
        if pm.get_status() != ServerStatus::Running {
            return None;
        }
        let active = pm.active_server()?;
        let host = crate::process::connect_host(&active.connect_host);
        let host = if host.contains(':') && !host.starts_with('[') {
            format!("[{}]", host)
        } else {
            host
        };
        Some((
            id,
            format!("http://{}:{}", host, active.port),
            active.api_key.clone(),
        ))
    }

    /// Makes sure a server runs `def` and the entry points at it. `Some` when this call
    /// loaded it.
    async fn ensure_ready(
        &self,
        entry: &Arc<Entry>,
        def: &ModelDefinition,
    ) -> Result<Option<LoadReport>, SchedError> {
        let _ready = entry.ready.lock().await;
        loop {
            match self.serving_server(def) {
                Some((id, ServerStatus::Running)) => {
                    self.adopt(entry, &id, false).await;
                    return Ok(None);
                }
                Some((id, _)) => {
                    // Starting (someone else loads it) or stopping: wait for it to settle.
                    self.wait_settled(&id).await;
                }
                None => break,
            }
        }
        // A transient definition (a server the user started) cannot be loaded: it is gone.
        if !self.definitions().await?.iter().any(|d| d.id == def.id) {
            return Err(SchedError::ModelNotFound {
                model: def.alias.clone(),
                available: Vec::new(),
            });
        }
        // A gateway instance of this definition that is still stopping (TTL/eviction).
        let previous = entry.state().server_id.clone();
        if let Some(id) = previous {
            self.wait_settled(&id).await;
        }
        let report = self.jit_load(entry, def).await?;
        Ok(Some(report))
    }

    /// Points the entry at a running server and sizes its queue by the server's slots.
    ///
    /// `fresh`: the server was just loaded by us, so slots, context and pool are read again
    /// (a reload may have changed them).
    async fn adopt(&self, entry: &Arc<Entry>, server_id: &str, fresh: bool) {
        let (known, current) = {
            let s = entry.state();
            (s.server_id.as_deref() == Some(server_id), s.concurrency)
        };
        let managed = server_id.starts_with(SERVER_PREFIX)
            || self
                .ctx
                .registry
                .get(server_id)
                .and_then(|pm| pm.launch_config())
                .is_some_and(|c| {
                    crate::engine::server_for(&c).family() == crate::engine::AUDIO_CPP
                });
        if !known || current == 0 || fresh {
            let props = self.props_of(server_id).await.unwrap_or_default();
            let (slots, ctx_size) = (props.slots, props.ctx_size);
            let slots = slots.unwrap_or(1).clamp(1, 256);
            let pool = self.pool_of(server_id, slots, ctx_size).await;
            let backend = self.backend_of(server_id);
            let mut s = entry.state();
            s.ctx_size = ctx_size;
            s.template_tools = props.template_tools;
            s.backend = backend;
            if !known {
                s.managed = managed;
            }
            s.server_id = Some(server_id.to_string());
            s.draining = false;
            if !known || fresh {
                s.waited = 0;
                s.waited_tokens = 0;
            }
            if pool > s.pool {
                entry.tokens.add_permits((pool - s.pool) as usize);
                s.pool = pool;
            } else if pool < s.pool {
                let gone = entry.tokens.forget_permits((s.pool - pool) as usize) as u64;
                s.pool -= gone;
            }
            if slots > s.concurrency {
                entry.permits.add_permits((slots - s.concurrency) as usize);
                s.concurrency = slots;
            } else if slots < s.concurrency {
                // Reloaded with fewer slots: retire the extra permits (free ones now; permits
                // still held by running requests stay until the next adoption).
                let gone = entry
                    .permits
                    .forget_permits((s.concurrency - slots) as usize)
                    as u32;
                s.concurrency -= gone;
            }
        }
    }

    /// Token budget of a server: the unified KV pool (`-c`) when several slots share it,
    /// else 0 (one slot, or every slot has its own cache and fails alone). Reads the effective
    /// launch values (parsed from the server log after start; given a moment to appear).
    async fn pool_of(&self, server_id: &str, slots: u32, n_ctx: Option<u64>) -> u64 {
        if slots <= 1 {
            return 0;
        }
        let Some(pm) = self.ctx.registry.get(server_id) else {
            return 0;
        };
        // The effective values are read from a llama-server log; another server has no pool
        // Harness accounts (mistral.rs manages its own KV cache).
        if pm.active_server().is_some_and(|a| !a.is_llama()) {
            return 0;
        }
        for _ in 0..50 {
            if let Some(eff) = pm.effective_values() {
                return match eff.kv_unified {
                    Some(true) => eff.n_ctx.or(n_ctx).unwrap_or(0),
                    _ => 0,
                };
            }
            if pm.get_status() != ServerStatus::Running {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        0
    }

    /// ggml backend of a server's build (from the libraries next to its binary).
    fn backend_of(&self, server_id: &str) -> Option<String> {
        let config = self.ctx.registry.get(server_id)?.launch_config()?;
        if crate::engine::audio_cpp::is_model(&config.model_path) {
            return Some(crate::engine::audio_cpp::backend(&config));
        }
        crate::runtime::detect_backend_in_dir(config.binary_path.parent()?)
    }

    /// `total_slots` and the per-slot context (`default_generation_settings.n_ctx`) of a
    /// running llama-server (`/props`).
    async fn props_of(&self, server_id: &str) -> Option<Props> {
        let pm = self.ctx.registry.get(server_id)?;
        let active = pm.active_server()?;
        // Only llama-server has `/props`: the others are sized from their launch.
        if !active.is_llama() {
            return Some(non_llama_props(pm.launch_config().as_ref()));
        }
        let url = format!(
            "http://{}:{}/props",
            crate::process::connect_host(&active.connect_host),
            active.port
        );
        let mut req = self.http.get(url);
        if let Some(key) = &active.api_key {
            req = req.bearer_auth(key);
        }
        let v: serde_json::Value = req.send().await.ok()?.json().await.ok()?;
        let slots = v
            .get("total_slots")
            .and_then(|n| n.as_u64())
            .map(|n| n as u32);
        let ctx = v
            .get("default_generation_settings")
            .and_then(|g| g.get("n_ctx"))
            .or_else(|| v.get("n_ctx"))
            .and_then(|n| n.as_u64())
            .filter(|n| *n > 0);
        let template_tools = v
            .get("chat_template_caps")
            .and_then(|c| c.get("supports_tools"))
            .and_then(|b| b.as_bool());
        Some(Props {
            slots,
            ctx_size: ctx,
            template_tools,
        })
    }

    /// Waits until a server is neither starting nor stopping.
    async fn wait_settled(&self, server_id: &str) -> ServerStatus {
        let limit = Duration::from_secs(u64::from(
            crate::config::AppSettings::load().server.load_timeout_secs,
        )) + LOAD_GRACE;
        let deadline = Instant::now() + limit;
        loop {
            let status = self
                .ctx
                .registry
                .get(server_id)
                .map(|pm| pm.get_status())
                .unwrap_or(ServerStatus::Stopped);
            if !matches!(status, ServerStatus::Starting | ServerStatus::Stopping)
                || Instant::now() > deadline
            {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Plans a JIT load: the definition's own settings on `gw-<alias>`, loopback only, on an
    /// automatic port.
    async fn plan(
        &self,
        def: &ModelDefinition,
    ) -> Result<(String, crate::process::LaunchConfig), SchedError> {
        self.plan_with(def, &Default::default()).await
    }

    /// [`Self::plan`] with one-time load settings over the definition.
    async fn plan_with(
        &self,
        def: &ModelDefinition,
        overrides: &crate::params::ParamValues,
    ) -> Result<(String, crate::process::LaunchConfig), SchedError> {
        use crate::orchestrator::{plan_load_definition_with, LoadPlan, LoadTarget};
        let plan = plan_load_definition_with(
            &self.ctx.registry,
            &def.id,
            LoadTarget::NewServer,
            overrides,
        )
        .await
        .map_err(|e| SchedError::LoadFailed {
            alias: def.alias.clone(),
            code: "plan_failed".into(),
            message: e,
        })?;
        let LoadPlan::Config {
            server_id: planned,
            config,
        } = plan
        else {
            return Err(SchedError::Internal("unexpected profile plan".into()));
        };
        let mut config = *config;
        // The gateway is the front door: its models only listen on loopback.
        config.host = "127.0.0.1".into();
        // Its clients are API clients and agents; the scheduler's token admission keeps their
        // requests inside the shared KV pool, so the automatic slot count may exceed what a
        // direct agent server gets (`slots`).
        config.launched_by = crate::slots::SlotOrigin::Gateway;
        // The port comes from Harness's range at launch (`ports::AUTO_RANGE`, the same rule
        // as every server without a port of its own); a bind race moves it silently.
        config.port = 0;
        config.port_auto = true;
        let profiles = blocking(|| crate::domain::store::Domain::global().profiles())
            .await?
            .unwrap_or_default();
        let mut id = gw_server_id(&def.alias);
        let taken_by_other = |id: &str| {
            profiles.iter().any(|p| p.id == id)
                || self.ctx.registry.get(id).is_some_and(|pm| {
                    let ours = pm
                        .launch_config()
                        .and_then(|c| c.definition_id)
                        .is_none_or(|d| d == def.id);
                    !ours
                        && matches!(
                            pm.get_status(),
                            ServerStatus::Starting | ServerStatus::Running | ServerStatus::Stopping
                        )
                })
        };
        if taken_by_other(&id) {
            id = planned;
        }
        Ok((id, config))
    }

    pub async fn model_memory_hints(&self) -> Result<Vec<ModelMemoryHint>, SchedError> {
        let mut hints = Vec::new();
        for def in self.servable().await? {
            let (_, config) = self.plan(&def).await?;
            let runtime = self.ctx.runtime.clone();
            let configured = self.settings().memory_headroom_mib;
            let picture = blocking(move || {
                memory_picture_full(&runtime, &config, false)
                    .map(|p| (p, admission_headroom(&config, p.2, configured)))
            })
            .await?;
            let status = match picture {
                Some(((need, free, total), headroom)) => {
                    memory_hint_status(need, free, total, headroom)
                }
                None => "unknown",
            };
            hints.push(ModelMemoryHint {
                alias: def.alias,
                status: status.into(),
            });
        }
        Ok(hints)
    }

    async fn jit_load(
        &self,
        entry: &Arc<Entry>,
        def: &ModelDefinition,
    ) -> Result<LoadReport, SchedError> {
        let _load = self.load_lock.lock().await;
        let started = Instant::now();
        let overrides = self.take_load_override(&def.id);
        let (server_id, mut config) = self.plan_with(def, &overrides).await?;
        let explicit_layers = crate::domain::launch::load_sets_gpu_layers(def, &overrides);
        let (defaulted, full) = {
            let c = config.clone();
            blocking(move || {
                let ctx = default_context_for(&c);
                // No live limit here: an idle model the admission may still evict is not a reason
                // to shrink the context. `make_room` steps the rule down when the card cannot
                // give the memory even then.
                let full = ctx.filter(|_| !explicit_layers).and_then(|ctx| {
                    let hw = crate::hardware::detect_hardware();
                    full_offload_context_with(&c, ctx, |c| crate::vram::fit_launch(c, &hw), None)
                });
                (ctx, full)
            })
            .await?
        };
        if let Some(ctx) = defaulted {
            config.context_size = ctx;
        }
        if let Some(f) = full {
            config.context_size = f.context;
            config.gpu_layers = f.layers;
            if f.slots.is_some() {
                config.parallel_slots = f.slots;
            }
        }
        let (evicted, mut adapted) = self.make_room(def, &mut config, full).await?;
        // The definition set no context: the adaptation started from the automatic one.
        if let (Some(a), Some(ctx)) = (adapted.as_mut(), defaulted) {
            a.requested_context = ctx;
        }
        let mut report = LoadReport {
            evicted,
            adapted: adapted.clone(),
            ..Default::default()
        };
        {
            let mut s = entry.state();
            s.adapted = adapted.clone();
            s.server_id = Some(server_id.clone());
            s.managed = true;
            s.draining = false;
        }
        let result = crate::orchestrator::run_load_plan(
            &self.ctx.registry,
            &self.ctx.runtime,
            &self.ctx.gate,
            crate::orchestrator::LoadPlan::Config {
                server_id: server_id.clone(),
                config: Box::new(config),
            },
        )
        .await;
        let mut outcome = match result {
            Ok(_) => self.wait_settled(&server_id).await,
            Err(e) => {
                let failure = self
                    .ctx
                    .registry
                    .get(&server_id)
                    .and_then(|pm| pm.last_failure());
                self.publish(def, &server_id, "load_failed", Some(e.clone()));
                return Err(match failure {
                    Some(f) if f.is_out_of_memory() => {
                        self.insufficient(def, &server_id, None, None).await
                    }
                    other => SchedError::LoadFailed {
                        alias: def.alias.clone(),
                        code: other.map(|f| f.code).unwrap_or_else(|| {
                            if e.starts_with(crate::orchestrator::ERR_PORT_IN_USE) {
                                crate::orchestrator::ERR_PORT_IN_USE.to_string()
                            } else {
                                "servers.start_failed".to_string()
                            }
                        }),
                        message: e,
                    },
                });
            }
        };
        // OOM recovery hook: out of memory while loading → evict the least recently used idle model
        // and retry once.
        if outcome != ServerStatus::Running {
            let failure = self
                .ctx
                .registry
                .get(&server_id)
                .and_then(|pm| pm.last_failure());
            let oom = failure.as_ref().is_some_and(|f| f.is_out_of_memory());
            if oom {
                if let Some((victim, victim_server)) = self.lru_victim(&def.id) {
                    self.publish(
                        def,
                        &server_id,
                        "oom_retry",
                        Some(crate::failure::FailureKind::OutOfMemory.code()),
                    );
                    let alias = victim.state().alias.clone();
                    self.evict(&victim, &victim_server, "evicted", Some(def.alias.clone()))
                        .await;
                    report.evicted.push(alias);
                    let retried = crate::orchestrator::retry_with_overrides(
                        &self.ctx.registry,
                        &self.ctx.runtime,
                        &self.ctx.gate,
                        &server_id,
                        Default::default(),
                        Vec::new(),
                        false,
                    )
                    .await;
                    outcome = match retried {
                        Ok(()) => self.wait_settled(&server_id).await,
                        Err(e) => ServerStatus::Error(e),
                    };
                    report.recovered = outcome == ServerStatus::Running;
                }
            }
            if outcome != ServerStatus::Running {
                let failure = self
                    .ctx
                    .registry
                    .get(&server_id)
                    .and_then(|pm| pm.last_failure())
                    .or(failure);
                let code = failure
                    .as_ref()
                    .map(|f| f.code.clone())
                    .unwrap_or_else(|| "servers.start_failed".into());
                self.publish(def, &server_id, "load_failed", Some(code.clone()));
                entry.state().server_id = None;
                if failure.as_ref().is_some_and(|f| f.is_out_of_memory()) {
                    return Err(self.insufficient(def, &server_id, None, None).await);
                }
                let message = match &outcome {
                    ServerStatus::Error(e) => e.clone(),
                    other => format!("{:?}", other),
                };
                return Err(SchedError::LoadFailed {
                    alias: def.alias.clone(),
                    code,
                    message,
                });
            }
        }
        self.adopt(entry, &server_id, true).await;
        entry.state().last_used = Some(Instant::now());
        report.ms = started.elapsed().as_millis() as u64;
        if let Some(pm) = self.ctx.registry.get(&server_id) {
            let language = crate::config::AppSettings::load().language;
            if let Some(ctx) = defaulted {
                pm.append_log(default_context_log_line(ctx, &language));
                if let Some(f) = full.filter(|f| f.context < ctx) {
                    pm.append_log(full_offload_log_line(f.context, ctx, &language));
                }
            }
            if let Some(a) = &adapted {
                pm.append_log(a.log_line(&language));
            }
        }
        self.publish(
            def,
            &server_id,
            "loaded",
            adapted.as_ref().map(|a| a.summary()),
        );
        Ok(report)
    }

    /// `503 insufficient_memory` with the recovery options of the failed load (if any).
    async fn insufficient(
        &self,
        def: &ModelDefinition,
        server_id: &str,
        need: Option<u64>,
        available: Option<u64>,
    ) -> SchedError {
        let plan = crate::orchestrator::recovery_plan(&self.ctx.registry, server_id)
            .await
            .ok();
        // Idle models may leave (a retry can free them): only the rest is "not evictable".
        let evictable: Vec<String> = self
            .lru_victims(&def.id)
            .into_iter()
            .map(|(_, server, _)| server)
            .collect();
        SchedError::InsufficientMemory {
            alias: def.alias.clone(),
            need_bytes: need.or_else(|| plan.as_ref().and_then(|p| p.estimated_need_bytes)),
            available_bytes: available.or_else(|| plan.as_ref().and_then(|p| p.free_vram_bytes)),
            blocking: self.blocking_models(&def.id, &evictable),
            after_full_eviction: false,
            tried: Box::default(),
            options: plan
                .map(|p| p.options.into_iter().map(|o| o.label_key).collect())
                .unwrap_or_default(),
        }
    }

    /// Other loaded models the scheduler may not evict (busy, pinned, or started by the user).
    /// `evictable`: server ids of the models it may evict (`lru_victims`); they are left out.
    fn blocking_models(&self, except: &str, evictable: &[String]) -> Vec<String> {
        let loaded = self
            .ctx
            .registry
            .all()
            .into_iter()
            .filter(|(_, pm)| {
                matches!(
                    pm.get_status(),
                    ServerStatus::Running | ServerStatus::Starting
                )
            })
            .filter_map(|(id, pm)| {
                let c = pm.launch_config()?;
                if c.definition_id.as_deref() == Some(except) {
                    return None;
                }
                let name = c.alias.unwrap_or_else(|| id.clone());
                Some((id, name))
            })
            .collect();
        not_evictable(loaded, evictable)
    }

    /// Frees room for `config` under the limits. Returns the evicted aliases and, when the
    /// definition's automatic cache / context did not fit even with every idle model gone,
    /// the smaller settings `config` now carries (this load only).
    async fn make_room(
        &self,
        def: &ModelDefinition,
        config: &mut crate::process::LaunchConfig,
        full: Option<FullOffload>,
    ) -> Result<(Vec<String>, Option<LoadAdaptation>), SchedError> {
        let timeout = self.settings().request_timeout_secs;
        let budget = Duration::from_secs(if timeout == 0 {
            5
        } else {
            u64::from(timeout).min(5)
        });
        let deadline = tokio::time::Instant::now() + budget;
        let mut evicted = Vec::new();
        let mut adapted = None;
        loop {
            let result = self
                .make_room_now(def, config, full, &mut evicted, &mut adapted)
                .await;
            if !wait_for_busy(&result, deadline).await {
                return result.map(|()| (evicted, adapted));
            }
        }
    }

    /// Gateway-owned, unpinned active models that could become eviction victims.
    fn busy_models(&self, except: &str) -> Vec<(String, String)> {
        let pinned = self.pinned_ids();
        let entries: Vec<_> = self.entries.lock().unwrap().values().cloned().collect();
        entries
            .into_iter()
            .filter_map(|e| {
                let s = e.state();
                if e.def_id == except
                    || pinned.contains(&e.def_id)
                    || !s.managed
                    || s.in_flight == 0
                {
                    return None;
                }
                let server = s.server_id.as_ref()?;
                self.ctx
                    .registry
                    .get(server)
                    .filter(|pm| pm.get_status() == ServerStatus::Running)
                    .map(|_| (s.alias.clone(), server.clone()))
            })
            .collect()
    }

    fn same_device(&self, server: &str, config: &crate::process::LaunchConfig) -> bool {
        let other = self
            .ctx
            .registry
            .get(server)
            .and_then(|pm| pm.launch_config());
        other.is_none_or(|other| {
            !cpu_only(&other) && same_gpu(footprint_gpu(config), footprint_gpu(&other))
        })
    }

    async fn make_room_now(
        &self,
        def: &ModelDefinition,
        config: &mut crate::process::LaunchConfig,
        full: Option<FullOffload>,
        evicted: &mut Vec<String>,
        adapted: &mut Option<LoadAdaptation>,
    ) -> Result<(), SchedError> {
        let settings = self.settings();
        if settings.max_loaded_models > 0 {
            while self.managed_loaded(&def.id) >= settings.max_loaded_models as usize {
                let Some((victim, server)) = self.lru_victim(&def.id) else {
                    let busy = self.busy_models(&def.id);
                    if !busy.is_empty()
                        && self.managed_loaded(&def.id).saturating_sub(busy.len())
                            < settings.max_loaded_models as usize
                    {
                        return Err(SchedError::ModelBusy {
                            alias: def.alias.clone(),
                            busy: busy.into_iter().map(|(alias, _)| alias).collect(),
                            reason: "max_loaded_models".into(),
                            retry_after: 5,
                        });
                    }
                    return Err(SchedError::MaxLoaded {
                        alias: def.alias.clone(),
                        limit: settings.max_loaded_models,
                        retry_after: self.shortest_retry(),
                    });
                };
                evicted.push(victim.state().alias.clone());
                self.evict(&victim, &server, "evicted", Some(def.alias.clone()))
                    .await;
            }
        }
        if !settings.memory_aware || cpu_only(config) {
            return Ok(());
        }
        let memory_config = config.clone();
        let runtime = self.ctx.runtime.clone();
        // Always a fresh device report: the 3 s cache may still show the memory from before the
        // previous request's load / eviction, which refused (and had already unloaded) models
        // that fit (measured: whisper turbo + 0.5B loaded, 3B chat request right after).
        let picture = blocking(move || memory_picture_full(&runtime, &memory_config, true)).await?;
        let Some((need, available, total)) = picture else {
            return Ok(()); // no GPU information: let the load decide
        };
        let (available, cap) = self.unified_limits(available, total).await;
        // A load the full-offload rule chose is admitted against the limit the rule used, not
        // the automatic reserve on top (`vram::full_offload_headroom`).
        let whole = full.is_some_and(|f| f.whole) && fit_applies(config);
        let configured = settings.memory_headroom_mib * MIB;
        let load_headroom = if whole {
            crate::vram::full_offload_headroom(total, configured)
        } else {
            admission_headroom(config, total, settings.memory_headroom_mib)
        };
        let need_total = need.saturating_add(load_headroom);
        if need_total <= available {
            return Ok(());
        }
        // Compute first, evict afterwards: with everything that may leave the card gone, would
        // the request fit at all? If not, nothing is evicted (an idle model must not be
        // unloaded for a request that is refused anyway). Otherwise the LRU order is unchanged.
        let victims: Vec<_> = self
            .lru_victims(&def.id)
            .into_iter()
            .filter(|(_, server, _)| self.same_device(server, config))
            .collect();
        let mut footprints = Vec::with_capacity(victims.len());
        for (_, server, _) in &victims {
            footprints.push(self.footprint(server).await);
        }
        let mut need = need;
        let mut count = eviction_count(need_total, available, &footprints, cap);
        let mut tried = Vec::new();
        if count.is_none() && fit_applies(config) {
            // The definition's automatic cache / context does not fit even with the idle models
            // gone: step the cache down and shrink the context (this load only), as "Prepare
            // Server" does, before refusing.
            let headroom = if whole {
                load_headroom
            } else {
                crate::vram::memory_headroom(total, settings.memory_headroom_mib * MIB)
            };
            // The rule and the admission must agree: when the full offload does not fit even
            // with the idle models gone, the rule steps down (12k to 8k) before layers are cut.
            if whole {
                let limit = footprints
                    .iter()
                    .fold(available, |free, f| free.saturating_add(*f))
                    .min(cap)
                    .saturating_sub(headroom);
                let candidate = config.clone();
                let stepped = blocking(move || {
                    let hw = crate::hardware::detect_hardware();
                    full_offload_step_down(&candidate, limit, |c| crate::vram::fit_launch(c, &hw))
                })
                .await?;
                if let Some((stepped, adaptation)) = stepped {
                    *config = stepped;
                    *adapted = Some(adaptation);
                    need = estimate_vram(&crate::vram::input_from_launch(config), total);
                    count =
                        eviction_count(need.saturating_add(headroom), available, &footprints, cap);
                }
            }
            let (candidate, free) = (config.clone(), footprints.clone());
            let auto_layers = !crate::domain::launch::definition_sets_gpu_layers(def);
            let fitted = if count.is_some() {
                None
            } else {
                blocking(move || {
                    let ladder = fit_levels(&candidate)?;
                    // The definition's own cache types stay; only the context may shrink.
                    let levels = if crate::kv_policy::is_automatic(&candidate) {
                        ladder
                    } else {
                        &KEEP_CACHE
                    };
                    let fits = |c: &crate::process::LaunchConfig| {
                        let need = estimate_vram(&crate::vram::input_from_launch(c), total);
                        eviction_count(need.saturating_add(headroom), available, &free, cap)
                            .map(|_| need)
                    };
                    let fitted = fit_to_memory(&candidate, levels, FIT_MIN_CONTEXT, &fits);
                    // An automatic `-ngl` may leave layers in system memory: slower, but it loads
                    // (the recommendation counts the whole card, not what other apps left free).
                    Some(match fitted {
                        Err(steps) if auto_layers => {
                            fit_with_fewer_layers(&candidate, levels, FIT_MIN_CONTEXT, &fits)
                                .ok_or(steps)
                        }
                        other => other,
                    })
                })
                .await?
            };
            match fitted {
                Some(Ok((fitted, adaptation, fitted_need))) => {
                    *config = fitted;
                    *adapted = Some(adaptation);
                    need = fitted_need;
                    let adapted_total = need.saturating_add(headroom);
                    count = eviction_count(adapted_total, available, &footprints, cap);
                }
                Some(Err(steps)) => tried = steps,
                None => {}
            }
        }
        let Some(count) = count else {
            let busy: Vec<_> = self
                .busy_models(&def.id)
                .into_iter()
                .filter(|(_, server)| self.same_device(server, config))
                .collect();
            let mut eventual = footprints.clone();
            for (_, server) in &busy {
                eventual.push(self.footprint(server).await);
            }
            if !busy.is_empty() && eviction_count(need_total, available, &eventual, cap).is_some() {
                return Err(SchedError::ModelBusy {
                    alias: def.alias.clone(),
                    busy: busy.into_iter().map(|(alias, _)| alias).collect(),
                    reason: "insufficient_memory".into(),
                    retry_after: 5,
                });
            }
            return Err(SchedError::InsufficientMemory {
                alias: def.alias.clone(),
                need_bytes: Some(need),
                available_bytes: Some(available),
                // Only what could not leave: pinned, busy or user-started models. The idle
                // models were already counted as gone (`footprints`), so they are not listed.
                blocking: self.blocking_models(
                    &def.id,
                    &victims
                        .iter()
                        .map(|(_, server, _)| server.clone())
                        .collect::<Vec<_>>(),
                ),
                after_full_eviction: true,
                tried: Box::new(tried),
                options: vec![
                    "recovery.lower_context".into(),
                    "recovery.lower_gpu_layers".into(),
                    "recovery.kv_quant".into(),
                ],
            });
        };
        for (victim, server, _) in victims.into_iter().take(count) {
            evicted.push(victim.state().alias.clone());
            self.evict(&victim, &server, "evicted", Some(def.alias.clone()))
                .await;
        }
        Ok(())
    }

    /// macOS (unified memory): `--list-devices` reports the free memory as its own process
    /// sees it and there is no per-process GPU counter, so [`footprint`](Self::footprint)
    /// falls back to the estimate of a server that may never have been resident. Evicting
    /// can then credit more than the device has (measured on an 8 GB M1: a 39 GB estimate
    /// "freed" for a 7.7 GB load that then ran out of Metal memory). `available` is limited
    /// to the device total minus what the running GPU servers take, and the second value caps
    /// the free memory after evictions at the device total. Elsewhere `(available, u64::MAX)`.
    async fn unified_limits(&self, available: u64, total: u64) -> (u64, u64) {
        if !cfg!(target_os = "macos") {
            return (available, u64::MAX);
        }
        let mut used = 0u64;
        for (id, pm) in self.ctx.registry.all() {
            if !matches!(
                pm.get_status(),
                ServerStatus::Running | ServerStatus::Starting
            ) || pm.launch_config().is_none_or(|c| cpu_only(&c))
            {
                continue;
            }
            used = used.saturating_add(self.footprint(&id).await);
        }
        (available.min(total.saturating_sub(used)), total)
    }

    /// GPU memory a running server uses: the OS's per-process value, else the estimate.
    async fn footprint(&self, server_id: &str) -> u64 {
        let Some(pm) = self.ctx.registry.get(server_id) else {
            return 0;
        };
        let pid = pm.pid();
        let config = pm.launch_config();
        blocking(move || {
            pid.and_then(crate::gpu::process_vram_bytes)
                .or_else(|| {
                    let c = config?;
                    if crate::engine::server_for(&c).family() != crate::engine::LLAMA_CPP {
                        return Some(crate::engine::non_llama_vram_need(&c));
                    }
                    let input = crate::vram::input_from_launch(&c);
                    Some(estimate_vram(&input, 16 * 1024 * MIB))
                })
                .unwrap_or(0)
        })
        .await
        .unwrap_or(0)
    }

    /// Gateway-loaded models that are loaded or loading (other than `except`).
    fn managed_loaded(&self, except: &str) -> usize {
        let entries: Vec<Arc<Entry>> = self.entries.lock().unwrap().values().cloned().collect();
        entries
            .iter()
            .filter(|e| e.def_id != except)
            .filter(|e| {
                let s = e.state();
                s.managed
                    && s.server_id.as_deref().is_some_and(|id| {
                        self.ctx.registry.get(id).is_some_and(|pm| {
                            matches!(
                                pm.get_status(),
                                ServerStatus::Running | ServerStatus::Starting
                            )
                        })
                    })
            })
            .count()
    }

    /// Whether `models` can be served at the same time (Playground split view): the limits a
    /// JIT load checks, applied to all of them at once without loading anything. Loaded
    /// models of other definitions that are idle and not pinned count as evictable. An
    /// estimate before sending; each request still goes through [`acquire`](Self::acquire).
    pub async fn co_residency(&self, models: &[String]) -> Result<CoResidency, SchedError> {
        let settings = self.settings();
        let mut defs: Vec<ModelDefinition> = Vec::new();
        for m in models {
            let def = self.resolve(Some(m)).await?;
            if !defs.iter().any(|d| d.id == def.id) {
                defs.push(def);
            }
        }
        let mut out = CoResidency {
            concurrent: true,
            ..Default::default()
        };
        if let [def] = defs.as_slice() {
            // One model: its slots decide (more requests than slots wait in its queue).
            let concurrency = self
                .entries
                .lock()
                .unwrap()
                .get(&def.id)
                .map(|e| e.state().concurrency)
                .filter(|c| *c > 0);
            out.slots = concurrency;
            return Ok(out);
        }
        let ours: Vec<&str> = defs.iter().map(|d| d.id.as_str()).collect();
        let pending: Vec<&ModelDefinition> = defs
            .iter()
            .filter(|d| !matches!(self.serving_server(d), Some((_, ServerStatus::Running))))
            .collect();
        out.to_load = pending.iter().map(|d| d.alias.clone()).collect();
        if settings.max_loaded_models > 0 {
            // Gateway-loaded models that must stay: ours (loaded or about to be) and other
            // models that are busy or pinned.
            let entries: Vec<Arc<Entry>> = self.entries.lock().unwrap().values().cloned().collect();
            let pinned = self.pinned_ids();
            let mut kept = pending.len();
            for e in &entries {
                let (managed, loaded) = {
                    let s = e.state();
                    (s.managed, s.server_id.clone())
                };
                let running = managed
                    && loaded.as_deref().is_some_and(|id| {
                        self.ctx.registry.get(id).is_some_and(|pm| {
                            matches!(
                                pm.get_status(),
                                ServerStatus::Running | ServerStatus::Starting
                            )
                        })
                    });
                if !running {
                    continue;
                }
                let is_ours = ours.contains(&e.def_id.as_str());
                if is_ours || !e.idle() || pinned.contains(&e.def_id) {
                    kept += 1;
                }
            }
            if kept > settings.max_loaded_models as usize {
                out.concurrent = false;
                out.reason = Some("max_loaded_models".into());
                out.limit = Some(settings.max_loaded_models);
                return Ok(out);
            }
        }
        if !settings.memory_aware || pending.is_empty() {
            return Ok(out);
        }
        let mut need_total = 0u64;
        let mut available: Option<(u64, u64)> = None;
        for def in &pending {
            let (_, config) = self.plan(def).await?;
            if cpu_only(&config) {
                continue;
            }
            let runtime = self.ctx.runtime.clone();
            let fixed = crate::engine::server_for(&config).family() != crate::engine::LLAMA_CPP;
            let picture = blocking(move || memory_picture_full(&runtime, &config, false)).await?;
            let Some((need, free, total)) = picture else {
                return Ok(out); // no GPU information: the loads decide
            };
            need_total += need
                + if fixed {
                    crate::vram::fixed_footprint_headroom(total, settings.memory_headroom_mib * MIB)
                } else {
                    crate::vram::memory_headroom(total, settings.memory_headroom_mib * MIB)
                };
            available = Some((free, total));
        }
        let (mut available, cap) = match available {
            Some((free, total)) => self.unified_limits(free, total).await,
            None => (0, u64::MAX),
        };
        // Idle, unpinned models of other definitions would be evicted to make room.
        let pinned = self.pinned_ids();
        let entries: Vec<Arc<Entry>> = self.entries.lock().unwrap().values().cloned().collect();
        for e in entries {
            if ours.contains(&e.def_id.as_str()) || pinned.contains(&e.def_id) || !e.idle() {
                continue;
            }
            let server = {
                let s = e.state();
                s.server_id.clone().filter(|_| s.managed)
            };
            if let Some(server) = server {
                available = available
                    .saturating_add(self.footprint(&server).await)
                    .min(cap);
            }
        }
        out.need_bytes = Some(need_total);
        out.available_bytes = Some(available);
        if need_total > available {
            out.concurrent = false;
            out.reason = Some("insufficient_memory".into());
        }
        Ok(out)
    }

    fn pinned_ids(&self) -> Vec<String> {
        self.defs
            .lock()
            .unwrap()
            .as_ref()
            .map(|(_, d)| {
                d.iter()
                    .filter(|d| d.pinned)
                    .map(|d| d.id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Least recently used gateway-loaded model that is running, idle and not pinned.
    fn lru_victim(&self, except: &str) -> Option<(Arc<Entry>, String)> {
        self.lru_victims(except)
            .into_iter()
            .next()
            .map(|(e, s, _)| (e, s))
    }

    /// All models `lru_victim` may pick, least recently used first.
    fn lru_victims(&self, except: &str) -> Vec<(Arc<Entry>, String, Option<Instant>)> {
        let pinned: Vec<String> = self
            .defs
            .lock()
            .unwrap()
            .as_ref()
            .map(|(_, d)| {
                d.iter()
                    .filter(|d| d.pinned)
                    .map(|d| d.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let entries: Vec<Arc<Entry>> = self.entries.lock().unwrap().values().cloned().collect();
        let mut out: Vec<_> = entries
            .into_iter()
            .filter(|e| e.def_id != except && !pinned.contains(&e.def_id) && e.idle())
            .filter_map(|e| {
                let (managed, server, last) = {
                    let s = e.state();
                    (s.managed, s.server_id.clone(), s.last_used)
                };
                let server = server.filter(|_| managed)?;
                let running = self
                    .ctx
                    .registry
                    .get(&server)
                    .is_some_and(|pm| pm.get_status() == ServerStatus::Running);
                running.then_some((e, server, last))
            })
            .collect();
        out.sort_by_key(|(_, _, last)| *last);
        out
    }

    /// Stops a gateway-loaded server.
    async fn evict(
        &self,
        entry: &Arc<Entry>,
        server_id: &str,
        action: &str,
        reason: Option<String>,
    ) {
        {
            // The override belonged to the instance being unloaded; a request arriving while
            // it stops sets its own.
            let mut s = entry.state();
            s.draining = true;
            s.keep_alive = None;
        }
        if let Some(pm) = self.ctx.registry.get(server_id) {
            if let Err(e) = pm.stop().await {
                eprintln!(
                    "[Harness Gateway] {} could not be stopped: {}",
                    server_id, e
                );
            }
        }
        let alias = {
            let mut s = entry.state();
            s.draining = false;
            s.alias.clone()
        };
        eprintln!(
            "[Harness Gateway] {} ({}) unloaded: {}{}",
            alias,
            server_id,
            action,
            reason
                .as_deref()
                .map(|r| format!(" ({})", r))
                .unwrap_or_default()
        );
        crate::events::publish(crate::events::CoreEvent::GatewayModel {
            definition_id: entry.def_id.clone(),
            alias,
            server_id: server_id.to_string(),
            action: action.to_string(),
            reason,
        });
    }

    fn publish(
        &self,
        def: &ModelDefinition,
        server_id: &str,
        action: &str,
        reason: Option<String>,
    ) {
        eprintln!(
            "[Harness Gateway] {} ({}): {}{}",
            def.alias,
            server_id,
            action,
            reason
                .as_deref()
                .map(|r| format!(" ({})", r))
                .unwrap_or_default()
        );
        crate::events::publish(crate::events::CoreEvent::GatewayModel {
            definition_id: def.id.clone(),
            alias: def.alias.clone(),
            server_id: server_id.to_string(),
            action: action.to_string(),
            reason,
        });
    }

    fn shortest_retry(&self) -> u64 {
        let entries: Vec<Arc<Entry>> = self.entries.lock().unwrap().values().cloned().collect();
        entries
            .iter()
            .filter(|e| e.state().in_flight > 0)
            .map(|e| retry_after(&e.state()))
            .min()
            .unwrap_or(5)
    }

    /// Unloads a definition the gateway serves (GUI / API "unload"). Servers the user started
    /// are stopped too (explicit request). Returns whether something was stopped.
    pub async fn unload(&self, definition_id: &str) -> Result<bool, String> {
        let _load = self.load_lock.lock().await;
        let ids = crate::orchestrator::unload_definition(&self.ctx.registry, definition_id).await?;
        self.clear_keep_alive(definition_id);
        if let Some(e) = self.entries.lock().unwrap().get(definition_id).cloned() {
            let alias = e.state().alias.clone();
            for id in &ids {
                crate::events::publish(crate::events::CoreEvent::GatewayModel {
                    definition_id: definition_id.to_string(),
                    alias: alias.clone(),
                    server_id: id.clone(),
                    action: "unloaded".into(),
                    reason: None,
                });
            }
        }
        Ok(!ids.is_empty())
    }

    /// Drops the Ollama `keep_alive` override of a definition (its model was unloaded).
    pub fn clear_keep_alive(&self, definition_id: &str) {
        if let Some(e) = self.entries.lock().unwrap().get(definition_id) {
            e.state().keep_alive = None;
        }
    }

    /// Idle TTL: unloads gateway-loaded models whose TTL passed. Runs until dropped.
    pub async fn maintenance_loop(self: Arc<Self>) {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            self.expire_idle().await;
        }
    }

    /// One TTL pass (see [`Self::maintenance_loop`]).
    pub async fn expire_idle(&self) {
        self.expire_lemonade().await;
        let audio_running = self
            .ctx
            .registry
            .active_servers()
            .iter()
            .any(|(_, a)| crate::engine::audio_cpp::is_model(&a.model_path));
        if audio_running {
            if let Ok(defs) = self.definitions().await {
                for def in defs.iter().filter(|d| is_audio_cpp(d)) {
                    if let Some((id, ServerStatus::Running)) = self.serving_server(def) {
                        let entry = self.entry(def);
                        self.adopt(&entry, &id, false).await;
                        entry.state().last_used.get_or_insert_with(Instant::now);
                    }
                }
            }
        }
        // Nothing the gateway loaded is running: skip the definitions read (a state.db query
        // every second on an idle `harness serve` / GUI otherwise).
        let entries: Vec<Arc<Entry>> = self
            .entries
            .lock()
            .unwrap()
            .values()
            .filter(|e| {
                let s = e.state();
                s.managed && s.server_id.is_some()
            })
            .cloned()
            .collect();
        if entries.is_empty() {
            return;
        }
        let Ok(defs) = self.definitions().await else {
            return;
        };
        let default_ttl = self.settings().default_ttl_secs;
        for entry in entries {
            let Some(def) = defs.iter().find(|d| d.id == entry.def_id) else {
                continue;
            };
            if def.pinned && !is_audio_cpp(def) {
                continue;
            }
            let keep_alive = entry.state().keep_alive;
            let Some(ttl) = effective_ttl(def, keep_alive, default_ttl) else {
                continue;
            };
            let (managed, server, last) = {
                let s = entry.state();
                (s.managed, s.server_id.clone(), s.last_used)
            };
            let (true, Some(server), Some(last)) = (managed, server, last) else {
                continue;
            };
            if !entry.idle() || last.elapsed() < Duration::from_secs(ttl) {
                continue;
            }
            let running = self
                .ctx
                .registry
                .get(&server)
                .is_some_and(|pm| pm.get_status() == ServerStatus::Running);
            if !running {
                continue;
            }
            let _load = self.load_lock.lock().await;
            // Re-check under the lock: a request may have arrived.
            if !entry.idle() || entry.state().last_used != Some(last) {
                continue;
            }
            self.evict(&entry, &server, "unloaded_ttl", Some(format!("{} s", ttl)))
                .await;
        }
    }

    /// Loads pinned definitions (warm models) that are not running yet.
    pub async fn warm_pinned(self: Arc<Self>) {
        let Ok(defs) = self.servable().await else {
            return;
        };
        for def in defs.into_iter().filter(|d| d.pinned && !is_audio_cpp(d)) {
            let entry = self.entry(&def);
            {
                entry.state().waiting += 1;
            }
            let waiting = Waiting(entry.clone());
            if let Err(e) = self.ensure_ready(&entry, &def).await {
                eprintln!(
                    "[Harness Gateway] Pinned model {} could not be loaded: {}",
                    def.alias,
                    e.message()
                );
            }
            drop(waiting);
        }
    }

    /// Every definition that is loaded, loading or known to the scheduler.
    pub async fn snapshot(&self) -> Vec<ModelSlot> {
        let defs = self.definitions().await.unwrap_or_default();
        let default_ttl = self.settings().default_ttl_secs;
        let mut out = Vec::new();
        for def in &defs {
            let serving = self.serving_server(def);
            let entry = self.entries.lock().unwrap().get(&def.id).cloned();
            if serving.is_none() && entry.is_none() {
                continue;
            }
            let (
                managed,
                server,
                in_flight,
                queued,
                concurrency,
                waited,
                last,
                draining,
                keep_alive,
            ) = match &entry {
                Some(e) => {
                    let s = e.state();
                    (
                        s.managed,
                        s.server_id.clone(),
                        s.in_flight,
                        s.waiting,
                        s.concurrency,
                        s.waited,
                        s.last_used,
                        s.draining,
                        s.keep_alive,
                    )
                }
                None => (false, None, 0, 0, 0, 0, None, false, None),
            };
            let (server_id, status) = match serving {
                Some((id, st)) => (Some(id), Some(st)),
                None => (server, None),
            };
            let state = match status {
                _ if draining => SlotState::Draining,
                Some(ServerStatus::Running) => SlotState::Ready,
                Some(ServerStatus::Starting) => SlotState::Loading,
                Some(ServerStatus::Stopping) => SlotState::Draining,
                _ => SlotState::Unloaded,
            };
            let (token_pool, tokens_in_use, waited_tokens) = entry
                .as_ref()
                .map(|e| {
                    let s = e.state();
                    (
                        s.pool,
                        s.pool.saturating_sub(e.tokens.available_permits() as u64),
                        s.waited_tokens,
                    )
                })
                .unwrap_or((0, 0, 0));
            let port = server_id
                .as_deref()
                .and_then(|id| self.ctx.registry.get(id))
                .and_then(|pm| pm.active_server())
                .map(|a| a.port);
            let managed = managed
                || server_id
                    .as_deref()
                    .is_some_and(|id| id.starts_with(SERVER_PREFIX));
            out.push(ModelSlot {
                definition_id: def.id.clone(),
                alias: def.alias.clone(),
                state,
                server_id,
                port,
                managed,
                pinned: def.pinned,
                ttl_secs: effective_ttl(def, keep_alive, default_ttl),
                keep_alive,
                in_flight,
                queued,
                concurrency,
                waited,
                token_pool,
                tokens_in_use,
                waited_tokens,
                adapted: entry.as_ref().and_then(|e| e.state().adapted.clone()),
                idle_secs: last
                    .filter(|_| in_flight == 0)
                    .map(|l| l.elapsed().as_secs()),
            });
        }
        out
    }
}

fn is_audio_cpp(def: &ModelDefinition) -> bool {
    def.base
        .local_path()
        .is_some_and(crate::engine::audio_cpp::is_model)
}

/// audio.cpp always expires after five idle minutes; other engines honor pinning and TTL overrides.
fn effective_ttl(
    def: &ModelDefinition,
    keep_alive: Option<KeepAlive>,
    default_ttl: Option<u64>,
) -> Option<u64> {
    if is_audio_cpp(def) {
        return Some(crate::engine::audio_cpp::IDLE_SECONDS);
    }
    if def.pinned {
        return None;
    }
    match keep_alive {
        Some(KeepAlive::Forever) => None,
        Some(KeepAlive::Secs(s)) => Some(s),
        None => def.ttl_seconds.filter(|&t| t > 0).or(default_ttl),
    }
}

/// Id prefix of the transient definitions built for servers without one.
pub const EXTERNAL_PREFIX: &str = "srv-";

/// Running (non-router) servers that no stored definition describes, as transient definitions
/// named by the id the server serves (`--alias`, else the file stem). A stored definition with
/// the same alias always wins, so these only add reachable models.
pub fn external_models(
    defs: &[ModelDefinition],
    servers: &[crate::domain::status::ServerView],
) -> Vec<ModelDefinition> {
    use crate::domain::status::definition_states;
    let covered: Vec<String> = definition_states(defs, servers)
        .into_iter()
        .flat_map(|s| s.servers.into_iter().map(|x| x.server_id))
        .collect();
    let mut out: Vec<ModelDefinition> = Vec::new();
    for (id, status, config) in servers {
        if *status != ServerStatus::Running || covered.contains(id) {
            continue;
        }
        let Some(config) = config else { continue };
        let Some(alias) = crate::process::served_alias(config) else {
            continue;
        };
        if defs.iter().any(|d| d.alias.eq_ignore_ascii_case(&alias))
            || out.iter().any(|d| d.alias.eq_ignore_ascii_case(&alias))
        {
            continue;
        }
        let mut def = ModelDefinition::new(
            alias,
            crate::domain::ModelRef::Local {
                path: config.model_path.clone(),
            },
        );
        // `serves()` matches a launch by the definition id it was resolved from.
        def.id = config
            .definition_id
            .clone()
            .unwrap_or_else(|| format!("{}{}", EXTERNAL_PREFIX, id));
        out.push(def);
    }
    out
}

/// Id of the transient definition of a model served by router server `server_id`.
pub fn router_def_id(server_id: &str, model: &str) -> String {
    format!("{EXTERNAL_PREFIX}{server_id}/{model}")
}

/// The router server a transient definition id (see [`router_def_id`]) belongs to.
pub fn router_def_server(def_id: &str) -> Option<&str> {
    def_id
        .strip_prefix(EXTERNAL_PREFIX)?
        .split_once('/')
        .map(|(server, _)| server)
}

/// Transient definitions for the models of running router servers, named by the id the router
/// serves them under. A stored definition with the same alias wins (its own server is used).
/// `routers`: `(server id, served directory, the router's models)`.
pub fn router_external_models(
    defs: &[ModelDefinition],
    routers: &[(String, std::path::PathBuf, Vec<crate::router::RouterModel>)],
) -> Vec<ModelDefinition> {
    let mut out: Vec<ModelDefinition> = Vec::new();
    for (server, dir, models) in routers {
        for m in models {
            let taken = |d: &ModelDefinition| d.alias.eq_ignore_ascii_case(&m.id);
            if defs.iter().any(taken) || out.iter().any(taken) {
                continue;
            }
            let path = m
                .path
                .as_ref()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| dir.clone());
            let mut def =
                ModelDefinition::new(m.id.clone(), crate::domain::ModelRef::Local { path });
            def.id = router_def_id(server, &m.id);
            out.push(def);
        }
    }
    out
}

/// Retry-After estimate of an entry: average request time × waiting / slots, 1..=60 s.
fn retry_after(s: &EntryState) -> u64 {
    let avg = if s.avg_ms > 0.0 { s.avg_ms } else { 2000.0 };
    let per = f64::from(s.waiting.max(1)) / f64::from(s.concurrency.max(1));
    ((avg * per / 1000.0).ceil() as u64).clamp(1, 60)
}

fn estimate_vram(input: &crate::vram::FitInput, gpu_total: u64) -> u64 {
    let budget = crate::vram::MemoryBudget {
        vram_bytes: gpu_total,
        ram_bytes: 0,
        unified: false,
        shared_igpu: false,
        igpu_offload_faster: None,
        cuda: false,
        rocm: false,
    };
    let meta = input
        .model_path
        .as_deref()
        .and_then(|p| crate::models::cached_metadata(std::path::Path::new(p)));
    let measured = input
        .model_path
        .as_deref()
        .map(crate::vram::measurements_for)
        .unwrap_or_default();
    let engine = input
        .model_path
        .as_deref()
        .and_then(|p| crate::engine::server_for_model(std::path::Path::new(p)))
        .unwrap_or(crate::engine::llama_cpp());
    engine
        .estimate_memory(input, meta.as_ref(), &budget, &measured)
        .estimate
        .vram_bytes
}

/// Names of the loaded `(server id, name)` models whose server is not in `evictable`.
fn not_evictable(loaded: Vec<(String, String)>, evictable: &[String]) -> Vec<String> {
    loaded
        .into_iter()
        .filter(|(id, _)| !evictable.contains(id))
        .map(|(_, name)| name)
        .collect()
}

/// How many victims (least recently used first, with their GPU footprints) must leave for
/// `need` to fit into `available`; `None` when it does not fit even with all of them gone.
/// The free memory never exceeds `cap` (the device total on unified memory, else `u64::MAX`).
fn eviction_count(need: u64, available: u64, footprints: &[u64], cap: u64) -> Option<usize> {
    let mut free = available.min(cap);
    if need <= free {
        return Some(0);
    }
    for (i, f) in footprints.iter().enumerate() {
        free = free.saturating_add(*f).min(cap);
        if need <= free {
            return Some(i + 1);
        }
    }
    None
}

/// [`crate::full_offload::step_down`] with the adaptation to report.
fn full_offload_step_down(
    config: &crate::process::LaunchConfig,
    need_limit: u64,
    fit: impl Fn(&crate::process::LaunchConfig) -> crate::vram::FitResult,
) -> Option<(crate::process::LaunchConfig, LoadAdaptation)> {
    let stepped = crate::full_offload::step_down(config, need_limit, &fit)?;
    let (k, v) = fit(&stepped)
        .kv
        .and_then(|kv| kv.level.types())
        .unwrap_or(("f16", "f16"));
    let adaptation = LoadAdaptation {
        cache_type_k: k.into(),
        cache_type_v: v.into(),
        context_size: stepped.context_size,
        requested_context: config.context_size,
        gpu_layers: None,
    };
    Some((stepped, adaptation))
}

/// The ladder of a definition with its own cache types: those types, at smaller contexts.
const KEEP_CACHE: [crate::kv_policy::KvLevel; 1] = [crate::kv_policy::KvLevel::Custom];

/// The load may be fitted into the free memory with other cache types / a smaller context:
/// a llama.cpp chat launch with a context. Cache types the automatic policy owns step down;
/// a definition's own cache types stay and only the context shrinks (S34: q4_0/q4_0 at 65k
/// was refused for 1 MiB). `-ctk`/`-ctv` arguments are never touched.
fn fit_applies(config: &crate::process::LaunchConfig) -> bool {
    !config.router_mode
        && !crate::kv_policy::custom_args_set_cache_type(config.custom_args.as_deref())
        && config.context_size > 0
        && crate::engine::server_for(config).family() == crate::engine::LLAMA_CPP
}

/// Server-log line for a context the gateway chose (`lang`: `tr`, else English).
fn default_context_log_line(context: u32, lang: &str) -> String {
    if lang == "tr" {
        format!("[Harness Studio] Tanım bağlam belirtmiyor: llama.cpp'nin 4096 varsayılanı yerine modelin eğitim bağlamıyla (en çok 32768) yüklendi: {context} (tanım değişmedi).")
    } else {
        format!("[Harness Studio] The definition sets no context: loaded with the model's trained context (at most 32768) instead of llama.cpp's 4096: {context} (the definition is unchanged).")
    }
}

/// Last resort of the memory fit for an automatic `-ngl`: at the floor context, the most layers
/// any cache rung of `levels` leaves on the GPU within what `fits` accepts (ties keep the better
/// cache). `None` when not even one layer fits. Blocking.
fn fit_with_fewer_layers(
    config: &crate::process::LaunchConfig,
    levels: &[crate::kv_policy::KvLevel],
    floor: u32,
    mut fits: impl FnMut(&crate::process::LaunchConfig) -> Option<u64>,
) -> Option<(crate::process::LaunchConfig, LoadAdaptation, u64)> {
    let context = floor.min(config.context_size);
    let mut best: Option<(crate::process::LaunchConfig, u64)> = None;
    for &level in levels {
        let mut base = config.clone();
        base.context_size = context;
        if let Some((k, v)) = level.types() {
            base.cache_type_k = Some(k.into());
            base.cache_type_v = Some(v.into());
        }
        let at = |n: u32| {
            let mut c = base.clone();
            c.gpu_layers = n;
            c
        };
        // Memory grows with the layer count: the largest count that fits.
        let (mut lo, mut hi) = (0u32, config.gpu_layers);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if fits(&at(mid)).is_some() {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        if lo == 0 || best.as_ref().is_some_and(|(b, _)| b.gpu_layers >= lo) {
            continue;
        }
        let c = at(lo);
        if let Some(need) = fits(&c) {
            best = Some((c, need));
        }
    }
    let (candidate, need) = best?;
    let adaptation = LoadAdaptation {
        cache_type_k: candidate.cache_type_k.clone().unwrap_or_default(),
        cache_type_v: candidate.cache_type_v.clone().unwrap_or_default(),
        context_size: candidate.context_size,
        requested_context: config.context_size,
        gpu_layers: Some(candidate.gpu_layers),
    };
    Some((candidate, adaptation, need))
}

/// Names of the ladder rungs a fit tried (for the refusal message).
fn tried_steps(
    config: &crate::process::LaunchConfig,
    levels: &[crate::kv_policy::KvLevel],
    context: u32,
    floor: u32,
) -> Vec<String> {
    let own = || {
        let t = |t: &Option<String>| t.clone().unwrap_or_else(|| "f16".into());
        (t(&config.cache_type_k), t(&config.cache_type_v))
    };
    let rungs: Vec<String> = levels
        .iter()
        .map(|l| match l.types() {
            Some((k, v)) => (k.to_string(), v.to_string()),
            None => own(),
        })
        .map(|(k, v)| {
            if k == v {
                format!("KV {k}")
            } else {
                format!("K {k} / V {v}")
            }
        })
        .collect();
    let mut out = vec![format!("cache types {}", rungs.join(", "))];
    if floor < context {
        out.push(format!("context {context} down to {floor}"));
    } else {
        out.push(format!("context {context}"));
    }
    out
}

/// Fits `config` into the memory `fits` accepts: [`crate::prepare::first_ladder_fit`] over the
/// machine's cache ladder ([`crate::kv_policy::ladder_for`]) and the contexts down to `floor`.
/// `fits` returns the estimated GPU need when the candidate fits. `Ok` carries the candidate
/// (explicit cache types and context set; the stored definition is not touched), what changed
/// and the need; `Err` what was tried. Blocking.
fn fit_to_memory(
    config: &crate::process::LaunchConfig,
    levels: &[crate::kv_policy::KvLevel],
    floor: u32,
    mut fits: impl FnMut(&crate::process::LaunchConfig) -> Option<u64>,
) -> Result<(crate::process::LaunchConfig, LoadAdaptation, u64), Vec<String>> {
    let mut found = None;
    let hit = crate::prepare::first_ladder_fit(levels, config.context_size, floor, |level, ctx| {
        let mut candidate = config.clone();
        if let Some((k, v)) = level.types() {
            candidate.cache_type_k = Some(k.into());
            candidate.cache_type_v = Some(v.into());
        }
        candidate.context_size = ctx;
        match fits(&candidate) {
            Some(need) => {
                found = Some((candidate, need));
                true
            }
            None => false,
        }
    });
    match (hit, found) {
        (Some(_), Some((candidate, need))) => {
            let adaptation = LoadAdaptation {
                cache_type_k: candidate.cache_type_k.clone().unwrap_or_default(),
                cache_type_v: candidate.cache_type_v.clone().unwrap_or_default(),
                context_size: candidate.context_size,
                requested_context: config.context_size,
                gpu_layers: None,
            };
            Ok((candidate, adaptation, need))
        }
        _ => Err(tried_steps(
            config,
            levels,
            config.context_size,
            floor.min(config.context_size),
        )),
    }
}

/// The machine's cache ladder for a launch (chat launches only; `None` for embedding or rerank
/// servers, whose caches are tiny and whose context must stay as set). Blocking.
fn fit_levels(
    config: &crate::process::LaunchConfig,
) -> Option<&'static [crate::kv_policy::KvLevel]> {
    if crate::engine::server_for(config).family() == crate::engine::ONNX_FAMILY {
        return None;
    }
    let meta = crate::models::cached_metadata(&config.model_path);
    let mut probe = config.clone();
    let mode = crate::serve_mode::prepare(&mut probe, meta.as_ref(), None).mode;
    if mode != crate::serve_mode::ServeMode::Chat {
        return None;
    }
    let budget = crate::vram::budget_for_model(
        &crate::hardware::detect_hardware(),
        config.model_path.to_str(),
    );
    let fa = !crate::kv_policy::custom_args_disable_flash_attn(config.custom_args.as_deref());
    Some(crate::kv_policy::ladder_for(&budget, fa))
}

/// What the scheduler knows of a server without `/props` (whisper, sd, ONNX, mistral.rs): a
/// mistral.rs chat server batches [`crate::engine::mistralrs_concurrency`] requests at once and
/// has the context of its launch; the others take one request at a time.
fn non_llama_props(config: Option<&crate::process::LaunchConfig>) -> Props {
    match config {
        Some(c) if crate::engine::server_for(c).family() == crate::engine::MISTRALRS => Props {
            slots: Some(crate::engine::mistralrs_concurrency(c)),
            ctx_size: (c.context_size > 0).then_some(u64::from(c.context_size)),
            template_tools: None,
        },
        _ => Props::default(),
    }
}

fn cpu_only(config: &crate::process::LaunchConfig) -> bool {
    config.gpu_layers == 0
        && !crate::process::fit_chooses_layers(config)
        && !crate::capabilities::verify::custom_args_set_gpu_layers(config.custom_args.as_deref())
        && crate::engine::server_for(config).family() == crate::engine::LLAMA_CPP
}

/// A split footprint cannot be attributed to one adapter; keep the previous behavior.
fn footprint_gpu(config: &crate::process::LaunchConfig) -> Option<u32> {
    if crate::capabilities::verify::custom_args_set_gpu_layers(config.custom_args.as_deref())
        || config
            .tensor_split
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
    {
        return None;
    }
    config.main_gpu.or_else(crate::gpu::primary_gpu_index)
}

fn same_gpu(target: Option<u32>, footprint: Option<u32>) -> bool {
    match (target, footprint) {
        (Some(target), Some(footprint)) => target == footprint,
        _ => true,
    }
}

async fn wait_for_busy<T>(result: &Result<T, SchedError>, deadline: tokio::time::Instant) -> bool {
    if !matches!(result, Err(SchedError::ModelBusy { .. }))
        || tokio::time::Instant::now() >= deadline
    {
        return false;
    }
    tokio::time::sleep_until(
        (tokio::time::Instant::now() + Duration::from_millis(100)).min(deadline),
    )
    .await;
    true
}

/// The VRAM reserve a load is admitted with: the full one for llama.cpp, half of it for the
/// fixed-footprint servers (image, speech, ONNX; see [`crate::vram::fixed_footprint_headroom`]).
fn admission_headroom(
    config: &crate::process::LaunchConfig,
    total: u64,
    configured_mib: u64,
) -> u64 {
    if crate::engine::server_for(config).family() == crate::engine::LLAMA_CPP {
        crate::vram::memory_headroom(total, configured_mib * MIB)
    } else {
        crate::vram::fixed_footprint_headroom(total, configured_mib * MIB)
    }
}

fn memory_hint_status(need: u64, free: u64, total: u64, headroom: u64) -> &'static str {
    if need == 0 {
        return "fits";
    }
    let required = need.saturating_add(headroom);
    if required <= free {
        "fits"
    } else if required <= total {
        "unload"
    } else {
        "too_big"
    }
}

/// `(need, available, total)` GPU bytes for a launch on its main device, from the estimate
/// (or measurement) and `--list-devices` (free memory corrected by the OS usage). `None`
/// without a GPU. Blocking.
fn memory_picture_full(
    runtime: &RuntimeManager,
    config: &crate::process::LaunchConfig,
    fresh: bool,
) -> Option<(u64, u64, u64)> {
    if cpu_only(config) {
        return Some((0, 0, 0));
    }
    // A whisper model has no llama-server of its own to list devices with: the device list
    // (free VRAM) comes from the official llama.cpp runtime, the model's own footprint from
    // the whisper engine (zero for a CPU build).
    let whisper = crate::engine::server_for(config).family() != crate::engine::LLAMA_CPP;
    let binary = crate::runtimes::resolve_binary(
        runtime,
        &crate::config::AppSettings::load(),
        config.runtime.as_deref().filter(|_| !whisper),
    )
    .ok()?;
    let max_age = if fresh {
        Duration::ZERO
    } else {
        Duration::from_secs(3)
    };
    let report = crate::devices::report(&binary, max_age);
    let device = match config.main_gpu {
        // An adapter index (see `devices::pair_adapters`).
        Some(adapter) => {
            let i = crate::devices::runtime_index(
                &report.devices,
                &crate::devices::os_adapters(),
                adapter,
            );
            report.devices.iter().find(|d| d.index == i)
        }
        None => report
            .devices
            .iter()
            .find(|d| !d.integrated && d.total_bytes.is_some()),
    }?;
    let total = device.total_bytes?;
    let available = device.available_bytes.or(device.free_bytes)?;
    let need = if whisper {
        crate::engine::non_llama_vram_need(config)
    } else {
        estimate_vram(&crate::vram::input_from_launch(config), total)
    };
    Some((need, available, total))
}

/// Unix seconds (for callers that show times).
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn onnx_servers_have_no_llama_kv_ladder() {
        let config = crate::process::LaunchConfig {
            model_path: "speech/onnx/model.onnx".into(),
            ..Default::default()
        };
        assert!(super::fit_levels(&config).is_none());
    }

    use super::*;

    /// A mistral.rs server has no `/props`: its slots and context come from the launch (no
    /// request is made), and it is not a llama-server for the telemetry and probes.
    #[test]
    fn servers_without_props_are_sized_from_their_launch() {
        let root = tempfile::Builder::new()
            .prefix(&crate::paths::test_dir_prefix("mrs-props"))
            .tempdir()
            .unwrap();
        let dir = root.path().join("SmolLM2-360M-Instruct");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.json"),
            r#"{"architectures":["LlamaForCausalLM"],"model_type":"llama"}"#,
        )
        .unwrap();
        std::fs::write(dir.join("model.safetensors"), [0u8; 16]).unwrap();
        let mut c = crate::process::LaunchConfig {
            model_path: dir.clone(),
            context_size: 8192,
            gpu_layers: 999,
            ..Default::default()
        };
        let p = non_llama_props(Some(&c));
        assert_eq!(p.slots, Some(crate::engine::mistralrs::DEFAULT_CONCURRENCY));
        assert_eq!(p.ctx_size, Some(8192));
        assert_eq!(p.template_tools, None);
        c.parallel_slots = Some(3);
        assert_eq!(non_llama_props(Some(&c)).slots, Some(3));
        // whisper keeps one request at a time; no launch known, nothing assumed.
        let whisper = crate::process::LaunchConfig {
            model_path: "m/ggml-base.bin".into(),
            ..Default::default()
        };
        assert_eq!(non_llama_props(Some(&whisper)).slots, None);
        assert_eq!(non_llama_props(None).slots, None);

        let active = crate::process::ActiveServerInfo {
            connect_host: "127.0.0.1".into(),
            bind_host: "127.0.0.1".into(),
            port: 11460,
            api_key: None,
            api_key_set: false,
            api_key_required: false,
            model_path: dir.clone(),
            router_mode: false,
            exposure_acknowledged: false,
            cors_origins: None,
            serve_mode: crate::serve_mode::ServeMode::Chat,
            engine_family: String::new(),
        };
        assert!(!active.is_llama());
        // Its memory comes from the mistral.rs estimate, not a GGUF header.
        assert!(crate::engine::non_llama_vram_need(&c) > 0);
        assert!(!cpu_only(&c));
        assert!(!fit_applies(&c));
        assert_eq!(default_context_for(&c), None);
    }

    #[tokio::test]
    async fn busy_wait_retries_only_transient_errors_and_is_bounded() {
        let error = SchedError::ModelBusy {
            alias: "next".into(),
            busy: vec!["active".into()],
            reason: "insufficient_memory".into(),
            retry_after: 5,
        };
        let busy: Result<(), _> = Err(error.clone());
        let deadline = tokio::time::Instant::now() + Duration::from_millis(20);
        assert!(wait_for_busy(&busy, deadline).await);
        assert!(!wait_for_busy(&busy, deadline).await);
        assert!(!wait_for_busy(&Ok(()), deadline + Duration::from_secs(5)).await);
        assert!(
            !wait_for_busy::<()>(
                &Err(SchedError::Internal("failure".into())),
                deadline + Duration::from_secs(5)
            )
            .await
        );
        assert_eq!(error.http_status(), 503);
        assert_eq!(error.retry_after(), Some(5));
        assert!(error
            .message()
            .contains("active are busy; retry after their requests finish"));
    }

    #[test]
    fn eviction_uses_only_the_target_adapter_or_unknown_footprints() {
        let footprints = [(Some(1), 900), (Some(0), 60), (None, 50)];
        let relevant: Vec<_> = footprints
            .into_iter()
            .filter(|(gpu, _)| same_gpu(Some(0), *gpu))
            .map(|(_, bytes)| bytes)
            .collect();
        assert_eq!(relevant, vec![60, 50]);
        assert_eq!(eviction_count(200, 100, &relevant, u64::MAX), Some(2));
        assert_eq!(eviction_count(300, 100, &relevant, u64::MAX), None);
        assert!(same_gpu(None, Some(1)));
    }

    #[test]
    fn explicit_cpu_load_needs_no_vram_or_headroom() {
        let mut config = crate::process::LaunchConfig {
            gpu_layers: 0,
            ..Default::default()
        };
        assert!(cpu_only(&config));
        assert_eq!(
            memory_picture_full(&RuntimeManager::default(), &config, true),
            Some((0, 0, 0))
        );
        assert_eq!(memory_hint_status(0, 0, 0, 512 * MIB), "fits");
        config
            .params
            .insert("fit".into(), crate::params::ParamValue::Text("on".into()));
        assert!(!cpu_only(&config));
        config.params.clear();
        config.custom_args = Some("-ngl 10".into());
        assert!(!cpu_only(&config));
    }

    #[test]
    fn eviction_plan_never_evicts_for_a_request_that_cannot_fit() {
        // Fits already: nothing leaves.
        assert_eq!(eviction_count(100, 100, &[50, 50], u64::MAX), Some(0));
        // LRU order is kept: the first victim is enough.
        assert_eq!(eviction_count(150, 100, &[60, 500], u64::MAX), Some(1));
        // Two are needed.
        assert_eq!(eviction_count(150, 100, &[20, 30], u64::MAX), Some(2));
        // Even all of them leave too little: refuse without evicting anything.
        assert_eq!(eviction_count(1000, 100, &[200, 300], u64::MAX), None);
        // Only pinned / busy models hold the memory (no victims at all).
        assert_eq!(eviction_count(1000, 100, &[], u64::MAX), None);
    }

    #[test]
    fn unified_eviction_never_frees_more_than_the_device() {
        const M: u64 = 1024 * 1024;
        // Without a cap the estimate of a never-resident server is credited in full.
        assert_eq!(
            eviction_count(7759 * M, 5460 * M, &[39727 * M], u64::MAX),
            Some(1)
        );
        // Capped at the device total (8 GB M1): the load cannot fit whatever leaves.
        assert_eq!(eviction_count(7759 * M, 0, &[39727 * M], 5461 * M), None);
        // A load that fits within the device after one eviction still evicts one.
        assert_eq!(
            eviction_count(3512 * M, 2460 * M, &[3000 * M], 5461 * M),
            Some(1)
        );
    }

    #[test]
    fn blocking_list_has_only_what_cannot_leave() {
        let loaded = vec![
            ("s1".to_string(), "pinned".to_string()),
            ("s2".to_string(), "idle".to_string()),
        ];
        // The idle model is a victim: only the pinned one blocks.
        assert_eq!(
            not_evictable(loaded.clone(), &["s2".to_string()]),
            vec!["pinned".to_string()]
        );
        // Nothing evictable: both are listed.
        assert_eq!(not_evictable(loaded, &[]).len(), 2);
        let msg = SchedError::InsufficientMemory {
            alias: "big".into(),
            need_bytes: Some(4 << 30),
            available_bytes: Some(1 << 30),
            blocking: vec!["pinned".into()],
            after_full_eviction: true,
            options: vec![],
            tried: Box::default(),
        }
        .message();
        assert!(msg.contains("even with the idle models unloaded"), "{msg}");
        assert!(msg.contains("loaded and not evictable: pinned"), "{msg}");
        // OOM after the load: no such claim, idle models are not in the list.
        let oom = SchedError::InsufficientMemory {
            alias: "big".into(),
            need_bytes: None,
            available_bytes: None,
            blocking: vec!["pinned".into()],
            after_full_eviction: false,
            options: vec![],
            tried: Box::default(),
        }
        .message();
        assert!(!oom.contains("idle models unloaded"), "{oom}");
        assert!(oom.contains("loaded and not evictable: pinned"), "{oom}");
        assert!(!oom.contains("idle"), "{oom}");
    }

    #[test]
    fn picker_memory_includes_admission_headroom() {
        assert_eq!(memory_hint_status(80, 100, 200, 20), "fits");
        assert_eq!(memory_hint_status(81, 100, 200, 20), "unload");
        assert_eq!(memory_hint_status(181, 100, 200, 20), "too_big");
        assert_eq!(memory_hint_status(u64::MAX, 100, 200, 20), "too_big");
    }

    #[test]
    fn running_servers_without_definition_are_routable_by_alias() {
        use crate::domain::ModelRef;
        use crate::process::LaunchConfig;
        let dir = tempfile::tempdir().unwrap();
        let chat = dir.path().join("Qwen3-4B-Q4_K_M.gguf");
        let embed = dir.path().join("nomic-embed-text-v1.5.Q8_0.gguf");
        std::fs::write(&chat, b"GGUF").unwrap();
        std::fs::write(&embed, b"GGUF").unwrap();
        let mut def = ModelDefinition::new("qwen3", ModelRef::Local { path: chat.clone() });
        def.id = "md-chat".into();
        let cfg = |path: &std::path::Path, definition_id: Option<&str>| LaunchConfig {
            model_path: path.to_path_buf(),
            definition_id: definition_id.map(str::to_string),
            ..Default::default()
        };
        let servers = vec![
            // Covered by the definition.
            (
                "default".to_string(),
                ServerStatus::Running,
                Some(cfg(&chat, Some("md-chat"))),
            ),
            // A "New server" for an embedding model: no definition.
            (
                "s-2".to_string(),
                ServerStatus::Running,
                Some(cfg(&embed, None)),
            ),
            // Not running: not routable.
            (
                "s-3".to_string(),
                ServerStatus::Stopped,
                Some(cfg(&embed, None)),
            ),
        ];
        let ext = external_models(&[def.clone()], &servers);
        assert_eq!(ext.len(), 1);
        assert_eq!(ext[0].alias, "nomic-embed-text-v1.5.Q8_0");
        assert_eq!(ext[0].id, "srv-s-2");
        // The transient definition is seen as served by that very server.
        let states = crate::domain::status::definition_states(&ext, &servers);
        assert_eq!(states[0].servers[0].server_id, "s-2");
        // A stored definition with the same alias wins.
        let mut same = ModelDefinition::new(
            "nomic-embed-text-v1.5.Q8_0",
            ModelRef::Local { path: embed },
        );
        same.id = "md-e".into();
        assert!(external_models(&[same], &servers)
            .iter()
            .all(|d| d.alias != "nomic-embed-text-v1.5.Q8_0"));
    }

    #[test]
    fn router_models_are_routable_through_their_server() {
        use crate::domain::ModelRef;
        use crate::process::LaunchConfig;
        use crate::router::{RouterModel, RouterModelState};
        let dir = std::path::PathBuf::from("models");
        let model = |id: &str, path: Option<&str>| RouterModel {
            id: id.to_string(),
            state: RouterModelState::Unloaded,
            source: None,
            path: path.map(str::to_string),
            context_length: None,
            exit_code: None,
        };
        let mut stored = ModelDefinition::new(
            "qwen3",
            ModelRef::Local {
                path: "q.gguf".into(),
            },
        );
        stored.id = "md-q".into();
        let routers = vec![(
            "r1".to_string(),
            dir.clone(),
            vec![
                model("gemma", Some("models/gemma.gguf")),
                model("qwen3", None),
                model("hub/model:Q4", None),
            ],
        )];
        let ext = router_external_models(&[stored], &routers);
        assert_eq!(ext.len(), 2, "a stored alias wins");
        assert_eq!(ext[0].alias, "gemma");
        assert_eq!(ext[0].id, "srv-r1/gemma");
        assert_eq!(router_def_server(&ext[0].id), Some("r1"));
        assert_eq!(
            ext[0].base.local_path(),
            Some(std::path::Path::new("models/gemma.gguf"))
        );
        assert_eq!(ext[1].base.local_path(), Some(dir.as_path()));
        assert_eq!(router_def_server("srv-s-2"), None);
        assert_eq!(router_def_server("md-1"), None);

        // The router server is seen as serving its models, and only its own.
        let config = LaunchConfig {
            router_mode: true,
            ..Default::default()
        };
        let servers = vec![
            (
                "r1".to_string(),
                ServerStatus::Running,
                Some(config.clone()),
            ),
            ("r2".to_string(), ServerStatus::Running, Some(config)),
        ];
        let states = crate::domain::status::definition_states(&ext, &servers);
        assert_eq!(states[0].servers.len(), 1);
        assert_eq!(states[0].servers[0].server_id, "r1");
    }

    #[test]
    fn server_ids_and_errors() {
        assert_eq!(gw_server_id("Qwen3-4B:Q4_K_M"), "gw-qwen3-4b-q4-k-m");
        assert_eq!(gw_server_id("..."), "gw-model");
        let e = SchedError::QueueFull {
            alias: "a".into(),
            retry_after: 3,
        };
        assert_eq!(
            (e.http_status(), e.code(), e.retry_after()),
            (503, "queue_full", Some(3))
        );
        let e = SchedError::ModelNotFound {
            model: "x".into(),
            available: vec!["a".into(), "b".into()],
        };
        assert_eq!(e.http_status(), 404);
        assert!(e.message().contains("a, b"));
        let s = EntryState {
            waiting: 4,
            concurrency: 2,
            avg_ms: 3000.0,
            ..Default::default()
        };
        assert_eq!(retry_after(&s), 6);
        assert_eq!(retry_after(&EntryState::default()), 2);
    }

    #[tokio::test]
    async fn keep_alive_lives_only_for_the_request_that_sent_it() {
        let sched = Scheduler::new(
            SchedulerContext {
                registry: Arc::default(),
                runtime: Arc::default(),
                gate: Arc::default(),
            },
            GatewaySettings::default(),
        );
        let def = ModelDefinition::new(
            "m",
            crate::domain::ModelRef::Local {
                path: "m.gguf".into(),
            },
        );
        let ka = |sched: &Arc<Scheduler>| sched.entry(&def).state().keep_alive;
        // The acquires fail (nothing can load) after they set or clear the override.
        let _ = sched
            .acquire_keep_alive(&def, None, Some(KeepAlive::Secs(60)))
            .await;
        assert_eq!(ka(&sched), Some(KeepAlive::Secs(60)));
        // An Ollama request without keep_alive: the definition's TTL applies again.
        let _ = sched.acquire_keep_alive(&def, None, None).await;
        assert_eq!(ka(&sched), None);
        // An OpenAI / Anthropic / Responses request clears it as well.
        let _ = sched
            .acquire_keep_alive(&def, None, Some(KeepAlive::Forever))
            .await;
        assert_eq!(ka(&sched), Some(KeepAlive::Forever));
        let _ = sched.acquire_tokens(&def, None).await;
        assert_eq!(ka(&sched), None);
        // Any unload clears it.
        sched.entry(&def).state().keep_alive = Some(KeepAlive::Secs(5));
        sched.clear_keep_alive(&def.id);
        assert_eq!(ka(&sched), None);
    }

    #[test]
    fn audio_cpp_idle_expiry_cannot_be_disabled_by_pinning_or_overrides() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio.gguf");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(b"GGUF").unwrap();
        file.write_all(&3u32.to_le_bytes()).unwrap();
        file.write_all(&0u64.to_le_bytes()).unwrap();
        file.write_all(&1u64.to_le_bytes()).unwrap();
        for value in ["general.architecture", "audiocpp"] {
            file.write_all(&(value.len() as u64).to_le_bytes()).unwrap();
            file.write_all(value.as_bytes()).unwrap();
            if value == "general.architecture" {
                file.write_all(&8u32.to_le_bytes()).unwrap();
            }
        }
        drop(file);
        let mut def = ModelDefinition::new("audio", crate::domain::ModelRef::Local { path });
        for pinned in [false, true] {
            def.pinned = pinned;
            def.ttl_seconds = Some(86400);
            for keep_alive in [None, Some(KeepAlive::Forever), Some(KeepAlive::Secs(86400))] {
                assert_eq!(effective_ttl(&def, keep_alive, None), Some(300));
            }
        }
    }

    #[test]
    fn keep_alive_overrides_ttl() {
        let mut def = ModelDefinition::new(
            "m",
            crate::domain::ModelRef::Local {
                path: "m.gguf".into(),
            },
        );
        assert_eq!(effective_ttl(&def, None, None), None);
        assert_eq!(effective_ttl(&def, None, Some(300)), Some(300));
        def.ttl_seconds = Some(60);
        assert_eq!(effective_ttl(&def, None, Some(300)), Some(60));
        assert_eq!(effective_ttl(&def, Some(KeepAlive::Secs(0)), None), Some(0));
        assert_eq!(effective_ttl(&def, Some(KeepAlive::Secs(5)), None), Some(5));
        assert_eq!(effective_ttl(&def, Some(KeepAlive::Forever), Some(9)), None);
        // Pinned: never unloaded for idleness, whatever keep_alive says.
        def.pinned = true;
        assert_eq!(effective_ttl(&def, Some(KeepAlive::Secs(3)), Some(9)), None);
        assert_eq!(effective_ttl(&def, None, Some(9)), None);
        assert_eq!(
            serde_json::to_value(KeepAlive::Secs(5)).unwrap(),
            serde_json::json!({"secs": 5})
        );
        assert_eq!(
            serde_json::to_value(KeepAlive::Forever).unwrap(),
            serde_json::json!("forever")
        );
    }

    // qwen2.5-coder-14b Q4_K_M on a 16 GB card with 4.3 GB used by other apps (measured):
    // weights 9.0 GB, f16 cache of the 65k context about 2.9 GB.
    const WEIGHTS_MIB: u64 = 9000;
    const TOTAL_MIB: u64 = 16384;

    fn config_65k() -> crate::process::LaunchConfig {
        crate::process::LaunchConfig {
            context_size: 65_536,
            ..Default::default()
        }
    }

    fn need_mib(c: &crate::process::LaunchConfig) -> u64 {
        let bpe = |t: &Option<String>| crate::vram::kv_bytes_per_element(t.as_deref());
        let f16 = 2900.0 * f64::from(c.context_size) / 65_536.0;
        let scale = (bpe(&c.cache_type_k) + bpe(&c.cache_type_v)) / 4.0;
        WEIGHTS_MIB + (f16 * scale) as u64
    }

    /// `fits` of a card with `free_mib` free and the automatic reserve.
    fn fits_in(free_mib: u64) -> impl FnMut(&crate::process::LaunchConfig) -> Option<u64> {
        let headroom = crate::vram::headroom_with(TOTAL_MIB * MIB, 0, 0) / MIB;
        move |c| {
            let need = need_mib(c);
            (need + headroom <= free_mib).then_some(need)
        }
    }

    #[test]
    fn a_load_that_does_not_fit_f16_steps_the_cache_down_first() {
        use crate::kv_policy::AUTO_LADDER;
        // The 503 of the field test: f16 needs 11.9 GB, 11 973 MiB are free (1.3 GB reserve).
        assert!(fits_in(11_973)(&config_65k()).is_none());
        let (c, a, need) =
            fit_to_memory(&config_65k(), &AUTO_LADDER, 16_384, fits_in(11_973)).unwrap();
        assert_eq!(
            (c.cache_type_k.as_deref(), c.cache_type_v.as_deref()),
            (Some("q8_0"), Some("q8_0"))
        );
        assert_eq!(
            c.context_size, 65_536,
            "the context stays while the cache can give"
        );
        assert_eq!(a.requested_context, 65_536);
        assert_eq!(need, need_mib(&c));
    }

    #[test]
    fn a_tighter_card_also_shrinks_the_context_but_not_below_the_floor() {
        use crate::kv_policy::AUTO_LADDER;
        let (c, a, _) =
            fit_to_memory(&config_65k(), &AUTO_LADDER, 16_384, fits_in(11_000)).unwrap();
        assert_eq!(
            (c.cache_type_k.as_deref(), c.cache_type_v.as_deref()),
            (Some("q8_0"), Some("q4_0"))
        );
        assert_eq!(c.context_size, 32_768);
        assert!(a.summary().contains("q4_0") && a.summary().contains("32768"));
        // Nothing fits: every rung down to the floor was tried and says so.
        let tried = fit_to_memory(&config_65k(), &AUTO_LADDER, 16_384, fits_in(9_500)).unwrap_err();
        assert_eq!(
            tried,
            vec![
                "cache types KV f16, KV q8_0, K q8_0 / V q4_0".to_string(),
                "context 65536 down to 16384".to_string()
            ]
        );
        let error = SchedError::InsufficientMemory {
            alias: "qwen2.5-coder-14b-instruct-q4_k_m".into(),
            need_bytes: Some(11_918 * MIB),
            available_bytes: Some(9_500 * MIB),
            blocking: Vec::new(),
            after_full_eviction: true,
            options: Vec::new(),
            tried: Box::new(tried),
        };
        assert_eq!(error.http_status(), 503);
        let message = error.message();
        assert!(message.contains("does not fit even with the idle models unloaded"));
        assert!(message.contains("tried to fit it: cache types KV f16, KV q8_0, K q8_0 / V q4_0; context 65536 down to 16384"));
    }

    #[test]
    fn the_definitions_own_cache_types_keep_and_only_the_context_shrinks() {
        let mut c = config_65k();
        c.model_path = "model.gguf".into();
        assert!(fit_applies(&c));
        c.cache_type_k = Some("q4_0".into());
        c.cache_type_v = Some("q4_0".into());
        assert!(
            fit_applies(&c),
            "an explicit cache type may still shrink the context"
        );
        // The field case: q4_0/q4_0 at 65k missed by 1 MiB.
        let headroom = crate::vram::headroom_with(TOTAL_MIB * MIB, 0, 0) / MIB;
        let free = need_mib(&c) + headroom - 1;
        assert!(fits_in(free)(&c).is_none());
        let (fitted, a, _) = fit_to_memory(&c, &KEEP_CACHE, 16_384, fits_in(free)).unwrap();
        assert_eq!(
            (
                fitted.cache_type_k.as_deref(),
                fitted.cache_type_v.as_deref()
            ),
            (Some("q4_0"), Some("q4_0"))
        );
        assert!(fitted.context_size < 65_536 && fitted.context_size >= 16_384);
        assert_eq!(a.cache_type_k, "q4_0");
        let tried = fit_to_memory(&c, &KEEP_CACHE, 16_384, fits_in(1)).unwrap_err();
        assert_eq!(tried[0], "cache types KV q4_0");
        let mut c = config_65k();
        c.custom_args = Some("--temp 0.2 -ctv q8_0".into());
        assert!(!fit_applies(&c), "the user's own arguments win");
        let mut c = config_65k();
        c.context_size = 0;
        assert!(!fit_applies(&c));
    }

    #[test]
    fn the_adaptation_is_logged_in_both_languages() {
        let a = LoadAdaptation {
            cache_type_k: "q8_0".into(),
            cache_type_v: "q8_0".into(),
            context_size: 32_768,
            requested_context: 65_536,
            gpu_layers: None,
        };
        let layered = LoadAdaptation {
            gpu_layers: Some(30),
            ..a.clone()
        };
        assert!(layered.log_line("tr").contains("ekran kartında 30 katman"));
        assert!(layered.log_line("en").contains("30 layers on the GPU"));
        assert!(layered.summary().ends_with(", 30 GPU layers"));
        assert!(!a.summary().contains("layers"));
        assert!(a
            .log_line("tr")
            .contains("Belleğe sığdırmak için K q8_0 / V q8_0"));
        assert!(a.log_line("en").contains("Fitted into free memory"));
    }

    #[test]
    fn an_unset_chat_context_defaults_to_the_trained_one_capped_at_32k() {
        use crate::prepare::default_chat_context as d;
        assert_eq!(d(Some(131_072)), 32_768);
        assert_eq!(d(Some(32_768)), 32_768);
        assert_eq!(d(Some(8_192)), 8_192);
        assert_eq!(d(Some(0)), 32_768);
        assert_eq!(d(None), 32_768);
    }

    #[test]
    fn the_default_context_leaves_set_contexts_and_non_chat_servers_alone() {
        let mut c = crate::process::LaunchConfig {
            context_size: 8_192,
            ..Default::default()
        };
        assert_eq!(default_context_for(&c), None, "an explicit context stays");
        c.context_size = 0;
        c.custom_args = Some("--ctx-size 2048".into());
        assert_eq!(
            default_context_for(&c),
            None,
            "the user's own arguments win"
        );
        c.custom_args = None;
        c.router_mode = true;
        assert_eq!(default_context_for(&c), None);
    }

    #[test]
    fn an_automatic_ngl_keeps_the_most_layers_that_fit_at_the_floor() {
        // Live (RTX 3050 Ti, 3295 MiB free): Qwen3-4B needed ~3500 MiB at 16k fully offloaded.
        use crate::kv_policy::{KvLevel, CUDA_LADDER};
        let config = crate::process::LaunchConfig {
            context_size: 32_768,
            gpu_layers: 37,
            ..Default::default()
        };
        // MiB per layer: f16 cache 90, q8_0 70; fits below 2500.
        let fits = |c: &crate::process::LaunchConfig| {
            let per = match KvLevel::of(c.cache_type_k.as_deref(), c.cache_type_v.as_deref()) {
                KvLevel::Full => 90,
                _ => 70,
            };
            let need = u64::from(c.gpu_layers) * per;
            (need <= 2500 && c.context_size == FIT_MIN_CONTEXT).then_some(need)
        };
        let (c, a, need) =
            fit_with_fewer_layers(&config, &CUDA_LADDER, FIT_MIN_CONTEXT, fits).unwrap();
        assert_eq!(c.gpu_layers, 35, "q8_0 leaves more layers than f16 (27)");
        assert_eq!(c.cache_type_k.as_deref(), Some("q8_0"));
        assert_eq!(
            (a.context_size, a.requested_context, a.gpu_layers),
            (16_384, 32_768, Some(35))
        );
        assert_eq!(need, 35 * 70);
        assert!(fit_with_fewer_layers(&config, &CUDA_LADDER, FIT_MIN_CONTEXT, |_| None).is_none());
    }

    #[test]
    fn an_unset_context_flows_through_the_fit_ladder_and_shrinks() {
        let mut c = config_65k();
        c.model_path = "model.gguf".into();
        c.context_size = crate::prepare::default_chat_context(Some(131_072));
        assert!(fit_applies(&c), "a defaulted context takes the fit path");
        let headroom = crate::vram::headroom_with(TOTAL_MIB * MIB, 0, 0) / MIB;
        let free = need_mib(&c) + headroom - 1;
        assert!(fits_in(free)(&c).is_none());
        let (fitted, a, _) = fit_to_memory(&c, &KEEP_CACHE, 16_384, fits_in(free)).unwrap();
        assert!(fitted.context_size < 32_768 && fitted.context_size >= 16_384);
        assert_eq!(a.requested_context, 32_768);
    }

    #[test]
    fn the_chosen_default_context_is_logged_in_both_languages() {
        assert!(default_context_log_line(32_768, "tr").contains("bağlam belirtmiyor"));
        assert!(default_context_log_line(32_768, "en").contains("sets no context"));
    }
}
