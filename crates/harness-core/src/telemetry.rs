use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sysinfo::System;

const HISTORY_CAPACITY: usize = 600;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TelemetrySnapshot {
    pub server_online: bool,
    /// Current generation speed (from counter deltas over the last sample; 0 when idle).
    pub predicted_tokens_per_sec: f64,
    /// Current prompt processing speed (from counter deltas; 0 when idle).
    pub prompt_tokens_per_sec: f64,
    /// Server-reported averages since start.
    pub avg_predicted_tokens_per_sec: f64,
    pub avg_prompt_tokens_per_sec: f64,
    /// KV cache usage ratio, when the server still exposes it (older builds).
    pub kv_cache_usage_ratio: f64,
    pub kv_cache_available: bool,
    pub active_slots: usize,
    pub total_slots: usize,
    pub requests_processing: u64,
    pub requests_deferred: u64,
    /// Speculative decoding: accepted / drafted tokens (None when not drafting).
    pub spec_acceptance_rate: Option<f64>,
    pub system_ram_used_bytes: u64,
    pub system_ram_total_bytes: u64,
    /// Where the used system RAM goes; None until a system sample ran.
    pub ram_breakdown: Option<RamBreakdown>,
    pub vram_used_bytes: u64,
    pub vram_total_bytes: u64,
    /// Where the used VRAM goes (same split as the RAM one, from per-process GPU memory
    /// counters). None where the OS has no such counter for the GPU (macOS, NVIDIA on Linux)
    /// or no sample ran: the view then shows an unsplit bar.
    pub vram_breakdown: Option<MemoryBreakdown>,
    /// Every graphics adapter with its own VRAM and split, in adapter order (the first is the
    /// one `vram_*` above describe). Empty with a single adapter: the fields above are all.
    pub gpus: Vec<GpuMemory>,
    /// Context window size and tokens held by busy slots (from /slots); None without slot data.
    pub context_size: Option<u64>,
    pub context_used_tokens: Option<u64>,
    /// Per-slot state, in slot order.
    pub slots: Vec<SlotInfo>,
    /// Counters since the server started (None until /metrics answered).
    pub totals: Option<ServerTotals>,
    /// Latest completed request seen by the poller.
    pub last_request: Option<LastRequest>,
}

/// Used memory (system RAM or VRAM) split into Harness Studio, its llama-server processes
/// and everything else. Process figures are working sets (RAM) or dedicated GPU memory
/// (VRAM); shared RAM pages can be counted in more than one process, so `other_bytes`
/// saturates at 0 instead of going negative.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MemoryBreakdown {
    /// This app's process and its child processes (the WebView2 ones on Windows).
    pub harness_bytes: u64,
    pub servers: Vec<ServerRam>,
    /// Used - Harness Studio - servers.
    pub other_bytes: u64,
}

/// VRAM of one graphics adapter (a machine with several: discrete card plus integrated GPU).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct GpuMemory {
    pub index: u32,
    pub name: String,
    pub used_bytes: u64,
    pub total_bytes: u64,
    /// Used VRAM of this adapter by consumer; None where the OS has no per-process counter.
    pub breakdown: Option<MemoryBreakdown>,
}

/// The RAM split (kept under its first name).
pub type RamBreakdown = MemoryBreakdown;

/// A running llama-server (with its child processes).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ServerRam {
    pub id: String,
    /// Model file name of the run.
    pub name: String,
    pub bytes: u64,
}

/// A llama-server process whose memory is attributed to it.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerProc {
    pub id: String,
    pub name: String,
    pub pid: u32,
}

/// One row of the process table: `(pid, parent pid, working set bytes)`.
pub type ProcRow = (u32, Option<u32>, u64);

/// Which processes belong to whom: each server's process tree, and Harness Studio's own tree
/// without those subtrees.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProcessTrees {
    pub harness: Vec<u32>,
    /// In the order of the servers given.
    pub servers: Vec<Vec<u32>>,
}

impl ProcessTrees {
    /// Every pid that is attributed to something.
    pub fn all_pids(&self) -> Vec<u32> {
        let mut all = self.harness.clone();
        all.extend(self.servers.iter().flatten());
        all
    }
}

/// `own_pid` and its descendants are Harness Studio, except the subtrees of the server
/// processes, which belong to those servers.
pub fn process_trees(own_pid: u32, servers: &[ServerProc], procs: &[ProcRow]) -> ProcessTrees {
    let tree = |root: u32| {
        let mut pids = vec![root];
        let mut i = 0;
        while i < pids.len() {
            let parent = pids[i];
            let next: Vec<u32> = procs
                .iter()
                .filter(|p| p.1 == Some(parent) && !pids.contains(&p.0))
                .map(|p| p.0)
                .collect();
            pids.extend(next);
            i += 1;
        }
        pids
    };
    let mut claimed: Vec<u32> = Vec::new();
    let server_trees: Vec<Vec<u32>> = servers
        .iter()
        .map(|s| {
            let pids = tree(s.pid);
            claimed.extend(&pids);
            pids
        })
        .collect();
    ProcessTrees {
        harness: tree(own_pid)
            .into_iter()
            .filter(|p| !claimed.contains(p))
            .collect(),
        servers: server_trees,
    }
}

/// Splits `used` by process tree, summing `bytes_of(pid)` over each tree. Never negative:
/// `other_bytes` saturates, and sums saturate instead of wrapping.
pub fn attribute(
    used: u64,
    servers: &[ServerProc],
    trees: &ProcessTrees,
    bytes_of: impl Fn(u32) -> u64,
) -> MemoryBreakdown {
    let sum = |pids: &[u32]| {
        pids.iter()
            .fold(0u64, |acc, p| acc.saturating_add(bytes_of(*p)))
    };
    let rows: Vec<ServerRam> = servers
        .iter()
        .zip(&trees.servers)
        .map(|(s, pids)| ServerRam {
            id: s.id.clone(),
            name: s.name.clone(),
            bytes: sum(pids),
        })
        .collect();
    let harness_bytes = sum(&trees.harness);
    let servers_total = rows.iter().fold(0u64, |acc, s| acc.saturating_add(s.bytes));
    MemoryBreakdown {
        harness_bytes,
        servers: rows,
        other_bytes: other_bytes(used, harness_bytes, servers_total),
    }
}

/// RAM split of `used` by working set per process tree. `shared_gpu` (`pid -> bytes`, empty
/// where the OS reports none) is the system RAM the GPU driver holds for a process: not part
/// of its working set, so it is added to the process's share instead of landing in "other".
pub fn ram_breakdown(
    used: u64,
    own_pid: u32,
    servers: &[ServerProc],
    procs: &[ProcRow],
    shared_gpu: &HashMap<u32, u64>,
) -> MemoryBreakdown {
    let trees = process_trees(own_pid, servers, procs);
    let working_set: HashMap<u32, u64> = procs.iter().map(|p| (p.0, p.2)).collect();
    attribute(used, servers, &trees, |pid| {
        working_set
            .get(&pid)
            .copied()
            .unwrap_or(0)
            .saturating_add(shared_gpu.get(&pid).copied().unwrap_or(0))
    })
}

/// VRAM split of `used` by dedicated GPU memory per process tree (`per_pid`: `pid -> bytes`
/// on the monitored adapter).
pub fn vram_breakdown(
    used: u64,
    own_pid: u32,
    servers: &[ServerProc],
    procs: &[ProcRow],
    per_pid: &HashMap<u32, u64>,
) -> MemoryBreakdown {
    let trees = process_trees(own_pid, servers, procs);
    attribute(used, servers, &trees, |pid| {
        per_pid.get(&pid).copied().unwrap_or(0)
    })
}

