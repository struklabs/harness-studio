//! Out-of-memory recovery options: after a start failed with `servers.out_of_memory`, what to
//! change so the model fits, with concrete values.
//!
//! Every option is a transient override layer (catalog ids, see `domain::launch`) and
//! optionally servers to stop first. [`crate::orchestrator::retry_with_overrides`] runs one:
//! the last launch of the server plus the layer, a real start, nothing saved unless asked.
//!
//! The numbers are estimates from the GGUF header (layers, KV heads, head sizes), the model
//! file size and the free VRAM the OS reports now (the failed process is gone by then). They
//! are deliberately conservative; the measured measured footprints can replace
//! [`MemoryPicture`] later without changing the option list.

use crate::capabilities::RuntimeCapabilities;
use crate::failure::ServerFailure;
use crate::gguf::GgufMetadata;
use crate::params::{ParamValue, ParamValues};
use crate::process::LaunchConfig;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;
/// Head room kept free on the device (llama-server's own `--fit-target` default is 1 GiB).
const MARGIN: u64 = GIB;
/// System memory left for the OS and other programs when layers move to the CPU.
const RAM_MARGIN: u64 = GIB;
/// Smallest context an option suggests.
const MIN_CTX: u32 = 2048;
/// Smallest `-ub` an option suggests.
const MIN_UBATCH: u32 = 128;
/// llama-server's default `-ub`.
const DEFAULT_UBATCH: u32 = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum RecoveryKind {
    /// Fewer layers on the GPU (the rest runs on the CPU: a CPU/GPU hybrid).
    LowerGpuLayers,
    LowerContext,
    LowerUbatch,
    /// Quantized KV cache (f16 → q8_0 → K q8_0 + V q4_0, `kv_policy` levels).
    KvCacheQuant,
    /// Let llama-server pick the GPU layers that fit (`--fit on`, `-ngl` not sent).
    EnableFit,
    /// MoE: keep the expert weights of some layers on the CPU (`--n-cpu-moe` / `--cpu-moe`).
    CpuMoe,
    /// Stop another running server first.
    UnloadServer,
}

impl RecoveryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RecoveryKind::LowerGpuLayers => "lower_gpu_layers",
            RecoveryKind::LowerContext => "lower_context",
            RecoveryKind::LowerUbatch => "lower_ubatch",
            RecoveryKind::KvCacheQuant => "kv_cache_quant",
            RecoveryKind::EnableFit => "enable_fit",
            RecoveryKind::CpuMoe => "cpu_moe",
            RecoveryKind::UnloadServer => "unload_server",
        }
    }
}

/// One way to retry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RecoveryOption {
    /// Unique within a plan (`lower_gpu_layers`, `unload_server:m-qwen`...).
    pub id: String,
    pub kind: RecoveryKind,
    /// i18n key (`recovery.lower_gpu_layers`...).
    pub label_key: String,
    /// Values for the label (`from`, `to`, `total`, `server`, `saved_mib`...).
    pub params: BTreeMap<String, String>,
    /// Transient layer for the retry (catalog ids).
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub overrides: ParamValues,
    /// Servers to stop before the retry.
    pub unload: Vec<String>,
    /// The estimate says this alone should make the model fit.
    pub recommended: bool,
    /// i18n key of a caveat shown with the option (`recovery.ram_insufficient`), its values
    /// are in `params`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning_key: Option<String>,
}

impl RecoveryOption {
    fn new(kind: RecoveryKind) -> Self {
        Self {
            id: kind.as_str().to_string(),
            kind,
            label_key: format!("recovery.{}", kind.as_str()),
            params: BTreeMap::new(),
            overrides: ParamValues::new(),
            unload: Vec::new(),
            recommended: false,
            warning_key: None,
        }
    }

    fn with(mut self, key: &str, value: impl ToString) -> Self {
        self.params.insert(key.to_string(), value.to_string());
        self
    }

    fn set(mut self, id: &str, value: ParamValue) -> Self {
        self.overrides.insert(id.to_string(), value);
        self
    }
}

/// The options for a failed server plus what they are based on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RecoveryPlan {
    pub server_id: String,
    pub failure: ServerFailure,
    /// Best first.
    pub options: Vec<RecoveryOption>,
    /// Free memory of the target GPU when the plan was made.
    pub free_vram_bytes: Option<u64>,
    /// Estimated need of the failed launch on the GPU (weights + KV), bytes.
    pub estimated_need_bytes: Option<u64>,
}

