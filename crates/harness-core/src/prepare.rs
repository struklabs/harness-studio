//! "Prepare the server" for a tool (GUI `Sunucuyu hazırla`, `harness launch --prepare`):
//! picks the load settings an agent tool needs without going past the machine's memory.
//!
//! The choice is the largest context that
//! 1. the model supports: the trained context (`<arch>.context_length`), extended only by
//!    RoPE scaling the GGUF itself declares (`rope.scaling.type` yarn/linear with a factor and
//!    the original context: `original * factor`); never by flags the file does not back;
//! 2. fits in device memory: full offload, flash attention, one slot, at most 90% of the VRAM
//!    budget by [`crate::vram`] (header-derived estimate corrected with measured loads). The
//!    definition gets the automatic slot count ([`crate::slots`]), which adds slots only
//!    while the model still fits fully, so one slot is the memory the plan has to hold;
//! 3. the tool can use: at least its minimum, aiming for its recommended context (Open Design
//!    128k) and at most its ceiling (a bigger window than that only costs memory).
//!
//! The KV cache follows the automatic ladder of [`crate::kv_policy`] (f16, q8_0 / q8_0,
//! K q8_0 + V q4_0): at the recommended context or above, the best-quality rung that
//! fits wins; below it, the largest context wins (with the best rung that fits there). What
//! the tool needs but the machine cannot hold is reported, with alternatives (better KV
//! quality at a smaller context, partial CPU offload), never applied silently.

use serde::{Deserialize, Serialize};

use crate::gguf::GgufMetadata;
use crate::integrations::launch::LaunchTool;
use crate::kv_policy::{KvLevel, AUTO_LADDER};
use crate::vram::{self, EstimateSource, FitInput, Measurement, MemoryBudget};

/// Share of the VRAM budget a full-GPU choice may use (the same limit
/// [`vram::recommend_gpu_layers`] uses; the rest is what Windows and other apps keep busy).
pub const VRAM_LIMIT: f64 = 0.9;
/// Share of system RAM a CPU-only or hybrid choice may use.
pub const RAM_LIMIT: f64 = 0.8;

/// Context sizes tried, besides the model's own maximum and the tool's numbers.
const LADDER: [u32; 12] = [
    1_048_576, 524_288, 262_144, 196_608, 131_072, 98_304, 65_536, 49_152, 32_768, 24_576, 16_384,
    8_192,
];

/// What a tool needs from the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ToolNeed {
    /// Below this the tool does not work.
    pub min: u32,
    /// What "prepare" aims for.
    pub recommended: u32,
    /// The most it is worth going to.
    pub ceiling: u32,
}

impl ToolNeed {
    /// `id` is an integration id (`claude`, `codex`, `opencode`, `opendesign`, `aider`,
    /// `goose`...); `min_ctx` is the minimum the integration list reports (0 = none).
    pub fn for_tool(id: &str, min_ctx: u32) -> ToolNeed {
        if id == "playground" {
            return ToolNeed {
                min: 4_096,
                recommended: 16_384,
                ceiling: 32_768,
            };
        }
        if id == "opendesign" {
            // Design sessions carry whole pages plus the system prompt: 128k or more.
            return ToolNeed {
                min: min_ctx.max(32_768),
                recommended: 131_072,
                ceiling: 262_144,
            };
        }
        if let Some(t) = LaunchTool::parse(id) {
            return ToolNeed {
                min: t.min_ctx(),
                recommended: t.recommended_ctx(),
                ceiling: t.recommended_ctx().saturating_mul(2),
            };
        }
        let min = min_ctx.max(8_192);
        let recommended = min.max(65_536);
        ToolNeed {
            min,
            recommended,
            ceiling: recommended.saturating_mul(2),
        }
    }
}

/// The context a model can take.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelLimit {
    /// Trained context (`context_length`); 0 = the header could not be read.
    pub trained: u64,
    /// Highest context the file backs.
    pub max: u64,
    /// `max` is above `trained` because the file declares RoPE scaling.
    pub extended_by_rope: bool,
    /// Scaling the file declares (`yarn x32`), whether or not it raises the limit.
    pub rope: Option<String>,
    /// The GGUF header was read (else `max` is a conservative guess).
    pub known: bool,
}