/// Used RAM not explained by Harness Studio and its servers (never below 0).
pub fn other_bytes(used: u64, harness: u64, servers: u64) -> u64 {
    used.saturating_sub(harness).saturating_sub(servers)
}

/// One slot as reported by /slots.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SlotInfo {
    pub id: i64,
    pub busy: bool,
    pub prompt_tokens: u64,
    pub decoded_tokens: u64,
}

/// Server-side counters since start (`/metrics`).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ServerTotals {
    pub prompt_tokens: u64,
    pub cached_prompt_tokens: Option<u64>,
    pub generated_tokens: u64,
    pub prompt_seconds: f64,
    pub generation_seconds: f64,
    /// Largest sequence length (prompt + generation) seen so far.
    pub max_sequence_tokens: Option<u64>,
    /// Requests the poller saw complete (counters only move when a request finishes,
    /// so requests finishing within the same second count once).
    pub requests_observed: u64,
}

/// Work done by the request(s) that finished between two samples.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LastRequest {
    pub finished_at: u64,
    pub prompt_tokens: u64,
    pub cached_tokens: u64,
    pub generated_tokens: u64,
    /// Time spent on the prompt: the closest thing to time-to-first-token /metrics offers.
    pub prompt_seconds: f64,
    pub generation_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TelemetryHistoryPoint {
    pub timestamp: u64,
    pub predicted_tokens_per_sec: f64,
    pub prompt_tokens_per_sec: f64,
    pub vram_used_bytes: u64,
    /// This server's dedicated VRAM across adapters; None without a breakdown.
    #[serde(default)]
    pub server_vram_used_bytes: Option<u64>,
    pub vram_total_bytes: u64,
    pub ram_used_bytes: u64,
    /// This server's RAM share; None without a breakdown.
    #[serde(default)]
    pub server_ram_used_bytes: Option<u64>,
    pub ram_total_bytes: u64,
    pub active_slots: usize,
    pub total_slots: usize,
    /// Share of the context window in use; None when the server exposes no such data.
    pub kv_usage_ratio: Option<f64>,
    pub requests_processing: u64,
    pub requests_deferred: u64,
}

/// This server's bytes in a split; None when the split does not list it (not sampled yet).
fn server_memory_bytes(breakdown: &MemoryBreakdown, server_id: &str) -> Option<u64> {
    breakdown
        .servers
        .iter()
        .filter(|server| server.id == server_id)
        .map(|server| server.bytes)
        .reduce(u64::saturating_add)
}

fn server_vram_bytes(snapshot: &TelemetrySnapshot, server_id: &str) -> Option<u64> {
    if snapshot.gpus.is_empty() {
        snapshot
            .vram_breakdown
            .as_ref()
            .and_then(|breakdown| server_memory_bytes(breakdown, server_id))
    } else {
        snapshot
            .gpus
            .iter()
            .filter_map(|gpu| gpu.breakdown.as_ref())
            .filter_map(|breakdown| server_memory_bytes(breakdown, server_id))
            .reduce(u64::saturating_add)
    }
}

/// Raw Prometheus values keyed by metric name (without the `llamacpp:` prefix).
pub type Metrics = HashMap<String, f64>;

/// Per-server state: counter deltas need the previous sample, history is kept per server.
#[derive(Default)]
struct ServerTelemetry {
    /// Sampled in the last round (starting or running).
    live: bool,
    history: VecDeque<TelemetryHistoryPoint>,
    last: TelemetrySnapshot,
    prev_metrics: Option<Metrics>,
    /// Per-slot progress from the previous /slots sample (live speed during long requests).
    prev_slots: Option<(Instant, HashMap<i64, SlotProgress>)>,
    last_request: Option<LastRequest>,
    requests_observed: u64,
}

/// Progress of the task a slot is working on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlotProgress {
    pub task: i64,
    pub prompt_done: f64,
    pub decoded: f64,
}

pub struct TelemetryPoller {
    http_client: Client,
    primary_gpu: Option<crate::gpu::GpuAdapter>,
    servers: HashMap<String, ServerTelemetry>,
    /// Latest system-wide sample (RAM/VRAM), shared by all servers.
    system: TelemetrySnapshot,
    /// When `system` was sampled (None = never).
    system_at: Option<Instant>,
    probe: Arc<std::sync::Mutex<SystemProbe>>,
}

/// What a system sample reuses between rounds: sysinfo's `System` (memory only) and the open
/// VRAM query (see [`crate::gpu::VramMonitor`]).
pub struct SystemProbe {
    sys: System,
    /// llama-server processes of the last round (a sample without a round reuses them).
    targets: Vec<ServerProc>,
    /// One monitor per adapter, the primary first.
    vram: Vec<crate::gpu::VramMonitor>,
    shared: Option<crate::gpu::SharedProbe>,
    shared_opened: bool,
    /// A sample has completed (later ones only refresh what is open).
    warm: bool,
}

impl SystemProbe {
    fn new(adapters: Vec<crate::gpu::GpuAdapter>) -> Self {
        Self {
            sys: System::new(),
            targets: Vec::new(),
            vram: adapters
                .into_iter()
                .map(crate::gpu::VramMonitor::new)
                .collect(),
            shared: None,
            shared_opened: false,
            warm: false,
        }
    }

    /// Shared GPU memory per process (Windows), one read of the OS counter.
    fn shared_gpu(&mut self) -> HashMap<u32, u64> {
        if !self.shared_opened {
            self.shared_opened = true;
            self.shared = crate::gpu::SharedProbe::open();
        }
        self.shared
            .as_mut()
            .and_then(|p| p.sample())
            .unwrap_or_default()
    }

    /// Reads only the working sets (no CPU, disk or command lines) of the process table.
    fn process_table(&mut self) -> Vec<ProcRow> {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate};
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            // On Linux every thread is otherwise listed as a process with the whole RSS.
            ProcessRefreshKind::nothing().with_memory().without_tasks(),
        );
        self.sys
            .processes()
            .iter()
            .filter(|(_, p)| p.thread_kind().is_none())
            .map(|(pid, p)| (pid.as_u32(), p.parent().map(|p| p.as_u32()), p.memory()))
            .collect()
    }

    /// System RAM and the VRAM of every adapter, now.
    fn sample(&mut self) -> TelemetrySnapshot {
        let mut snapshot = TelemetrySnapshot::default();
        self.sys.refresh_memory();
        snapshot.system_ram_total_bytes = self.sys.total_memory();
        snapshot.system_ram_used_bytes = self.sys.used_memory();
        let procs = self.process_table();
        let own_pid = std::process::id();
        let shared_gpu = self.shared_gpu();
        snapshot.ram_breakdown = Some(ram_breakdown(
            snapshot.system_ram_used_bytes,
            own_pid,
            &self.targets,
            &procs,
            &shared_gpu,
        ));
        let pids = process_trees(own_pid, &self.targets, &procs).all_pids();
        let multi = self.vram.len() > 1;
        for (i, vram) in self.vram.iter_mut().enumerate() {
            let adapter = vram.adapter();
            let mut gpu = GpuMemory {
                index: adapter.index,
                name: adapter.name.clone(),
                total_bytes: adapter.dedicated_vram.unwrap_or(0),
                ..GpuMemory::default()
            };
            if let Some((used, total)) = vram.usage() {
                gpu.used_bytes = used;
                gpu.total_bytes = total;
                // One more read of an OS counter on the same tick (no extra timer).
                gpu.breakdown = vram
                    .process_usage(&pids)
                    .map(|per_pid| vram_breakdown(used, own_pid, &self.targets, &procs, &per_pid));
            }
            if i == 0 {
                snapshot.vram_used_bytes = gpu.used_bytes;
                snapshot.vram_total_bytes = gpu.total_bytes;
                snapshot.vram_breakdown = gpu.breakdown.clone();
            }
            if multi {
                snapshot.gpus.push(gpu);
            }
        }
        self.warm = true;
        snapshot
    }
}

