//! Memory estimate for running a GGUF with llama-server: the single source for the GUI,
//! the library, the Control API and the CLI.
//!
//! The estimate follows what llama.cpp allocates (checked against real `-lv 4` load logs of
//! b11236 on Vulkan, see `tests/data/b11236-*.load-log.txt`):
//! - **weights**: the repeating layers go to the GPU in proportion to `-ngl`; the output
//!   layer only when `-ngl` exceeds the layer count; `token_embd` stays in host memory (tied
//!   embeddings are uploaded once more as the output layer). Tensor sizes come from the GGUF
//!   tensor table.
//! - **KV cache**: `n_ctx` (padded to 256) cells per full-attention layer. Sliding-window
//!   layers (gpt-oss, Gemma 2/3, Cohere 2, ...) only keep
//!   `pad256(n_swa * n_seq + n_ubatch)` cells (unified KV), unless `--swa-full`. Bytes per
//!   element follow the cache type (`q8_0` = 34/32 bytes...).
//!   Recurrent / hybrid models (Mamba, Qwen3.5 gated delta net) keep one fixed-size f32
//!   state per sequence on their recurrent layers; only their attention layers have a KV
//!   cache. Sliding-window and recurrent state grow with the slots (`-np`), see
//!   [`crate::slots`].
//! - **context checkpoints** (host memory): for SWA / recurrent models llama-server keeps up
//!   to `--ctx-checkpoints` copies of a slot's window / state per slot (Harness default 4).
//! - **compute buffer** (flash attention on): `4 * n_ubatch * n_ff * 4` bytes plus the
//!   `n_ubatch * n_ctx` f16 attention mask; MoE models use `n_expert_used * n_ff_exp`.
//!   Without flash attention the f32 `KQ` matrix (`n_ubatch * n_ctx * n_head`) is added.
//! - **mmproj**: the projector file is offloaded as a whole.
//! - **runtime overhead**: ~150 MiB of device memory per process (measured 131-149 MiB on
//!   Vulkan/RDNA3).
//!
//! After a successful load the real footprint is parsed from the server log (`model buffer
//! size`, `KV buffer size`, `compute buffer size`) and, where the OS exposes it, the
//! process's dedicated GPU memory; see [`record_after_load`]. Later estimates of the same
//! model are corrected with the most recent measurement ([`EstimateSource`]).

use crate::gguf::GgufMetadata;
use crate::hardware::{RecommendedBackend, SystemHardwareInfo};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;
/// Keep driver allocations, transient graphs and desktop growth away from WDDM spill.
/// 8% scales with card capacity; 1 GiB is the floor on smaller cards. User settings may
/// replace it with an explicit value; 0 means this automatic reserve. It is a budget reserve,
/// not model memory. A discrete AMD card on Windows needs more ([`AMD_WINDOWS_DRIVER_RESERVE`]).
pub const VRAM_HEADROOM_PERCENT: u64 = 8;
pub const MIN_VRAM_HEADROOM: u64 = GIB;

/// Windows with a discrete AMD card (Vulkan): the driver starts to demote allocations to shared
/// system memory once all processes together hold more than about the card's dedicated memory
/// minus this, long before the free memory the runtime reports (which ignores the desktop's
/// share) is gone. Measured on an RX 7900 GRE 16 GB: Qwen2.5-Coder-14B at 8k/16k context (10.0
/// GiB for the process, 14.2 GiB for the card) stayed dedicated, 10.4 GiB (14.5 GiB) spilled 0.9
/// GiB and 11.8 GiB spilled 2.1 GiB. NVIDIA cards measured much closer to the limit.
pub const AMD_WINDOWS_DRIVER_RESERVE: u64 = 2200 * MIB;

/// [`AMD_WINDOWS_DRIVER_RESERVE`] when the primary GPU is a discrete AMD card on Windows, else 0.
fn driver_reserve() -> u64 {
    static RESERVE: OnceLock<u64> = OnceLock::new();
    *RESERVE.get_or_init(|| {
        if !cfg!(windows) {
            return 0;
        }
        crate::gpu::list_adapters()
            .first()
            .filter(|a| a.is_discrete && a.vendor_id == Some(crate::gpu::VENDOR_AMD))
            .map_or(0, |_| AMD_WINDOWS_DRIVER_RESERVE)
    })
}

pub fn memory_headroom(total: u64, configured: u64) -> u64 {
    headroom_with(total, configured, driver_reserve())
}

/// The reserve for a server that is not llama.cpp (stable-diffusion.cpp, whisper.cpp, ONNX
/// Runtime): half of the automatic reserve, never below the driver reserve (on AMD Windows a
/// spill threshold, not a margin). These allocate their compute buffers at load (the estimate
/// counts them) and grow no KV cache while serving; the full reserve refused SD-Turbo (2.2 GiB
/// measured peak) on a 4 GB RTX 3050 Ti with 3.2 GiB free. A configured value stays.
pub fn fixed_footprint_headroom(total: u64, configured: u64) -> u64 {
    fixed_footprint_headroom_with(total, configured, driver_reserve())
}

/// [`fixed_footprint_headroom`] with the driver reserve of the machine given.
pub fn fixed_footprint_headroom_with(total: u64, configured: u64, reserve: u64) -> u64 {
    if configured > 0 {
        return configured;
    }
    (headroom_with(total, 0, 0) / 2).max(reserve)
}

/// [`memory_headroom`] with the driver reserve of the machine given.
pub fn headroom_with(total: u64, configured: u64, reserve: u64) -> u64 {
    if configured > 0 {
        return configured;
    }
    MIN_VRAM_HEADROOM
        .max(total / 100 * VRAM_HEADROOM_PERCENT)
        .max(reserve)
}
/// The reserve a load chosen by the full-offload rule ([`crate::full_offload`]) keeps free: what
/// the rule's own 90% limit ([`crate::prepare::VRAM_LIMIT`]) leaves of the card, at least the
/// driver reserve (a spill threshold on AMD Windows). The rule judged the estimate against that
/// limit already, so the automatic reserve of [`memory_headroom`] (1 GiB or 8%) must not be added
/// on top: on a 4 GB card it cut Qwen3-4B from 37 to 24 layers although 37 had fitted. A
/// configured value stays.
pub fn full_offload_headroom(total: u64, configured: u64) -> u64 {
    full_offload_headroom_with(total, configured, driver_reserve())
}

/// [`full_offload_headroom`] with the driver reserve of the machine given.
pub fn full_offload_headroom_with(total: u64, configured: u64, reserve: u64) -> u64 {
    if configured > 0 {
        return configured;
    }
    let limit = (total as f64 * crate::prepare::VRAM_LIMIT) as u64;
    total.saturating_sub(limit).max(reserve)
}

/// What a model may take of a card: `available` (free memory after the OS usage, see
/// [`os_available_vram`]) minus [`full_offload_headroom`]. The one figure the full-offload rule,
/// the gateway's admission and the Dashboard's choice compare an estimate with.
pub fn full_offload_need_limit(available: u64, total: u64, configured: u64) -> u64 {
    full_offload_need_limit_with(available, total, configured, driver_reserve())
}

/// [`full_offload_need_limit`] with the driver reserve of the machine given.
pub fn full_offload_need_limit_with(
    available: u64,
    total: u64,
    configured: u64,
    reserve: u64,
) -> u64 {
    available.saturating_sub(full_offload_headroom_with(total, configured, reserve))
}

/// Free dedicated memory of the primary GPU as the OS reports it (`total - used by all
/// processes`: NVML / nvidia-smi, DXGI and performance counters). Not the runtime's own free
/// figure: on Windows the `--list-devices` process does not count its own CUDA context (about
/// 650 MiB under-reported on an RTX 3050 Ti), which made the same load look 650 MiB too big.
/// `None` without a discrete GPU or a usage counter (CPU, unified memory).
pub fn os_available_vram(hw: &SystemHardwareInfo) -> Option<(u64, u64)> {
    if hw.recommended_backend == RecommendedBackend::Cpu
        || hw.unified_memory_budget_bytes.is_some_and(|b| b > 0)
    {
        return None;
    }
    let adapter = crate::gpu::list_adapters().into_iter().next()?;
    if !adapter.is_discrete {
        return None;
    }
    let (used, total) = crate::gpu::vram_usage(&adapter)?;
    (total > 0).then(|| (total.saturating_sub(used), total))
}