/// Another active server (candidate to stop).
#[derive(Debug, Clone)]
pub struct OtherServer {
    pub id: String,
    /// Definition alias or model file name.
    pub label: String,
    pub gpu_layers: u32,
}

/// Everything the options are computed from.
#[derive(Debug, Clone)]
pub struct RecoveryInput<'a> {
    pub config: &'a LaunchConfig,
    pub failure: &'a ServerFailure,
    pub meta: Option<&'a GgufMetadata>,
    /// Total size of the model file(s).
    pub model_bytes: Option<u64>,
    pub runtime: Option<&'a RuntimeCapabilities>,
    /// Free memory of the target GPU now.
    pub free_vram_bytes: Option<u64>,
    /// Available system memory now (the failed process is gone), for the layers that move
    /// to the CPU.
    pub free_ram_bytes: Option<u64>,
    pub others: Vec<OtherServer>,
}

/// Bytes per KV element of a cache type.
fn kv_elem_bytes(cache_type: Option<&str>) -> f64 {
    match cache_type.map(|s| s.trim().to_lowercase()).as_deref() {
        Some("f32") => 4.0,
        Some("q8_0") => 34.0 / 32.0,
        Some("q5_0") => 22.0 / 32.0,
        Some("q5_1") => 24.0 / 32.0,
        Some("q4_0") => 18.0 / 32.0,
        Some("q4_1") => 20.0 / 32.0,
        Some("iq4_nl") => 18.0 / 32.0,
        _ => 2.0, // f16 / bf16 / default
    }
}