/// Samples system memory on a blocking thread. `None` when it took longer than 800 ms (the first
/// sample of a probe: 5 s) or a previous sample still holds the probe (the caller keeps the last
/// value).
async fn sample_system(
    probe: Arc<std::sync::Mutex<SystemProbe>>,
    targets: Option<Vec<ServerProc>>,
) -> Option<TelemetrySnapshot> {
    // The first sample reads the whole process table into a new `System` and opens the OS GPU
    // counters: ~0.3 s when idle, over 1.2 s on a loaded machine. Cut off at 800 ms, its finished
    // result was dropped and the view showed 0 B until a later sample.
    let cold = probe.try_lock().is_ok_and(|p| !p.warm);
    tokio::time::timeout(
        Duration::from_millis(if cold { 5000 } else { 800 }),
        tokio::task::spawn_blocking(move || {
            probe.try_lock().ok().map(|mut p| {
                if let Some(targets) = targets {
                    p.targets = targets;
                }
                p.sample()
            })
        }),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .flatten()
}

/// System memory moves a little all the time; a view is only told about steps this big.
const MEMORY_STEP: u64 = 64 * 1024 * 1024;

/// Whether `next` differs from `prev` in anything a view shows: any server field, or system
/// RAM / VRAM by at least [`MEMORY_STEP`]. Lets a front end push telemetry only on change.
pub fn material_change(prev: &TelemetrySnapshot, next: &TelemetrySnapshot) -> bool {
    let moved = |a: u64, b: u64| a.abs_diff(b) >= MEMORY_STEP;
    if moved(prev.system_ram_used_bytes, next.system_ram_used_bytes)
        || moved(prev.vram_used_bytes, next.vram_used_bytes)
    {
        return true;
    }
    if breakdown_moved(prev.ram_breakdown.as_ref(), next.ram_breakdown.as_ref())
        || breakdown_moved(prev.vram_breakdown.as_ref(), next.vram_breakdown.as_ref())
        || gpus_moved(&prev.gpus, &next.gpus)
    {
        return true;
    }
    let strip = |s: &TelemetrySnapshot| TelemetrySnapshot {
        ram_breakdown: None,
        vram_breakdown: None,
        gpus: Vec::new(),
        system_ram_used_bytes: 0,
        vram_used_bytes: 0,
        ..s.clone()
    };
    strip(prev) != strip(next)
}

/// An adapter joined or left, or one's used VRAM or split moved by a step.
fn gpus_moved(a: &[GpuMemory], b: &[GpuMemory]) -> bool {
    a.len() != b.len()
        || a.iter().zip(b).any(|(x, y)| {
            x.index != y.index
                || x.total_bytes != y.total_bytes
                || x.used_bytes.abs_diff(y.used_bytes) >= MEMORY_STEP
                || breakdown_moved(x.breakdown.as_ref(), y.breakdown.as_ref())
        })
}

/// The breakdown appeared or vanished, a server joined or left, or any part moved by a step.
fn breakdown_moved(a: Option<&MemoryBreakdown>, b: Option<&MemoryBreakdown>) -> bool {
    let (a, b) = match (a, b) {
        (None, None) => return false,
        (Some(a), Some(b)) => (a, b),
        _ => return true,
    };
    a.harness_bytes.abs_diff(b.harness_bytes) >= MEMORY_STEP
        || a.other_bytes.abs_diff(b.other_bytes) >= MEMORY_STEP
        || a.servers.len() != b.servers.len()
        || a.servers.iter().zip(&b.servers).any(|(x, y)| {
            x.id != y.id || x.name != y.name || x.bytes.abs_diff(y.bytes) >= MEMORY_STEP
        })
}

/// What one server returned in a sampling round.
struct ServerSample {
    online: bool,
    metrics: Option<Metrics>,
    slots: Option<serde_json::Value>,
}

pub struct TelemetryRound {
    /// None: the system sample failed or timed out (the last one is kept).
    system: Option<TelemetrySnapshot>,
    servers: Vec<(String, ServerSample)>,
}

impl TelemetryPoller {
    pub fn new(primary_gpu: Option<crate::gpu::GpuAdapter>) -> Self {
        Self::with_adapters(primary_gpu.into_iter().collect())
    }

    /// Samples the VRAM of every adapter (the first is the primary one).
    pub fn with_adapters(adapters: Vec<crate::gpu::GpuAdapter>) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_millis(800))
            .build()
            .unwrap_or_default();

        Self {
            http_client,
            probe: Arc::new(std::sync::Mutex::new(SystemProbe::new(adapters.clone()))),
            primary_gpu: adapters.into_iter().next(),
            servers: HashMap::new(),
            system: TelemetrySnapshot::default(),
            system_at: None,
        }
    }

    /// Most recent sample of a server (server fields zeroed if the server isn't running).
    /// System fields are as fresh as the last round; see [`Self::fresh_snapshot`].
    pub fn last(&self, server_id: &str) -> TelemetrySnapshot {
        match self.servers.get(server_id) {
            Some(s) if s.live => s.last.clone(),
            _ => self.system.clone(),
        }
    }

    /// [`Self::last`] with system RAM/VRAM sampled now when no round ran in the last 1.5 s
    /// (the background sampler rests while no server runs and no view watches).
    pub async fn fresh_snapshot(
        poller: &Arc<tokio::sync::Mutex<Self>>,
        server_id: &str,
    ) -> TelemetrySnapshot {
        let probe = {
            let p = poller.lock().await;
            let fresh = p
                .system_at
                .is_some_and(|t| t.elapsed() < Duration::from_millis(1500));
            (!fresh).then(|| p.probe.clone())
        };
        if let Some(probe) = probe {
            if let Some(system) = sample_system(probe, None).await {
                poller.lock().await.set_system(system);
            }
        }
        poller.lock().await.last(server_id)
    }

    /// Sets the GPU whose VRAM is sampled (found after start, off the startup path).
    pub fn set_primary_gpu(&mut self, primary_gpu: Option<crate::gpu::GpuAdapter>) {
        self.set_adapters(primary_gpu.into_iter().collect());
    }

    /// Sets the GPUs whose VRAM is sampled, the primary first.
    pub fn set_adapters(&mut self, adapters: Vec<crate::gpu::GpuAdapter>) {
        self.probe = Arc::new(std::sync::Mutex::new(SystemProbe::new(adapters.clone())));
        self.primary_gpu = adapters.into_iter().next();
        self.system_at = None;
    }

    /// When system memory was last sampled (None = never).
    pub fn system_sampled_at(&self) -> Option<Instant> {
        self.system_at
    }

    fn set_system(&mut self, system: TelemetrySnapshot) {
        self.system = system;
        self.system_at = Some(Instant::now());
    }

    /// Some server is working on a request (busy slot, request processing or waiting) as of
    /// the last round.
    pub fn any_busy(&self) -> bool {
        self.servers.values().any(|s| {
            s.live
                && (s.last.active_slots > 0
                    || s.last.requests_processing > 0
                    || s.last.requests_deferred > 0)
        })
    }

    /// Ids of the servers sampled in the last round, sorted.
    pub fn live_server_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .servers
            .iter()
            .filter(|(_, s)| s.live)
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        ids
    }

    /// Some server was sampled in the last round (its live values are still shown).
    pub fn any_live(&self) -> bool {
        self.servers.values().any(|s| s.live)
    }

    pub fn sampling_context(&self) -> (Client, Option<crate::gpu::GpuAdapter>) {
        (self.http_client.clone(), self.primary_gpu.clone())
    }

    /// What [`Self::gather`] needs: the HTTP client and the reusable system probe.
    pub fn round_context(&self) -> (Client, Arc<std::sync::Mutex<SystemProbe>>) {
        (self.http_client.clone(), self.probe.clone())
    }

    #[cfg(test)]
    pub async fn poll(&mut self, servers: &[(String, crate::process::ActiveServerInfo)]) {
        let (client, probe) = self.round_context();
        let round = Self::gather(client, probe, servers.to_vec(), Vec::new()).await;
        self.apply_round(round);
    }

    async fn fetch_server(
        client: Client,
        server: crate::process::ActiveServerInfo,
    ) -> ServerSample {
        let base = format!("http://{}:{}", server.connect_host, server.port);
        // whisper-server, sd-server and harness-onnx have no `/metrics` or `/slots`: they are
        // online while their health route answers.
        if !server.is_llama() {
            let engine = if server.serve_mode == crate::serve_mode::ServeMode::Image {
                crate::engine::sd_cpp()
            } else {
                crate::engine::server_for_model(&server.model_path)
                    .unwrap_or_else(crate::engine::whisper_cpp)
            };
            let online = client
                .get(format!("{}{}", base, engine.health_path()))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success());
            return ServerSample {
                online,
                metrics: None,
                slots: None,
            };
        }
        let engine = crate::engine::llama_cpp();
        let source = engine.metrics_source();
        let auth = |req: reqwest::RequestBuilder| match &server.api_key {
            Some(k) => req.bearer_auth(k),
            None => req,
        };
        let slots_url = source.slots_path.map(|p| format!("{}{}", base, p));
        let (metrics_res, slots_res) = tokio::join!(
            auth(client.get(format!("{}{}", base, source.metrics_path))).send(),
            async {
                match &slots_url {
                    Some(u) => Some(auth(client.get(u)).send().await),
                    None => None,
                }
            }
        );

        let mut sample = ServerSample {
            online: false,
            metrics: None,
            slots: None,
        };
        if let Ok(resp) = metrics_res {
            if resp.status().is_success() {
                sample.online = true;
                if let Ok(text) = resp.text().await {
                    sample.metrics = Some(engine.parse_metrics(&text));
                }
            }
        }
        if let Some(Ok(resp)) = slots_res {
            if resp.status().is_success() {
                sample.online = true;
                sample.slots = resp.json::<serde_json::Value>().await.ok();
            }
        }
        sample
    }

    /// Samples system memory/VRAM once and every given server concurrently; `processes` are
    /// the llama-server processes whose RAM is listed apart.
    /// History is only recorded while a server is online, so idle periods don't dilute it.
    pub async fn gather(
        client: Client,
        probe: Arc<std::sync::Mutex<SystemProbe>>,
        servers: Vec<(String, crate::process::ActiveServerInfo)>,
        processes: Vec<ServerProc>,
    ) -> TelemetryRound {
        let (system, samples) = tokio::join!(
            sample_system(probe, Some(processes)),
            futures_util::future::join_all(
                servers
                    .iter()
                    .map(|(_, info)| Self::fetch_server(client.clone(), info.clone())),
            )
        );
        TelemetryRound {
            system,
            servers: servers
                .into_iter()
                .zip(samples)
                .map(|((id, _), sample)| (id, sample))
                .collect(),
        }
    }

    pub fn apply_round(&mut self, round: TelemetryRound) {
        // A failed system sample keeps the last values.
        if let Some(system) = round.system {
            self.set_system(system);
        }

        // Stopped servers keep their history (Performance view) but lose live values.
        for (id, state) in self.servers.iter_mut() {
            if !round.servers.iter().any(|(active, _)| active == id) {
                state.live = false;
                state.prev_metrics = None;
                state.prev_slots = None;
                state.last = TelemetrySnapshot::default();
            }
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        for (id, sample) in round.servers {
            let state = self.servers.entry(id.clone()).or_default();
            state.live = true;
            let mut snapshot = self.system.clone();
            snapshot.server_online = sample.online;
            if let Some(metrics) = sample.metrics {
                apply_metrics(&metrics, state.prev_metrics.as_ref(), &mut snapshot);
                if let Some(prev) = &state.prev_metrics {
                    if let Some(request) = detect_request(&metrics, prev, now) {
                        state.last_request = Some(request);
                        state.requests_observed += 1;
                    } else if counters_reset(&metrics, prev) {
                        state.last_request = None;
                        state.requests_observed = 0;
                    }
                }
                let mut totals = server_totals(&metrics);
                totals.requests_observed = state.requests_observed;
                snapshot.totals = Some(totals);
                state.prev_metrics = Some(metrics);
            }
            snapshot.last_request = state.last_request.clone();
            if let Some(slots) = &sample.slots {
                parse_slots_json(slots, &mut snapshot);
                // llama-server only updates its /metrics counters when a request finishes, so a
                // long prompt would show 0 t/s for minutes. Use live slot progress instead.
                let progress = slot_progress(slots);
                let now_instant = Instant::now();
                if let Some((then, prev)) = &state.prev_slots {
                    let secs = now_instant.duration_since(*then).as_secs_f64();
                    let (prompt, gen) = live_rates(prev, &progress, secs);
                    if prompt > 0.0 {
                        snapshot.prompt_tokens_per_sec = prompt;
                    }
                    if gen > 0.0 {
                        snapshot.predicted_tokens_per_sec = gen;
                    }
                }
                snapshot.slots = slot_infos(slots);
                if let Some((used, n_ctx)) = context_tokens(slots) {
                    snapshot.context_size = Some(n_ctx as u64);
                    snapshot.context_used_tokens = Some(used as u64);
                }
                if let Some(ratio) = context_usage(slots) {
                    if !snapshot.kv_cache_available {
                        snapshot.kv_cache_usage_ratio = ratio;
                        snapshot.kv_cache_available = true;
                    }
                }
                state.prev_slots = Some((now_instant, progress));
            }

            if snapshot.server_online {
                if state.history.len() >= HISTORY_CAPACITY {
                    state.history.pop_front();
                }
                state.history.push_back(TelemetryHistoryPoint {
                    timestamp: now,
                    predicted_tokens_per_sec: snapshot.predicted_tokens_per_sec,
                    prompt_tokens_per_sec: snapshot.prompt_tokens_per_sec,
                    vram_used_bytes: snapshot.vram_used_bytes,
                    server_vram_used_bytes: server_vram_bytes(&snapshot, &id),
                    vram_total_bytes: snapshot.vram_total_bytes,
                    ram_used_bytes: snapshot.system_ram_used_bytes,
                    server_ram_used_bytes: snapshot
                        .ram_breakdown
                        .as_ref()
                        .and_then(|breakdown| server_memory_bytes(breakdown, &id)),
                    ram_total_bytes: snapshot.system_ram_total_bytes,
                    active_slots: snapshot.active_slots,
                    total_slots: snapshot.total_slots,
                    kv_usage_ratio: snapshot
                        .kv_cache_available
                        .then_some(snapshot.kv_cache_usage_ratio),
                    requests_processing: snapshot.requests_processing,
                    requests_deferred: snapshot.requests_deferred,
                });
            }
            state.last = snapshot;
        }
    }

    /// Latest raw `/metrics` of every server that answered the last round (unified
    /// Prometheus exposition, `analytics::prometheus`).
    pub fn raw_metrics(&self) -> Vec<(String, Metrics)> {
        let mut out: Vec<(String, Metrics)> = self
            .servers
            .iter()
            .filter_map(|(id, s)| s.prev_metrics.clone().map(|m| (id.clone(), m)))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Drops everything recorded for a removed server.
    pub fn forget(&mut self, server_id: &str) {
        self.servers.remove(server_id);
    }

    pub fn get_history(&self, server_id: &str, limit: Option<usize>) -> Vec<TelemetryHistoryPoint> {
        let Some(state) = self.servers.get(server_id) else {
            return Vec::new();
        };
        let max_items = limit.unwrap_or(HISTORY_CAPACITY);
        let skip = state.history.len().saturating_sub(max_items);
        state.history.iter().skip(skip).cloned().collect()
    }
}

/// Parses Prometheus text format into `name -> value` (labels ignored, `llamacpp:` prefix stripped).
pub fn parse_prometheus(text: &str) -> Metrics {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let name = parts.next()?;
            let value = parts.last()?.parse::<f64>().ok()?;
            let name = name.split('{').next()?.trim_start_matches("llamacpp:");
            Some((name.to_string(), value))
        })
        .collect()
}

