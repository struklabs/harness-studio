//! Concurrent requests of a llama-server (`-np` slots): the "Auto" rule and what Harness
//! passes for a set value. private hs-internal architecture notes ("Concurrent requests") has the reasoning,
//! private hs-internal performance notes the measurements.
//!
//! llama-server's own default (`-np -1`) is 4 slots sharing one unified KV pool of `-c`
//! tokens: any request may use the whole context, the slots share it. For full-attention
//! layers an extra slot therefore costs (almost) nothing; sliding-window layers
//! (`n_swa * slots + ubatch` cells) and recurrent layers (one state per slot) do grow with the
//! slots, and so do the context checkpoints in host memory. When requests together need more
//! than the pool, llama-server fails all running requests ("context size has been
//! exceeded"), so the slots also have to leave each concurrent request room.
//!
//! The automatic rule ([`advise`]):
//! 1. **Pool**: `context / request size` slots at most (1, 2 or 4), request size 8k tokens
//!    for a server the user starts (chat), 2k for the gateway's JIT loads and 32k for a server
//!    prepared for an agent tool. The gateway can take the smallest size because its scheduler
//!    holds each request's estimated tokens against the shared pool (`Scheduler::acquire`,
//!    token admission): requests that together exceed the pool wait instead of failing, and in
//!    the unified pool every request can still use the whole context. Before this, an API
//!    client got 1 slot at the default 32k context and four parallel streams ran one after the
//!    other (comparison benchmark: 53 t/s in total against LM Studio's 157); with 8k, the same
//!    happened on a 4 GB card, where the 4B's context is 12k (RTX 3050 Ti, Qwen3-4B: 54 t/s in
//!    total and up to 14.5 s to the first token; 148 t/s and 0.8 s with 4 slots).
//! 2. **Memory**: when one more slot costs more than [`CHEAP_SLOT_BYTES`] (SWA window or
//!    recurrent state + its checkpoints), a server the user starts gets 1 slot and an agent
//!    server at most 2, and only while the model still fits fully on the GPU (85%).
//! 3. 4 slots = llama-server's own default: Harness passes nothing. Fewer: `-np N` and
//!    `--kv-unified` (an explicit `-np` turns the unified pool off otherwise, which would cut
//!    every request's context to `context / N`).
//!
//! Order at launch: the slots first, then the automatic KV precision (`kv_policy`) for that
//! slot count; the slot rule itself assumes full-precision (f16) KV when the types are
//! automatic.
//!
//! A value the user chose is used as is (with `--kv-unified` for N > 1 unless the user set
//! it); `-np` / `--parallel` in the custom arguments wins over everything.

use crate::gguf::GgufMetadata;
use crate::vram::{FitCategory, FitInput, Measurement, MemoryBudget};
use serde::{Deserialize, Serialize};

const MIB: u64 = 1024 * 1024;
/// One more slot costing at most this much (device + host) counts as free.
pub const CHEAP_SLOT_BYTES: u64 = 64 * MIB;
/// Tokens a concurrent request of a user-started server (chat, Playground) should have.
pub const USER_REQUEST_TOKENS: u64 = 8192;
/// Tokens a concurrent request of an agent server (gateway JIT load, `harness launch`) should
/// have: the smallest context agent tools accept (`integrations::launch::LaunchTool::min_ctx`).
pub const AGENT_REQUEST_TOKENS: u64 = 32_768;
/// Tokens a concurrent request of a gateway-loaded server should have. The gateway's token
/// admission (see the module documentation) keeps requests inside the shared pool, so this is
/// only a share of it, not a limit: a request may still use the whole unified pool, and one
/// that does not fit next to the others waits. A pool of 8k or more gets 4 slots.
pub const GATEWAY_REQUEST_TOKENS: u64 = 2048;
/// Most slots the automatic rule picks (= llama-server's own automatic value).
pub const AUTO_MAX: u32 = 4;