fn param_text(c: &LaunchConfig, id: &str) -> Option<String> {
    match c.params.get(id)? {
        ParamValue::Text(s) => Some(s.trim().to_lowercase()),
        ParamValue::Bool(b) => Some(b.to_string()),
        ParamValue::Int(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Memory estimate of one launch on the GPU.
#[derive(Debug, Clone, Copy)]
struct MemoryPicture {
    /// Repeating layers (`block_count`).
    layers: u32,
    /// Layers on the GPU (`-ngl` capped at `layers + 1`, the output layer).
    gpu_layers: u32,
    model_bytes: u64,
    /// KV bytes per context token over all layers (with the configured cache types).
    kv_per_token: f64,
    kv_on_gpu: bool,
}

impl MemoryPicture {
    fn of(input: &RecoveryInput) -> Option<Self> {
        let meta = input.meta?;
        let layers = meta.block_count? as u32;
        if layers == 0 {
            return None;
        }
        let c = input.config;
        let heads = meta.head_count.unwrap_or(0);
        let head_dim = meta
            .embedding_length
            .zip(meta.head_count)
            .filter(|(_, h)| *h > 0)
            .map(|(e, h)| e / h)
            .unwrap_or(128);
        let kv_heads = meta.head_count_kv.unwrap_or(heads).max(1);
        let k = meta.key_length.unwrap_or(head_dim) as f64;
        let v = meta.value_length.unwrap_or(head_dim) as f64;
        let per_layer = kv_heads as f64
            * (k * kv_elem_bytes(c.cache_type_k.as_deref())
                + v * kv_elem_bytes(c.cache_type_v.as_deref()));
        Some(Self {
            layers,
            gpu_layers: c.gpu_layers.min(layers + 1),
            model_bytes: input.model_bytes?,
            kv_per_token: per_layer * layers as f64,
            kv_on_gpu: param_text(c, "kv_offload").as_deref() != Some("false"),
        })
    }

    fn offloaded(&self, gpu_layers: u32) -> f64 {
        gpu_layers.min(self.layers) as f64 / self.layers as f64
    }

    fn weights_on_gpu(&self, gpu_layers: u32) -> u64 {
        (self.model_bytes as f64 * self.offloaded(gpu_layers)) as u64
    }

    fn kv_on_gpu_bytes(&self, ctx: u32, gpu_layers: u32) -> u64 {
        if !self.kv_on_gpu {
            return 0;
        }
        (self.kv_per_token * ctx as f64 * self.offloaded(gpu_layers)) as u64
    }

    fn need(&self, ctx: u32, gpu_layers: u32) -> u64 {
        self.weights_on_gpu(gpu_layers) + self.kv_on_gpu_bytes(ctx, gpu_layers)
    }
}

fn round_down(n: u32, step: u32) -> u32 {
    n / step * step
}

/// The context the model was trained on, when nothing in the launch stretches it.
fn trained_context(input: &RecoveryInput) -> Option<u32> {
    let scaled = input
        .config
        .rope_scaling
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty() && s != "none");
    if scaled {
        return None;
    }
    input
        .meta?
        .context_length
        .filter(|&n| n > 0)
        .map(|n| n.min(u32::MAX as u64) as u32)
}

/// The recovery options of an out-of-memory failure, best first.
pub fn oom_options(input: &RecoveryInput) -> Vec<RecoveryOption> {
    let c = input.config;
    let stage = input.failure.params.get("stage").map(String::as_str);
    let pic = MemoryPicture::of(input);
    let budget = input.free_vram_bytes.map(|f| f.saturating_sub(MARGIN));
    let has_flag = |f: &str| input.runtime.is_some_and(|r| r.has_flag(f));
    let fits = |need: u64| budget.is_some_and(|b| need <= b);
    let ctx = c.context_size;
    let fit_on = param_text(c, "fit").as_deref() == Some("on");
    let mut out: Vec<RecoveryOption> = Vec::new();

    // Let llama-server measure and choose the GPU layers itself.
    if has_flag("--fit") && !fit_on && c.gpu_layers > 0 {
        out.push(
            RecoveryOption::new(RecoveryKind::EnableFit)
                .set("fit", ParamValue::Text("on".into()))
                .with("ctx", ctx),
        );
    }

    // KV cache: context and cache type.
    if let Some(p) = pic {
        let weights = p.weights_on_gpu(p.gpu_layers);
        if ctx > MIN_CTX {
            let target = match budget {
                Some(b) if p.kv_on_gpu && p.kv_per_token > 0.0 => {
                    let kv_budget = b.saturating_sub(weights) as f64;
                    let fit = kv_budget / (p.kv_per_token * p.offloaded(p.gpu_layers));
                    round_down(fit.min(u32::MAX as f64) as u32, 1024)
                }
                _ => 0,
            };
            // The estimate can be wrong in both directions: never suggest more than half
            // when it says "fits already", never less than MIN_CTX.
            let to = if target >= ctx || target == 0 {
                round_down(ctx / 2, 1024).max(MIN_CTX)
            } else {
                target.max(MIN_CTX)
            };
            // Nor more than the model was trained on (unless RoPE scaling stretches it): a
            // 0.5B model asked for 20M tokens should get its 32k back, not 1M that fits.
            let to = match trained_context(input) {
                Some(train) if train < to => train.max(MIN_CTX),
                _ => to,
            };
            if to < ctx {
                let saved =
                    p.kv_on_gpu_bytes(ctx, p.gpu_layers) - p.kv_on_gpu_bytes(to, p.gpu_layers);
                out.push(RecoveryOption {
                    recommended: fits(p.need(to, p.gpu_layers)),
                    ..RecoveryOption::new(RecoveryKind::LowerContext)
                        .set("context_size", ParamValue::Int(to as i64))
                        .with("from", ctx)
                        .with("to", to)
                        .with("saved_mib", saved / MIB)
                });
            }
        }
        // The next rung of the KV precision ladder (`kv_policy`): never 4-bit keys.
        use crate::kv_policy::KvLevel;
        let (cur_k, cur_v) = (c.cache_type_k.as_deref(), c.cache_type_v.as_deref());
        let next = match KvLevel::of(cur_k, cur_v) {
            KvLevel::Full | KvLevel::KeysOnly => Some(KvLevel::Balanced),
            KvLevel::Balanced => Some(KvLevel::Saver),
            _ => None,
        };
        if let Some((nk, nv)) = next.and_then(KvLevel::types) {
            let before = p.kv_on_gpu_bytes(ctx, p.gpu_layers);
            let ratio = (kv_elem_bytes(Some(nk)) + kv_elem_bytes(Some(nv)))
                / (kv_elem_bytes(cur_k) + kv_elem_bytes(cur_v));
            let after = (before as f64 * ratio) as u64;
            let text = |k: &str, v: &str| {
                if k == v {
                    k.to_string()
                } else {
                    format!("{k}/{v}")
                }
            };
            let norm = |t: Option<&str>| {
                t.map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "f16".into())
            };
            // A quantized V cache needs flash attention.
            out.push(RecoveryOption {
                recommended: fits(weights + after),
                ..RecoveryOption::new(RecoveryKind::KvCacheQuant)
                    .set("cache_type_k", ParamValue::Text(nk.into()))
                    .set("cache_type_v", ParamValue::Text(nv.into()))
                    .set("flash_attn", ParamValue::Bool(true))
                    .with("from", text(&norm(cur_k), &norm(cur_v)))
                    .with("to", text(nk, nv))
                    .with("saved_mib", before.saturating_sub(after) / MIB)
            });
        }
    } else if ctx > MIN_CTX {
        let to = round_down(ctx / 2, 1024).max(MIN_CTX);
        out.push(
            RecoveryOption::new(RecoveryKind::LowerContext)
                .set("context_size", ParamValue::Int(to as i64))
                .with("from", ctx)
                .with("to", to),
        );
    }

    // Fewer GPU layers (CPU/GPU hybrid).
    if c.gpu_layers > 0 {
        let (current, total) = match pic {
            Some(p) => (p.gpu_layers, p.layers + 1),
            None => (c.gpu_layers, c.gpu_layers),
        };
        let estimate = match (pic, budget) {
            (Some(p), Some(b)) => {
                let per_layer = p.need(ctx, p.layers) as f64 / p.layers as f64;
                (per_layer > 0.0).then(|| (b as f64 / per_layer) as u32)
            }
            _ => None,
        };
        let mut to = match estimate {
            Some(n) if n < current => n,
            // Unknown, or the estimate says it fits (so it is wrong): a third fewer.
            _ => current * 2 / 3,
        };
        // The layers that leave the GPU need system memory: with too little, the load
        // fails or the OS pages the weights. Prefer the fewest layers moved that fit in RAM.
        let ram_budget = input.free_ram_bytes.map(|f| f.saturating_sub(RAM_MARGIN));
        let ram_fits = |n: u32| match (host_need(input, n), ram_budget) {
            (Some(need), Some(b)) => need <= b,
            _ => true,
        };
        let mut ram_short = None;
        if to < current && !ram_fits(to) {
            // Host need falls as layers are added: the smallest n in (to, current) that fits.
            let (mut lo, mut hi) = (to + 1, current);
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                if ram_fits(mid) {
                    hi = mid;
                } else {
                    lo = mid + 1;
                }
            }
            if lo < current {
                to = lo;
            } else {
                ram_short = host_need(input, to).zip(input.free_ram_bytes);
            }
        }
        // A KV cache overflow is only helped when the KV lives on the GPU and the layers
        // moved free enough of it (TC-09: 20M tokens were offered 25 → 24, the output layer).
        // With no layer count fitting system memory either, nothing is left to move.
        let helps_kv = stage != Some("kv_cache")
            || pic.is_none_or(|p| {
                p.kv_on_gpu && ram_short.is_none() && (budget.is_none() || fits(p.need(ctx, to)))
            });
        if to < current && helps_kv {
            let recommended = ram_short.is_none()
                && estimate.is_some_and(|n| n < current)
                && pic.is_some_and(|p| fits(p.need(ctx, to)));
            let mut opt = RecoveryOption {
                recommended,
                ..RecoveryOption::new(RecoveryKind::LowerGpuLayers)
                    .set("gpu_layers", ParamValue::Int(to as i64))
                    .with("from", current)
                    .with("to", to)
                    .with("total", total)
            };
            if let Some(need) = host_need(input, to) {
                opt = opt.with("ram_need_mib", need / MIB);
            }
            if let Some((need, free)) = ram_short {
                opt = opt
                    .with("ram_need_mib", need / MIB)
                    .with("ram_free_mib", free / MIB);
                opt.warning_key = Some("recovery.ram_insufficient".into());
            }
            out.push(opt);
        }
    }

    // MoE: expert weights of some layers on the CPU (keeps attention on the GPU).
    let is_moe = input
        .meta
        .and_then(|m| m.expert_count)
        .is_some_and(|n| n > 1);
    if is_moe && c.gpu_layers > 0 && (has_flag("--n-cpu-moe") || has_flag("--cpu-moe")) {
        let layers = pic.map(|p| p.layers).unwrap_or(0);
        let n = match (pic, budget) {
            (Some(p), Some(b)) => {
                let deficit = p.need(ctx, p.gpu_layers).saturating_sub(b) as f64;
                // Experts are most of an MoE model's weights.
                let per_layer_experts = p.model_bytes as f64 * 0.85 / p.layers as f64;
                ((deficit / per_layer_experts).ceil() as u32).clamp(1, p.layers)
            }
            _ => (layers / 2).max(1),
        };
        let all = layers > 0 && n >= layers;
        let mut opt = RecoveryOption::new(RecoveryKind::CpuMoe);
        if all || !has_flag("--n-cpu-moe") {
            opt = opt
                .set("cpu_moe", ParamValue::Bool(true))
                .with("layers", "all");
        } else {
            opt = opt
                .set("n_cpu_moe", ParamValue::Int(n as i64))
                .with("layers", n);
        }
        opt.recommended = pic.is_some() && budget.is_some();
        out.push(opt.with("total", layers));
    }

    // Compute buffers grow with the physical batch.
    let ub = c
        .physical_batch
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_UBATCH);
    if ub > MIN_UBATCH {
        let to = (ub / 2).max(MIN_UBATCH);
        let mut opt = RecoveryOption::new(RecoveryKind::LowerUbatch)
            .set("physical_batch", ParamValue::Int(to as i64))
            .with("from", ub)
            .with("to", to);
        // `-b` must not be smaller than `-ub`; only lower it when it was set below.
        if c.logical_batch.is_some_and(|b| b > 0 && b < to) {
            opt = opt.set("logical_batch", ParamValue::Int(to as i64));
        }
        opt.recommended = stage == Some("compute");
        out.push(opt);
    }

    // Stop another server that holds GPU memory.
    for other in input.others.iter().filter(|o| o.gpu_layers > 0) {
        let mut opt = RecoveryOption::new(RecoveryKind::UnloadServer).with("server", &other.label);
        opt.id = format!("unload_server:{}", other.id);
        opt.params.insert("server_id".into(), other.id.clone());
        opt.unload = vec![other.id.clone()];
        out.push(opt);
    }

    // Order: what the log says did not fit first, then the estimate's picks.
    let rank = |o: &RecoveryOption| -> u8 {
        let by_stage = match (stage, o.kind) {
            (Some("kv_cache"), RecoveryKind::LowerContext | RecoveryKind::KvCacheQuant) => 0,
            (Some("weights"), RecoveryKind::LowerGpuLayers | RecoveryKind::CpuMoe) => 0,
            (Some("compute"), RecoveryKind::LowerUbatch) => 0,
            _ => 1,
        };
        let by_kind = match o.kind {
            RecoveryKind::EnableFit => 0,
            RecoveryKind::CpuMoe => 1,
            RecoveryKind::LowerContext => 2,
            RecoveryKind::KvCacheQuant => 3,
            RecoveryKind::LowerGpuLayers => 4,
            RecoveryKind::LowerUbatch => 5,
            RecoveryKind::UnloadServer => 6,
        };
        (!o.recommended as u8) * 20 + by_stage * 10 + by_kind
    };
    out.sort_by_key(rank);
    out
}

