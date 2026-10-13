//! Conversation-memory precision (KV cache types, `--cache-type-k` / `--cache-type-v`).
//!
//! Policy (measured on this project's reference machine, see `docs/en/PERFORMANCE.md`
//! "KV cache precision"):
//! - **Automatic** (the definition sets no cache type): f16 while the requested context fits
//!   in memory. Only when it does not, the cache steps down, one rung at a time, to the first
//!   that fits: q8_0 / q8_0, then K q8_0 + V q4_0 on Vulkan. K stays at least q8_0 there
//!   (q4_0 K costs 3.5-8.5x the KL divergence of q8_0 K + q4_0 V on every model measured).
//!   Without flash attention (V cannot be quantized) the only step is K q8_0 + V f16; with
//!   it, that step is skipped: it measured no better than q8_0 / q8_0 (same KLD, slower on
//!   gpt-oss) for half the savings.
//!   When even the most compact rung does not fit but the weights do, that rung is used (it
//!   spills the least); when the weights alone do not fit, the cache stays f16 (quantizing
//!   it would cost quality for nothing).
//! - On **CUDA** the ladder has equal K/V pairs only: f16 / f16, then q8_0 / q8_0. q4_0 / q4_0
//!   is never chosen automatically: on CUDA (b11342, RTX 3050 Ti) it broke Qwen3-0.6B's output
//!   at 8k and 32k (f16 and q8_0 / q8_0 answered correctly) and garbled a Qwen3-4B tool call.
//!   When q8_0 / q8_0 does not fit, the gateway shrinks the context instead.
//!   The default CUDA build has no flash-attention vector kernel for mixed pairs: it converts K
//!   and V to f16 on every token (slow, and it needs a temporary f16 buffer that can overflow
//!   VRAM). Measured on an RTX 3050 Ti Laptop (b11327): generation is 27-38 % slower with
//!   q8_0 / f16 and q8_0 / q4_0, the matching pairs keep f16's speed (`docs/en/PERFORMANCE.md`
//!   "CUDA"). Mixed pairs stay available when the user chooses them explicitly; the memory
//!   estimate then counts the conversion buffer ([`cuda_convert_buffer_bytes`]).
//! - On **Metal** (Apple Silicon, unified memory) the ladder is the same: measured on an M1
//!   (b11344, Qwen3-4B at depth 4096), the mixed pairs fall to a slow path, prompt processing
//!   87-90 % and generation 57-60 % slower than f16, while q8_0 / q8_0 and q4_0 / q4_0 are as
//!   fast as f16 or faster (`docs/en/PERFORMANCE.md` "Metal").
//! - On **ROCm** the ladder also uses equal pairs only (decision 06.10): mixed pairs lose
//!   20-33 % generation speed at depth 16k; q4_0 / q4_0 keeps speed but lowers K precision.
//! - On **Apple unified memory** a rung must also leave macOS its share of RAM and, on Macs up
//!   to 16 GB, keep the cache under the default load's cap (one eighth of RAM, contexts above
//!   8k; [`crate::full_offload::unified_fits`]). Measured on an 8 GB M1: f16 fit Metal's working
//!   set but not the cap, so the default load of Qwen3-4B fell to 8k; q8_0 / q8_0 at the same
//!   context stays under it. Only with flash attention (the q8_0 / f16 step is slow on Metal).
//! - **Explicit** choices (Full / Balanced / Saver or custom K/V types) are used as given.
//! - A quantized V cache needs flash attention (llama.cpp refuses to start otherwise, and
//!   `-fa auto` can resolve to off): it is launched with `--flash-attn on`; when the user's
//!   own arguments turn flash attention off, V stays f16.
//! - The user's own `-ctk`/`-ctv` arguments win: nothing is chosen then.
//!
//! The same choice feeds the memory estimate ([`crate::vram::fit_with`] with
//! `kv_auto`), so the Dashboard shows what the launch will do.

use serde::{Deserialize, Serialize};

use crate::gguf::GgufMetadata;
use crate::process::LaunchConfig;
use crate::vram::{self, FitInput, FitResult, Measurement, MemoryBudget};

/// Share of the VRAM budget an automatic choice may use (the same limit as
/// [`crate::prepare::VRAM_LIMIT`] and [`vram::recommend_gpu_layers`]).
pub const VRAM_LIMIT: f64 = 0.9;
/// Share of system RAM a CPU-only or partly offloaded run may use.
pub const RAM_LIMIT: f64 = 0.8;