/// Rate of `tokens` per `seconds` between two samples of monotonically increasing counters.
fn counter_rate(cur: &Metrics, prev: Option<&Metrics>, tokens: &str, seconds: &str) -> Option<f64> {
    let prev = prev?;
    let d_tok = cur.get(tokens)? - prev.get(tokens)?;
    let d_sec = cur.get(seconds)? - prev.get(seconds)?;
    if d_tok < 0.0 || d_sec < 0.0 {
        return None; // server restarted, counters reset
    }
    Some(if d_sec > 0.0 { d_tok / d_sec } else { 0.0 })
}

pub fn apply_metrics(cur: &Metrics, prev: Option<&Metrics>, snapshot: &mut TelemetrySnapshot) {
    let get = |k: &str| cur.get(k).copied().unwrap_or(0.0);
    snapshot.avg_predicted_tokens_per_sec = get("predicted_tokens_seconds");
    snapshot.avg_prompt_tokens_per_sec = get("prompt_tokens_seconds");
    snapshot.predicted_tokens_per_sec = counter_rate(
        cur,
        prev,
        "tokens_predicted_total",
        "tokens_predicted_seconds_total",
    )
    .unwrap_or(0.0);
    snapshot.prompt_tokens_per_sec =
        counter_rate(cur, prev, "prompt_tokens_total", "prompt_seconds_total").unwrap_or(0.0);
    if let Some(v) = cur.get("kv_cache_usage_ratio") {
        snapshot.kv_cache_usage_ratio = *v;
        snapshot.kv_cache_available = true;
    }
    snapshot.requests_processing = get("requests_processing") as u64;
    snapshot.requests_deferred = get("requests_deferred") as u64;
    let drafted = get("spec_decode_num_draft_tokens_total");
    if drafted > 0.0 {
        snapshot.spec_acceptance_rate =
            Some(get("spec_decode_num_accepted_tokens_total") / drafted);
    }
}