/// System memory a launch with `gpu_layers` needs (weights and KV of the layers on the CPU,
/// host compute buffers, process overhead), from the shared estimate
/// ([`crate::vram::estimate_raw`]). `None` without the model size.
fn host_need(input: &RecoveryInput, gpu_layers: u32) -> Option<u64> {
    let mut fi = crate::vram::input_from_launch(input.config);
    fi.model_size_bytes = input.model_bytes?;
    fi.gpu_layers = gpu_layers;
    Some(crate::vram::estimate_raw(&fi, input.meta, true).ram_bytes)
}

/// Available system memory now (`None` when the OS does not report it).
pub fn free_ram_bytes() -> Option<u64> {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    Some(sys.available_memory()).filter(|&b| b > 0)
}

/// Estimated GPU need of the failed launch (weights + KV), for display.
pub fn estimated_need(input: &RecoveryInput) -> Option<u64> {
    MemoryPicture::of(input).map(|p| p.need(input.config.context_size, p.gpu_layers))
}

/// Free memory of the GPU a launch targets (`main_gpu`, else the primary GPU), from the OS
/// (DXGI on Windows, sysfs on Linux). `None` when unknown (e.g. macOS, CPU only).
///
/// Kept behind this one function so the device list (`--list-devices`) can replace
/// it without touching the options.
pub fn free_vram_bytes(config: &LaunchConfig) -> Option<u64> {
    let adapters = crate::gpu::list_adapters();
    // `list_adapters` is sorted (discrete first): find by adapter index, not by position.
    let index = config.main_gpu.or_else(crate::gpu::primary_gpu_index)?;
    let adapter = adapters
        .iter()
        .find(|a| a.index == index)
        .or_else(|| adapters.first())?;
    let (used, total) = crate::gpu::vram_usage(adapter)?;
    Some(total.saturating_sub(used))
}