/// Device memory a llama.cpp process uses besides its buffers (driver context, staging).
pub const GPU_RUNTIME_OVERHEAD: u64 = 150 * MIB;
/// Host memory of the server process besides weights and KV (threads, HTTP, prompt cache
/// bookkeeping).
const HOST_OVERHEAD: u64 = 400 * MIB;
/// llama.cpp pads the context (and SWA cache) to multiples of 256 cells.
const CTX_PAD: u64 = 256;
/// `-ub` default.
const DEFAULT_UBATCH: u64 = 512;
/// `-np auto` in current llama-server builds: 4 slots sharing one unified KV cache.
pub const AUTO_SLOTS: u64 = 4;
/// `--ctx-checkpoints` Harness sends when the user set none (`params.toml` `harness_default`).
pub const DEFAULT_CTX_CHECKPOINTS: u64 = 4;

/// What to estimate. Field names are the same in the Tauri command, the Control API and
/// the CLI.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FitInput {
    /// Launch path of a local model: its GGUF header and measured runs are looked up.
    #[serde(default)]
    pub model_path: Option<String>,
    /// Size of all files of the model; 0 = read from `model_path`.
    #[serde(default)]
    pub model_size_bytes: u64,
    /// `-c`; 0 = the trained context of the model.
    #[serde(default)]
    pub context_size: u32,
    /// `-ngl`; anything above the layer count is a full offload.
    #[serde(default = "full_offload")]
    pub gpu_layers: u32,
    #[serde(default)]
    pub cache_type_k: Option<String>,
    #[serde(default)]
    pub cache_type_v: Option<String>,
    /// `-np`; `None` = automatic: [`fit`] applies Harness's rule ([`crate::slots`]), the
    /// lower-level estimate llama-server's own value (4 slots, unified KV).
    #[serde(default)]
    pub parallel_slots: Option<u32>,
    /// Who starts the server (decides the automatic slot count, see [`crate::slots`]).
    #[serde(default)]
    pub origin: crate::slots::SlotOrigin,
    /// `--ctx-checkpoints`; `None` = Harness's default (4).
    #[serde(default)]
    pub ctx_checkpoints: Option<u32>,
    /// `--kv-unified`; `None` = llama-server's default (unified only with `-np auto`).
    #[serde(default)]
    pub kv_unified: Option<bool>,
    /// `Some(false)` = flash attention explicitly off (bigger compute buffer).
    #[serde(default)]
    pub flash_attn: Option<bool>,
    /// `-ub` (physical batch); default 512.
    #[serde(default)]
    pub ubatch: Option<u32>,
    /// `--swa-full`: sliding-window layers keep the whole context.
    #[serde(default)]
    pub swa_full: bool,
    /// `--no-kv-offload`: KV cache stays in host memory.
    #[serde(default)]
    pub kv_offload: Option<bool>,
    /// Multimodal projector (`--mmproj`).
    #[serde(default)]
    pub mmproj_path: Option<String>,
    /// Projector size when there is no path (0 = none or read from `mmproj_path`).
    #[serde(default)]
    pub mmproj_size_bytes: u64,
    /// Automatic KV precision ([`crate::kv_policy`]): the cache types are chosen the way the
    /// launch will choose them (f16 unless the context does not fit) and `cache_type_k/v`
    /// are ignored. `false` = the given types (missing = f16).
    #[serde(default)]
    pub kv_auto: bool,
    /// Draft model of speculative decoding (`-md`): its weights and its own KV cache come on
    /// top of the target's.
    #[serde(default)]
    pub draft: Option<DraftFit>,
}

/// The draft model of a launch. llama-server gives it the target's context (b11333 has no
/// separate draft context any more) and f16 KV unless `-ctkd`/`-ctvd` say otherwise.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DraftFit {
    #[serde(default)]
    pub model_path: Option<String>,
    /// 0 = read from `model_path`.
    #[serde(default)]
    pub model_size_bytes: u64,
    /// `-ngld`; `None` = llama-server's default (`auto`: all layers).
    #[serde(default)]
    pub gpu_layers: Option<u32>,
    #[serde(default)]
    pub cache_type_k: Option<String>,
    #[serde(default)]
    pub cache_type_v: Option<String>,
}

fn full_offload() -> u32 {
    999
}