/// User-facing precision levels ("Konuşma hafızası hassasiyeti").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum KvLevel {
    /// f16 / f16: llama.cpp's own default.
    Full,
    /// Only K at 8 bits (no flash attention needed); an automatic intermediate step.
    KeysOnly,
    /// q8_0 / q8_0: half the memory, KL divergence ~0.001 on dense models.
    Balanced,
    /// q8_0 K + q4_0 V: ~41% of the f16 memory.
    Saver,
    /// q4_0 / q4_0: ~28% of the f16 memory. Only when chosen explicitly: it breaks small models
    /// (see the module documentation), so no automatic ladder has it.
    Compact,
    /// Any other combination (set by hand).
    Custom,
}

impl KvLevel {
    /// `(K, V)` of a named level; `None` for [`KvLevel::Custom`].
    pub fn types(self) -> Option<(&'static str, &'static str)> {
        Some(match self {
            KvLevel::Full => ("f16", "f16"),
            KvLevel::KeysOnly => ("q8_0", "f16"),
            KvLevel::Balanced => ("q8_0", "q8_0"),
            KvLevel::Saver => ("q8_0", "q4_0"),
            KvLevel::Compact => ("q4_0", "q4_0"),
            KvLevel::Custom => return None,
        })
    }

    /// Level of a K/V pair (empty or missing = f16, llama.cpp's default).
    pub fn of(k: Option<&str>, v: Option<&str>) -> KvLevel {
        let norm = |t: Option<&str>| {
            t.map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "f16".into())
        };
        let (k, v) = (norm(k), norm(v));
        [
            KvLevel::Full,
            KvLevel::KeysOnly,
            KvLevel::Balanced,
            KvLevel::Saver,
            KvLevel::Compact,
        ]
        .into_iter()
        .find(|l| l.types() == Some((k.as_str(), v.as_str())))
        .unwrap_or(KvLevel::Custom)
    }
}

/// The automatic ladder, best quality first (flash attention available).
pub const AUTO_LADDER: [KvLevel; 3] = [KvLevel::Full, KvLevel::Balanced, KvLevel::Saver];
/// The ladder on CUDA, Metal and ROCm: matching K/V pairs only, never q4_0 / q4_0 (see module
/// documentation).
pub const CUDA_LADDER: [KvLevel; 2] = [KvLevel::Full, KvLevel::Balanced];
/// The ladder when V cannot be quantized (flash attention off or unsupported).
pub const NO_FA_LADDER: [KvLevel; 2] = [KvLevel::Full, KvLevel::KeysOnly];

/// The cache type quantizes (anything but f16/bf16/f32).
pub fn is_quantized(t: Option<&str>) -> bool {
    t.map(|s| s.trim().to_ascii_lowercase())
        .is_some_and(|s| !s.is_empty() && !matches!(s.as_str(), "f16" | "bf16" | "f32"))
}

/// Why the automatic choice is what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum KvReason {
    /// f16 fits: nothing to save.
    Fits,
    /// f16 did not fit; this is the best-quality rung that does.
    ToFit,
    /// No rung fits; the weights do, so the most compact rung keeps the spill smallest.
    Tightest,
    /// The weights alone do not fit: f16 stays (quantizing would not make it fit).
    TooBig,
}

/// An automatic choice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct KvChoice {
    pub level: KvLevel,
    pub cache_type_k: String,
    pub cache_type_v: String,
    pub reason: KvReason,
    /// Whole KV cache with f16 and with this choice (bytes).
    pub kv_bytes_f16: u64,
    pub kv_bytes: u64,
    /// Typical prompt-processing slowdown of this choice vs f16 on the machine's backend
    /// (percent, measured; `None` = not measured for this backend).
    #[serde(default)]
    pub prompt_slowdown_pct: Option<u32>,
}