/// Highest context a GGUF supports.
pub fn model_limit(meta: Option<&GgufMetadata>, fallback: u32) -> ModelLimit {
    let Some(trained) = meta.and_then(|m| m.context_length).filter(|c| *c > 0) else {
        return ModelLimit {
            trained: 0,
            max: u64::from(fallback),
            extended_by_rope: false,
            rope: None,
            known: false,
        };
    };
    let m = meta.expect("trained context came from the header");
    let kind = m.rope_scaling_type.as_deref().map(str::to_ascii_lowercase);
    let factor = m.rope_scaling_factor.filter(|f| *f > 1.0);
    let rope = match (&kind, factor) {
        (Some(k), Some(f)) if k != "none" => Some(format!("{} x{}", k, trim_factor(f))),
        _ => None,
    };
    let scaled = match (&kind, factor, m.rope_scaling_original_ctx) {
        (Some(k), Some(f), Some(orig)) if matches!(k.as_str(), "yarn" | "linear") && orig > 0 => {
            (orig as f64 * f).round() as u64
        }
        _ => 0,
    };
    ModelLimit {
        trained,
        max: trained.max(scaled),
        extended_by_rope: scaled > trained,
        rope,
        known: true,
    }
}

fn trim_factor(f: f64) -> String {
    if (f - f.round()).abs() < 1e-6 {
        format!("{}", f.round() as u64)
    } else {
        format!("{:.2}", f)
    }
}

/// Where one candidate puts the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PrepareOption {
    pub context_size: u32,
    /// KV cache precision ([`crate::kv_policy::AUTO_LADDER`]) and its types.
    pub kv_level: KvLevel,
    pub cache_type_k: String,
    pub cache_type_v: String,
    /// `-ngl`; `None` = everything on the GPU (CLI and GUI write 999).
    pub gpu_layers: Option<u32>,
    pub vram_bytes: u64,
    pub ram_bytes: u64,
    /// Share of the VRAM budget (CPU-only: of RAM).
    pub percent: u32,
    pub kind: OptionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum OptionKind {
    /// The choice: fully on the GPU.
    Chosen,
    /// q8_0 / q8_0 KV cache (when the choice needed less), at the largest context it allows.
    Q8Kv,
    /// Higher-quality f16 KV cache, at the largest context it allows.
    F16Kv,
    /// A larger context than the choice (up to the tool's ceiling) with a q8_0 / q8_0 cache.
    LargerCtx,
    /// The tool's recommended context with some layers on the CPU (slower).
    Offload,
}

/// How the choice compares with what the tool wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Recommended context or more, fully on the GPU.
    Recommended,
    /// At least the minimum but below the recommendation.
    BelowRecommended,
    /// Not even the minimum fits on the GPU; see the alternatives.
    BelowMinimum,
}

/// What stopped the context from going higher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Limit {
    /// The tool's ceiling.
    Ceiling,
    /// What the model supports.
    Model,
    /// Device memory.
    Memory,
    /// Memory currently available, including idle models the gateway can evict.
    Available,
}

/// A fresh memory picture of the device the server will use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AvailableMemory {
    pub total_vram_bytes: u64,
    pub available_vram_bytes: u64,
    pub headroom_bytes: u64,
}

impl AvailableMemory {
    fn fits(self, bytes: u64) -> bool {
        bytes.saturating_add(self.headroom_bytes) <= self.available_vram_bytes
    }
}

/// Runtime device and OS adapter used by a launch config.
pub fn main_device(
    report: &crate::devices::DeviceReport,
    adapters: &[(u32, String)],
    main_gpu: Option<u32>,
) -> Option<(u32, u32)> {
    let device = match main_gpu {
        Some(adapter) => {
            let index = crate::devices::runtime_index(&report.devices, adapters, adapter);
            report.devices.iter().find(|d| d.index == index)
        }
        None => report
            .devices
            .iter()
            .find(|d| !d.integrated && d.total_bytes.is_some()),
    }?;
    Some((
        device.index,
        crate::devices::adapter_index(&report.devices, adapters, device.index),
    ))
}

/// A server's measured footprint on one OS adapter; missing measurements add nothing.
pub struct ServerMemory {
    pub server_id: String,
    pub adapter: u32,
    pub bytes: u64,
}