/// Where the memory goes, in bytes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MemoryEstimate {
    pub vram_bytes: u64,
    pub ram_bytes: u64,
    pub model_gpu_bytes: u64,
    pub model_host_bytes: u64,
    /// Whole KV cache (device + host).
    pub kv_bytes: u64,
    pub kv_gpu_bytes: u64,
    pub compute_gpu_bytes: u64,
    /// Compute buffer in host memory (CPU-only runs).
    pub compute_host_bytes: u64,
    /// llama.cpp's host-visible logits output buffer (one row per physical batch token).
    #[serde(default)]
    pub output_host_bytes: u64,
    pub mmproj_bytes: u64,
    pub overhead_bytes: u64,
    pub offloaded_layers: u64,
    pub total_layers: u64,
    /// KV cells of the full-attention layers (padded context).
    pub kv_cells: u64,
    /// Layers with sliding-window attention and their (smaller) cache size.
    pub swa_layers: u64,
    pub swa_cells: u64,
    /// Recurrent state of all sequences (part of `kv_bytes`; Mamba / gated delta net layers).
    #[serde(default)]
    pub recurrent_bytes: u64,
    /// Context checkpoints at most (host memory, part of `ram_bytes`): slots x
    /// `--ctx-checkpoints` x one window / state copy. 0 for models without SWA / recurrent
    /// layers (they need none).
    #[serde(default)]
    pub checkpoint_bytes: u64,
    /// Server slots the estimate assumes.
    #[serde(default)]
    pub slots: u64,
    /// Layer count and KV shape came from the GGUF header (else a size-based guess).
    pub exact: bool,
    /// Draft model (weights + its KV cache + compute) in device memory, part of `vram_bytes`.
    #[serde(default)]
    pub draft_gpu_bytes: u64,
    /// Draft model in host memory, part of `ram_bytes`.
    #[serde(default)]
    pub draft_host_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum EstimateSource {
    /// Computed from the GGUF header only.
    Estimate,
    /// Corrected with a measured load of the same model under other settings.
    Corrected,
    /// A load with exactly these settings was measured.
    Measured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum FitCategory {
    FullGpu,
    Tight,
    Hybrid,
    CpuHeavy,
    OomRisk,
}

/// Memory usable for the model.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MemoryBudget {
    /// Device memory for offload (0 = CPU only).
    pub vram_bytes: u64,
    pub ram_bytes: u64,
    /// Apple Silicon: `vram_bytes` is the GPU share of unified memory.
    pub unified: bool,
    /// Integrated GPU without dedicated memory (Intel HD/UHD/Iris, AMD APU): `vram_bytes` is
    /// the part of system RAM the driver lets it use, the same pool as `ram_bytes`.
    #[serde(default)]
    pub shared_igpu: bool,
    /// Integrated GPU: whether offloading the weights generates faster than keeping them on
    /// the CPU. From a llama-bench run of this model with both `-ngl 0` and a full offload
    /// (Performance > Benchmark); otherwise from the device class (older Intel HD/UHD/Iris:
    /// no). `None`: unknown, the memory budget decides.
    #[serde(default)]
    pub igpu_offload_faster: Option<bool>,
    /// The machine's backend is CUDA: its default build has no flash-attention kernel for
    /// mixed K/V cache types (see [`crate::kv_policy`]).
    #[serde(default)]
    pub cuda: bool,
    /// ROCm/HIP: matching K/V pairs avoid the measured slow generation path.
    #[serde(default)]
    pub rocm: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FitResult {
    pub category: FitCategory,
    pub estimate: MemoryEstimate,
    pub budget: MemoryBudget,
    pub vram_percentage: u32,
    pub ram_percentage: u32,
    pub source: EstimateSource,
    /// Unix ms of the measurement used (`source` corrected/measured).
    pub measured_at: Option<i64>,
    /// The `-ngl` that keeps the estimate under 90% of the budget (layer count + 1 = full).
    pub recommended_gpu_layers: u32,
    /// Concurrent requests (slots): the automatic choice or the set value and the
    /// recommendation ([`fit`] only).
    #[serde(default)]
    pub slots: Option<crate::slots::SlotAdvice>,
    /// The automatic KV precision the estimate used (`kv_auto` inputs only).
    #[serde(default)]
    pub kv: Option<crate::kv_policy::KvChoice>,
}

/// Memory budget of this machine: dedicated VRAM of the primary GPU, or the GPU share of
/// unified memory on Apple Silicon; 0 when the recommended backend is the CPU.
pub fn budget(hw: &SystemHardwareInfo) -> MemoryBudget {
    let ram_bytes = if hw.total_ram_bytes > 0 {
        hw.total_ram_bytes
    } else {
        16 * GIB
    };
    if hw.recommended_backend == RecommendedBackend::Cpu {
        return MemoryBudget {
            vram_bytes: 0,
            ram_bytes,
            unified: false,
            shared_igpu: false,
            igpu_offload_faster: None,
            cuda: false,
            rocm: false,
        };
    }
    if let Some(unified) = hw.unified_memory_budget_bytes.filter(|b| *b > 0) {
        return MemoryBudget {
            vram_bytes: unified,
            ram_bytes,
            unified: true,
            shared_igpu: false,
            igpu_offload_faster: None,
            cuda: false,
            rocm: false,
        };
    }
    let primary = hw.gpus.first();
    let dedicated = primary.and_then(|g| g.vram_bytes).unwrap_or(0);
    if dedicated == 0 && primary.is_some_and(|g| !g.is_discrete) {
        return MemoryBudget {
            vram_bytes: igpu_shared_bytes(ram_bytes),
            ram_bytes,
            unified: true,
            shared_igpu: true,
            igpu_offload_faster: primary.filter(|g| is_legacy_intel_igpu(g)).map(|_| false),
            cuda: false,
            rocm: hw.recommended_backend == RecommendedBackend::Rocm,
        };
    }
    MemoryBudget {
        vram_bytes: dedicated,
        ram_bytes,
        unified: false,
        shared_igpu: false,
        igpu_offload_faster: None,
        cuda: hw.recommended_backend == RecommendedBackend::Cuda12,
        rocm: hw.recommended_backend == RecommendedBackend::Rocm,
    }
}

/// Intel's integrated graphics before Xe (Gen8-Gen11: "HD Graphics", "UHD Graphics", "Iris
/// Graphics/Pro/Plus"). Measured on an HD 6000 (Broadwell, Vulkan): a full offload of Q4_K_M
/// 0.5-1.7B models generates 15-40% slower than `-ngl 0`. Xe / Arc and AMD APUs are not
/// included: newer integrated GPUs are usually faster than the CPU.
pub fn is_legacy_intel_igpu(g: &crate::hardware::GpuInfo) -> bool {
    let n = g.name.to_ascii_lowercase();
    g.vendor == crate::hardware::GpuVendor::Intel
        && !g.is_discrete
        && !n.contains(" xe")
        && !n.contains("arc")
        && (n.contains("hd graphics") || n.contains("uhd graphics") || n.contains("iris"))
}

/// Integrated GPU, from this machine's benchmark history: `Some(true)` when a run of this model
/// measured faster generation with a full offload than with `-ngl 0`.
pub fn igpu_offload_faster_measured(model_path: &str) -> Option<bool> {
    let runs = crate::benchmark::load_history();
    let target = crate::paths::normalize(std::path::Path::new(model_path));
    let same = |p: &std::path::Path| {
        let p = crate::paths::normalize(p);
        if cfg!(windows) {
            p.to_string_lossy()
                .eq_ignore_ascii_case(&target.to_string_lossy())
        } else {
            p == target
        }
    };
    let mut cpu: Option<(u64, f64)> = None;
    let mut gpu: Option<(u64, f64)> = None;
    for run in runs.iter().filter(|r| same(&r.request.model_path)) {
        for r in run
            .results
            .iter()
            .filter(|r| r.n_gen > 0 && r.n_prompt == 0)
        {
            let slot = if r.n_gpu_layers == 0 {
                &mut cpu
            } else if r.n_gpu_layers >= 99 {
                &mut gpu
            } else {
                continue;
            };
            if slot.is_none_or(|(at, _)| run.finished_at >= at) {
                *slot = Some((run.finished_at, r.avg_ts));
            }
        }
    }
    Some(gpu?.1 > cpu?.1)
}

/// [`budget`] for running `model_path`: on an integrated GPU, a benchmark of this model decides
/// whether offloading the weights is faster (see [`igpu_offload_faster_measured`]).
pub fn budget_for_model(hw: &SystemHardwareInfo, model_path: Option<&str>) -> MemoryBudget {
    let mut b = budget(hw);
    if b.shared_igpu {
        if let Some(faster) = model_path.and_then(igpu_offload_faster_measured) {
            b.igpu_offload_faster = Some(faster);
        }
    }
    b
}

/// System memory an integrated GPU may use: half of RAM (Windows' "shared GPU memory"; Mesa's
/// Vulkan heap on a 4 GB machine: 1912 MiB of 3.7 GiB, measured on an Intel HD 6000).
pub fn igpu_shared_bytes(ram_bytes: u64) -> u64 {
    ram_bytes / 2
}

/// Bytes per KV element of a llama.cpp cache type.
pub fn kv_bytes_per_element(cache_type: Option<&str>) -> f64 {
    match cache_type
        .map(|t| t.trim().to_ascii_lowercase())
        .as_deref()
        .unwrap_or("f16")
    {
        "f32" => 4.0,
        "q8_0" => 34.0 / 32.0,
        "q5_1" => 24.0 / 32.0,
        "q5_0" => 22.0 / 32.0,
        "q4_1" => 20.0 / 32.0,
        "q4_0" | "iq4_nl" => 18.0 / 32.0,
        _ => 2.0, // f16, bf16 and unknown
    }
}

/// Bytes of the f16 buffer a conversion of one layer's quantized K and/or V cache needs
/// (padded context x K/V width x 2 bytes; the widest layer, the buffer is reused).
pub fn kv_convert_layer_f16_bytes(
    input: &FitInput,
    meta: Option<&GgufMetadata>,
    k: bool,
    v: bool,
) -> u64 {
    let s = shape(input.model_size_bytes, meta);
    let ctx = match input.context_size {
        0 => meta.and_then(|m| m.context_length).unwrap_or(4096),
        c => u64::from(c),
    };
    let n_ctx = pad(ctx.max(1), CTX_PAD);
    let width = u64::from(k) * s.k_per_token + u64::from(v) * s.v_per_token;
    n_ctx * width * 2
}

/// Sliding-window pattern llama.cpp hard-codes for architectures whose GGUF has no
/// `attention.sliding_window_pattern` key (`set_swa_pattern(N)`: every N-th layer is full).
fn default_swa_period(arch: &str) -> Option<u64> {
    Some(match arch {
        "gpt-oss" | "gemma2" => 2,
        "gemma3" | "gemma-embedding" => 6,
        "gemma3n" => 5,
        "cohere2" | "exaone4" => 4,
        _ => return None,
    })
}

struct Shape {
    layers: u64,
    /// KV elements per token per full-attention layer (K and V).
    k_per_token: u64,
    v_per_token: u64,
    /// The same for the sliding-window layers (Gemma 4 has other head counts / sizes there).
    k_per_token_swa: u64,
    v_per_token_swa: u64,
    /// Layers with a KV cache (all but the recurrent ones).
    attn_layers: u64,
    swa_layers: u64,
    n_swa: u64,
    /// Recurrent layers and the bytes of one sequence's state on one of them.
    recurrent_layers: u64,
    recurrent_bytes_per_seq: u64,
    n_head: u64,
    ff: u64,
    exact: bool,
}

fn shape(size: u64, g: Option<&GgufMetadata>) -> Shape {
    let layers = g
        .and_then(|g| g.block_count)
        .filter(|&l| l > 0)
        .unwrap_or(if size > 30 * GIB {
            80
        } else if size > 10 * GIB {
            48
        } else {
            32
        });
    let heads = g.and_then(|g| g.head_count).filter(|&h| h > 0);
    let kv_heads = g.and_then(|g| g.head_count_kv).or(heads);
    let embd = g.and_then(|g| g.embedding_length);
    let head_k = g
        .and_then(|g| g.key_length)
        .or_else(|| Some(embd? / heads?));
    let head_v = g.and_then(|g| g.value_length).or(head_k);
    let exact = g.is_some_and(|g| g.block_count.is_some())
        && kv_heads.is_some()
        && head_k.is_some()
        && head_v.is_some();
    let (k_per_token, v_per_token, k_per_token_swa, v_per_token_swa) = if exact {
        let kv = kv_heads.unwrap_or(8);
        let kv_full = g.and_then(|g| g.head_count_kv_full).unwrap_or(kv);
        let kv_swa = g.and_then(|g| g.head_count_kv_swa).unwrap_or(kv);
        let (hk, hv) = (head_k.unwrap_or(128), head_v.unwrap_or(128));
        let hk_swa = g.and_then(|g| g.key_length_swa).unwrap_or(hk);
        let hv_swa = g.and_then(|g| g.value_length_swa).unwrap_or(hv);
        (kv_full * hk, kv_full * hv, kv_swa * hk_swa, kv_swa * hv_swa)
    } else {
        // Typical GQA model: 8 KV heads x 128 dims.
        (1024, 1024, 1024, 1024)
    };
    // Recurrent / hybrid: attention only on every `full_attention_interval`-th layer or where
    // the per-layer KV head count is not 0; pure state-space models have none.
    let recurrent = g.and_then(|g| g.ssm_state_size).is_some();
    let attn_layers = match g {
        Some(g) if recurrent => g
            .attention_layer_count
            .or_else(|| g.full_attention_interval.map(|n| layers / n))
            .unwrap_or(0)
            .min(layers),
        _ => layers,
    };
    let recurrent_layers = layers - attn_layers;
    // llama.cpp `n_embd_r` (conv state) + `n_embd_s` (SSM state), f32.
    let recurrent_bytes_per_seq = g
        .filter(|_| recurrent_layers > 0)
        .map(|g| {
            let d_state = g.ssm_state_size.unwrap_or(0);
            let d_inner = g.ssm_inner_size.unwrap_or(0);
            let groups = g.ssm_group_count.unwrap_or(0);
            let conv = g.ssm_conv_kernel.unwrap_or(1).saturating_sub(1);
            4 * (conv * (d_inner + 2 * groups * d_state) + d_state * d_inner)
        })
        .unwrap_or(0);
    let (swa_layers, n_swa) =
        match g.and_then(|g| g.known_metadata_fixes().sliding_window.map(|w| (g, w))) {
            Some((g, w)) => {
                let count = g.swa_layer_count.or_else(|| {
                    let period = g
                        .sliding_window_pattern
                        .or_else(|| default_swa_period(g.architecture.as_deref()?))?;
                    (period > 1).then(|| layers - layers / period)
                });
                (count.unwrap_or(0).min(attn_layers), w)
            }
            None => (0, 0),
        };
    // Widest feed-forward activation of one layer: dense FFN or the active experts of a MoE
    // layer; unknown -> the usual 4x embedding width.
    let ff = g
        .map(|g| {
            let dense = g.feed_forward_length.unwrap_or(0);
            let moe = g
                .expert_feed_forward_length
                .unwrap_or(0)
                .saturating_mul(g.expert_used_count.unwrap_or(0));
            match dense.max(moe) {
                0 => g.embedding_length.unwrap_or(0).saturating_mul(4),
                widest => widest,
            }
        })
        .unwrap_or(0);
    Shape {
        layers,
        k_per_token,
        v_per_token,
        k_per_token_swa,
        v_per_token_swa,
        attn_layers,
        swa_layers,
        n_swa,
        recurrent_layers,
        recurrent_bytes_per_seq,
        n_head: heads.unwrap_or(32),
        ff,
        exact,
    }
}

fn pad(v: u64, to: u64) -> u64 {
    v.div_ceil(to) * to
}

fn ratio(part: u64, of: u64, total: u64) -> u64 {
    if of == 0 {
        0
    } else {
        ((total as u128 * part as u128) / of as u128) as u64
    }
}

/// The estimate from the model's shape alone (no measurements). `gpu_budget` > 0 means a GPU
/// backend is available (else nothing is offloaded).
pub fn estimate_raw(
    input: &FitInput,
    meta: Option<&GgufMetadata>,
    gpu_available: bool,
) -> MemoryEstimate {
    let size = input.model_size_bytes;
    let s = shape(size, meta);
    let layers = s.layers;
    let ngl = u64::from(input.gpu_layers);
    let offloaded = if gpu_available { ngl.min(layers) } else { 0 };
    let output_on_gpu = gpu_available && ngl > layers;

    // Weights.
    let (model_gpu, model_host) = match meta.and_then(|m| m.token_embd_bytes.map(|t| (m, t))) {
        Some((m, tok)) if tok < size => {
            let tied = m.output_bytes.is_none();
            let out = m.output_bytes.unwrap_or(tok);
            let repeating = size.saturating_sub(tok + if tied { 0 } else { out });
            let gpu_rep = ratio(offloaded, layers, repeating);
            let gpu = gpu_rep + if output_on_gpu { out } else { 0 };
            let host = tok + (repeating - gpu_rep) + if output_on_gpu || tied { 0 } else { out };
            (gpu, host)
        }
        _ => {
            let gpu = if offloaded >= layers && gpu_available {
                size
            } else {
                ratio(offloaded, layers, size)
            };
            (gpu, size - gpu)
        }
    };

    // KV cache.
    let ctx = match input.context_size {
        0 => meta.and_then(|m| m.context_length).unwrap_or(4096),
        c => u64::from(c),
    };
    let n_ctx = pad(ctx.max(1), CTX_PAD);
    let ubatch = input
        .ubatch
        .map(u64::from)
        .filter(|&u| u > 0)
        .unwrap_or(DEFAULT_UBATCH);
    let n_seq = input
        .parallel_slots
        .map(u64::from)
        .filter(|&n| n > 0)
        .unwrap_or(AUTO_SLOTS);
    let unified = input.kv_unified.unwrap_or(input.parallel_slots.is_none());
    let swa_cells = if s.swa_layers == 0 {
        0
    } else if input.swa_full {
        n_ctx
    } else if unified {
        n_ctx.min(pad(s.n_swa * n_seq + ubatch, CTX_PAD))
    } else {
        (n_ctx / n_seq).min(pad(s.n_swa + ubatch, CTX_PAD)) * n_seq
    };
    let (bk, bv) = (
        kv_bytes_per_element(input.cache_type_k.as_deref()),
        kv_bytes_per_element(input.cache_type_v.as_deref()),
    );
    let per_cell = s.k_per_token as f64 * bk + s.v_per_token as f64 * bv;
    let per_cell_swa = s.k_per_token_swa as f64 * bk + s.v_per_token_swa as f64 * bv;
    let full_layers = s.attn_layers - s.swa_layers;
    let kv_layer_full = n_ctx as f64 * per_cell;
    let kv_layer_swa = swa_cells as f64 * per_cell_swa;
    // One recurrent state per sequence (`recurrent_rs_size = max(1, n_seq_max)`).
    let recurrent = s.recurrent_layers * s.recurrent_bytes_per_seq * n_seq;
    let kv_total = (full_layers as f64 * kv_layer_full + s.swa_layers as f64 * kv_layer_swa) as u64
        + recurrent;
    // Context checkpoints (host memory): each a copy of one sequence's window / state; every
    // slot keeps up to `--ctx-checkpoints` of them. `--swa-full` windows need none.
    let window_copy = if input.swa_full {
        0
    } else {
        (s.swa_layers as f64 * s.n_swa.min(n_ctx) as f64 * per_cell_swa) as u64
    };
    let checkpoint_bytes = (window_copy + s.recurrent_layers * s.recurrent_bytes_per_seq)
        * n_seq
        * input
            .ctx_checkpoints
            .map_or(DEFAULT_CTX_CHECKPOINTS, u64::from);
    let kv_gpu = if input.kv_offload == Some(false) {
        0
    } else {
        ratio(offloaded, layers, kv_total)
    };

    // Compute buffer.
    let compute = {
        let base = if s.ff > 0 {
            4 * ubatch * s.ff * 4
        } else {
            128 * MIB
        };
        let mask = ubatch * n_ctx * 2;
        let kq = if input.flash_attn == Some(false) {
            ubatch * n_ctx * s.n_head * 4
        } else {
            0
        };
        base + mask + kq
    };

    let mmproj = input.mmproj_size_bytes;
    // llama.cpp reserves logits for the physical batch even when only the last row is
    // requested. Vulkan_Host / CPU output is system memory, not dedicated VRAM.
    let output_host = meta
        .and_then(|m| m.tokenizer.as_ref())
        .and_then(|t| t.vocab_size)
        .unwrap_or(0)
        * ubatch
        * 4;
    let uses_gpu = offloaded > 0 || (gpu_available && mmproj > 0);
    let overhead = if uses_gpu { GPU_RUNTIME_OVERHEAD } else { 0 };
    let (compute_gpu, compute_host) = if offloaded > 0 {
        (compute, 0)
    } else {
        (0, compute)
    };
    let (mmproj_gpu, mmproj_host) = if gpu_available {
        (mmproj, 0)
    } else {
        (0, mmproj)
    };

    MemoryEstimate {
        vram_bytes: model_gpu + kv_gpu + compute_gpu + mmproj_gpu + overhead,
        ram_bytes: model_host
            + (kv_total - kv_gpu)
            + compute_host
            + output_host
            + mmproj_host
            + checkpoint_bytes
            + HOST_OVERHEAD,
        model_gpu_bytes: model_gpu,
        model_host_bytes: model_host,
        kv_bytes: kv_total,
        kv_gpu_bytes: kv_gpu,
        compute_gpu_bytes: compute_gpu,
        compute_host_bytes: compute_host,
        output_host_bytes: output_host,
        mmproj_bytes: mmproj,
        overhead_bytes: overhead,
        offloaded_layers: offloaded,
        total_layers: layers,
        kv_cells: n_ctx,
        swa_layers: s.swa_layers,
        swa_cells,
        recurrent_bytes: recurrent,
        checkpoint_bytes,
        slots: n_seq,
        exact: s.exact,
        draft_gpu_bytes: 0,
        draft_host_bytes: 0,
    }
}

/// Adds the draft model to an estimate of the target: the draft estimated like a model of its
/// own at the target's context (padded cells) and slots, its cache types, `-ngld` layers. The
/// runtime overhead is the target's (one process). Real case (RTX 3050 Ti Laptop, b11333,
/// Qwen3-4B Q4_K_M + Qwen3-0.6B Q8_0 all on the GPU): +862 MiB at 2k, +1098 MiB at 4k.
fn add_draft(input: &FitInput, est: &mut MemoryEstimate, gpu_available: bool) {
    let Some(d) = input.draft.as_ref().filter(|d| d.model_size_bytes > 0) else {
        return;
    };
    let meta = d
        .model_path
        .as_deref()
        .and_then(|p| crate::models::cached_metadata(std::path::Path::new(p)));
    let draft = FitInput {
        model_path: d.model_path.clone(),
        model_size_bytes: d.model_size_bytes,
        context_size: u32::try_from(est.kv_cells).unwrap_or(u32::MAX),
        gpu_layers: d.gpu_layers.unwrap_or_else(full_offload),
        cache_type_k: d.cache_type_k.clone(),
        cache_type_v: d.cache_type_v.clone(),
        parallel_slots: input.parallel_slots,
        kv_unified: input.kv_unified,
        flash_attn: input.flash_attn,
        ubatch: input.ubatch,
        kv_offload: input.kv_offload,
        ..Default::default()
    };
    let r = estimate_raw(&draft, meta.as_ref(), gpu_available);
    let gpu = r.model_gpu_bytes + r.kv_gpu_bytes + r.compute_gpu_bytes;
    let host = r.model_host_bytes + (r.kv_bytes - r.kv_gpu_bytes) + r.compute_host_bytes;
    est.draft_gpu_bytes = gpu;
    est.draft_host_bytes = host;
    est.vram_bytes += gpu;
    est.ram_bytes += host;
}

// ---------------------------------------------------------------------------------------
// Measurements

/// Settings that decide the footprint; part of the measurement key.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MeasuredSettings {
    pub context_size: u32,
    pub gpu_layers: u32,
    pub cache_type_k: String,
    pub cache_type_v: String,
    pub parallel_slots: Option<u32>,
    pub kv_unified: Option<bool>,
    pub flash_attn_off: bool,
    pub ubatch: Option<u32>,
    pub swa_full: bool,
    pub kv_offload: bool,
    pub mmproj: bool,
}