/// Who starts a server.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum SlotOrigin {
    /// Dashboard, Playground, Library, CLI `serve`: usually one person chatting.
    #[default]
    User,
    /// A server prepared for an agent tool (`harness launch`, the Connections page): it is
    /// reached directly, nothing holds requests back when they would exceed the pool.
    Agents,
    /// The gateway's JIT loads (API clients, `harness launch` through the managed endpoint).
    /// The scheduler admits requests against the shared pool.
    Gateway,
}

impl SlotOrigin {
    pub fn is_user(&self) -> bool {
        *self == SlotOrigin::User
    }

    /// Requests of this origin are held back by the gateway when they would not fit the pool.
    pub fn is_gateway(&self) -> bool {
        *self == SlotOrigin::Gateway
    }
}

/// How many concurrent requests a server serves, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SlotAdvice {
    /// Slots the server runs with.
    pub slots: u32,
    /// `true` = chosen by the automatic rule (the setting is "Auto").
    pub auto: bool,
    /// What the automatic rule picks for this model, context and machine.
    pub recommended: u32,
    /// What it picks when the model serves agents directly (a server prepared for an agent
    /// tool; the gateway's own loads use the smaller request size, see [`SlotOrigin::Gateway`]).
    pub recommended_agents: u32,
    /// i18n key of the one-line reason of `recommended` (`slots.reason.*`).
    pub reason: String,
    /// Memory one more slot adds (sliding-window / recurrent state on the device +
    /// checkpoints in host memory).
    pub per_slot_bytes: u64,
    /// Context each of the `recommended` concurrent requests can count on.
    pub ctx_per_slot: u64,
}

impl SlotAdvice {
    /// Plain-language reason of `recommended` for the server log (`lang`: "tr" or other).
    pub fn reason_text(&self, lang: &str) -> String {
        let tr = lang == "tr";
        let mb = format!("{} MiB", self.per_slot_bytes >> 20);
        let ctx = self.ctx_per_slot;
        match (self.reason.as_str(), tr) {
            ("slots.reason.shared_pool", true) => {
                "ek istekler bağlam belleğini paylaşır, neredeyse ek bellek istemez".into()
            }
            ("slots.reason.shared_pool", false) => {
                "extra requests share the context memory and cost almost nothing".into()
            }
            ("slots.reason.small_context", true) => {
                format!("her eşzamanlı isteğe ~{ctx} token bağlam kalsın diye")
            }
            ("slots.reason.small_context", false) => {
                format!("so each concurrent request keeps ~{ctx} tokens of context")
            }
            ("slots.reason.single_user", true) => {
                format!("bu modelde her ek istek ~{mb} bellek ister; tek kullanıcı için 1 yeterli")
            }
            ("slots.reason.single_user", false) => {
                format!(
                    "each extra request costs ~{mb} with this model; 1 is enough for one person"
                )
            }
            ("slots.reason.agents_costly", true) => {
                format!("ajanlar için 2; her ek istek ~{mb} bellek ister")
            }
            ("slots.reason.agents_costly", false) => {
                format!("2 for agents; each extra request costs ~{mb}")
            }
            ("slots.reason.memory_tight", true) => {
                format!("bellek dar: her ek istek ~{mb} bellek ister")
            }
            ("slots.reason.memory_tight", false) => {
                format!("memory is tight: each extra request costs ~{mb}")
            }
            ("slots.reason.runtime_default", true) => {
                "bu llama-server sürümü paylaşımlı bağlamı desteklemiyor".into()
            }
            ("slots.reason.runtime_default", false) => {
                "this llama-server build has no shared context".into()
            }
            (other, _) => other.to_string(),
        }
    }

    /// One server-log line: the slot count, and either the automatic reason or the
    /// user's choice next to the recommendation.
    pub fn log_line(&self, lang: &str) -> String {
        let tr = lang == "tr";
        let reason = self.reason_text(lang);
        match (self.auto, tr) {
            (true, true) => format!(
                "[Harness Studio] Eşzamanlı istek: {} (otomatik; {reason}).",
                self.slots
            ),
            (true, false) => format!(
                "[Harness Studio] Concurrent requests: {} (automatic; {reason}).",
                self.slots
            ),
            (false, true) => format!(
                "[Harness Studio] Eşzamanlı istek: {} (elle seçildi; önerilen {}: {reason}).",
                self.slots, self.recommended
            ),
            (false, false) => format!(
                "[Harness Studio] Concurrent requests: {} (set by you; recommended {}: {reason}).",
                self.slots, self.recommended
            ),
        }
    }
}