pub fn parse_slots_json(json: &serde_json::Value, snapshot: &mut TelemetrySnapshot) {
    if let Some(array) = json.as_array() {
        snapshot.total_slots = array.len();
        snapshot.active_slots = array
            .iter()
            .filter(|slot| {
                let is_processing = slot
                    .get("is_processing")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let state = slot.get("state").and_then(|v| v.as_i64()).unwrap_or(0);
                is_processing || state == 1 // older builds: state 1 = processing
            })
            .count();
    }
}

fn num(v: Option<&serde_json::Value>) -> Option<f64> {
    v.and_then(|v| v.as_f64())
}

/// Progress of every busy slot, keyed by slot id.
pub fn slot_progress(json: &serde_json::Value) -> HashMap<i64, SlotProgress> {
    let mut out = HashMap::new();
    for slot in json.as_array().into_iter().flatten() {
        if !slot
            .get("is_processing")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            continue;
        }
        let (Some(id), Some(task)) = (
            slot.get("id").and_then(|v| v.as_i64()),
            slot.get("id_task").and_then(|v| v.as_i64()),
        ) else {
            continue;
        };
        let decoded = slot
            .get("next_token")
            .and_then(|t| t.as_array().and_then(|a| a.first()).or(Some(t)))
            .and_then(|t| num(t.get("n_decoded")))
            .unwrap_or(0.0);
        out.insert(
            id,
            SlotProgress {
                task,
                prompt_done: num(slot.get("n_prompt_tokens_processed")).unwrap_or(0.0),
                decoded,
            },
        );
    }
    out
}

/// Prompt and generation speed across all slots between two samples `secs` apart.
/// Only tasks seen in both samples count, so a finished-and-replaced task never inflates the rate.
pub fn live_rates(
    prev: &HashMap<i64, SlotProgress>,
    cur: &HashMap<i64, SlotProgress>,
    secs: f64,
) -> (f64, f64) {
    if secs <= 0.05 {
        return (0.0, 0.0);
    }
    let (mut prompt, mut gen) = (0.0, 0.0);
    for (id, now) in cur {
        if let Some(before) = prev.get(id).filter(|b| b.task == now.task) {
            prompt += (now.prompt_done - before.prompt_done).max(0.0);
            gen += (now.decoded - before.decoded).max(0.0);
        }
    }
    (prompt / secs, gen / secs)
}

/// Tokens held by busy slots and the context window size `(used, n_ctx)`.
/// Slots share one unified cache, so usage is summed.
pub fn context_tokens(json: &serde_json::Value) -> Option<(f64, f64)> {
    let slots = json.as_array()?;
    let n_ctx = slots.iter().find_map(|s| num(s.get("n_ctx")))?;
    if n_ctx <= 0.0 {
        return None;
    }
    let used: f64 = slots
        .iter()
        .filter(|s| is_busy(s))
        .map(|s| num(s.get("n_prompt_tokens")).unwrap_or(0.0) + decoded_tokens(s))
        .sum();
    Some((used.min(n_ctx), n_ctx))
}

/// Share of the context window used by busy slots (newer llama-server builds dropped the
/// kv_cache_usage_ratio metric).
pub fn context_usage(json: &serde_json::Value) -> Option<f64> {
    context_tokens(json).map(|(used, n_ctx)| used / n_ctx)
}

fn is_busy(slot: &serde_json::Value) -> bool {
    slot.get("is_processing")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn decoded_tokens(slot: &serde_json::Value) -> f64 {
    slot.get("next_token")
        .and_then(|t| t.as_array().and_then(|a| a.first()).or(Some(t)))
        .and_then(|t| num(t.get("n_decoded")))
        .unwrap_or(0.0)
}

/// Per-slot state for the Performance view.
pub fn slot_infos(json: &serde_json::Value) -> Vec<SlotInfo> {
    json.as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, s)| {
            let busy = is_busy(s);
            SlotInfo {
                id: s.get("id").and_then(|v| v.as_i64()).unwrap_or(i as i64),
                busy,
                prompt_tokens: if busy {
                    num(s.get("n_prompt_tokens")).unwrap_or(0.0) as u64
                } else {
                    0
                },
                decoded_tokens: if busy { decoded_tokens(s) as u64 } else { 0 },
            }
        })
        .collect()
}

/// Counter totals since the server started.
pub fn server_totals(m: &Metrics) -> ServerTotals {
    let get = |k: &str| m.get(k).copied().unwrap_or(0.0);
    ServerTotals {
        prompt_tokens: get("prompt_tokens_total") as u64,
        cached_prompt_tokens: m.get("prompt_tokens_cached_total").map(|v| *v as u64),
        generated_tokens: get("tokens_predicted_total") as u64,
        prompt_seconds: get("prompt_seconds_total"),
        generation_seconds: get("tokens_predicted_seconds_total"),
        max_sequence_tokens: m.get("n_tokens_max").map(|v| *v as u64),
        requests_observed: 0,
    }
}