impl MeasuredSettings {
    fn from_input(input: &FitInput, total_layers: u64) -> Self {
        let cache = |c: &Option<String>| {
            c.as_deref()
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "f16".into())
        };
        Self {
            context_size: input.context_size,
            // 99 and 999 are the same full offload.
            gpu_layers: input.gpu_layers.min(total_layers as u32 + 1),
            cache_type_k: cache(&input.cache_type_k),
            cache_type_v: cache(&input.cache_type_v),
            parallel_slots: input.parallel_slots.filter(|&n| n > 0),
            kv_unified: input.kv_unified,
            flash_attn_off: input.flash_attn == Some(false),
            ubatch: input
                .ubatch
                .filter(|&u| u > 0 && u64::from(u) != DEFAULT_UBATCH),
            swa_full: input.swa_full,
            kv_offload: input.kv_offload != Some(false),
            mmproj: input.mmproj_size_bytes > 0,
        }
    }

    pub fn key(&self) -> String {
        format!(
            "c{}|ngl{}|k{}|v{}|np{}|kvu{}|fa{}|ub{}|swa{}|kvo{}|mm{}",
            self.context_size,
            self.gpu_layers,
            self.cache_type_k,
            self.cache_type_v,
            self.parallel_slots.map_or("auto".into(), |n| n.to_string()),
            self.kv_unified
                .map_or("auto", |b| if b { "1" } else { "0" }),
            if self.flash_attn_off { "off" } else { "auto" },
            self.ubatch.unwrap_or(DEFAULT_UBATCH as u32),
            self.swa_full as u8,
            self.kv_offload as u8,
            self.mmproj as u8,
        )
    }

    fn to_input(&self, base: &FitInput) -> FitInput {
        FitInput {
            context_size: self.context_size,
            gpu_layers: self.gpu_layers,
            cache_type_k: Some(self.cache_type_k.clone()),
            cache_type_v: Some(self.cache_type_v.clone()),
            parallel_slots: self.parallel_slots,
            kv_unified: self.kv_unified,
            flash_attn: self.flash_attn_off.then_some(false),
            ubatch: self.ubatch,
            swa_full: self.swa_full,
            kv_offload: Some(self.kv_offload),
            mmproj_size_bytes: if self.mmproj {
                base.mmproj_size_bytes
            } else {
                0
            },
            ..base.clone()
        }
    }
}