/// The automatic rule's choice for one origin.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub slots: u32,
    pub reason: &'static str,
    pub per_slot_bytes: u64,
    pub ctx_per_slot: u64,
}

/// `input` with `n` slots in a shared (unified) pool, as llama-server runs them.
pub fn with_slots(input: &FitInput, n: u32) -> FitInput {
    FitInput {
        parallel_slots: Some(n.max(1)),
        kv_unified: input.kv_unified.or(Some(n > 1)),
        ..input.clone()
    }
}

/// Largest of 1, 2, 4 not above `n`.
fn pow2_floor(n: u64) -> u32 {
    match n {
        0..=1 => 1,
        2..=3 => 2,
        _ => 4,
    }
}

/// The automatic rule (module docs) for `origin`.
pub fn decide(
    input: &FitInput,
    meta: Option<&GgufMetadata>,
    budget: &MemoryBudget,
    measured: &[Measurement],
    origin: SlotOrigin,
) -> Decision {
    // The slots are chosen before the KV precision (`kv_policy`, which then picks the cache
    // types for these slots): with automatic types, judge the slots at full precision so a
    // slot is never paid for by quantizing the cache.
    let input = &FitInput {
        kv_auto: false,
        ..input.clone()
    };
    let est = |n: u32| crate::vram::fit_with(&with_slots(input, n), meta, budget, measured);
    let one = est(1);
    let two = est(2);
    let per_slot_bytes = two
        .estimate
        .vram_bytes
        .saturating_sub(one.estimate.vram_bytes)
        + two
            .estimate
            .ram_bytes
            .saturating_sub(one.estimate.ram_bytes);
    let n_ctx = one.estimate.kv_cells.max(1);
    let request = match origin {
        SlotOrigin::User => USER_REQUEST_TOKENS,
        SlotOrigin::Agents => AGENT_REQUEST_TOKENS,
        SlotOrigin::Gateway => GATEWAY_REQUEST_TOKENS,
    };
    let by_ctx = pow2_floor(n_ctx / request).min(AUTO_MAX);
    let (slots, reason) = if per_slot_bytes <= CHEAP_SLOT_BYTES {
        if by_ctx >= AUTO_MAX {
            (AUTO_MAX, "slots.reason.shared_pool")
        } else {
            (by_ctx, "slots.reason.small_context")
        }
    } else if one.category != FitCategory::FullGpu {
        (1, "slots.reason.memory_tight")
    } else if origin == SlotOrigin::User {
        (1, "slots.reason.single_user")
    } else if by_ctx < 2 {
        (1, "slots.reason.small_context")
    } else if two.category != FitCategory::FullGpu {
        (1, "slots.reason.memory_tight")
    } else {
        (2, "slots.reason.agents_costly")
    };
    Decision {
        slots,
        reason,
        per_slot_bytes,
        ctx_per_slot: n_ctx / u64::from(slots),
    }
}

/// Slots for `input` (`parallel_slots: None` = automatic) with the recommendation.
pub fn advise(
    input: &FitInput,
    meta: Option<&GgufMetadata>,
    budget: &MemoryBudget,
    measured: &[Measurement],
) -> SlotAdvice {
    let own = decide(input, meta, budget, measured, input.origin);
    let agents = if input.origin == SlotOrigin::Agents {
        own.slots
    } else {
        decide(input, meta, budget, measured, SlotOrigin::Agents).slots
    };
    let set = input.parallel_slots.filter(|&n| n > 0);
    SlotAdvice {
        slots: set.unwrap_or(own.slots),
        auto: set.is_none(),
        recommended: own.slots,
        recommended_agents: agents,
        reason: own.reason.to_string(),
        per_slot_bytes: own.per_slot_bytes,
        ctx_per_slot: own.ctx_per_slot,
    }
}