/// Worst prompt-processing slowdown vs f16 measured with `llama-bench` (b11312, Vulkan, RX
/// 7900 GRE; Qwen3-4B, gpt-oss-20b and Qwen2.5-Coder-14B at depth 0 and 16k, the worst case is
/// gpt-oss at 16k; dense models are as fast or faster at depth), see `docs/en/PERFORMANCE.md`.
/// ROCm (b11320, same card, Qwen3-4B and gpt-oss-20b at depth 0 and 16k): prompt processing
/// loses at most 5 % with every level. Generation is another matter there: the mixed pairs
/// (q8_0/f16, q8_0/q4_0) run 20-33 % slower than f16 at 16k of context, only the matching
/// pairs (q8_0/q8_0, q4_0/q4_0) keep f16's speed (PERFORMANCE "ROCm"). CUDA (b11327, RTX 3050
/// Ti Laptop, Qwen3-4B at depth 0 and 8k): prompt processing loses at most 6 %; the mixed pairs
/// also generate 27-38 % slower ([`CUDA_MIXED_GENERATION_SLOWDOWN_PCT`]). Metal (b11344, M1,
/// Qwen3-4B at depth 4096): the mixed pairs lose 87-90 %, the matching pairs none
/// ([`METAL_MIXED_GENERATION_SLOWDOWN_PCT`]). Other backends have no measurement here.
pub fn prompt_slowdown_pct(
    level: KvLevel,
    backend: crate::hardware::RecommendedBackend,
) -> Option<u32> {
    use crate::hardware::RecommendedBackend as B;
    match (backend, level) {
        (_, KvLevel::Full) => Some(0),
        (B::Vulkan, KvLevel::KeysOnly) => Some(SLOWDOWN_VULKAN[0]),
        (B::Vulkan, KvLevel::Balanced) => Some(SLOWDOWN_VULKAN[1]),
        (B::Vulkan, KvLevel::Saver) => Some(SLOWDOWN_VULKAN[2]),
        (B::Rocm, KvLevel::KeysOnly) => Some(SLOWDOWN_ROCM[0]),
        (B::Rocm, KvLevel::Balanced) => Some(SLOWDOWN_ROCM[1]),
        (B::Rocm, KvLevel::Saver) => Some(SLOWDOWN_ROCM[2]),
        (B::Rocm, KvLevel::Compact) => Some(SLOWDOWN_ROCM[3]),
        (B::Cuda12, KvLevel::KeysOnly) => Some(SLOWDOWN_CUDA[0]),
        (B::Cuda12, KvLevel::Balanced) => Some(SLOWDOWN_CUDA[1]),
        (B::Cuda12, KvLevel::Saver) => Some(SLOWDOWN_CUDA[2]),
        (B::Cuda12, KvLevel::Compact) => Some(SLOWDOWN_CUDA[3]),
        (B::Metal, KvLevel::KeysOnly) => Some(SLOWDOWN_METAL[0]),
        (B::Metal, KvLevel::Balanced) => Some(SLOWDOWN_METAL[1]),
        (B::Metal, KvLevel::Saver) => Some(SLOWDOWN_METAL[2]),
        (B::Metal, KvLevel::Compact) => Some(SLOWDOWN_METAL[3]),
        _ => None,
    }
}

/// KeysOnly, Balanced, Saver (percent, "up to").
const SLOWDOWN_VULKAN: [u32; 3] = [10, 7, 14];

/// Same for ROCm/HIP (prompt processing only).
const SLOWDOWN_ROCM: [u32; 4] = [5, 5, 5, 5];

/// KeysOnly, Balanced, Saver, Compact on CUDA (prompt processing, depth 8192: 1534 t/s with
/// f16 against 1488, 1441, 1450 and 1459).
const SLOWDOWN_CUDA: [u32; 4] = [3, 6, 6, 5];

/// Generation slowdown of the mixed pairs (q8_0/f16, q8_0/q4_0) on CUDA, range over the
/// measured depths (8192 against f16; 16384 against q8_0/q8_0): the default build converts K
/// and V to f16 on every token.
pub const CUDA_MIXED_GENERATION_SLOWDOWN_PCT: (u32, u32) = (27, 38);

/// KeysOnly, Balanced, Saver, Compact on Metal (prompt processing, depth 4096: 53.3 t/s with
/// f16 against 7.0, 59.2, 5.4 and 60.5; the matching pairs are faster than f16).
const SLOWDOWN_METAL: [u32; 4] = [87, 0, 90, 0];

/// Generation slowdown of the mixed pairs on Metal (depth 4096: 7.56 t/s with f16 against
/// 3.25 for q8_0/f16 and 3.01 for q8_0/q4_0).
pub const METAL_MIXED_GENERATION_SLOWDOWN_PCT: (u32, u32) = (57, 60);