/// Buffer sizes a llama-server load printed (bytes), from [`parse_load_log`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LoadLog {
    /// Buffer name (`Vulkan0`, `CUDA0`, `CPU_Mapped`...) -> bytes.
    pub model_buffers: BTreeMap<String, u64>,
    pub kv_buffers: BTreeMap<String, u64>,
    pub compute_buffers: BTreeMap<String, u64>,
    /// `llama_kv_cache: size = ... (N cells, L layers ...)`, one per cache (two with SWA).
    pub kv_caches: Vec<(u64, u64)>,
    pub offloaded_layers: Option<u64>,
    pub total_layers: Option<u64>,
    pub n_ctx: Option<u64>,
    pub n_seq_max: Option<u64>,
}

/// Host-side buffers (`CPU`, `CPU_Mapped`, `CPU_REPACK`, `Vulkan_Host`, `CUDA_Host`...).
pub fn is_host_buffer(name: &str) -> bool {
    name == "CPU" || name.starts_with("CPU_") || name.ends_with("_Host") || name.contains("Host")
}

impl LoadLog {
    fn sum(map: &BTreeMap<String, u64>, host: bool) -> u64 {
        map.iter()
            .filter(|(k, _)| is_host_buffer(k) == host)
            .map(|(_, v)| *v)
            .sum()
    }
    pub fn model_gpu(&self) -> u64 {
        Self::sum(&self.model_buffers, false)
    }
    pub fn model_host(&self) -> u64 {
        Self::sum(&self.model_buffers, true)
    }
    pub fn kv_gpu(&self) -> u64 {
        Self::sum(&self.kv_buffers, false)
    }
    pub fn kv_host(&self) -> u64 {
        Self::sum(&self.kv_buffers, true)
    }
    pub fn compute_gpu(&self) -> u64 {
        Self::sum(&self.compute_buffers, false)
    }
    pub fn is_empty(&self) -> bool {
        self.model_buffers.is_empty() && self.kv_buffers.is_empty()
    }
}