/// Size of a model: the file, or every part of a split GGUF (`-00001-of-00003.gguf`).
pub fn model_bytes(path: &std::path::Path) -> Option<u64> {
    let size = std::fs::metadata(path).ok()?.len();
    let name = path.file_name()?.to_string_lossy().to_string();
    let Some(pos) = name.find("-00001-of-") else {
        return Some(size);
    };
    let prefix = &name[..pos];
    let dir = path.parent()?;
    let total: u64 = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.starts_with(prefix) && n.contains("-of-") && n.ends_with(".gguf")
        })
        .filter_map(|e| e.metadata().ok().map(|m| m.len()))
        .sum();
    Some(total.max(size))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::failure::{FailureKind, FailurePhase};

    fn qwen14b() -> GgufMetadata {
        // Qwen2.5-Coder-14B-Instruct Q4_K_M header values.
        GgufMetadata {
            block_count: Some(48),
            embedding_length: Some(5120),
            head_count: Some(40),
            head_count_kv: Some(8),
            ..Default::default()
        }
    }

    fn cfg(ngl: u32, ctx: u32) -> LaunchConfig {
        LaunchConfig {
            gpu_layers: ngl,
            context_size: ctx,
            ..Default::default()
        }
    }

    fn oom(stage: &str) -> ServerFailure {
        ServerFailure::new(FailureKind::OutOfMemory, FailurePhase::Loading).with("stage", stage)
    }

    fn caps() -> RuntimeCapabilities {
        crate::capabilities::testdata::caps("b11236")
    }

    #[test]
    fn kv_cache_oom_of_14b_at_128k_suggests_real_values() {
        let (meta, config, failure, caps) = (qwen14b(), cfg(99, 131072), oom("kv_cache"), caps());
        let input = RecoveryInput {
            config: &config,
            failure: &failure,
            meta: Some(&meta),
            model_bytes: Some(8_988_110_016),
            runtime: Some(&caps),
            free_vram_bytes: Some(15_000 * MIB),
            free_ram_bytes: None,
            others: vec![OtherServer {
                id: "m-other".into(),
                label: "other".into(),
                gpu_layers: 99,
            }],
        };
        let opts = oom_options(&input);
        let by = |k: RecoveryKind| opts.iter().find(|o| o.kind == k).unwrap();
        // 48 layers * 8 heads * 128 * 2 (K+V) * 2 bytes = 192 KiB per token.
        let ctx = by(RecoveryKind::LowerContext);
        let to: u32 = ctx.params["to"].parse().unwrap();
        assert!((20_000..=30_000).contains(&to), "{ctx:?}");
        assert!(ctx.recommended);
        assert_eq!(ctx.overrides["context_size"], ParamValue::Int(to as i64));
        let kv = by(RecoveryKind::KvCacheQuant);
        assert_eq!(kv.params["to"], "q8_0");
        assert_eq!(kv.overrides["flash_attn"], ParamValue::Bool(true));
        assert!(!kv.recommended, "q8_0 halves 24 GiB: still too big");
        let ngl = by(RecoveryKind::LowerGpuLayers);
        let n: u32 = ngl.params["to"].parse().unwrap();
        assert!(n < 49 && n > 5, "{ngl:?}");
        assert_eq!(ngl.params["total"], "49");
        assert!(opts.iter().any(|o| o.kind == RecoveryKind::EnableFit));
        let unload = by(RecoveryKind::UnloadServer);
        assert_eq!(unload.unload, vec!["m-other"]);
        assert_eq!(unload.id, "unload_server:m-other");
        // Recommended and matching the stage first.
        assert_eq!(opts[0].kind, RecoveryKind::LowerContext);
        assert!(opts.iter().all(|o| !o.id.is_empty()));
        assert!(estimated_need(&input).unwrap() > 30 * GIB);
    }

    /// A context far beyond what the model was trained on comes back to the trained length
    /// (TC-09: Qwen2.5-0.5B with 20M tokens was offered 1M, which loaded into 12 GB).
    #[test]
    fn lower_context_stops_at_the_trained_context() {
        let mut meta = qwen14b();
        meta.context_length = Some(32_768);
        let (config, failure) = (cfg(99, 20_000_000), oom("kv_cache"));
        let input = |config: &'_ LaunchConfig| -> Vec<RecoveryOption> {
            oom_options(&RecoveryInput {
                config,
                failure: &failure,
                meta: Some(&meta),
                model_bytes: Some(400 * MIB),
                runtime: Some(&caps()),
                free_vram_bytes: Some(15_000 * MIB),
                free_ram_bytes: None,
                others: vec![],
            })
        };
        let opts = input(&config);
        let ctx = opts
            .iter()
            .find(|o| o.kind == RecoveryKind::LowerContext)
            .unwrap();
        assert_eq!(ctx.params["to"], "32768");
        assert_eq!(ctx.overrides["context_size"], ParamValue::Int(32_768));

        // With RoPE scaling the trained length is no limit.
        let scaled = LaunchConfig {
            rope_scaling: Some("yarn".into()),
            ..cfg(99, 20_000_000)
        };
        let opts = input(&scaled);
        let to: u32 = opts
            .iter()
            .find(|o| o.kind == RecoveryKind::LowerContext)
            .unwrap()
            .params["to"]
            .parse()
            .unwrap();
        assert!(to > 32_768, "{to}");
    }

    /// On a KV cache overflow, fewer GPU layers is offered only when it closes the gap and the
    /// KV is on the GPU at all (TC-09: 20M tokens got "25 → 24", which saves nothing useful).
    #[test]
    fn kv_overflow_hides_lower_gpu_layers_that_cannot_help() {
        let meta = qwen14b();
        let failure = oom("kv_cache");
        let caps = caps();
        let run = |config: &LaunchConfig, free_ram: Option<u64>| -> Vec<RecoveryOption> {
            oom_options(&RecoveryInput {
                config,
                failure: &failure,
                meta: Some(&meta),
                model_bytes: Some(400 * MIB),
                runtime: Some(&caps),
                free_vram_bytes: Some(15_000 * MIB),
                free_ram_bytes: free_ram,
                others: vec![],
            })
        };
        let has_ngl =
            |opts: &[RecoveryOption]| opts.iter().any(|o| o.kind == RecoveryKind::LowerGpuLayers);
        // 20M tokens: the KV fits neither one GPU layer nor system memory.
        let huge = cfg(49, 20_000_000);
        assert!(!has_ngl(&run(&huge, Some(64 * GIB))));
        // KV kept in system memory: GPU layers do not change it.
        let kv_off = LaunchConfig {
            params: [("kv_offload".to_string(), ParamValue::Bool(false))].into(),
            ..cfg(49, 131_072)
        };
        assert!(!has_ngl(&run(&kv_off, None)));
        // A gap that moving layers closes still gets the option.
        assert!(has_ngl(&run(&cfg(49, 131_072), None)));
        // Other stages are unchanged.
        let weights = oom_options(&RecoveryInput {
            config: &huge,
            failure: &oom("weights"),
            meta: Some(&meta),
            model_bytes: Some(400 * MIB),
            runtime: Some(&caps),
            free_vram_bytes: Some(15_000 * MIB),
            free_ram_bytes: None,
            others: vec![],
        });
        assert!(has_ngl(&weights));
    }

    /// `lower_gpu_layers` moves weights to the CPU: with little free RAM the option keeps more
    /// layers on the GPU, and warns when even one layer fewer does not fit.
    #[test]
    fn lower_gpu_layers_checks_system_ram() {
        let (meta, config, failure) = (qwen14b(), cfg(99, 8192), oom("weights"));
        let base = |free_ram: Option<u64>| RecoveryInput {
            config: &config,
            failure: &failure,
            meta: Some(&meta),
            model_bytes: Some(8_988_110_016),
            runtime: None,
            free_vram_bytes: Some(4 * GIB),
            free_ram_bytes: free_ram,
            others: Vec::new(),
        };
        let ngl = |input: &RecoveryInput| {
            oom_options(input)
                .into_iter()
                .find(|o| o.kind == RecoveryKind::LowerGpuLayers)
                .unwrap()
        };
        // Plenty of RAM: the VRAM estimate decides.
        let plenty = ngl(&base(Some(64 * GIB)));
        let to: u32 = plenty.params["to"].parse().unwrap();
        assert!(plenty.warning_key.is_none());
        assert!(plenty.recommended, "{plenty:?}");
        let need: u64 = plenty.params["ram_need_mib"].parse().unwrap();
        assert!(
            need > 3000,
            "most of 8.4 GB of weights on the CPU: {need} MiB"
        );

        // RAM for only part of the CPU share: more layers stay on the GPU, not recommended
        // because that no longer fits the VRAM.
        let host_at = |n: u32| host_need(&base(None), n).unwrap();
        let limited = host_at(to + 10) + RAM_MARGIN;
        let tight = ngl(&base(Some(limited)));
        let tight_to: u32 = tight.params["to"].parse().unwrap();
        assert!(tight_to > to && tight_to <= to + 10, "{tight:?}");
        assert!(host_at(tight_to) + RAM_MARGIN <= limited);
        assert!(tight.warning_key.is_none());
        assert!(!tight.recommended);

        // Not even one layer fewer fits: the option stays, with the warning and the numbers.
        let none = ngl(&base(Some(host_at(48) + RAM_MARGIN - 1)));
        assert_eq!(
            none.warning_key.as_deref(),
            Some("recovery.ram_insufficient")
        );
        assert!(!none.recommended);
        assert!(none.params.contains_key("ram_free_mib"));

        // Unknown RAM: no check.
        assert!(ngl(&base(None)).warning_key.is_none());
    }

    #[test]
    fn without_metadata_or_free_memory_options_halve() {
        let (config, failure) = (cfg(40, 16384), oom("weights"));
        let input = RecoveryInput {
            config: &config,
            failure: &failure,
            meta: None,
            model_bytes: None,
            runtime: None,
            free_vram_bytes: None,
            free_ram_bytes: None,
            others: vec![],
        };
        let opts = oom_options(&input);
        let ids: Vec<&str> = opts.iter().map(|o| o.id.as_str()).collect();
        assert!(!ids.contains(&"enable_fit"), "no runtime → no --fit");
        let ngl = opts.iter().find(|o| o.id == "lower_gpu_layers").unwrap();
        assert_eq!(ngl.params["to"], "26");
        assert_eq!(
            opts[0].id, "lower_gpu_layers",
            "weights stage first: {ids:?}"
        );
        let ctx = opts.iter().find(|o| o.id == "lower_context").unwrap();
        assert_eq!(ctx.params["to"], "8192");
        let ub = opts.iter().find(|o| o.id == "lower_ubatch").unwrap();
        assert_eq!(ub.params["to"], "256");
        assert!(opts.iter().all(|o| !o.recommended));
    }

    #[test]
    fn moe_gets_cpu_moe_and_quantized_cache_steps() {
        let meta = GgufMetadata {
            block_count: Some(24),
            embedding_length: Some(2880),
            head_count: Some(64),
            head_count_kv: Some(8),
            key_length: Some(64),
            value_length: Some(64),
            expert_count: Some(32),
            ..Default::default()
        };
        let mut config = cfg(99, 8192);
        config.cache_type_k = Some("q8_0".into());
        config.cache_type_v = Some("q8_0".into());
        let (failure, caps) = (oom("weights"), caps());
        let input = RecoveryInput {
            config: &config,
            failure: &failure,
            meta: Some(&meta),
            model_bytes: Some(12_109_566_560),
            runtime: Some(&caps),
            free_vram_bytes: Some(8 * GIB),
            free_ram_bytes: None,
            others: vec![],
        };
        let opts = oom_options(&input);
        let moe = opts
            .iter()
            .find(|o| o.kind == RecoveryKind::CpuMoe)
            .unwrap();
        let n = match moe.overrides.get("n_cpu_moe") {
            Some(ParamValue::Int(n)) => *n,
            other => panic!("{other:?}"),
        };
        assert!((1..24).contains(&n), "{moe:?}");
        let kv = opts
            .iter()
            .find(|o| o.kind == RecoveryKind::KvCacheQuant)
            .unwrap();
        // q8_0 / q8_0 steps down to 4-bit values only; keys stay 8-bit.
        assert_eq!(kv.params["to"], "q8_0/q4_0");
        assert_eq!(kv.params["from"], "q8_0");
        assert_eq!(
            kv.overrides["cache_type_k"],
            ParamValue::Text("q8_0".into())
        );
        assert_eq!(
            kv.overrides["cache_type_v"],
            ParamValue::Text("q4_0".into())
        );
        // ngl 0: nothing to offload less, no fit, no MoE option.
        let config = cfg(0, 4096);
        let input = RecoveryInput {
            config: &config,
            ..input
        };
        let ids: Vec<String> = oom_options(&input).into_iter().map(|o| o.id).collect();
        assert!(
            !ids.iter()
                .any(|i| i == "lower_gpu_layers" || i == "enable_fit" || i == "cpu_moe"),
            "{ids:?}"
        );
    }

    #[test]
    fn fit_is_not_offered_again_when_on() {
        let mut config = cfg(99, 4096);
        config
            .params
            .insert("fit".into(), ParamValue::Text("on".into()));
        let (failure, caps) = (oom("weights"), caps());
        let input = RecoveryInput {
            config: &config,
            failure: &failure,
            meta: None,
            model_bytes: None,
            runtime: Some(&caps),
            free_vram_bytes: None,
            free_ram_bytes: None,
            others: vec![],
        };
        assert!(!oom_options(&input)
            .iter()
            .any(|o| o.kind == RecoveryKind::EnableFit));
    }
}