/// Read the selected adapter only, including the legacy single-adapter snapshot.
pub fn server_vram(
    snapshot: &crate::telemetry::TelemetrySnapshot,
    adapter: u32,
    server: &str,
) -> u64 {
    let breakdown = if snapshot.gpus.is_empty() {
        (adapter == 0)
            .then_some(snapshot.vram_breakdown.as_ref())
            .flatten()
    } else {
        snapshot
            .gpus
            .iter()
            .find(|g| g.index == adapter)
            .and_then(|g| g.breakdown.as_ref())
    };
    breakdown
        .and_then(|b| b.servers.iter().find(|s| s.id == server))
        .map_or(0, |s| s.bytes)
}

/// Free memory plus measured servers that will stop, counted once per server.
pub fn available_memory(
    report: &crate::devices::DeviceReport,
    selected: (u32, u32),
    headroom_bytes: u64,
    target: &str,
    slots: &[crate::gateway::scheduler::ModelSlot],
    measured: &[ServerMemory],
    stopping: &[String],
) -> Option<AvailableMemory> {
    if report.error.is_some() {
        return None;
    }
    let device = report.devices.iter().find(|d| d.index == selected.0)?;
    let total = device.total_bytes?;
    let free = device.available_bytes.or(device.free_bytes)?;
    let mut reclaim: std::collections::HashSet<&str> =
        stopping.iter().map(String::as_str).collect();
    reclaim.extend(
        slots
            .iter()
            .filter(|s| evictable_model(s, target))
            .filter_map(|s| s.server_id.as_deref()),
    );
    let mut counted = std::collections::HashSet::new();
    let reclaimed = measured
        .iter()
        .filter(|m| {
            m.adapter == selected.1
                && reclaim.contains(m.server_id.as_str())
                && counted.insert(m.server_id.as_str())
        })
        .fold(0u64, |sum, m| sum.saturating_add(m.bytes));
    let available = reclaimable_available(
        free,
        total,
        device
            .free_bytes
            .filter(|_| cfg!(windows) && device.backend == "CUDA"),
        device.os_used_bytes,
        reclaimed,
    );
    Some(AvailableMemory {
        total_vram_bytes: total,
        available_vram_bytes: available.min(total),
        headroom_bytes: vram::memory_headroom(total, headroom_bytes),
    })
}

// WDDM allocations may exceed physical residency. Preserve the runtime reservation
// relative to OS free memory: that reservation does not stop with the old server.
fn reclaimable_available(
    free: u64,
    total: u64,
    runtime_free: Option<u64>,
    os_used: Option<u64>,
    reclaimed: u64,
) -> u64 {
    let used = os_used.unwrap_or_else(|| total.saturating_sub(free));
    let other = used.saturating_sub(reclaimed);
    let reservation = total.saturating_sub(used).saturating_sub(free);
    let available = free
        .saturating_add(reclaimed.min(used))
        .min(total.saturating_sub(other).saturating_sub(reservation));
    // A separate CUDA probe on WDDM can report its per-process budget rather than
    // physical free memory. Stopping another process cannot enlarge that budget.
    match (runtime_free, os_used) {
        (Some(runtime_free), Some(os_used)) if runtime_free > total.saturating_sub(os_used) => {
            available.min(runtime_free)
        }
        _ => available,
    }
}

#[cfg(test)]
mod reclaim_tests {
    use super::reclaimable_available;

    #[test]
    fn loaded_footprint_is_clipped_and_matches_stopped_capacity() {
        let stopped = reclaimable_available(3295, 4096, Some(3295), Some(4), 0);
        let loaded = reclaimable_available(243, 4096, Some(3295), Some(3853), 3850);
        assert_eq!(stopped, 3295);
        assert_eq!(loaded, stopped);
        assert_eq!(
            reclaimable_available(200, 4096, None, Some(3896), 4500),
            4096
        );
        // A runtime with genuinely live free memory still reclaims stopped models.
        assert_eq!(
            reclaimable_available(243, 4096, Some(243), Some(3853), 3000),
            3243
        );
    }
}