fn mib_to_bytes(text: &str) -> u64 {
    (text.parse::<f64>().unwrap_or(0.0) * MIB as f64).round() as u64
}

/// Parses the buffer sizes of the **last** model load in `lines` (llama-server prints them
/// at log verbosity 4, `-lv 4` / `--verbose`; lines may carry prefixes such as `[stderr]`
/// and timestamps).
pub fn parse_load_log(lines: &[String]) -> LoadLog {
    static RE: OnceLock<[Regex; 7]> = OnceLock::new();
    let [model, kv, kv_size, compute, offload, n_ctx, n_seq] = RE.get_or_init(|| {
        [
            Regex::new(r"load_tensors:\s+(\S+) model buffer size =\s*([\d.]+) MiB").unwrap(),
            Regex::new(r"llama_kv_cache:\s+(\S+) KV buffer size =\s*([\d.]+) MiB").unwrap(),
            Regex::new(r"llama_kv_cache: size =\s*[\d.]+ MiB \(\s*(\d+) cells,\s*(\d+) layers")
                .unwrap(),
            Regex::new(
                r"(?:sched_reserve|llama_context|graph_reserve):\s+(\S+) compute buffer size =\s*([\d.]+) MiB",
            )
            .unwrap(),
            Regex::new(r"offloaded (\d+)/(\d+) layers to GPU").unwrap(),
            Regex::new(r"llama_context: n_ctx\s+=\s+(\d+)").unwrap(),
            Regex::new(r"llama_context: n_seq_max\s+=\s+(\d+)").unwrap(),
        ]
    });
    // Only the last load (a restart appends to the same buffer).
    let start = lines
        .iter()
        .rposition(|l| l.contains("load_model: loading model"))
        .unwrap_or(0);
    let mut out = LoadLog::default();
    // A draft model's load follows the main one; stop at a second `load_tensors` block
    // after the main context was created.
    let mut main_context_done = false;
    for line in &lines[start..] {
        if let Some(c) = model.captures(line) {
            if main_context_done {
                break;
            }
            *out.model_buffers.entry(c[1].to_string()).or_default() += mib_to_bytes(&c[2]);
        } else if let Some(c) = kv.captures(line) {
            *out.kv_buffers.entry(c[1].to_string()).or_default() += mib_to_bytes(&c[2]);
        } else if let Some(c) = kv_size.captures(line) {
            out.kv_caches
                .push((c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0)));
        } else if let Some(c) = compute.captures(line) {
            *out.compute_buffers.entry(c[1].to_string()).or_default() += mib_to_bytes(&c[2]);
            main_context_done = true;
        } else if let Some(c) = offload.captures(line) {
            if out.offloaded_layers.is_none() {
                out.offloaded_layers = c[1].parse().ok();
                out.total_layers = c[2].parse().ok();
            }
        } else if let Some(c) = n_ctx.captures(line) {
            out.n_ctx.get_or_insert(c[1].parse().unwrap_or(0));
        } else if let Some(c) = n_seq.captures(line) {
            out.n_seq_max.get_or_insert(c[1].parse().unwrap_or(0));
        }
    }
    out
}

/// A measured load (stored as JSON in `state.db`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Measurement {
    pub model_key: String,
    pub model_size_bytes: u64,
    pub settings: MeasuredSettings,
    /// From the log (`None` when the log did not print buffer sizes).
    pub log: Option<LoadLog>,
    /// Dedicated GPU memory of the server process as the OS reports it (Windows PDH
    /// `GPU Process Memory`, `nvidia-smi --query-compute-apps`, amdgpu `fdinfo`).
    pub process_vram_bytes: Option<u64>,
    pub build: Option<u64>,
    /// Unix ms.
    pub measured_at: i64,
}

fn clamp_ratio(measured: u64, estimated: u64) -> Option<f64> {
    (measured > 0 && estimated > 0).then(|| (measured as f64 / estimated as f64).clamp(0.25, 4.0))
}

fn scale(v: u64, r: Option<f64>) -> u64 {
    r.map_or(v, |r| (v as f64 * r).round() as u64)
}

/// Applies a measurement of the same model: every component is scaled by measured /
/// estimated at the measured settings, so the same settings reproduce the measurement and
/// other settings inherit the correction.
pub fn correct(
    input: &FitInput,
    meta: Option<&GgufMetadata>,
    gpu_available: bool,
    m: &Measurement,
) -> MemoryEstimate {
    let now = estimate_raw(input, meta, gpu_available);
    let then = estimate_raw(&m.settings.to_input(input), meta, gpu_available);
    let mut out = now.clone();
    if let Some(log) = m.log.as_ref().filter(|l| !l.is_empty()) {
        let r_model = clamp_ratio(log.model_gpu(), then.model_gpu_bytes);
        let r_kv = clamp_ratio(log.kv_gpu() + log.kv_host(), then.kv_bytes);
        let r_compute = clamp_ratio(log.compute_gpu(), then.compute_gpu_bytes);
        out.model_gpu_bytes = scale(now.model_gpu_bytes, r_model);
        out.kv_bytes = scale(now.kv_bytes, r_kv);
        out.kv_gpu_bytes = scale(now.kv_gpu_bytes, r_kv);
        out.compute_gpu_bytes = scale(now.compute_gpu_bytes, r_compute);
        if let Some(process) = m.process_vram_bytes {
            let buffers = log.model_gpu() + log.kv_gpu() + log.compute_gpu() + then.mmproj_bytes;
            if now.overhead_bytes > 0 && process > buffers {
                out.overhead_bytes = (process - buffers).min(2 * GIB);
            }
        }
    } else if let Some(process) = m.process_vram_bytes.filter(|p| *p > 0) {
        // Only the total is known: carry the difference as overhead.
        if now.overhead_bytes > 0 {
            let delta = process as i64 - then.vram_bytes as i64;
            out.overhead_bytes =
                (now.overhead_bytes as i64 + delta).clamp(0, 4 * GIB as i64) as u64;
        }
    }
    let mmproj_gpu = if gpu_available { out.mmproj_bytes } else { 0 };
    out.vram_bytes = out.model_gpu_bytes
        + out.kv_gpu_bytes
        + out.compute_gpu_bytes
        + mmproj_gpu
        + out.overhead_bytes;
    out.ram_bytes = out.model_host_bytes
        + out.kv_bytes.saturating_sub(out.kv_gpu_bytes)
        + out.compute_host_bytes
        + out.output_host_bytes
        + (out.mmproj_bytes - mmproj_gpu)
        + out.checkpoint_bytes
        + HOST_OVERHEAD;
    out
}

fn categorize(est: &MemoryEstimate, budget: &MemoryBudget) -> FitCategory {
    let vram = budget.vram_bytes as f64;
    let ram = budget.ram_bytes as f64;
    let full = est.total_layers > 0 && est.offloaded_layers >= est.total_layers;
    let v = est.vram_bytes as f64;
    let r = est.ram_bytes as f64;
    // An integrated GPU's memory is system RAM: offloaded weights still come out of the same
    // pool, so both parts together must fit.
    if budget.shared_igpu && v + r > ram * 0.9 {
        return FitCategory::OomRisk;
    }
    // Windows and open apps keep 1.5-2.5 GB of VRAM busy. Past ~85% the driver starts
    // spilling into shared system memory, which makes prompt processing several times slower.
    if vram > 0.0 && full && v <= vram * 0.85 {
        FitCategory::FullGpu
    } else if vram > 0.0 && full && v <= vram {
        FitCategory::Tight
    } else if vram > 0.0 && est.offloaded_layers > 0 && v <= vram && r <= ram * 0.9 {
        FitCategory::Hybrid
    } else if v <= vram && r <= ram * 0.9 {
        FitCategory::CpuHeavy
    } else {
        FitCategory::OomRisk
    }
}