/// The K/V pair is mixed (K and V differ; an empty type is f16).
pub fn is_mixed_pair(k: Option<&str>, v: Option<&str>) -> bool {
    let norm = |t: Option<&str>| {
        t.map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "f16".into())
    };
    norm(k) != norm(v)
}

/// Key head width above which the CUDA flash-attention kernels no longer read a quantized
/// cache directly (the vector kernels stop at 128): the cache is converted to f16 one layer
/// at a time, for equal K/V pairs too. Measured on an RTX 3050 Ti Laptop (b11456) with
/// Qwen3.5 (head width 256): the compute buffer grows by one layer of K and V in f16 minus
/// ~20 MiB (2B, 65536 cells q8_0: 224 MiB against 116 MiB with f16; 262144 cells: 800 against
/// 308); Qwen2.5-3B (128) grows by nothing at 65536 cells (148 against 144 MiB).
const CUDA_NATIVE_QUANT_HEAD_WIDTH: u64 = 128;

fn wide_heads(meta: Option<&GgufMetadata>) -> bool {
    meta.and_then(|m| {
        m.key_length
            .or_else(|| Some(m.embedding_length? / m.head_count.filter(|&h| h > 0)?))
    })
    .is_some_and(|w| w > CUDA_NATIVE_QUANT_HEAD_WIDTH)
}

/// Extra device memory the CUDA flash-attention fallback needs: K and V are converted to f16
/// one layer at a time, so one layer's quantized sides in f16. It happens for a mixed K/V
/// pair (no vector kernel) and, with key heads wider than 128, for equal quantized pairs too
/// (0 without flash attention or when no side is quantized).
pub fn cuda_convert_buffer_bytes(input: &FitInput, meta: Option<&GgufMetadata>) -> u64 {
    let (k, v) = (input.cache_type_k.as_deref(), input.cache_type_v.as_deref());
    if input.flash_attn == Some(false) {
        return 0;
    }
    if !is_mixed_pair(k, v) && !(wide_heads(meta) && (is_quantized(k) || is_quantized(v))) {
        return 0;
    }
    vram::kv_convert_layer_f16_bytes(input, meta, is_quantized(k), is_quantized(v))
}

fn fits(r: &FitResult) -> bool {
    let e = &r.estimate;
    let b = &r.budget;
    // An integrated GPU's buffers (and the compute buffer it keeps even with `-ngl 0`) come out
    // of the same RAM.
    let shared = if b.shared_igpu { e.vram_bytes } else { 0 };
    let ram_ok = (e.ram_bytes + shared) as f64 <= b.ram_bytes as f64 * RAM_LIMIT;
    if b.vram_bytes > 0 && e.offloaded_layers > 0 {
        ram_ok && e.vram_bytes as f64 <= b.vram_bytes as f64 * VRAM_LIMIT
    } else {
        ram_ok
    }
}

/// The automatic ladder of a machine (see the module documentation): shared by the launch
/// policy, "Prepare Server" and the gateway's fit-to-memory load.
pub fn ladder_for(budget: &MemoryBudget, v_quant_ok: bool) -> &'static [KvLevel] {
    let metal = budget.unified && !budget.shared_igpu;
    match (v_quant_ok, budget.cuda || budget.rocm || metal) {
        (false, _) => &NO_FA_LADDER,
        (true, true) => &CUDA_LADDER,
        (true, false) => &AUTO_LADDER,
    }
}

/// The launch has no cache type of its own (neither fields nor `-ctk`/`-ctv` arguments): the
/// automatic policy owns it.
pub fn is_automatic(config: &LaunchConfig) -> bool {
    let blank = |t: &Option<String>| t.as_deref().is_none_or(|s| s.trim().is_empty());
    !config.router_mode
        && blank(&config.cache_type_k)
        && blank(&config.cache_type_v)
        && !custom_args_set_cache_type(config.custom_args.as_deref())
}