/// What Harness passes for one launch.
#[derive(Debug, Clone, PartialEq)]
pub struct SlotPlan {
    pub advice: SlotAdvice,
    /// `-np` Harness passes (`None` = none: llama-server's default or the custom arguments').
    pub np: Option<u32>,
    /// `--kv-unified` Harness adds (`None` = nothing).
    pub kv_unified: Option<bool>,
}

/// `-np` / `--parallel` value in the custom arguments (`-1` = llama-server's automatic 4).
pub fn custom_slots(custom_args: Option<&str>) -> Option<u32> {
    let args = crate::process::split_args(custom_args?).ok()?;
    let mut found = None;
    for (i, a) in args.iter().enumerate() {
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) => (f, Some(v.to_string())),
            None => (a.as_str(), None),
        };
        if flag == "-np" || flag == "--parallel" {
            let value = inline.or_else(|| args.get(i + 1).cloned());
            found = value.and_then(|v| v.trim().parse::<i64>().ok());
        }
    }
    found.map(|n| if n < 1 { AUTO_MAX } else { n as u32 })
}

/// The custom arguments set `--kv-unified` / `--no-kv-unified` themselves.
fn custom_sets_kv_unified(custom_args: Option<&str>) -> bool {
    custom_args
        .and_then(|c| crate::process::split_args(c).ok())
        .is_some_and(|args| {
            args.iter().any(|a| {
                matches!(
                    a.split('=').next().unwrap_or(a),
                    "--kv-unified" | "-kvu" | "--no-kv-unified" | "-no-kvu"
                )
            })
        })
}

/// Slots of a launch and the flags Harness adds for them; `None` in router mode (every model
/// gets llama-server's default). `base` is [`crate::vram::base_input_from_launch`]. Blocking
/// (GGUF header, measurements, hardware; all cached).
pub fn plan_for_launch(config: &crate::process::LaunchConfig, base: &FitInput) -> Option<SlotPlan> {
    if config.router_mode {
        return None;
    }
    let has_unified = runtime_has_unified(config);
    let path = base.model_path.clone().unwrap_or_default();
    let meta = crate::models::cached_metadata(std::path::Path::new(&path));
    let measured = crate::vram::measurements_for(&path);
    let budget = crate::vram::budget_for_model(&crate::hardware::detect_hardware(), Some(&path));
    plan_for_launch_with(config, base, meta.as_ref(), &budget, &measured, has_unified)
}

/// Whether the selected runtime supports a unified KV pool. Unknown/unprobed current builds
/// are treated as supporting it; a successfully parsed older help output is authoritative.
pub fn runtime_has_unified(config: &crate::process::LaunchConfig) -> bool {
    let caps = crate::capabilities::registry().cached(&config.binary_path);
    let known = caps.as_ref().filter(|c| c.help_parsed);
    // Builds before the unified pool (late 2025) have neither `--kv-unified` nor `-np auto`
    // (their default is 1 slot).
    known.is_none_or(|c| c.has_flag("--kv-unified"))
}

/// The custom arguments turn the unified pool off (`--no-kv-unified`, `-no-kvu`).
fn custom_disables_kv_unified(custom_args: Option<&str>) -> bool {
    custom_args
        .and_then(|c| crate::process::split_args(c).ok())
        .is_some_and(|args| {
            args.iter().any(|a| {
                matches!(
                    a.split('=').next().unwrap_or(a),
                    "--no-kv-unified" | "-no-kvu"
                )
            })
        })
}

/// The server will run several slots in one shared (unified) KV pool: the plan has more than one
/// slot and nothing turned the pool off; either Harness adds `--kv-unified`, the user did, or
/// llama-server's own automatic 4 slots (no `-np`) are unified by default. A custom `-np` alone
/// gives every slot its own cache.
pub fn runs_unified_pool(plan: &SlotPlan, config: &crate::process::LaunchConfig) -> bool {
    use crate::params::ParamValue;
    if plan.advice.slots <= 1 || custom_disables_kv_unified(config.custom_args.as_deref()) {
        return false;
    }
    if let Some(ParamValue::Bool(on)) = config.params.get("kv_unified") {
        return *on;
    }
    plan.kv_unified == Some(true)
        || custom_sets_kv_unified(config.custom_args.as_deref())
        || (plan.np.is_none()
            && custom_slots(config.custom_args.as_deref()).is_none()
            && plan.advice.slots == AUTO_MAX)
}