fn percent(part: u64, of: u64) -> u32 {
    if of == 0 {
        0
    } else {
        ((part as f64 / of as f64) * 100.0).round() as u32
    }
}

/// The estimate for `input`, corrected with the newest matching measurement of `measured`
/// (same model, same file size); an exact settings match wins.
pub fn fit_with(
    input: &FitInput,
    meta: Option<&GgufMetadata>,
    budget: &MemoryBudget,
    measured: &[Measurement],
) -> FitResult {
    if input.kv_auto {
        let v_quant_ok = input.flash_attn != Some(false);
        let (choice, mut r) = crate::kv_policy::choose(input, meta, budget, measured, v_quant_ok);
        r.kv = Some(choice);
        return r;
    }
    let gpu = budget.vram_bytes > 0;
    let raw = estimate_raw(input, meta, gpu);
    let total_layers = raw.total_layers;
    let key = MeasuredSettings::from_input(input, total_layers).key();
    let usable: Vec<&Measurement> = measured
        .iter()
        .filter(|m| m.model_size_bytes == input.model_size_bytes || input.model_size_bytes == 0)
        .collect();
    let exact = usable.iter().find(|m| m.settings.key() == key);
    let newest = usable.iter().max_by_key(|m| m.measured_at);
    let (mut estimate, source, measured_at) = match exact.or(newest) {
        Some(m) if gpu => (
            correct(input, meta, gpu, m),
            if exact.is_some() {
                EstimateSource::Measured
            } else {
                EstimateSource::Corrected
            },
            Some(m.measured_at),
        ),
        _ => (raw, EstimateSource::Estimate, None),
    };
    // CUDA converts a mixed K/V pair to f16 on every token: the temporary buffer is extra.
    if budget.cuda && estimate.offloaded_layers > 0 {
        let extra = crate::kv_policy::cuda_convert_buffer_bytes(input, meta);
        estimate.compute_gpu_bytes += extra;
        estimate.vram_bytes += extra;
    }
    add_draft(input, &mut estimate, gpu);
    // The target's layers get what the draft leaves.
    let target_budget = MemoryBudget {
        vram_bytes: budget.vram_bytes.saturating_sub(estimate.draft_gpu_bytes),
        ..*budget
    };
    let recommended = measured_full_offload(input, meta, total_layers, &target_budget, &usable)
        .unwrap_or_else(|| {
            recommend_gpu_layers(input, meta, &target_budget, exact.or(newest).copied())
        });
    FitResult {
        category: categorize(&estimate, budget),
        vram_percentage: percent(estimate.vram_bytes, budget.vram_bytes),
        ram_percentage: percent(estimate.ram_bytes, budget.ram_bytes),
        estimate,
        budget: *budget,
        source,
        measured_at,
        recommended_gpu_layers: recommended,
        slots: None,
        kv: None,
    }
}

/// Share of dedicated VRAM a measured full offload may use: the number is measured, not
/// estimated, so it needs no estimate margin, only room for what other apps take later.
const MEASURED_FULL_LIMIT: f64 = 0.97;

/// Full offload when a full-offload run of this model with the same context and cache types
/// was measured and the full offload, corrected by it, stays below [`MEASURED_FULL_LIMIT`].
/// Slots may differ (the form's automatic count is not always the launch's): the correction
/// carries them. Real case (RTX 3050 Ti Laptop 4 GB, WDDM, Qwen3-4B Q4_K_M 16k q8_0): 3801 of
/// 3962 MiB with 2 slots, no spill to shared memory, 55 t/s per request; the 90% rule kept
/// recommending -ngl 36 (and -ngl 33 gave 35 t/s).
fn measured_full_offload(
    input: &FitInput,
    meta: Option<&GgufMetadata>,
    total_layers: u64,
    budget: &MemoryBudget,
    measured: &[&Measurement],
) -> Option<u32> {
    if budget.vram_bytes == 0 || budget.unified || budget.shared_igpu {
        return None;
    }
    let full_layers = total_layers as u32 + 1;
    let full = FitInput {
        gpu_layers: full_layers,
        ..input.clone()
    };
    let want = MeasuredSettings::from_input(&full, total_layers);
    let m = measured
        .iter()
        .filter(|m| {
            let s = &m.settings;
            s.gpu_layers == full_layers
                && s.context_size == want.context_size
                && s.cache_type_k == want.cache_type_k
                && s.cache_type_v == want.cache_type_v
                && m.process_vram_bytes.is_some()
        })
        .max_by_key(|m| m.measured_at)?;
    let mut need = correct(&full, meta, true, m).vram_bytes;
    if budget.cuda {
        need += crate::kv_policy::cuda_convert_buffer_bytes(&full, meta);
    }
    (need as f64 <= budget.vram_bytes as f64 * MEASURED_FULL_LIMIT).then_some(full_layers)
}

/// Largest `-ngl` whose estimate stays under 90% of the VRAM budget (layer count + 1 for a
/// full offload including the output layer; 0 = CPU only).
pub fn recommend_gpu_layers(
    input: &FitInput,
    meta: Option<&GgufMetadata>,
    budget: &MemoryBudget,
    measurement: Option<&Measurement>,
) -> u32 {
    if budget.vram_bytes == 0 {
        return 0;
    }
    // Integrated GPU where the weights generate faster on the CPU (measured, or an older Intel
    // iGPU): keep them there. Memory is the same RAM either way, and with -ngl 0 llama.cpp
    // still sends large prompt batches to the GPU.
    if budget.shared_igpu && budget.igpu_offload_faster == Some(false) {
        return 0;
    }
    let limit = budget.vram_bytes as f64 * 0.9;
    let total = estimate_raw(input, meta, true).total_layers;
    let vram_at = |n: u64| {
        let i = FitInput {
            gpu_layers: n as u32,
            ..input.clone()
        };
        match measurement {
            Some(m) => correct(&i, meta, true, m).vram_bytes,
            None => estimate_raw(&i, meta, true).vram_bytes,
        }
    };
    if vram_at(total + 1) as f64 <= limit {
        return total as u32 + 1;
    }
    // VRAM grows with the layer count: binary search the largest fitting value.
    let (mut lo, mut hi) = (0u64, total);
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if vram_at(mid) as f64 <= limit {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo as u32
}

// ---------------------------------------------------------------------------------------
// Process-wide store

type Cache = HashMap<String, Vec<Measurement>>;

fn cache() -> &'static Mutex<Option<Cache>> {
    static CACHE: OnceLock<Mutex<Option<Cache>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn load_all(db: &crate::state::StateDb) -> Cache {
    let mut map: Cache = HashMap::new();
    for row in db.vram_measurements().unwrap_or_default() {
        if let Ok(m) = serde_json::from_str::<Measurement>(&row.data_json) {
            map.entry(row.model_key).or_default().push(m);
        }
    }
    map
}

/// Measurements of a model file (normalized key, see [`crate::capabilities::model_key`]).
pub fn measurements_for(model_path: &str) -> Vec<Measurement> {
    let key = crate::capabilities::model_key(model_path);
    let mut guard = cache().lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Some(
            crate::state::shared()
                .map(|db| load_all(&db))
                .unwrap_or_default(),
        );
    }
    guard
        .as_ref()
        .and_then(|c| c.get(&key).cloned())
        .unwrap_or_default()
}

fn file_size(path: &str) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Fills in what can be read from disk (model and projector sizes).
pub fn complete_input(mut input: FitInput) -> FitInput {
    if input.model_size_bytes == 0 {
        if let Some(p) = input.model_path.as_deref() {
            input.model_size_bytes = file_size(p);
        }
    }
    if input.mmproj_size_bytes == 0 {
        if let Some(p) = input.mmproj_path.as_deref().filter(|p| !p.is_empty()) {
            input.mmproj_size_bytes = file_size(p);
        }
    }
    if let Some(d) = input.draft.as_mut().filter(|d| d.model_size_bytes == 0) {
        if let Some(p) = d.model_path.as_deref().filter(|p| !p.is_empty()) {
            d.model_size_bytes = file_size(p);
        }
    }
    input
}