/// The server restarted (or was replaced): its counters went backwards.
fn counters_reset(cur: &Metrics, prev: &Metrics) -> bool {
    ["prompt_tokens_total", "tokens_predicted_total"]
        .iter()
        .any(|k| matches!((cur.get(*k), prev.get(*k)), (Some(c), Some(p)) if c < p))
}

/// llama-server bumps its counters when a request finishes, so growth between two samples
/// is the work of the request(s) that just completed.
pub fn detect_request(cur: &Metrics, prev: &Metrics, now: u64) -> Option<LastRequest> {
    if counters_reset(cur, prev) {
        return None;
    }
    let delta = |k: &str| {
        (cur.get(k).copied().unwrap_or(0.0) - prev.get(k).copied().unwrap_or(0.0)).max(0.0)
    };
    let (prompt, generated) = (
        delta("prompt_tokens_total"),
        delta("tokens_predicted_total"),
    );
    if prompt <= 0.0 && generated <= 0.0 {
        return None;
    }
    Some(LastRequest {
        finished_at: now,
        prompt_tokens: prompt as u64,
        cached_tokens: delta("prompt_tokens_cached_total") as u64,
        generated_tokens: generated as u64,
        prompt_seconds: delta("prompt_seconds_total"),
        generation_seconds: delta("tokens_predicted_seconds_total"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_share(bytes: u64) -> MemoryBreakdown {
        MemoryBreakdown {
            servers: vec![
                ServerRam {
                    id: "target".into(),
                    name: "small.gguf".into(),
                    bytes,
                },
                ServerRam {
                    id: "other".into(),
                    name: "large.gguf".into(),
                    bytes: 14 << 30,
                },
            ],
            ..Default::default()
        }
    }

    fn recorded_point(snapshot: TelemetrySnapshot) -> TelemetryHistoryPoint {
        let mut poller = TelemetryPoller::new(None);
        poller.apply_round(TelemetryRound {
            system: Some(snapshot),
            servers: vec![(
                "target".into(),
                ServerSample {
                    online: true,
                    metrics: None,
                    slots: None,
                },
            )],
        });
        poller.get_history("target", None).pop().unwrap()
    }

    #[test]
    fn history_peak_uses_server_memory_share() {
        let point = recorded_point(TelemetrySnapshot {
            vram_used_bytes: 14 << 30,
            system_ram_used_bytes: 32 << 30,
            vram_breakdown: Some(memory_share(600 << 20)),
            ram_breakdown: Some(memory_share(900 << 20)),
            ..Default::default()
        });
        assert_eq!(point.server_vram_used_bytes, Some(600 << 20));
        assert_eq!(point.server_ram_used_bytes, Some(900 << 20));
        let start = point.timestamp * 1000;
        assert_eq!(
            crate::analytics::peak_in_window(&[point], start, 0),
            Some((600 << 20, 900 << 20))
        );
    }

    #[test]
    fn history_peak_without_breakdown_uses_device_memory() {
        let point = recorded_point(TelemetrySnapshot {
            vram_used_bytes: 14 << 30,
            system_ram_used_bytes: 32 << 30,
            ..Default::default()
        });
        assert_eq!(point.server_vram_used_bytes, None);
        assert_eq!(point.server_ram_used_bytes, None);
        // Old serialized history points also have no server shares.
        let mut legacy = serde_json::to_value(&point).unwrap();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("server_vram_used_bytes");
        legacy
            .as_object_mut()
            .unwrap()
            .remove("server_ram_used_bytes");
        let legacy: TelemetryHistoryPoint = serde_json::from_value(legacy).unwrap();
        assert_eq!(legacy.server_vram_used_bytes, None);
        assert_eq!(legacy.server_ram_used_bytes, None);
        let start = point.timestamp * 1000;
        assert_eq!(
            crate::analytics::peak_in_window(&[legacy], start, 0),
            Some((14 << 30, 32 << 30))
        );
    }

    #[test]
    fn history_sums_server_vram_across_adapters() {
        let gpu = |index, breakdown| GpuMemory {
            index,
            name: format!("GPU {index}"),
            used_bytes: 14 << 30,
            total_bytes: 24 << 30,
            breakdown,
        };
        let mut snapshot = TelemetrySnapshot {
            vram_breakdown: Some(memory_share(999 << 20)),
            gpus: vec![
                gpu(0, Some(memory_share(400 << 20))),
                gpu(1, Some(memory_share(200 << 20))),
                gpu(2, None),
            ],
            ..Default::default()
        };
        let point = recorded_point(snapshot.clone());
        assert_eq!(point.server_vram_used_bytes, Some(600 << 20));
        // A server the split does not list yet has no share (not a measured 0).
        assert_eq!(server_vram_bytes(&snapshot, "missing"), None);
        for gpu in &mut snapshot.gpus {
            gpu.breakdown = None;
        }
        assert_eq!(server_vram_bytes(&snapshot, "target"), None);
    }

    const SAMPLE: &str = r#"
# HELP llamacpp:prompt_tokens_total Number of prompt tokens processed.
# TYPE llamacpp:prompt_tokens_total counter
llamacpp:prompt_tokens_total 100
llamacpp:prompt_seconds_total 0.5
llamacpp:tokens_predicted_total 200
llamacpp:tokens_predicted_seconds_total 10
llamacpp:prompt_tokens_seconds 148.5
llamacpp:predicted_tokens_seconds 27.8
llamacpp:requests_processing 1
llamacpp:requests_deferred 2
llamacpp:spec_decode_num_draft_tokens_total 0
"#;

    #[test]
    fn material_change_ignores_small_memory_moves() {
        let base = TelemetrySnapshot {
            system_ram_used_bytes: 8 << 30,
            system_ram_total_bytes: 32 << 30,
            vram_used_bytes: 2 << 30,
            vram_total_bytes: 16 << 30,
            ..Default::default()
        };
        let mut small = base.clone();
        small.system_ram_used_bytes += 10 << 20;
        small.vram_used_bytes -= 5 << 20;
        assert!(!material_change(&base, &small));

        let mut ram = base.clone();
        ram.system_ram_used_bytes += MEMORY_STEP;
        assert!(material_change(&base, &ram));

        let mut vram = base.clone();
        vram.vram_used_bytes -= MEMORY_STEP;
        assert!(material_change(&base, &vram));

        // Any server field counts, however small.
        let mut tps = base.clone();
        tps.predicted_tokens_per_sec = 0.1;
        assert!(material_change(&base, &tps));
        let mut online = base.clone();
        online.server_online = true;
        assert!(material_change(&base, &online));
    }

    #[test]
    fn other_ram_is_used_minus_harness_minus_servers_saturating() {
        assert_eq!(other_bytes(10 << 30, 1 << 30, 4 << 30), 5 << 30);
        assert_eq!(other_bytes(2 << 30, 1 << 30, 4 << 30), 0, "no underflow");
        assert_eq!(other_bytes(0, 0, 0), 0);
    }

    #[test]
    fn breakdown_splits_by_process_tree() {
        // 1 = us, 2 = webview (ours), 3 = its renderer, 10 = llama-server (our child) with
        // child 11, 20 = unrelated.
        let procs: Vec<ProcRow> = vec![
            (1, Some(0), 100),
            (2, Some(1), 50),
            (3, Some(2), 30),
            (10, Some(1), 700),
            (11, Some(10), 5),
            (20, Some(0), 400),
        ];
        let servers = [ServerProc {
            id: "a".into(),
            name: "m.gguf".into(),
            pid: 10,
        }];
        let b = ram_breakdown(1500, 1, &servers, &procs, &HashMap::new());
        assert_eq!(b.harness_bytes, 180, "server subtree is not ours");
        assert_eq!(b.servers.len(), 1);
        assert_eq!(b.servers[0].bytes, 705);
        assert_eq!(b.other_bytes, 1500 - 180 - 705);
        // used smaller than the parts: saturates
        assert_eq!(
            ram_breakdown(10, 1, &servers, &procs, &HashMap::new()).other_bytes,
            0
        );
    }

    #[test]
    fn vram_split_attributes_per_server_and_saturates() {
        // 1 = us (+ webview 2), 10 and 30 = two servers (30 has a child 31), 20 = unrelated.
        let procs: Vec<ProcRow> = vec![
            (1, Some(0), 0),
            (2, Some(1), 0),
            (10, Some(1), 0),
            (20, Some(0), 0),
            (30, Some(1), 0),
            (31, Some(30), 0),
        ];
        let servers = [
            ServerProc {
                id: "a".into(),
                name: "a.gguf".into(),
                pid: 10,
            },
            ServerProc {
                id: "gw-b".into(),
                name: "b.gguf".into(),
                pid: 30,
            },
        ];
        let per_pid: HashMap<u32, u64> = [(2, 300), (10, 4000), (20, 900), (30, 1000), (31, 500)]
            .into_iter()
            .collect();
        let b = vram_breakdown(7000, 1, &servers, &procs, &per_pid);
        assert_eq!(b.harness_bytes, 300, "webview child is ours");
        assert_eq!(b.servers[0].bytes, 4000);
        assert_eq!(b.servers[1].bytes, 1500, "child of the server counts");
        assert_eq!(b.other_bytes, 7000 - 300 - 4000 - 1500);
        // The adapter reports less than the processes sum to: no underflow.
        assert_eq!(
            vram_breakdown(100, 1, &servers, &procs, &per_pid).other_bytes,
            0
        );
        // A server that holds no VRAM (CPU-only) is a zero segment, not an invented one.
        let none = vram_breakdown(1000, 1, &servers, &procs, &HashMap::new());
        assert_eq!(none.servers.iter().map(|s| s.bytes).sum::<u64>(), 0);
        assert_eq!(none.other_bytes, 1000);
        // Huge values do not wrap.
        let big: HashMap<u32, u64> = [(10, u64::MAX), (31, u64::MAX), (30, 5)]
            .into_iter()
            .collect();
        let sat = vram_breakdown(10, 1, &servers, &procs, &big);
        assert_eq!(sat.servers[1].bytes, u64::MAX);
        assert_eq!(sat.other_bytes, 0);
    }

    #[test]
    fn process_trees_do_not_double_count_a_pid() {
        let procs: Vec<ProcRow> = vec![(1, Some(0), 0), (10, Some(1), 0), (11, Some(10), 0)];
        let servers = [ServerProc {
            id: "a".into(),
            name: "a".into(),
            pid: 10,
        }];
        let t = process_trees(1, &servers, &procs);
        assert_eq!(t.harness, vec![1]);
        assert_eq!(t.servers, vec![vec![10, 11]]);
        let mut all = t.all_pids();
        all.sort();
        assert_eq!(all, vec![1, 10, 11]);
    }

    #[test]
    fn ram_split_adds_shared_gpu_memory_to_the_owner_once() {
        let procs: Vec<ProcRow> = vec![(1, Some(0), 100), (10, Some(1), 500), (20, Some(0), 50)];
        let servers = [ServerProc {
            id: "a".into(),
            name: "a.gguf".into(),
            pid: 10,
        }];
        let shared: HashMap<u32, u64> = [(10, 200), (20, 30), (99, 5)].into_iter().collect();
        let b = ram_breakdown(1000, 1, &servers, &procs, &shared);
        assert_eq!(b.harness_bytes, 100, "we hold no shared GPU memory");
        assert_eq!(b.servers[0].bytes, 700, "working set + shared GPU memory");
        // Each byte is in exactly one place: the unrelated process stays in "other".
        assert_eq!(b.other_bytes, 1000 - 100 - 700);
        assert_eq!(
            b.harness_bytes + b.servers[0].bytes + b.other_bytes,
            1000,
            "parts add up to used"
        );
    }

    /// Real hardware: with several adapters every one is sampled, the first being the
    /// primary one the plain `vram_*` fields describe; with one, `gpus` stays empty.
    #[test]
    fn system_sample_covers_every_adapter() {
        let adapters = crate::gpu::list_adapters();
        let mut probe = SystemProbe::new(adapters.clone());
        let snapshot = probe.sample();
        if adapters.len() < 2 {
            assert!(snapshot.gpus.is_empty());
            return;
        }
        assert_eq!(snapshot.gpus.len(), adapters.len());
        for (gpu, adapter) in snapshot.gpus.iter().zip(&adapters) {
            assert_eq!(gpu.index, adapter.index);
            assert_eq!(gpu.name, adapter.name);
            println!(
                "{}: {} / {} bytes, split: {}",
                gpu.name,
                gpu.used_bytes,
                gpu.total_bytes,
                gpu.breakdown.is_some()
            );
        }
        assert_eq!(snapshot.vram_used_bytes, snapshot.gpus[0].used_bytes);
        assert_eq!(snapshot.vram_total_bytes, snapshot.gpus[0].total_bytes);
        assert_eq!(snapshot.vram_breakdown, snapshot.gpus[0].breakdown);
    }

    #[test]
    fn material_change_sees_gpu_steps() {
        let gpu = |used: u64| GpuMemory {
            index: 1,
            name: "iGPU".into(),
            used_bytes: used,
            total_bytes: 1 << 30,
            breakdown: None,
        };
        let a = TelemetrySnapshot {
            gpus: vec![gpu(0)],
            ..Default::default()
        };
        let small = TelemetrySnapshot {
            gpus: vec![gpu(1 << 20)],
            ..Default::default()
        };
        let step = TelemetrySnapshot {
            gpus: vec![gpu(MEMORY_STEP)],
            ..Default::default()
        };
        assert!(!material_change(&a, &small));
        assert!(material_change(&a, &step));
        assert!(
            material_change(&a, &TelemetrySnapshot::default()),
            "adapter list changed"
        );
    }

    #[test]
    fn material_change_sees_vram_breakdown_steps() {
        let a = TelemetrySnapshot {
            vram_breakdown: Some(MemoryBreakdown::default()),
            ..Default::default()
        };
        let mut b = a.clone();
        b.vram_breakdown.as_mut().unwrap().harness_bytes = 1 << 20;
        assert!(!material_change(&a, &b));
        b.vram_breakdown.as_mut().unwrap().servers.push(ServerRam {
            id: "x".into(),
            name: "x".into(),
            bytes: 0,
        });
        assert!(material_change(&a, &b), "a server joined");
        assert!(
            material_change(&a, &TelemetrySnapshot::default()),
            "vanished"
        );
    }

    #[test]
    fn material_change_sees_breakdown_steps() {
        let a = TelemetrySnapshot {
            ram_breakdown: Some(RamBreakdown::default()),
            ..Default::default()
        };
        let mut b = a.clone();
        b.ram_breakdown.as_mut().unwrap().harness_bytes = 1 << 20;
        assert!(!material_change(&a, &b));
        b.ram_breakdown.as_mut().unwrap().harness_bytes = MEMORY_STEP;
        assert!(material_change(&a, &b));
    }

    #[tokio::test]
    async fn system_sample_has_a_breakdown_with_our_own_process() {
        let poller = Arc::new(tokio::sync::Mutex::new(TelemetryPoller::new(None)));
        let snap = TelemetryPoller::fresh_snapshot(&poller, "bd-test").await;
        let b = snap.ram_breakdown.expect("breakdown");
        assert!(b.harness_bytes > 0, "this test process has a working set");
        assert!(b.servers.is_empty());
    }

    #[test]
    fn only_the_first_sample_of_a_probe_is_cold() {
        let mut probe = SystemProbe::new(Vec::new());
        assert!(!probe.warm);
        probe.sample();
        assert!(probe.warm);
    }

    #[tokio::test]
    async fn fresh_snapshot_samples_system_memory_on_demand() {
        let poller = Arc::new(tokio::sync::Mutex::new(TelemetryPoller::new(None)));
        assert!(poller.lock().await.system_sampled_at().is_none());
        let snap = TelemetryPoller::fresh_snapshot(&poller, "fresh-test").await;
        assert!(snap.system_ram_total_bytes > 0);
        assert!(snap.system_ram_used_bytes > 0);
        let at = poller.lock().await.system_sampled_at().expect("sampled");
        // A second read within 1.5 s reuses the sample.
        TelemetryPoller::fresh_snapshot(&poller, "fresh-test").await;
        assert_eq!(poller.lock().await.system_sampled_at(), Some(at));
    }

    #[test]
    fn test_parse_prometheus_and_averages() {
        let m = parse_prometheus(SAMPLE);
        let mut snap = TelemetrySnapshot::default();
        apply_metrics(&m, None, &mut snap);
        assert_eq!(snap.avg_prompt_tokens_per_sec, 148.5);
        assert_eq!(snap.avg_predicted_tokens_per_sec, 27.8);
        assert_eq!(snap.predicted_tokens_per_sec, 0.0, "no previous sample yet");
        assert_eq!(snap.requests_deferred, 2);
        assert!(!snap.kv_cache_available);
        assert_eq!(snap.spec_acceptance_rate, None);
    }

    #[test]
    fn test_instant_rate_from_counter_deltas() {
        let prev = parse_prometheus(SAMPLE);
        let cur = parse_prometheus(
            "llamacpp:tokens_predicted_total 260\nllamacpp:tokens_predicted_seconds_total 11\n\
             llamacpp:prompt_tokens_total 100\nllamacpp:prompt_seconds_total 0.5\n\
             llamacpp:spec_decode_num_draft_tokens_total 10\nllamacpp:spec_decode_num_accepted_tokens_total 7\n\
             llamacpp:kv_cache_usage_ratio 0.25",
        );
        let mut snap = TelemetrySnapshot::default();
        apply_metrics(&cur, Some(&prev), &mut snap);
        assert_eq!(snap.predicted_tokens_per_sec, 60.0);
        assert_eq!(snap.prompt_tokens_per_sec, 0.0, "idle prompt processing");
        assert_eq!(snap.spec_acceptance_rate, Some(0.7));
        assert!(snap.kv_cache_available);
        assert_eq!(snap.kv_cache_usage_ratio, 0.25);
    }

    /// Real `/metrics` of b11236 drafting with `-md` + `--spec-type draft-simple` (79 drafted,
    /// 79 accepted; per-position counters carry labels and must not shadow the totals).
    #[test]
    fn test_spec_acceptance_real_draft_run() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/b11236-qwen3-4b-draft.metrics.txt");
        let m = parse_prometheus(&std::fs::read_to_string(path).unwrap());
        let mut snap = TelemetrySnapshot::default();
        apply_metrics(&m, None, &mut snap);
        assert_eq!(snap.spec_acceptance_rate, Some(1.0));
    }

    #[test]
    fn test_live_rates_from_slot_progress() {
        let before = serde_json::json!([
            {"id": 0, "is_processing": true, "id_task": 7, "n_ctx": 65536, "n_prompt_tokens": 40000,
             "n_prompt_tokens_processed": 10000, "next_token": [{"n_decoded": 0}]},
            {"id": 1, "is_processing": false, "n_ctx": 65536}
        ]);
        let after = serde_json::json!([
            {"id": 0, "is_processing": true, "id_task": 7, "n_ctx": 65536, "n_prompt_tokens": 40000,
             "n_prompt_tokens_processed": 12000, "next_token": [{"n_decoded": 0}]},
            {"id": 1, "is_processing": true, "id_task": 9, "n_ctx": 65536, "n_prompt_tokens": 100,
             "n_prompt_tokens_processed": 100, "next_token": [{"n_decoded": 50}]}
        ]);
        let (prompt, gen) = live_rates(&slot_progress(&before), &slot_progress(&after), 4.0);
        assert_eq!(prompt, 500.0); // slot 1 is a new task: not counted
        assert_eq!(gen, 0.0);
        let usage = context_usage(&after).unwrap();
        assert!((usage - (40000.0 + 150.0) / 65536.0).abs() < 1e-9);
    }

    #[test]
    fn test_parse_slots_json() {
        let sample_json = serde_json::json!([
            { "id": 0, "is_processing": true },
            { "id": 1, "is_processing": false },
            { "id": 2, "is_processing": true },
            { "id": 3, "is_processing": false }
        ]);
        let mut snapshot = TelemetrySnapshot::default();
        parse_slots_json(&sample_json, &mut snapshot);
        assert_eq!(snapshot.total_slots, 4);
        assert_eq!(snapshot.active_slots, 2);
    }

    #[test]
    fn test_detect_request_from_counter_growth() {
        let prev = parse_prometheus(
            "llamacpp:prompt_tokens_total 100\nllamacpp:prompt_tokens_cached_total 10\n\
             llamacpp:prompt_seconds_total 1\nllamacpp:tokens_predicted_total 50\n\
             llamacpp:tokens_predicted_seconds_total 2",
        );
        let cur = parse_prometheus(
            "llamacpp:prompt_tokens_total 600\nllamacpp:prompt_tokens_cached_total 30\n\
             llamacpp:prompt_seconds_total 3.5\nllamacpp:tokens_predicted_total 90\n\
             llamacpp:tokens_predicted_seconds_total 3.5",
        );
        let req = detect_request(&cur, &prev, 42).unwrap();
        assert_eq!(req.prompt_tokens, 500);
        assert_eq!(req.cached_tokens, 20);
        assert_eq!(req.generated_tokens, 40);
        assert_eq!(req.prompt_seconds, 2.5);
        assert_eq!(req.generation_seconds, 1.5);
        assert!(
            detect_request(&cur, &cur, 43).is_none(),
            "idle: nothing finished"
        );
        assert!(
            detect_request(&prev, &cur, 44).is_none(),
            "restart is not a request"
        );
        assert!(counters_reset(&prev, &cur));
    }

    #[test]
    fn test_server_totals_and_slot_infos() {
        let m = parse_prometheus(
            "llamacpp:prompt_tokens_total 104792\nllamacpp:prompt_tokens_cached_total 10\n\
             llamacpp:tokens_predicted_total 1039\nllamacpp:n_tokens_max 53156",
        );
        let totals = server_totals(&m);
        assert_eq!(totals.prompt_tokens, 104792);
        assert_eq!(totals.cached_prompt_tokens, Some(10));
        assert_eq!(totals.max_sequence_tokens, Some(53156));
        assert!(server_totals(&Metrics::new())
            .cached_prompt_tokens
            .is_none());

        let slots = serde_json::json!([
            {"id": 0, "is_processing": true, "n_ctx": 1000, "n_prompt_tokens": 300,
             "next_token": [{"n_decoded": 20}]},
            {"id": 1, "is_processing": false, "n_ctx": 1000, "n_prompt_tokens": 900}
        ]);
        let infos = slot_infos(&slots);
        assert_eq!(infos.len(), 2);
        assert!(infos[0].busy && infos[0].prompt_tokens == 300 && infos[0].decoded_tokens == 20);
        assert!(!infos[1].busy && infos[1].prompt_tokens == 0);
        assert_eq!(context_tokens(&slots), Some((320.0, 1000.0)));
    }
}