/// Backends of the devices a llama-server runs on: the `--main-gpu` device (the runtime's index)
/// when the model is not split, else every device of the runtime. Without a device list (a probe
/// that could not list them) the list is empty.
pub fn served_backends(
    caps: &crate::capabilities::RuntimeCapabilities,
    main_gpu: Option<u32>,
    split_mode: Option<&str>,
) -> Vec<String> {
    let single = split_mode.is_none_or(|m| m.trim().eq_ignore_ascii_case("none"));
    match main_gpu.and_then(|i| caps.devices.get(i as usize)) {
        Some(device) if single => vec![device.backend.clone()],
        _ => caps.backends(),
    }
}

/// Whether a server whose slots share one unified pool turns llama.cpp's host prompt cache off
/// (`--cache-ram 0`), by the backends of the devices it runs on ([`served_backends`]).
///
/// With the cache on, llama-server saves the idle slots to it when a new request starts and
/// clears them from the pool; without it they stay, and every later request attends over their
/// cells too. Which costs more depends on the backend:
/// - **Vulkan** (b11552, RX 7900 GRE): saving stalls every wave of concurrent requests by
///   ~3.7 s (4 streams of Qwen3-4B: 172 t/s in total against 400 with the cache off): off.
/// - **CUDA** (b11574, RTX 3050 Ti 4 GB): the save takes ~0.7 s, once, at the first request
///   after a wave; without it a single stream after concurrent use runs 10-33 % slower
///   (Qwen3-4B 38-50 t/s against 56), a small model up to 40 % (Qwen3-0.6B 108 against 180):
///   on.
///
/// Other backends keep the cache off until measured. A model split over devices of different
/// backends keeps it off as well (only an all-CUDA set turns it on).
pub fn host_cache_off_for_unified_pool(backends: &[String]) -> bool {
    backends.is_empty() || !backends.iter().all(|b| b.eq_ignore_ascii_case("CUDA"))
}

/// Pure launch-slot planner. The Dashboard estimate and the process launcher both enter here
/// with the same launch config; the outer wrapper only gathers cached runtime/model/machine data.
pub fn plan_for_launch_with(
    config: &crate::process::LaunchConfig,
    base: &FitInput,
    meta: Option<&GgufMetadata>,
    budget: &MemoryBudget,
    measured: &[Measurement],
    has_unified: bool,
) -> Option<SlotPlan> {
    if config.router_mode {
        return None;
    }
    let user_unified =
        base.kv_unified.is_some() || custom_sets_kv_unified(config.custom_args.as_deref());
    let unified_for = |n: u32| (n > 1 && has_unified && !user_unified).then_some(true);

    if let Some(n) = custom_slots(config.custom_args.as_deref()) {
        let mut advice = advise(
            &FitInput {
                parallel_slots: Some(n),
                ..base.clone()
            },
            meta,
            budget,
            measured,
        );
        advice.auto = false;
        return Some(SlotPlan {
            advice,
            np: None,
            kv_unified: None,
        });
    }
    if let Some(n) = config.parallel_slots.filter(|&n| n > 0) {
        let advice = advise(
            &FitInput {
                parallel_slots: Some(n),
                ..base.clone()
            },
            meta,
            budget,
            measured,
        );
        return Some(SlotPlan {
            advice,
            np: Some(n),
            kv_unified: unified_for(n),
        });
    }
    let auto = FitInput {
        parallel_slots: None,
        ..base.clone()
    };
    let mut advice = advise(&auto, meta, budget, measured);
    if !has_unified {
        advice.slots = 1;
        advice.reason = "slots.reason.runtime_default".into();
        return Some(SlotPlan {
            advice,
            np: None,
            kv_unified: None,
        });
    }
    let n = advice.slots;
    Some(SlotPlan {
        np: (n != AUTO_MAX).then_some(n),
        kv_unified: unified_for(n),
        advice,
    })
}

#[cfg(test)]
#[path = "slots_tests.rs"]
mod tests;