/// Matches the gateway's eviction rule; the target itself is never a victim.
pub fn evictable_model(slot: &crate::gateway::scheduler::ModelSlot, target: &str) -> bool {
    slot.definition_id != target
        && slot.managed
        && !slot.pinned
        && slot.state == crate::gateway::scheduler::SlotState::Ready
        && slot.in_flight == 0
        && slot.queued == 0
        && slot.server_id.is_some()
}

/// Only measured footprints on the selected adapter may be added to its free memory.
pub fn available_after_eviction<'a>(
    free: u64,
    total: u64,
    target: &str,
    models: impl IntoIterator<Item = (&'a crate::gateway::scheduler::ModelSlot, Option<u64>)>,
) -> u64 {
    models
        .into_iter()
        .filter(|(s, _)| evictable_model(s, target))
        .fold(free, |sum, (_, bytes)| {
            sum.saturating_add(bytes.unwrap_or(0))
        })
        .min(total)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PreparePlan {
    pub need: ToolNeed,
    pub model: ModelLimit,
    pub verdict: Verdict,
    /// What limits the choice's context.
    pub limit: Limit,
    /// Fully-on-GPU choice; `None` when even the minimum does not fit.
    pub choice: Option<PrepareOption>,
    /// Other ways to go, best first.
    pub alternatives: Vec<PrepareOption>,
    pub budget_vram_bytes: u64,
    pub budget_ram_bytes: u64,
    #[serde(default)]
    pub available_memory: Option<AvailableMemory>,
    /// No GPU, or an integrated one that is slower than the CPU for the weights: everything
    /// is judged against RAM and the weights stay on the CPU (`-ngl 0`).
    pub cpu_only: bool,
    /// The GPU is integrated and shares system RAM.
    #[serde(default)]
    pub shared_igpu: bool,
    /// A smaller model would help (the model or the memory is the limit).
    pub suggest_smaller_model: bool,
    /// The numbers come from the header only or were corrected with a measured load.
    pub source: EstimateSource,
}

struct Eval {
    vram: u64,
    ram: u64,
    fits: bool,
    percent: u32,
    layers: u32,
    source: EstimateSource,
    recommended_layers: u32,
    total_layers: u64,
}

#[allow(clippy::too_many_arguments)]
fn evaluate(
    base: &FitInput,
    meta: Option<&GgufMetadata>,
    budget: &MemoryBudget,
    measured: &[Measurement],
    ctx: u32,
    level: KvLevel,
    layers: Option<u32>,
    cpu_weights: bool,
) -> Eval {
    let (k, v) = level.types().unwrap_or(("f16", "f16"));
    let input = FitInput {
        context_size: ctx,
        cache_type_k: Some(k.into()),
        cache_type_v: Some(v.into()),
        kv_auto: false,
        flash_attn: Some(true),
        parallel_slots: Some(1),
        gpu_layers: if cpu_weights {
            0
        } else {
            layers.unwrap_or(999)
        },
        ..base.clone()
    };
    let r = vram::fit_with(&input, meta, budget, measured);
    let e = &r.estimate;
    let full = e.total_layers > 0 && e.offloaded_layers >= e.total_layers;
    let gpu = budget.vram_bytes > 0 && !cpu_weights;
    let fits = if gpu {
        full && e.vram_bytes as f64 <= budget.vram_bytes as f64 * VRAM_LIMIT
    } else {
        // An integrated GPU's buffers come out of the same RAM.
        (e.ram_bytes + e.vram_bytes) as f64 <= budget.ram_bytes as f64 * RAM_LIMIT
    };
    Eval {
        vram: e.vram_bytes,
        ram: e.ram_bytes,
        fits,
        percent: if gpu {
            r.vram_percentage
        } else {
            r.ram_percentage
        },
        layers: e.offloaded_layers as u32,
        source: r.source,
        recommended_layers: r.recommended_gpu_layers,
        total_layers: e.total_layers,
    }
}

fn candidates(need: &ToolNeed, model_max: u64) -> Vec<u32> {
    let cap = u64::from(need.ceiling)
        .min(model_max)
        .min(u64::from(u32::MAX)) as u32;
    let mut v: Vec<u32> = LADDER.to_vec();
    v.extend([cap, need.min, need.recommended]);
    v.retain(|c| *c >= need.min && *c <= cap);
    v.sort_unstable_by(|a, b| b.cmp(a));
    v.dedup();
    v
}

/// Lowest context the gateway shrinks a definition's context to when fitting a load into free
/// memory (smaller than this the agent tools stop working); never above the definition's own.
pub const FIT_MIN_CONTEXT: u32 = 16_384;

/// Context the gateway gives a chat definition that sets none (llama.cpp's own default is 4096,
/// which the Playground's tool list alone exceeds): the model's trained context, at most this.
pub const DEFAULT_CHAT_CONTEXT: u32 = 32_768;

/// The context for a chat load whose definition sets none: `trained` (GGUF `context_length`,
/// `None`/0 = unknown) capped at [`DEFAULT_CHAT_CONTEXT`].
pub fn default_chat_context(trained: Option<u64>) -> u32 {
    trained
        .filter(|t| *t > 0)
        .map_or(DEFAULT_CHAT_CONTEXT, |t| {
            t.min(u64::from(DEFAULT_CHAT_CONTEXT)) as u32
        })
}

/// What an unset context (`context_size` 0 = automatic, the default of `server.default_ctx`)
/// becomes at launch: a chat server follows [`default_chat_context`]; embedding, rerank,
/// whisper and router servers (and runs whose own arguments set `-c`) keep the 4096 they got
/// from the old shipped default, so `-c 0` (the model's whole trained context) is never sent.
pub fn resolve_auto_context(chat: bool, custom_args: Option<&str>, trained: Option<u64>) -> u32 {
    if chat && !custom_args_set_context(custom_args) {
        default_chat_context(trained)
    } else {
        LEGACY_CONTEXT
    }
}

/// The context non-chat servers get when none is set (the old shipped `default_ctx`).
pub const LEGACY_CONTEXT: u32 = 4096;

/// The user's own arguments set the context (`-c`, `--ctx-size`).
pub fn custom_args_set_context(custom_args: Option<&str>) -> bool {
    custom_args.is_some_and(|a| {
        a.split_whitespace().any(|t| {
            matches!(
                t.split('=').next().unwrap_or(t),
                "-c" | "--ctx-size" | "--context-size"
            )
        })
    })
}

/// The settings the ladder settled on: first the cache steps down at the requested context,
/// then the context shrinks (to [`LADDER`] sizes not below `floor`) and every rung is tried
/// again. Shared by [`plan`]'s tiers and the gateway's load-on-request fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LadderFit {
    pub level: KvLevel,
    pub context_size: u32,
}