/// Estimate for this machine: GGUF header and measurements are looked up by `model_path`.
pub fn fit(input: FitInput, hw: &SystemHardwareInfo) -> FitResult {
    let input = complete_input(input);
    let meta = input
        .model_path
        .as_deref()
        .and_then(|p| crate::models::cached_metadata(std::path::Path::new(p)));
    let measured = input
        .model_path
        .as_deref()
        .map(measurements_for)
        .unwrap_or_default();
    let budget = budget_for_model(hw, input.model_path.as_deref());
    fit_auto(&input, meta.as_ref(), &budget, &measured, hw)
}

/// Memory estimate of the exact launch form. Unlike a UI-side `LaunchConfig` -> `FitInput`
/// conversion, this includes custom slot arguments and the selected runtime's unified-KV
/// capability through the same launch planner used by [`crate::process::ProcessManager`].
pub fn fit_launch(config: &crate::process::LaunchConfig, hw: &SystemHardwareInfo) -> FitResult {
    let base = base_input_from_launch(config);
    let path = base.model_path.clone().unwrap_or_default();
    let meta = crate::models::cached_metadata(std::path::Path::new(&path));
    let measured = measurements_for(&path);
    let budget = budget_for_model(hw, Some(&path));
    fit_launch_with(
        config,
        &base,
        meta.as_ref(),
        &budget,
        &measured,
        crate::slots::runtime_has_unified(config),
        hw.recommended_backend,
    )
}

/// Pure form/launch estimate for tests and callers that already have model and machine facts.
pub fn fit_launch_with(
    config: &crate::process::LaunchConfig,
    base: &FitInput,
    meta: Option<&GgufMetadata>,
    budget: &MemoryBudget,
    measured: &[Measurement],
    has_unified: bool,
    backend: RecommendedBackend,
) -> FitResult {
    let plan =
        crate::slots::plan_for_launch_with(config, base, meta, budget, measured, has_unified);
    let input = match plan {
        Some(ref plan) => FitInput {
            parallel_slots: Some(plan.advice.slots),
            kv_unified: plan.kv_unified.or(base.kv_unified),
            ..base.clone()
        },
        None => base.clone(),
    };
    let mut result = fit_with(&input, meta, budget, measured);
    result.slots = plan.map(|p| p.advice);
    if let Some(kv) = result.kv.as_mut() {
        kv.prompt_slowdown_pct = crate::kv_policy::prompt_slowdown_pct(kv.level, backend);
    }
    result
}

/// [`fit_with`] where `parallel_slots: None` means Harness's automatic slot count
/// ([`crate::slots::advise`]); the KV precision (`kv_auto`) is then chosen for those slots.
/// The result carries both decisions.
pub fn fit_auto(
    input: &FitInput,
    meta: Option<&GgufMetadata>,
    budget: &MemoryBudget,
    measured: &[Measurement],
    hw: &SystemHardwareInfo,
) -> FitResult {
    let advice = crate::slots::advise(input, meta, budget, measured);
    let input = crate::slots::with_slots(input, advice.slots);
    let mut r = fit_with(&input, meta, budget, measured);
    r.slots = Some(advice);
    if let Some(kv) = r.kv.as_mut() {
        kv.prompt_slowdown_pct =
            crate::kv_policy::prompt_slowdown_pct(kv.level, hw.recommended_backend);
    }
    r
}

/// The fit input a launch config corresponds to, with the slots it runs with (an automatic
/// count resolved by [`crate::slots::plan_for_launch`]). A config without cache types (and
/// without `-ctk`/`-ctv` in its own arguments) gets the automatic KV precision, as the launch
/// does.
pub fn input_from_launch(config: &crate::process::LaunchConfig) -> FitInput {
    let base = base_input_from_launch(config);
    match crate::slots::plan_for_launch(config, &base) {
        Some(plan) => FitInput {
            parallel_slots: Some(plan.advice.slots),
            kv_unified: plan.kv_unified.or(base.kv_unified),
            ..base
        },
        None => base,
    }
}

/// [`input_from_launch`] without resolving the slots (`parallel_slots` as configured).
pub fn base_input_from_launch(config: &crate::process::LaunchConfig) -> FitInput {
    use crate::params::ParamValue;
    let flag = |id: &str| match config.params.get(id) {
        Some(ParamValue::Bool(b)) => Some(*b),
        _ => None,
    };
    let mmproj = config
        .mmproj_path
        .as_ref()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_string_lossy().to_string());
    complete_input(FitInput {
        model_path: Some(config.model_path.to_string_lossy().to_string()),
        model_size_bytes: 0,
        context_size: config.context_size,
        gpu_layers: config.gpu_layers,
        cache_type_k: config.cache_type_k.clone().filter(|s| !s.is_empty()),
        cache_type_v: config.cache_type_v.clone().filter(|s| !s.is_empty()),
        parallel_slots: config.parallel_slots.filter(|&n| n > 0),
        kv_unified: flag("kv_unified"),
        origin: config.launched_by,
        ctx_checkpoints: match config.params.get("ctx_checkpoints") {
            Some(ParamValue::Int(n)) => u32::try_from(*n).ok(),
            _ => None,
        },
        flash_attn: crate::kv_policy::custom_args_disable_flash_attn(config.custom_args.as_deref())
            .then_some(false),
        ubatch: config.physical_batch,
        swa_full: flag("swa_full").unwrap_or(false),
        kv_offload: flag("kv_offload"),
        mmproj_path: mmproj,
        mmproj_size_bytes: 0,
        kv_auto: crate::kv_policy::is_automatic(config),
        draft: config
            .draft_model_path
            .as_ref()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| DraftFit {
                model_path: Some(p.to_string_lossy().to_string()),
                gpu_layers: config.draft_gpu_layers,
                ..Default::default()
            }),
    })
}

/// After a successful start: parses the load log, asks the OS for the process's GPU memory
/// and stores the measured footprint (single-model loads without a draft model only).
/// Returns what was stored. Blocking (process query + SQLite).
pub fn record_after_load(
    config: &crate::process::LaunchConfig,
    lines: &[String],
    pid: Option<u32>,
    build: Option<u64>,
) -> Option<Measurement> {
    if config.router_mode
        || config
            .draft_model_path
            .as_ref()
            .is_some_and(|p| !p.as_os_str().is_empty())
    {
        return None;
    }
    let input = input_from_launch(config);
    let log = parse_load_log(lines);
    let process_vram_bytes = pid.and_then(crate::gpu::process_vram_bytes);
    if log.is_empty() && process_vram_bytes.is_none() {
        return None;
    }
    let path = input.model_path.clone()?;
    let meta = crate::models::cached_metadata(std::path::Path::new(&path));
    let layers = log
        .total_layers
        .map(|t| t.saturating_sub(1))
        .filter(|&l| l > 0)
        .or_else(|| meta.as_ref().and_then(|m| m.block_count))
        .unwrap_or(999);
    let m = Measurement {
        model_key: crate::capabilities::model_key(&path),
        model_size_bytes: input.model_size_bytes,
        settings: MeasuredSettings::from_input(&input, layers),
        log: (!log.is_empty()).then_some(log),
        process_vram_bytes,
        build,
        measured_at: crate::state::now_ms(),
    };
    store(&m);
    Some(m)
}

/// Persists a measurement and refreshes the in-memory cache.
pub fn store(m: &Measurement) {
    let Ok(db) = crate::state::shared() else {
        return;
    };
    let row = crate::state::vram::MeasurementRow {
        model_key: m.model_key.clone(),
        settings_key: m.settings.key(),
        model_size: m.model_size_bytes,
        measured_at: m.measured_at,
        data_json: serde_json::to_string(m).unwrap_or_default(),
    };
    if let Err(e) = db.put_vram_measurement(&row) {
        eprintln!("[Harness Studio] VRAM measurement not saved: {e}");
        return;
    }
    let mut guard = cache().lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(load_all(&db));
}

#[cfg(test)]
#[path = "vram_tests.rs"]
mod tests;