/// Picks the automatic cache types for `input` (its own cache types and `kv_auto` are
/// ignored). `v_quant_ok` = flash attention can be on (rungs that quantize V need it).
pub fn choose(
    input: &FitInput,
    meta: Option<&GgufMetadata>,
    budget: &MemoryBudget,
    measured: &[Measurement],
    v_quant_ok: bool,
) -> (KvChoice, FitResult) {
    let eval = |level: KvLevel| {
        let (k, v) = level.types().expect("ladder levels are named");
        let i = FitInput {
            cache_type_k: Some(k.into()),
            cache_type_v: Some(v.into()),
            kv_auto: false,
            ..input.clone()
        };
        vram::fit_with(&i, meta, budget, measured)
    };
    // CUDA, Metal and ROCm are slow with mixed pairs. Apple Silicon is the unified budget that is not
    // an integrated GPU's share of RAM.
    let ladder = ladder_for(budget, v_quant_ok);
    // Apple unified memory: a rung must also leave macOS its share and stay under the cache cap
    // of a small Mac (`full_offload::unified_fits`), so f16 that only fits Metal's working set
    // gives way to q8_0 / q8_0 at the same context instead of the default load stepping the
    // context down. Only where V can be quantized: without flash attention the next rung is the
    // slow mixed q8_0 / f16 pair, and the context steps down as before.
    let unified_rule = v_quant_ok && budget.unified && !budget.shared_igpu;
    let ok = |r: &FitResult| {
        fits(r) && (!unified_rule || crate::full_offload::unified_fits(r, input.context_size))
    };
    let full = eval(KvLevel::Full);
    let kv_f16 = full.estimate.kv_bytes;
    let pick = |level: KvLevel, reason: KvReason, r: FitResult| {
        let (k, v) = level.types().expect("ladder levels are named");
        (
            KvChoice {
                level,
                cache_type_k: k.into(),
                cache_type_v: v.into(),
                reason,
                kv_bytes_f16: kv_f16,
                kv_bytes: r.estimate.kv_bytes,
                prompt_slowdown_pct: None,
            },
            r,
        )
    };
    if ok(&full) {
        return pick(KvLevel::Full, KvReason::Fits, full);
    }
    let mut last = None;
    for &level in ladder.iter().skip(1) {
        let r = eval(level);
        if ok(&r) {
            return pick(level, KvReason::ToFit, r);
        }
        last = Some((level, r));
    }
    // Nothing fits. With the KV cache left out entirely, would the rest fit?
    let without_kv = {
        let mut r = full.clone();
        r.estimate.vram_bytes = r
            .estimate
            .vram_bytes
            .saturating_sub(r.estimate.kv_gpu_bytes);
        r.estimate.ram_bytes = r
            .estimate
            .ram_bytes
            .saturating_sub(r.estimate.kv_bytes - r.estimate.kv_gpu_bytes.min(r.estimate.kv_bytes));
        r
    };
    match last {
        // Apple unified memory at a context the default-load limits do not allow (one the user
        // set): the tightest rung still fits Metal's working set, so the load runs and the cache
        // was shrunk to fit; "does not fit even at the tightest" would contradict the fit label.
        Some((level, r)) if unified_rule && fits(&r) => pick(level, KvReason::ToFit, r),
        Some((level, r)) if fits(&without_kv) => pick(level, KvReason::Tightest, r),
        _ => pick(KvLevel::Full, KvReason::TooBig, full),
    }
}

/// What [`apply`] did to a launch.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Applied {
    /// The automatic choice (`None` = explicit types, custom arguments or not applicable).
    pub choice: Option<KvChoice>,
    /// `--flash-attn on` was added because V is quantized.
    pub flash_attn_forced: bool,
    /// V was quantized but flash attention is off/unavailable: V stays f16.
    pub v_kept_f16: bool,
}