/// First `(context, level)` of the ladder for which `fits` holds. Contexts: `context`, then
/// the [`LADDER`] sizes below it, down to `min(floor, context)`; per context all of `levels`
/// from the best quality on.
pub fn first_ladder_fit(
    levels: &[KvLevel],
    context: u32,
    floor: u32,
    mut fits: impl FnMut(KvLevel, u32) -> bool,
) -> Option<LadderFit> {
    let floor = floor.min(context);
    let mut contexts = vec![context];
    contexts.extend(
        LADDER
            .iter()
            .copied()
            .filter(|c| *c < context && *c >= floor),
    );
    for context_size in contexts {
        for &level in levels {
            if fits(level, context_size) {
                return Some(LadderFit {
                    level,
                    context_size,
                });
            }
        }
    }
    None
}

/// Picks the settings. `base` carries the model (path, size, projector); its context, cache
/// and offload fields are ignored.
pub fn plan(
    base: &FitInput,
    meta: Option<&GgufMetadata>,
    budget: &MemoryBudget,
    measured: &[Measurement],
    need: ToolNeed,
) -> PreparePlan {
    plan_with_available_memory(base, meta, budget, measured, need, None)
}

pub fn plan_with_available_memory(
    base: &FitInput,
    meta: Option<&GgufMetadata>,
    budget: &MemoryBudget,
    measured: &[Measurement],
    need: ToolNeed,
    available: Option<AvailableMemory>,
) -> PreparePlan {
    let model = model_limit(meta, need.min);
    // An older integrated GPU is slower than the CPU for the weights (see
    // `vram::recommend_gpu_layers`): plan as CPU-only, or `-ngl 999` would undo that.
    let cpu_only =
        budget.vram_bytes == 0 || (budget.shared_igpu && budget.igpu_offload_faster == Some(false));
    let mut cands = candidates(&need, model.max);
    // In RAM a larger context only costs memory the rest of the system needs (and is slow to
    // fill on a CPU): stop at what the tool recommends.
    if cpu_only {
        cands.retain(|c| *c <= need.recommended.max(need.min));
    }
    let ev = |ctx: u32, level: KvLevel, layers: Option<u32>| {
        let mut e = evaluate(base, meta, budget, measured, ctx, level, layers, cpu_only);
        if !cpu_only && available.is_some_and(|a| !a.fits(e.vram)) {
            e.fits = false;
        }
        e
    };
    let option =
        |ctx: u32, level: KvLevel, e: &Eval, kind: OptionKind, layers: Option<u32>| PrepareOption {
            context_size: ctx,
            kv_level: level,
            cache_type_k: level.types().unwrap_or(("f16", "f16")).0.into(),
            cache_type_v: level.types().unwrap_or(("f16", "f16")).1.into(),
            gpu_layers: if cpu_only { Some(0) } else { layers },
            vram_bytes: e.vram,
            ram_bytes: e.ram,
            percent: e.percent,
            kind,
        };

    // Tiers: reach the recommendation with the best KV quality that allows it; failing
    // that, the largest context (never below the minimum) with the best KV that fits there.
    let at_least_rec: Vec<u32> = cands
        .iter()
        .copied()
        .filter(|c| *c >= need.recommended)
        .collect();
    let below_rec: Vec<u32> = cands
        .iter()
        .copied()
        .filter(|c| *c < need.recommended)
        .collect();
    let mut chosen: Option<(u32, KvLevel, Eval)> = None;
    'rec: for level in AUTO_LADDER {
        for &ctx in &at_least_rec {
            let e = ev(ctx, level, None);
            if e.fits {
                chosen = Some((ctx, level, e));
                break 'rec;
            }
        }
    }
    if chosen.is_none() {
        'below: for &ctx in &below_rec {
            for level in AUTO_LADDER {
                let e = ev(ctx, level, None);
                if e.fits {
                    chosen = Some((ctx, level, e));
                    break 'below;
                }
            }
        }
    }

    let top = cands.first().copied();
    let (verdict, mut limit, choice, source) = match &chosen {
        Some((ctx, level, e)) => {
            let verdict = if *ctx >= need.recommended {
                Verdict::Recommended
            } else {
                Verdict::BelowRecommended
            };
            let limit = if Some(*ctx) == top {
                if u64::from(need.ceiling) <= model.max {
                    Limit::Ceiling
                } else {
                    Limit::Model
                }
            } else {
                Limit::Memory
            };
            (
                verdict,
                limit,
                Some(option(*ctx, *level, e, OptionKind::Chosen, None)),
                e.source,
            )
        }
        None => {
            let limit = if model.max < u64::from(need.min) {
                Limit::Model
            } else {
                Limit::Memory
            };
            let src = ev(need.min, KvLevel::Saver, None).source;
            (Verdict::BelowMinimum, limit, None, src)
        }
    };

    if !cpu_only && available.is_some() {
        let capacity = plan(base, meta, budget, measured, need);
        if capacity.choice != choice
            || capacity
                .alternatives
                .iter()
                .any(|o| available.is_some_and(|a| !a.fits(o.vram_bytes)))
        {
            limit = Limit::Available;
        }
    }

    let mut alternatives = Vec::new();
    // Better KV quality than the choice, at the largest context it allows.
    if let Some(c) = &choice {
        let better: &[(KvLevel, OptionKind)] = match c.kv_level {
            KvLevel::Saver => &[
                (KvLevel::Balanced, OptionKind::Q8Kv),
                (KvLevel::Full, OptionKind::F16Kv),
            ],
            KvLevel::Full => &[],
            _ => &[(KvLevel::Full, OptionKind::F16Kv)],
        };
        for (level, kind) in better {
            for &ctx in cands.iter().filter(|x| **x <= c.context_size) {
                let e = ev(ctx, *level, None);
                if e.fits {
                    alternatives.push(option(ctx, *level, &e, *kind, None));
                    break;
                }
            }
        }
    }
    // The choice kept full precision at the recommendation: a larger context with 8-bit
    // memory, when one fits.
    if let Some(c) = choice
        .as_ref()
        .filter(|c| matches!(c.kv_level, KvLevel::Full | KvLevel::KeysOnly))
    {
        for &ctx in cands.iter().filter(|x| **x > c.context_size) {
            let e = ev(ctx, KvLevel::Balanced, None);
            if e.fits {
                alternatives.push(option(
                    ctx,
                    KvLevel::Balanced,
                    &e,
                    OptionKind::LargerCtx,
                    None,
                ));
                break;
            }
        }
    }
    // Partial offload at the recommended context: only when the choice falls short of it.
    if verdict != Verdict::Recommended && !cpu_only {
        let target = u64::from(need.recommended).min(model.max) as u32;
        if target >= need.min {
            let full = ev(target, KvLevel::Balanced, None);
            let mut layers = full.recommended_layers.min(full.total_layers as u32);
            while layers > 0 {
                let part = ev(target, KvLevel::Balanced, Some(layers));
                if part.vram as f64 <= budget.vram_bytes as f64 * VRAM_LIMIT
                    && available.is_none_or(|a| a.fits(part.vram))
                {
                    break;
                }
                layers -= 1;
            }
            if layers > 0 {
                let part = ev(target, KvLevel::Balanced, Some(layers));
                // An integrated GPU's share comes out of the same RAM.
                let shared = if budget.shared_igpu { part.vram } else { 0 };
                let ram_ok = (part.ram + shared) as f64 <= budget.ram_bytes as f64 * RAM_LIMIT;
                let vram_ok = part.vram as f64 <= budget.vram_bytes as f64 * VRAM_LIMIT;
                if ram_ok
                    && vram_ok
                    && available.is_none_or(|a| a.fits(part.vram))
                    && u64::from(part.layers) <= full.total_layers
                {
                    alternatives.push(option(
                        target,
                        KvLevel::Balanced,
                        &part,
                        OptionKind::Offload,
                        Some(layers),
                    ));
                }
            }
        }
    }

    PreparePlan {
        need,
        suggest_smaller_model: verdict != Verdict::Recommended
            && !(limit == Limit::Model && model.max >= u64::from(need.min)),
        model,
        verdict,
        limit,
        choice,
        alternatives,
        budget_vram_bytes: budget.vram_bytes,
        budget_ram_bytes: budget.ram_bytes,
        available_memory: available,
        cpu_only,
        shared_igpu: budget.shared_igpu,
        source,
    }
}

/// Plan for a model on this machine: `base` names the model (path, size, projector); the
/// GGUF header and measured loads are looked up by its path.
pub fn plan_for_input(
    base: FitInput,
    hw: &crate::hardware::SystemHardwareInfo,
    need: ToolNeed,
) -> PreparePlan {
    plan_for_input_with_available_memory(base, hw, need, None)
}

pub fn plan_for_input_with_available_memory(
    base: FitInput,
    hw: &crate::hardware::SystemHardwareInfo,
    need: ToolNeed,
    available: Option<AvailableMemory>,
) -> PreparePlan {
    let base = vram::complete_input(base);
    let path = base.model_path.clone().unwrap_or_default();
    let meta = crate::models::cached_metadata(std::path::Path::new(&path));
    let measured = vram::measurements_for(&path);
    let mut budget = vram::budget_for_model(hw, Some(&path));
    if let Some(a) = available {
        budget.vram_bytes = a.total_vram_bytes;
    }
    plan_with_available_memory(&base, meta.as_ref(), &budget, &measured, need, available)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod auto_context_tests {
    use super::resolve_auto_context;

    #[test]
    fn an_automatic_context_follows_the_server_kind() {
        // Chat: the trained context, at most 32768.
        assert_eq!(resolve_auto_context(true, None, Some(131_072)), 32_768);
        assert_eq!(resolve_auto_context(true, None, Some(8_192)), 8_192);
        assert_eq!(resolve_auto_context(true, None, None), 32_768);
        // Embedding / rerank / whisper / router and the user's own `-c`: the old 4096.
        assert_eq!(resolve_auto_context(false, None, Some(131_072)), 4_096);
        assert_eq!(
            resolve_auto_context(true, Some("-c 2048"), Some(8_192)),
            4_096
        );
    }
}