impl Applied {
    /// Log lines for the server log.
    pub fn notes(&self, lang: &str) -> Vec<String> {
        let tr = lang == "tr";
        let mut out = Vec::new();
        if let Some(c) = &self.choice {
            let pct = |a: u64, b: u64| (a * 100).checked_div(b).map_or(0, |r| 100 - r.min(100));
            let line = match (c.reason, tr) {
                (KvReason::Fits, _) => None,
                (KvReason::TooBig, true) => Some(
                    "[Harness Studio] Konuşma hafızası (KV önbelleği) f16 kaldı: model ağırlıkları tek başına belleğe sığmıyor.".to_string(),
                ),
                (KvReason::TooBig, false) => Some(
                    "[Harness Studio] Conversation memory (KV cache) stays f16: the model weights alone do not fit in memory.".to_string(),
                ),
                (_, true) => Some(format!(
                    "[Harness Studio] Belleği sığdırmak için konuşma hafızası (KV önbelleği) -ctk {} -ctv {} ile saklanıyor: KV {} MiB yerine {} MiB (%{} tasarruf).",
                    c.cache_type_k,
                    c.cache_type_v,
                    c.kv_bytes_f16 >> 20,
                    c.kv_bytes >> 20,
                    pct(c.kv_bytes, c.kv_bytes_f16)
                )),
                (_, false) => Some(format!(
                    "[Harness Studio] To fit in memory the conversation memory (KV cache) uses -ctk {} -ctv {}: {} MiB instead of {} MiB ({}% saved).",
                    c.cache_type_k,
                    c.cache_type_v,
                    c.kv_bytes >> 20,
                    c.kv_bytes_f16 >> 20,
                    pct(c.kv_bytes, c.kv_bytes_f16)
                )),
            };
            out.extend(line);
        }
        if self.flash_attn_forced {
            out.push(if tr {
                "[Harness Studio] V önbelleği nicemlendiği için flash attention açıldı (--flash-attn on).".into()
            } else {
                "[Harness Studio] Flash attention turned on because the V cache is quantized (--flash-attn on).".into()
            });
        }
        if self.v_kept_f16 {
            out.push(if tr {
                "[Harness Studio Warn] Flash attention kapalı: V önbelleği f16 bırakıldı (nicemlenmiş V, flash attention ister).".into()
            } else {
                "[Harness Studio Warn] Flash attention is off: the V cache stays f16 (a quantized V cache needs flash attention).".into()
            });
        }
        out
    }
}

/// The user's own arguments set a KV cache type.
pub fn custom_args_set_cache_type(custom_args: Option<&str>) -> bool {
    custom_args.is_some_and(|a| {
        a.split_whitespace().any(|t| {
            matches!(
                t.split('=').next().unwrap_or(t),
                "-ctk" | "--cache-type-k" | "-ctv" | "--cache-type-v"
            )
        })
    })
}

/// The user's own arguments turn flash attention off (`-fa off`, `--flash-attn=0`...).
pub fn custom_args_disable_flash_attn(custom_args: Option<&str>) -> bool {
    let tokens: Vec<&str> = custom_args
        .map(|a| a.split_whitespace().collect())
        .unwrap_or_default();
    let off = |v: &str| matches!(v.to_ascii_lowercase().as_str(), "off" | "0" | "false");
    tokens.iter().enumerate().any(|(i, t)| {
        let (flag, inline) = match t.split_once('=') {
            Some((f, v)) => (f, Some(v)),
            None => (*t, None),
        };
        matches!(flag, "-fa" | "--flash-attn")
            && inline
                .or_else(|| tokens.get(i + 1).copied())
                .is_some_and(off)
    })
}

/// Launch-time policy: fills in the automatic cache types and makes a quantized V cache
/// safe. `config` is what gets launched (the stored settings stay as the user left them).
/// `budget`/`measured` come from [`vram::budget`] and [`vram::measurements_for`];
/// `flash_attn_supported` = the runtime has `--flash-attn`.
pub fn apply(
    config: &mut LaunchConfig,
    meta: Option<&GgufMetadata>,
    budget: &MemoryBudget,
    measured: &[Measurement],
    flash_attn_supported: bool,
) -> Applied {
    let mut out = Applied::default();
    if config.router_mode || custom_args_set_cache_type(config.custom_args.as_deref()) {
        return out;
    }
    let fa_possible =
        flash_attn_supported && !custom_args_disable_flash_attn(config.custom_args.as_deref());
    let blank = |t: &Option<String>| t.as_deref().is_none_or(|s| s.trim().is_empty());
    if blank(&config.cache_type_k) && blank(&config.cache_type_v) {
        let input = FitInput {
            kv_auto: false,
            ..vram::input_from_launch(config)
        };
        let (choice, _) = choose(&input, meta, budget, measured, fa_possible);
        if choice.level != KvLevel::Full {
            config.cache_type_k = Some(choice.cache_type_k.clone());
            config.cache_type_v = Some(choice.cache_type_v.clone());
        }
        out.choice = Some(choice);
    }
    if is_quantized(config.cache_type_v.as_deref()) {
        if !fa_possible {
            config.cache_type_v = None;
            out.v_kept_f16 = true;
        } else if !config.flash_attn {
            config.flash_attn = true;
            out.flash_attn_forced = true;
        }
    }
    out
}

#[cfg(test)]
#[path = "kv_policy_tests.rs"]
mod tests;
