//! The default load of a model whose definition leaves the context, `-ngl` and cache types
//! automatic: one rule for every path that starts llama-server (the gateway's load on request,
//! the Dashboard's Start, `harness load`, the Control API, the Library and the Playground).
//! A small card keeps the whole model on the GPU at a smaller context instead of a bigger context
//! with layers on the CPU; a big card keeps the full default context.

use crate::prepare::FIT_MIN_CONTEXT;

/// The context for a llama.cpp chat load whose definition sets none (`context_size` 0): the
/// model's trained context, at most [`crate::prepare::DEFAULT_CHAT_CONTEXT`], instead of
/// llama.cpp's 4096. `None` when the context is set or the server is not a chat one
/// (embedding, rerank, whisper, router). Blocking.
pub fn default_context_for(config: &crate::process::LaunchConfig) -> Option<u32> {
    if config.context_size > 0
        || config.router_mode
        || crate::engine::server_for(config).family() != crate::engine::LLAMA_CPP
        || crate::prepare::custom_args_set_context(config.custom_args.as_deref())
    {
        return None;
    }
    let meta = crate::models::cached_metadata(&config.model_path);
    let mut probe = config.clone();
    let mode = crate::serve_mode::prepare(&mut probe, meta.as_ref(), None).mode;
    if mode != crate::serve_mode::ServeMode::Chat {
        return None;
    }
    Some(crate::prepare::default_chat_context(
        meta.as_ref().and_then(|m| m.context_length),
    ))
}

/// What [`full_offload_context`] decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct FullOffload {
    pub context: u32,
    pub layers: u32,
    /// `Some(1)` when only one slot keeps the whole model on the GPU at this context; `None`
    /// leaves the automatic slot count alone.
    pub slots: Option<u32>,
    /// The automatic KV cache types (K, V) this load is estimated with and the launch's
    /// automatic policy picks ([`crate::kv_policy::choose`]): q8_0 / q8_0 when f16 at this
    /// context breaks Apple unified memory's limits ([`unified_fits`]). `None`: not known.
    pub cache_type_k: Option<&'static str>,
    pub cache_type_v: Option<&'static str>,
    /// The whole model is on the GPU (a full offload) rather than the hybrid fallback at the
    /// floor context; only a full offload is admitted against [`crate::vram::full_offload_headroom`].
    #[serde(skip)]
    pub whole: bool,
    /// The hybrid fallback keeps fewer layers than the fit advises for the card, because the
    /// card's free memory (other servers or programs on it) holds no more.
    #[serde(skip)]
    pub free_memory_cut: bool,
}

/// Smallest context at which the default load still prefers a full GPU offload over a bigger
/// context with a partial offload (product decision, RTX 3050 Ti 4 GB: Qwen3-4B at
/// 16k / 2 slots kept 22 of 37 layers on the GPU and generated 21 t/s; 4k with a full offload
/// 29 t/s, four parallel streams 33 against 72 t/s).
pub const FULL_OFFLOAD_MIN_CONTEXT: u32 = 8192;
/// Contexts tried below the automatic one when looking for a full offload, largest first.
const FULL_OFFLOAD_STEPS: [u32; 5] = [32_768, 24_576, 16_384, 12_288, FULL_OFFLOAD_MIN_CONTEXT];

/// For a context the gateway chose itself ([`default_context_for`]) with an automatic cache and
/// `-ngl`: the largest context from `context` down to [`FULL_OFFLOAD_MIN_CONTEXT`] (8k) at which
/// the automatic cache keeps the whole model on the GPU, first with the automatic slot count,
/// then with one slot. Full offload wins over context on small cards. When not even 8k with one
/// slot keeps the whole model on the GPU, the older rule: the floor ([`FIT_MIN_CONTEXT`]) with
/// its recommended `-ngl` (a smaller cache leaves more layers on the GPU). `None` keeps
/// `context`: cache types, slots or `-ngl` set, or no GPU. Blocking.
/// Without this a 4 GB card got Qwen3-4B at 32k with f16 and `-ngl 5` (14 t/s), later at 16k
/// with 22 of 37 layers (21 t/s); the user never asked for either.
pub fn full_offload_context(
    config: &crate::process::LaunchConfig,
    context: u32,
    hw: &crate::hardware::SystemHardwareInfo,
) -> Option<FullOffload> {
    full_offload_context_with(
        config,
        context,
        |c| crate::vram::fit_launch(c, hw),
        os_need_limit(hw, 0),
    )
}

/// The largest estimate (bytes) a full offload may have on this machine right now: what the OS
/// says is free on the primary GPU minus [`crate::vram::full_offload_headroom`]
/// (`configured_mib`: the gateway's memory headroom, 0 = automatic). The figure the gateway's
/// admission accepts such a load against, so the rule and the admission agree. `None` when the
/// card's usage is unknown (the rule then judges by the card's size alone).
pub fn os_need_limit(hw: &crate::hardware::SystemHardwareInfo, configured_mib: u64) -> Option<u64> {
    // Apple Silicon has no separate VRAM: the GPU draws from the same pool as the OS, the
    // browser and everything else, so what the OS says is free (not the Metal working set,
    // which is a fixed share of the total) is the live limit. A small margin stays for the
    // server's host-side buffers.
    if hw.unified_memory_budget_bytes.is_some_and(|b| b > 0) && hw.total_ram_bytes > 0 {
        return Some(
            hw.available_ram_bytes
                .saturating_sub(UNIFIED_AVAILABLE_MARGIN),
        );
    }
    let (available, total) = crate::vram::os_available_vram(hw)?;
    Some(crate::vram::full_offload_need_limit(
        available,
        total,
        configured_mib * 1024 * 1024,
    ))
}

/// Left out of the free memory of an Apple Silicon Mac when sizing a default load (the server's
/// host buffers, thread stacks and the Metal driver's own use).
pub const UNIFIED_AVAILABLE_MARGIN: u64 = 512 * 1024 * 1024;
/// Unified-memory Macs up to this much RAM get the stricter default-load limits below.
const UNIFIED_SMALL_RAM: u64 = 16 * 1024 * 1024 * 1024;

/// Memory an Apple Silicon Mac keeps for macOS and the apps around the model; a default load
/// never plans into it. Metal's working set (2/3 of RAM) is a ceiling for the GPU, not a promise
/// that the remaining third is enough for the rest of the system: measured on an 8 GB M1, a
/// default Qwen3-0.6B load (32k, f16 KV, 3.7 GB) left the machine at 89% memory. Rule: at least
/// 2.5 GiB, and 30% of RAM on machines up to 16 GiB (the OS and a browser take the same few
/// gigabytes however small the machine is), 15% above that.
pub fn unified_os_reserve(ram_bytes: u64) -> u64 {
    let share = if ram_bytes <= UNIFIED_SMALL_RAM {
        ram_bytes / 10 * 3
    } else {
        ram_bytes / 20 * 3
    };
    share.max(5 * 1024 * 1024 * 1024 / 2)
}

/// Largest KV cache (bytes) a default load may reserve on a small unified-memory Mac: one eighth
/// of RAM (1 GiB on 8 GB, 2 GiB on 16 GB). A tiny model has so few weights that the working set
/// alone would allow a multi-gigabyte cache at 32k that nothing in a chat needs. `None`: no cap
/// (bigger machines). The floor context ([`FULL_OFFLOAD_MIN_CONTEXT`]) is exempt, so a model
/// with a heavy cache still gets 8k.
pub fn unified_kv_cap(ram_bytes: u64) -> Option<u64> {
    (ram_bytes > 0 && ram_bytes <= UNIFIED_SMALL_RAM).then_some(ram_bytes / 8)
}

/// Whether the estimate `r` at context `ctx` keeps to the limits of Apple unified memory (not
/// an iGPU on Windows / Linux, which keeps its own rule; `true` elsewhere): it leaves the OS its
/// share ([`unified_os_reserve`]) and does not reserve a huge cache for a small model
/// ([`unified_kv_cap`], the floor context exempt). The automatic cache choice
/// ([`crate::kv_policy::choose`]) applies it too, so a rung that breaks it gives way to q8_0 /
/// q8_0 at the same context before the rule steps the context down.
pub fn unified_fits(r: &crate::vram::FitResult, ctx: u32) -> bool {
    if !r.budget.unified || r.budget.shared_igpu {
        return true;
    }
    let ram = r.budget.ram_bytes;
    r.estimate.vram_bytes <= ram.saturating_sub(unified_os_reserve(ram))
        && (ctx <= FULL_OFFLOAD_MIN_CONTEXT
            || unified_kv_cap(ram).is_none_or(|cap| r.estimate.kv_bytes <= cap))
}

/// [`full_offload_context`] with the fit and the largest estimate (`need_limit`, see
/// [`os_need_limit`]) given. A context whose full offload is estimated above `need_limit` is
/// skipped, so the rule steps down (12k to 8k) instead of the admission cutting layers later.
pub fn full_offload_context_with(
    config: &crate::process::LaunchConfig,
    context: u32,
    fit: impl Fn(&crate::process::LaunchConfig) -> crate::vram::FitResult,
    need_limit: Option<u64>,
) -> Option<FullOffload> {
    if !crate::kv_policy::is_automatic(config) || context == 0 {
        return None;
    }
    let at = |ctx: u32, slots: Option<u32>| {
        let mut c = config.clone();
        c.context_size = ctx;
        // Judged as a full offload: the automatic KV cache is chosen for the layers it is
        // estimated with, and a launch arrives with the recommended `-ngl` of the automatic
        // context already filled in (`domain::launch`). With Qwen3-4B on a 4 GB card that was
        // 22 of 37 layers, at which f16 "fits", so no context ever kept the whole model.
        c.gpu_layers = 999;
        if slots.is_some() {
            c.parallel_slots = slots;
        }
        fit(&c)
    };
    let whole = |r: &crate::vram::FitResult, ctx: u32| {
        u64::from(r.recommended_gpu_layers) > r.estimate.total_layers
            && unified_fits(r, ctx)
            && need_limit.is_none_or(|limit| r.estimate.vram_bytes <= limit)
    };
    let mut contexts = vec![context];
    contexts.extend(
        FULL_OFFLOAD_STEPS
            .iter()
            .copied()
            .filter(|c| *c < context && *c >= FULL_OFFLOAD_MIN_CONTEXT.min(context)),
    );
    // The slot count is the user's when set: only an automatic one may drop to 1.
    let slot_tries: &[Option<u32>] = if config.parallel_slots.is_none() {
        &[None, Some(1)]
    } else {
        &[None]
    };
    for &ctx in &contexts {
        for &slots in slot_tries {
            let r = at(ctx, slots);
            if whole(&r, ctx) {
                let (cache_type_k, cache_type_v) = cache_types(&r);
                return Some(FullOffload {
                    context: ctx,
                    layers: r.recommended_gpu_layers,
                    // Only worth forcing when the automatic count did not fit.
                    slots,
                    cache_type_k,
                    cache_type_v,
                    whole: true,
                    free_memory_cut: false,
                });
            }
        }
    }
    let floor = FIT_MIN_CONTEXT.min(context);
    let r = at(floor, None);
    let advised = r.recommended_gpu_layers;
    if advised == 0 {
        return None;
    }
    // The fit advises layers for the whole card. With another server (or program) on it, what
    // is free now may hold fewer: the most layers whose estimate stays within `need_limit`, as
    // the gateway's admission cuts them. Without this a second server started next to a
    // running one got every layer: Windows then pages both in and out of VRAM on every switch
    // (1.3-2 s on an RTX 3050 Ti), and drivers that cannot page fail the load.
    let at_layers = |n: u32| {
        let mut c = config.clone();
        c.context_size = floor;
        c.gpu_layers = n;
        fit(&c)
    };
    let limited = need_limit.filter(|limit| at_layers(advised).estimate.vram_bytes > *limit);
    let (layers, r) = match limited {
        None => (advised, r),
        Some(limit) => {
            // Memory grows with the layer count: the largest count within the limit.
            let (mut lo, mut hi) = (0u32, advised - 1);
            while lo < hi {
                let mid = (lo + hi).div_ceil(2);
                if at_layers(mid).estimate.vram_bytes <= limit {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            (lo, at_layers(lo))
        }
    };
    let (cache_type_k, cache_type_v) = cache_types(&r);
    Some(FullOffload {
        context: floor,
        layers,
        slots: None,
        cache_type_k,
        cache_type_v,
        whole: false,
        free_memory_cut: limited.is_some(),
    })
}

/// The automatic cache types of an estimate, `(None, None)` when it has no automatic choice.
fn cache_types(r: &crate::vram::FitResult) -> (Option<&'static str>, Option<&'static str>) {
    r.kv.as_ref()
        .and_then(|kv| kv.level.types())
        .map_or((None, None), |(k, v)| (Some(k), Some(v)))
}

/// Whether a start with the automatic context may take this rule: `-ngl` is the estimate's
/// (not the user's, `LaunchConfig::gpu_layers_auto`), `-fit` does not choose the layers, and the
/// server is a llama.cpp chat one whose context the launch resolves itself (`chat_auto_context`).
pub fn applies_to_start(config: &crate::process::LaunchConfig, chat_auto_context: bool) -> bool {
    chat_auto_context
        && config.gpu_layers_auto
        && !config.router_mode
        && !crate::process::fit_chooses_layers(config)
}

/// Applies [`full_offload_context_with`] to the launch whose automatic context resolved to
/// `context`: sets the context, `-ngl` and, when only one slot keeps the model on the GPU, the
/// slot count. `None` leaves the launch as it is.
pub fn apply_to_launch_with(
    launch: &mut crate::process::LaunchConfig,
    context: u32,
    fit: impl Fn(&crate::process::LaunchConfig) -> crate::vram::FitResult,
    need_limit: Option<u64>,
) -> Option<FullOffload> {
    let choice = full_offload_context_with(launch, context, fit, need_limit)?;
    launch.context_size = choice.context;
    launch.gpu_layers = choice.layers;
    if choice.slots.is_some() {
        launch.parallel_slots = choice.slots;
    }
    Some(choice)
}

/// The rule again for a load whose full offload did not fit the memory the card can give
/// (`need_limit` bytes for the estimate, see [`os_need_limit`]): the launch with the next
/// smaller context (or one slot) that still keeps the whole model on the GPU, so the gateway's
/// admission steps the rule down instead of cutting layers. `None` when none does.
pub fn step_down(
    config: &crate::process::LaunchConfig,
    need_limit: u64,
    fit: impl Fn(&crate::process::LaunchConfig) -> crate::vram::FitResult,
) -> Option<crate::process::LaunchConfig> {
    let mut stepped = config.clone();
    let choice = apply_to_launch_with(&mut stepped, config.context_size, fit, Some(need_limit))
        .filter(|f| f.whole)?;
    (choice.context != config.context_size || stepped.parallel_slots != config.parallel_slots)
        .then_some(stepped)
}

/// Everything an automatic default load starts with, for forms and `harness show` that display it
/// before Start: the full-offload choice plus the slot count and the KV cache the launch ends
/// up with (the cache types and the flash attention they need).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AutomaticLaunch {
    pub context: u32,
    pub layers: u32,
    /// Concurrent requests (`-np`) the launch runs with; `None` when no slot plan applies.
    pub slots: Option<u32>,
    /// Cache types of the automatic KV precision (`f16` for the full level).
    pub cache_type_k: Option<String>,
    pub cache_type_v: Option<String>,
    /// A quantized V cache needs flash attention: the launch turns it on.
    pub flash_attn: bool,
}

/// [`automatic_launch`] with the fit and the largest estimate given (tests, callers that
/// already have the machine facts). The launch the choice produces is estimated once more, so
/// the slot count and the KV precision are the ones that launch's plan arrives at.
pub fn automatic_launch_with(
    config: &crate::process::LaunchConfig,
    context: u32,
    fit: impl Fn(&crate::process::LaunchConfig) -> crate::vram::FitResult,
    need_limit: Option<u64>,
) -> Option<AutomaticLaunch> {
    let mut launch = config.clone();
    let choice = apply_to_launch_with(&mut launch, context, &fit, need_limit)?;
    let result = fit(&launch);
    let kv = result.kv;
    let quantized = kv
        .as_ref()
        .is_some_and(|k| crate::kv_policy::is_quantized(Some(&k.cache_type_v)));
    Some(AutomaticLaunch {
        context: choice.context,
        layers: choice.layers,
        slots: result.slots.map(|s| s.slots).or(choice.slots),
        cache_type_k: kv.as_ref().map(|k| k.cache_type_k.clone()),
        cache_type_v: kv.as_ref().map(|k| k.cache_type_v.clone()),
        flash_attn: quantized,
    })
}

/// What the default load of `config` (context and `-ngl` automatic) starts with on this
/// machine: the same choice the launch makes, for forms that show it before Start. `None` when
/// the rule does not apply (set values, non-chat or router server, user cache types). Blocking.
pub fn automatic_choice(
    config: &crate::process::LaunchConfig,
    hw: &crate::hardware::SystemHardwareInfo,
) -> Option<AutomaticLaunch> {
    let context = default_context_for(config)?;
    automatic_launch_with(
        config,
        context,
        |c| crate::vram::fit_launch(c, hw),
        os_need_limit(hw, 0),
    )
}

/// The launch as Start runs it when its context is automatic (`ProcessManager::start`): the
/// default chat context ([`default_context_for`]), shrunk on a small card so the whole model
/// stays on the GPU, with one slot when only one keeps it there. For estimates of a form, which
/// otherwise describe the full default context and its slot advice, not the load Start runs.
/// Leaves `config` as it is when the context is set or the server is no llama.cpp chat one.
/// Blocking.
pub fn resolve_for_start(
    config: &mut crate::process::LaunchConfig,
    hw: &crate::hardware::SystemHardwareInfo,
) {
    let Some(context) = default_context_for(config) else {
        return;
    };
    config.context_size = context;
    if applies_to_start(config, true) {
        apply_to_launch_with(
            config,
            context,
            |c| crate::vram::fit_launch(c, hw),
            os_need_limit(hw, 0),
        );
    }
}

/// Server-log line for a load whose layers the card's free memory cut ([`FullOffload::free_memory_cut`]).
pub fn free_memory_log_line(layers: u32, lang: &str) -> String {
    match (layers, lang == "tr") {
        (0, true) => "[Harness Studio] Ekran kartındaki boş bellek (başka sunucular ya da programlar kullanıyor) hiçbir katmana yetmiyor; model CPU'da çalışıyor. Diğer sunucuyu durdurursanız yeniden başlatınca katmanlar GPU'ya taşınır.".to_string(),
        (0, false) => "[Harness Studio] The GPU's free memory (other servers or programs use it) holds no layer; the model runs on the CPU. Stop the other server and restart this one to move layers to the GPU.".to_string(),
        (n, true) => format!("[Harness Studio] Ekran kartındaki boş bellek (başka sunucular ya da programlar kullanıyor) yalnız {n} katmana yetiyor; kalan katmanlar CPU'da. Diğer sunucuyu durdurursanız yeniden başlatınca daha çok katman GPU'ya sığar."),
        (n, false) => format!("[Harness Studio] The GPU's free memory (other servers or programs use it) holds only {n} layers; the rest run on the CPU. Stop the other server and restart this one to keep more layers on the GPU."),
    }
}

/// Server-log line for a default context shrunk so more of the model stays on the GPU.
pub fn full_offload_log_line(context: u32, requested: u32, lang: &str) -> String {
    if lang == "tr" {
        format!("[Harness Studio] Ekran kartında daha çok katman kalsın diye bağlam {context} (otomatik {requested} yerine; tanım değişmedi).")
    } else {
        format!("[Harness Studio] Context {context} so more layers stay on the GPU (instead of the automatic {requested}; the definition is unchanged).")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_default_context_shrinks_before_layers_leave_a_4gb_cuda_card() {
        // Live (RTX 3050 Ti 4 GB): an empty Qwen3-4B definition got 32k, f16 and -ngl 5 (14 t/s)
        // once q4_0 / q4_0 left the automatic ladder.
        let text = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/data/gguf-vram-shapes.json"),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let e = v
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["file"] == "Qwen3-4B-Instruct-2507-Q4_K_M.gguf")
            .unwrap();
        let size = e["size_bytes"].as_u64().unwrap();
        let meta: crate::gguf::GgufMetadata = serde_json::from_value(e["meta"].clone()).unwrap();
        let budget = crate::vram::MemoryBudget {
            vram_bytes: 4 << 30,
            ram_bytes: 16 << 30,
            unified: false,
            shared_igpu: false,
            igpu_offload_faster: None,
            cuda: true,
            rocm: false,
        };
        let fit = |c: &crate::process::LaunchConfig| {
            let mut base = crate::vram::base_input_from_launch(c);
            base.model_size_bytes = size;
            crate::vram::fit_launch_with(
                c,
                &base,
                Some(&meta),
                &budget,
                &[],
                false,
                crate::hardware::RecommendedBackend::Cuda12,
            )
        };
        let config = crate::process::LaunchConfig {
            context_size: 32_768,
            gpu_layers: 999,
            parallel_slots: Some(1),
            ..Default::default()
        };
        let at_32k = fit(&config).recommended_gpu_layers;
        let FullOffload {
            context: ctx,
            layers,
            ..
        } = full_offload_context_with(&config, 32_768, fit, None).unwrap();
        assert!((FULL_OFFLOAD_MIN_CONTEXT..32_768).contains(&ctx), "{ctx}");
        assert!(layers > at_32k, "{layers} vs {at_32k} at 32k");
        let chosen = fit(&crate::process::LaunchConfig {
            context_size: ctx,
            gpu_layers: layers,
            ..config.clone()
        });
        let kv = chosen.kv.unwrap();
        assert_ne!(kv.level, crate::kv_policy::KvLevel::Compact);
        // The definition's own cache types: nothing changes here (the memory fit handles it).
        let mut own = config.clone();
        own.cache_type_k = Some("q4_0".into());
        own.cache_type_v = Some("q4_0".into());
        assert_eq!(full_offload_context_with(&own, 32_768, fit, None), None);
        assert!(full_offload_log_line(16_384, 32_768, "tr").contains("daha çok katman"));
        assert!(full_offload_log_line(16_384, 32_768, "en").contains("more layers"));
    }

    /// `(size, meta)` of a shape in `tests/data/gguf-vram-shapes.json`.
    fn shape_of(file: &str) -> (u64, crate::gguf::GgufMetadata) {
        let text = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/data/gguf-vram-shapes.json"),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let e = v
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["file"] == file)
            .unwrap();
        (
            e["size_bytes"].as_u64().unwrap(),
            serde_json::from_value(e["meta"].clone()).unwrap(),
        )
    }

    /// The default load of `file` (`size` overrides the file size) on a card of `vram_gib`.
    fn default_load(
        file: &str,
        size: Option<u64>,
        vram_gib: u64,
        cuda: bool,
    ) -> (FullOffload, u64) {
        default_load_within(file, size, vram_gib, cuda, None)
    }

    /// [`default_load`] with the largest estimate the card can give (`need_limit`).
    fn default_load_within(
        file: &str,
        size: Option<u64>,
        vram_gib: u64,
        cuda: bool,
        need_limit: Option<u64>,
    ) -> (FullOffload, u64) {
        let (file_size, meta) = shape_of(file);
        let size = size.unwrap_or(file_size);
        let budget = crate::vram::MemoryBudget {
            vram_bytes: vram_gib << 30,
            ram_bytes: 32 << 30,
            unified: false,
            shared_igpu: false,
            igpu_offload_faster: None,
            cuda,
            rocm: false,
        };
        let fit = |c: &crate::process::LaunchConfig| {
            let mut base = crate::vram::base_input_from_launch(c);
            base.model_size_bytes = size;
            crate::vram::fit_launch_with(
                c,
                &base,
                Some(&meta),
                &budget,
                &[],
                false,
                crate::hardware::RecommendedBackend::Cuda12,
            )
        };
        let config = crate::process::LaunchConfig {
            context_size: 32_768,
            gpu_layers: 999,
            launched_by: crate::slots::SlotOrigin::Gateway,
            ..Default::default()
        };
        let total = fit(&config).estimate.total_layers;
        let r = full_offload_context_with(&config, 32_768, fit, need_limit).unwrap();
        (r, total)
    }

    /// The automatic launch of Qwen3-4B on a machine with the given budget.
    fn automatic_of(budget: crate::vram::MemoryBudget) -> AutomaticLaunch {
        let (size, meta) = shape_of("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
        let fit = |c: &crate::process::LaunchConfig| {
            let mut base = crate::vram::base_input_from_launch(c);
            base.model_size_bytes = size;
            crate::vram::fit_launch_with(
                c,
                &base,
                Some(&meta),
                &budget,
                &[],
                false,
                crate::hardware::RecommendedBackend::Cuda12,
            )
        };
        let config = crate::process::LaunchConfig {
            gpu_layers: 999,
            launched_by: crate::slots::SlotOrigin::Gateway,
            ..Default::default()
        };
        automatic_launch_with(&config, 32_768, fit, None).unwrap()
    }

    #[test]
    fn the_form_preview_shows_the_slots_and_cache_the_automatic_launch_uses() {
        // 4 GB card: the form used to show "Auto (4)" slots while the launch ran one.
        let small = automatic_of(crate::vram::MemoryBudget {
            vram_bytes: 4_154_458_112,
            ram_bytes: 16 << 30,
            unified: false,
            shared_igpu: false,
            igpu_offload_faster: None,
            cuda: true,
            rocm: false,
        });
        assert_eq!(small.context, 12_288, "{small:?}");
        assert_eq!(small.slots, Some(1), "{small:?}");
        assert!(small.cache_type_k.is_some() && small.cache_type_v.is_some());
        // Flash attention is on exactly when the V cache is quantized.
        assert_eq!(
            small.flash_attn,
            crate::kv_policy::is_quantized(small.cache_type_v.as_deref())
        );
        // A roomy card keeps the full context, f16 and no forced flash attention.
        let big = automatic_of(crate::vram::MemoryBudget {
            vram_bytes: 24 << 30,
            ram_bytes: 32 << 30,
            unified: false,
            shared_igpu: false,
            igpu_offload_faster: None,
            cuda: true,
            rocm: false,
        });
        assert_eq!(big.context, 32_768, "{big:?}");
        assert!(big.slots.is_some_and(|n| n >= 1));
        assert!(!big.flash_attn, "{big:?}");
    }

    #[test]
    fn a_4gb_card_prefers_full_offload_over_context_for_a_4b_model() {
        // RTX 3050 Ti 4 GB, Qwen3-4B Q4_K_M (2.3 GiB): before, 16k + q8_0 + 2 slots left
        // 22 of 37 layers on the GPU (21 t/s against ~29 t/s for a full offload).
        let (f, total) = default_load("Qwen3-4B-Instruct-2507-Q4_K_M.gguf", None, 4, true);
        assert!(
            u64::from(f.layers) > total,
            "full offload: {f:?} of {total}"
        );
        // Now: 12k and all 37 layers (the 16k step needs 22 of them on the CPU).
        assert_eq!(f.context, 12_288, "{f:?}");
    }

    #[test]
    fn the_recommended_layers_a_launch_arrives_with_do_not_hide_the_full_offload() {
        // RTX 3050 Ti 4 GB (3962 MiB): the gateway's plan carries `-ngl 22`, the recommendation
        // for the automatic 32k. Judged at 22 layers, f16 KV "fit" and the rule fell back to
        // 16k / 22 layers (22 t/s); a full offload at 12k generates 58 t/s.
        let (size, meta) = shape_of("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
        let budget = crate::vram::MemoryBudget {
            vram_bytes: 4_154_458_112,
            ram_bytes: 16 << 30,
            unified: false,
            shared_igpu: false,
            igpu_offload_faster: None,
            cuda: true,
            rocm: false,
        };
        let fit = |c: &crate::process::LaunchConfig| {
            let mut base = crate::vram::base_input_from_launch(c);
            base.model_size_bytes = size;
            crate::vram::fit_launch_with(
                c,
                &base,
                Some(&meta),
                &budget,
                &[],
                true,
                crate::hardware::RecommendedBackend::Cuda12,
            )
        };
        for ngl in [22, 26, 999] {
            let config = crate::process::LaunchConfig {
                gpu_layers: ngl,
                gpu_layers_auto: true,
                launched_by: crate::slots::SlotOrigin::Gateway,
                ..Default::default()
            };
            let f = full_offload_context_with(&config, 32_768, fit, None).unwrap();
            assert_eq!((f.context, f.layers), (12_288, 37), "-ngl {ngl}: {f:?}");
        }
    }

    #[test]
    fn the_os_figure_keeps_12k_where_the_probes_free_value_stepped_down() {
        // RTX 3050 Ti 4 GB, WDDM: the probe said 3295 MiB free, nvidia-smi 3962 MiB. The 12k
        // full offload is estimated at about 3537 MiB; a context the rule picks must be one the
        // admission takes (`vram::full_offload_need_limit`), so both read the same figure.
        const MIB: u64 = 1 << 20;
        let file = "Qwen3-4B-Instruct-2507-Q4_K_M.gguf";
        let os = crate::vram::full_offload_need_limit_with(3962 * MIB, 4096 * MIB, 0, 0);
        let (f, total) = default_load_within(file, None, 4, true, Some(os));
        assert_eq!(
            (f.context, f.layers, f.whole),
            (12_288, total as u32 + 1, true) // 36 layers + output = 37
        );
        // The probe's under-reported figure leaves not even 8k (3227 MiB): the hybrid rule.
        let probe = crate::vram::full_offload_need_limit_with(3295 * MIB, 4096 * MIB, 0, 0);
        let (g, _) = default_load_within(file, None, 4, true, Some(probe));
        assert!(!g.whole && g.context == FIT_MIN_CONTEXT, "{g:?}");
        // Nothing fits even at 8k: the old hybrid fallback (not a full offload).
        let (h, _) = default_load_within(file, None, 4, true, Some(1000 * MIB));
        assert!(!h.whole && h.context == FIT_MIN_CONTEXT, "{h:?}");
    }

    #[test]
    fn the_admission_steps_the_rule_down_instead_of_cutting_layers() {
        // The 12k full offload (about 3537 MiB) is above what the card can give: 8k, still
        // all 37 layers, instead of 12k with 24 layers.
        const MIB: u64 = 1 << 20;
        let (file_size, meta) = shape_of("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
        let budget = crate::vram::MemoryBudget {
            vram_bytes: 4 << 30,
            ram_bytes: 32 << 30,
            unified: false,
            shared_igpu: false,
            igpu_offload_faster: None,
            cuda: true,
            rocm: false,
        };
        let fit = |c: &crate::process::LaunchConfig| {
            let mut base = crate::vram::base_input_from_launch(c);
            base.model_size_bytes = file_size;
            crate::vram::fit_launch_with(
                c,
                &base,
                Some(&meta),
                &budget,
                &[],
                false,
                crate::hardware::RecommendedBackend::Cuda12,
            )
        };
        let chosen = crate::process::LaunchConfig {
            context_size: 12_288,
            gpu_layers: 37,
            ..Default::default()
        };
        let limit = crate::vram::full_offload_need_limit_with(3700 * MIB, 4096 * MIB, 0, 0);
        let stepped = step_down(&chosen, limit, fit).unwrap();
        assert_eq!(stepped.context_size, FULL_OFFLOAD_MIN_CONTEXT);
        assert_eq!(stepped.gpu_layers, 37, "all 37 layers and the output stay");
        // Room for 12k: nothing to step down to.
        let roomy = crate::vram::full_offload_need_limit_with(3962 * MIB, 4096 * MIB, 0, 0);
        assert!(step_down(&chosen, roomy, fit).is_none());
    }

    #[test]
    fn a_second_server_takes_only_the_layers_the_free_memory_holds() {
        // RTX 3050 Ti 4 GB with another server holding ~3.4 GB: the hybrid fallback took every
        // layer the card alone would hold, and Windows paged both servers on every switch.
        const MIB: u64 = 1 << 20;
        let file = "Qwen3-4B-Instruct-2507-Q4_K_M.gguf";
        let (card, total) = default_load_within(file, None, 4, true, None);
        let limit = 1500 * MIB;
        let (cut, _) = default_load_within(file, None, 4, true, Some(limit));
        assert!(!cut.whole && cut.free_memory_cut, "{cut:?}");
        assert_eq!(cut.context, FIT_MIN_CONTEXT);
        assert!(
            cut.layers > 0 && cut.layers < card.layers,
            "{cut:?} vs {card:?}"
        );
        // The cut keeps the most layers within the limit.
        let (file_size, meta) = shape_of(file);
        let budget = crate::vram::MemoryBudget {
            vram_bytes: 4 << 30,
            ram_bytes: 32 << 30,
            unified: false,
            shared_igpu: false,
            igpu_offload_faster: None,
            cuda: true,
            rocm: false,
        };
        let need = |layers: u32| {
            let c = crate::process::LaunchConfig {
                context_size: FIT_MIN_CONTEXT,
                gpu_layers: layers,
                launched_by: crate::slots::SlotOrigin::Gateway,
                ..Default::default()
            };
            let mut base = crate::vram::base_input_from_launch(&c);
            base.model_size_bytes = file_size;
            crate::vram::fit_launch_with(
                &c,
                &base,
                Some(&meta),
                &budget,
                &[],
                false,
                crate::hardware::RecommendedBackend::Cuda12,
            )
            .estimate
            .vram_bytes
        };
        assert!(need(cut.layers) <= limit && need(cut.layers + 1) > limit);
        // No room at all: the CPU, not the card's advice.
        let (none, _) = default_load_within(file, None, 4, true, Some(0));
        assert_eq!((none.layers, none.free_memory_cut), (0, true));
        // A roomy card or no usage figure: the fit's advice as before.
        assert!(!card.free_memory_cut && u64::from(card.layers) > total);
    }

    #[test]
    fn a_4gb_card_keeps_the_hybrid_rule_when_the_model_cannot_fit_even_at_8k() {
        // About an 8B Q4_K_M (4.9 GiB of weights): no full offload on 4 GB at any context.
        let (f, total) = default_load("Qwen3-4B-Instruct-2507-Q4_K_M.gguf", Some(5 << 30), 4, true);
        assert_eq!(f.context, FIT_MIN_CONTEXT, "{f:?}");
        assert!(u64::from(f.layers) <= total, "hybrid: {f:?} of {total}");
        assert_eq!(f.slots, None);
        // A 12B model: the same.
        let (g, total) = default_load("gemma-4-12B-it-Q4_K_M.gguf", None, 4, true);
        assert_eq!(g.context, FIT_MIN_CONTEXT, "{g:?}");
        assert!(u64::from(g.layers) <= total, "{g:?}");
    }

    #[test]
    fn a_16gb_card_keeps_the_full_default_context_with_the_automatic_slots() {
        // AMD (Vulkan) 16 GB: nothing changes, 32k stays and the slot count is not forced.
        for file in [
            "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
            "qwen2.5-coder-14b-instruct-q4_k_m.gguf",
        ] {
            let (f, total) = default_load(file, None, 16, false);
            assert_eq!(f.context, 32_768, "{file}: {f:?}");
            assert!(u64::from(f.layers) > total, "{file}: {f:?}");
            assert_eq!(f.slots, None, "{file}");
        }
    }

    /// What `ProcessManager::start` ends up with for a user's launch (`launched_by` user) of
    /// `file` whose definition left context, `-ngl` and cache automatic: the resolved launch
    /// (context 0 resolved to the chat default, `-ngl` the estimate's) after the shared rule.
    fn start_load(
        file: &str,
        size: Option<u64>,
        vram_gib: u64,
        cuda: bool,
    ) -> (crate::process::LaunchConfig, u64) {
        let (file_size, meta) = shape_of(file);
        let size = size.unwrap_or(file_size);
        let budget = crate::vram::MemoryBudget {
            vram_bytes: vram_gib << 30,
            ram_bytes: 32 << 30,
            unified: false,
            shared_igpu: false,
            igpu_offload_faster: None,
            cuda,
            rocm: false,
        };
        let fit = |c: &crate::process::LaunchConfig| {
            let mut base = crate::vram::base_input_from_launch(c);
            base.model_size_bytes = size;
            crate::vram::fit_launch_with(
                c,
                &base,
                Some(&meta),
                &budget,
                &[],
                false,
                crate::hardware::RecommendedBackend::Cuda12,
            )
        };
        // `resolve_launch`: layers are the estimate's (marked), the context is still automatic.
        let mut launch = crate::process::LaunchConfig {
            context_size: 0,
            gpu_layers: 999,
            gpu_layers_auto: true,
            ..Default::default()
        };
        let total = fit(&launch).estimate.total_layers;
        let requested = crate::prepare::resolve_auto_context(true, None, meta.context_length);
        launch.context_size = requested;
        assert!(applies_to_start(&launch, true));
        apply_to_launch_with(&mut launch, requested, fit, None).unwrap();
        (launch, total)
    }

    #[test]
    fn a_start_on_a_4gb_card_keeps_the_whole_4b_model_on_the_gpu() {
        // Dashboard Start / `harness load` / Control API / Library / Playground: same rule as
        // the gateway's load on request.
        let (l, total) = start_load("Qwen3-4B-Instruct-2507-Q4_K_M.gguf", None, 4, true);
        assert!(
            u64::from(l.gpu_layers) > total,
            "{} of {total}",
            l.gpu_layers
        );
        assert!(
            (FULL_OFFLOAD_MIN_CONTEXT..32_768).contains(&l.context_size),
            "{}",
            l.context_size
        );
    }

    #[test]
    fn a_start_on_a_4gb_card_keeps_the_hybrid_rule_for_a_model_that_cannot_fit() {
        let (l, total) = start_load("Qwen3-4B-Instruct-2507-Q4_K_M.gguf", Some(5 << 30), 4, true);
        assert_eq!(l.context_size, FIT_MIN_CONTEXT);
        assert!(u64::from(l.gpu_layers) <= total);
        assert_eq!(l.parallel_slots, None);
    }

    #[test]
    fn a_start_on_a_16gb_card_is_unchanged() {
        for file in [
            "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
            "qwen2.5-coder-14b-instruct-q4_k_m.gguf",
        ] {
            let (l, total) = start_load(file, None, 16, false);
            let (_, meta) = shape_of(file);
            assert_eq!(
                l.context_size,
                crate::prepare::default_chat_context(meta.context_length),
                "{file}"
            );
            assert!(u64::from(l.gpu_layers) > total, "{file}");
            assert_eq!(l.parallel_slots, None, "{file}");
        }
    }

    #[test]
    fn a_start_takes_the_rule_only_for_automatic_layers_and_chat_servers() {
        let auto = crate::process::LaunchConfig {
            gpu_layers_auto: true,
            ..Default::default()
        };
        assert!(applies_to_start(&auto, true));
        assert!(
            !applies_to_start(&auto, false),
            "embedding / rerank / own -c"
        );
        let set = crate::process::LaunchConfig {
            gpu_layers_auto: false,
            ..Default::default()
        };
        assert!(!applies_to_start(&set, true), "the user's -ngl stays");
        let router = crate::process::LaunchConfig {
            router_mode: true,
            ..auto.clone()
        };
        assert!(!applies_to_start(&router, true));
        let mut fit_on = auto;
        fit_on
            .params
            .insert("fit".into(), crate::params::ParamValue::Text("on".into()));
        assert!(!applies_to_start(&fit_on, true), "llama-server's own -fit");
    }

    #[test]
    fn a_start_keeps_user_cache_types_and_slot_counts() {
        let (file_size, meta) = shape_of("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
        let budget = crate::vram::MemoryBudget {
            vram_bytes: 4 << 30,
            ram_bytes: 32 << 30,
            unified: false,
            shared_igpu: false,
            igpu_offload_faster: None,
            cuda: true,
            rocm: false,
        };
        let fit = |c: &crate::process::LaunchConfig| {
            let mut base = crate::vram::base_input_from_launch(c);
            base.model_size_bytes = file_size;
            crate::vram::fit_launch_with(
                c,
                &base,
                Some(&meta),
                &budget,
                &[],
                false,
                crate::hardware::RecommendedBackend::Cuda12,
            )
        };
        let mut own = crate::process::LaunchConfig {
            context_size: 32_768,
            gpu_layers: 999,
            gpu_layers_auto: true,
            cache_type_k: Some("q8_0".into()),
            cache_type_v: Some("q8_0".into()),
            ..Default::default()
        };
        assert_eq!(apply_to_launch_with(&mut own, 32_768, fit, None), None);
        assert_eq!(own.context_size, 32_768);
        own.cache_type_k = None;
        own.cache_type_v = None;
        own.parallel_slots = Some(4);
        if apply_to_launch_with(&mut own, 32_768, fit, None).is_some() {
            assert_eq!(
                own.parallel_slots,
                Some(4),
                "the user's slots are never changed"
            );
        }
    }

    /// The default load on an Apple Silicon Mac of `ram_gib` with the usual Metal working set
    /// (2/3 of RAM), for a Qwen3-0.6B Q8_0 shaped model (28 layers, 8 KV heads of 128).
    fn unified_load(ram_gib: u64, need_limit: Option<u64>) -> FullOffload {
        let (_, mut meta) = shape_of("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
        meta.block_count = Some(28);
        meta.embedding_length = Some(1024);
        meta.head_count = Some(16);
        meta.feed_forward_length = Some(3072);
        let budget = crate::vram::MemoryBudget {
            vram_bytes: crate::hardware::macos_gpu_budget(ram_gib << 30, None),
            ram_bytes: ram_gib << 30,
            unified: true,
            shared_igpu: false,
            igpu_offload_faster: None,
            cuda: false,
            rocm: false,
        };
        let fit = |c: &crate::process::LaunchConfig| {
            let mut base = crate::vram::base_input_from_launch(c);
            base.model_size_bytes = 640 << 20;
            crate::vram::fit_launch_with(
                c,
                &base,
                Some(&meta),
                &budget,
                &[],
                true,
                crate::hardware::RecommendedBackend::Metal,
            )
        };
        let config = crate::process::LaunchConfig {
            context_size: 32_768,
            gpu_layers: 999,
            launched_by: crate::slots::SlotOrigin::Gateway,
            ..Default::default()
        };
        full_offload_context_with(&config, 32_768, fit, need_limit).unwrap()
    }

    #[test]
    fn an_8gb_unified_memory_mac_does_not_reserve_32k_of_cache_for_a_tiny_model() {
        // Live (MacBook Air M1 8 GB): Qwen3-0.6B Q8_0 started at -c 32768 with f16 KV; the
        // server took 3.7 GB and the machine sat at 89% memory.
        let f = unified_load(8, None);
        assert!(f.whole, "{f:?}");
        assert!(f.context < 32_768, "{f:?}");
        let kv_per_token = 28u64 * 2 * 8 * 128 * 2;
        assert!(
            u64::from(f.context) * kv_per_token <= unified_kv_cap(8 << 30).unwrap() * 2,
            "{f:?}"
        );
        // The same model on a 64 GB Mac keeps the full default context.
        assert_eq!(unified_load(64, None).context, 32_768);
    }

    /// The default load of Qwen3-4B-Instruct-2507 Q4_K_M (36 layers + output, 8 KV heads of
    /// 128) on an Apple Silicon Mac of `ram_gib` (Metal working set 2/3 of RAM), with the
    /// launch's `custom_args`. Also returns the fit of the chosen launch.
    fn unified_4b_load(
        ram_gib: u64,
        custom_args: Option<&str>,
    ) -> (FullOffload, crate::vram::FitResult) {
        let (size, meta) = shape_of("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
        let budget = crate::vram::MemoryBudget {
            vram_bytes: crate::hardware::macos_gpu_budget(ram_gib << 30, None),
            ram_bytes: ram_gib << 30,
            unified: true,
            shared_igpu: false,
            igpu_offload_faster: None,
            cuda: false,
            rocm: false,
        };
        let fit = |c: &crate::process::LaunchConfig| {
            let mut base = crate::vram::base_input_from_launch(c);
            base.model_size_bytes = size;
            crate::vram::fit_launch_with(
                c,
                &base,
                Some(&meta),
                &budget,
                &[],
                true,
                crate::hardware::RecommendedBackend::Metal,
            )
        };
        // `ProcessManager::start`: the context resolved to the chat default, then the rule.
        let mut launch = crate::process::LaunchConfig {
            context_size: crate::prepare::default_chat_context(meta.context_length),
            gpu_layers: 999,
            gpu_layers_auto: true,
            custom_args: custom_args.map(String::from),
            ..Default::default()
        };
        let requested = launch.context_size;
        let f = apply_to_launch_with(&mut launch, requested, fit, None).unwrap();
        (f, fit(&launch))
    }

    /// KV bytes per token of Qwen3-4B with f16: 36 layers x 8 KV heads x 128 x (K, V) x 2 bytes.
    const QWEN3_4B_KV_F16_PER_TOKEN: u64 = 36 * 8 * 128 * 2 * 2;

    #[test]
    fn an_8gb_mac_takes_q8_0_at_the_same_context_before_stepping_down() {
        // Live (MacBook Air M1 8 GB): f16 fit Metal's working set but not the 1 GiB cache cap at
        // any step above the floor, so Qwen3-4B started at -c 8192 -np 1 with f16 KV (1.1 GiB
        // of cache). q8_0 / q8_0 at a step keeps under the cap: a bigger context in less memory.
        // 12k (918 MiB of q8_0 cache); 16k with q8_0 is 1224 MiB, still above the cap.
        let (f, r) = unified_4b_load(8, None);
        let cap = unified_kv_cap(8 << 30).unwrap();
        assert!(f.whole, "{f:?}");
        assert_eq!((f.context, f.layers, f.slots), (12_288, 37, None), "{f:?}");
        assert!(
            16_384 * QWEN3_4B_KV_F16_PER_TOKEN / 32 * 17 > cap,
            "q8_0 at 16k"
        );
        assert_eq!(
            (f.cache_type_k, f.cache_type_v),
            (Some("q8_0"), Some("q8_0"))
        );
        // f16 at this context would break the cap; q8_0 does not.
        assert!(
            u64::from(f.context) * QWEN3_4B_KV_F16_PER_TOKEN > cap,
            "{f:?}"
        );
        let kv = r.kv.as_ref().unwrap();
        assert_eq!(
            kv.level,
            crate::kv_policy::KvLevel::Balanced,
            "the launch's own choice"
        );
        assert!(
            r.estimate.kv_bytes <= cap,
            "{} > {cap}",
            r.estimate.kv_bytes
        );
        assert!(
            r.estimate.kv_bytes < 8192 * QWEN3_4B_KV_F16_PER_TOKEN,
            "less than f16 at 8k"
        );
        assert!(unified_fits(&r, f.context));
        // The launch's KV policy picks the same types and turns flash attention on.
        let (_, meta) = shape_of("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
        let mut launch = crate::process::LaunchConfig {
            context_size: f.context,
            gpu_layers: f.layers,
            parallel_slots: f.slots,
            ..Default::default()
        };
        let applied = crate::kv_policy::apply(&mut launch, Some(&meta), &r.budget, &[], true);
        assert_eq!(launch.cache_type_k.as_deref(), Some("q8_0"));
        assert_eq!(launch.cache_type_v.as_deref(), Some("q8_0"));
        assert!(launch.flash_attn && applied.flash_attn_forced);
    }

    #[test]
    fn without_flash_attention_an_8gb_mac_keeps_f16_and_steps_down() {
        // A quantized V needs flash attention and q8_0 / f16 is slow on Metal: the context steps
        // down to the floor, as before.
        let (f, r) = unified_4b_load(8, Some("-fa off"));
        assert_eq!(f.context, FULL_OFFLOAD_MIN_CONTEXT, "{f:?}");
        assert_eq!((f.cache_type_k, f.cache_type_v), (Some("f16"), Some("f16")));
        assert_eq!(r.kv.unwrap().level, crate::kv_policy::KvLevel::Full);
    }

    #[test]
    fn f16_stays_where_it_fits_the_unified_limits() {
        // A 64 GB Mac: no cap, the full default context with f16.
        let (g, _) = unified_4b_load(64, None);
        assert_eq!(g.context, 32_768, "{g:?}");
        assert_eq!((g.cache_type_k, g.cache_type_v), (Some("f16"), Some("f16")));
        // A 16 GB Mac (2 GiB cap): f16 at 24k (3.4 GiB) breaks it, q8_0 at 24k does not.
        let (f, r16) = unified_4b_load(16, None);
        assert_eq!(f.context, 24_576, "{f:?}");
        assert_eq!(f.cache_type_k, Some("q8_0"), "{f:?}");
        // The automatic cache at a context whose f16 cache fits: f16.
        let (_, r8) = unified_4b_load(8, None);
        let (size, meta) = shape_of("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
        let auto_kv = |context_size: u32, budget: &crate::vram::MemoryBudget| {
            let mut input = crate::vram::input_from_launch(&crate::process::LaunchConfig {
                context_size,
                gpu_layers: 999,
                ..Default::default()
            });
            input.model_size_bytes = size;
            assert!(input.kv_auto);
            let r = crate::vram::fit_with(&input, Some(&meta), budget, &[]);
            r.kv.unwrap().level
        };
        use crate::kv_policy::KvLevel;
        // 16 GB at 12k: 1.7 GiB of f16, under the 2 GiB cap.
        assert_eq!(auto_kv(12_288, &r16.budget), KvLevel::Full);
        // 8 GB at 4k and at the 8k floor (exempt from the cap).
        assert_eq!(auto_kv(4096, &r8.budget), KvLevel::Full);
        assert_eq!(auto_kv(8192, &r8.budget), KvLevel::Full);
        assert_eq!(auto_kv(12_288, &r8.budget), KvLevel::Balanced);
    }

    #[test]
    fn a_user_context_above_the_unified_limits_is_shrunk_to_fit_not_reported_as_too_tight() {
        // Live (MacBook Air M1 8 GB, v0.1.0): a saved Qwen3-4B definition at 16k showed "fits in
        // memory" next to "this context does not fit even at the tightest setting". q8_0 at 16k
        // breaks the 1 GiB cache cap but fits Metal's working set: the cache is shrunk to fit.
        let (_, r8) = unified_4b_load(8, None);
        let (size, meta) = shape_of("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
        let mut input = crate::vram::input_from_launch(&crate::process::LaunchConfig {
            context_size: 16_384,
            gpu_layers: 999,
            ..Default::default()
        });
        input.model_size_bytes = size;
        let r = crate::vram::fit_with(&input, Some(&meta), &r8.budget, &[]);
        let kv = r.kv.as_ref().unwrap();
        assert!(!unified_fits(&r, 16_384), "above the unified limits");
        assert_eq!(kv.level, crate::kv_policy::KvLevel::Balanced);
        assert_eq!(kv.reason, crate::kv_policy::KvReason::ToFit, "{kv:?}");
    }

    #[test]
    fn a_discrete_card_keeps_its_cache_choice() {
        // Same model on a 4 GB CUDA card: the unified limits do not apply; 12k with the q8_0
        // the card's own memory needs, as before.
        let (f, r) = default_load("Qwen3-4B-Instruct-2507-Q4_K_M.gguf", None, 4, true);
        assert_eq!(f.context, 12_288, "{f:?}");
        assert_eq!(
            (f.cache_type_k, f.cache_type_v),
            (Some("q8_0"), Some("q8_0"))
        );
        assert_eq!(r, 36, "layers");
        // The unified limits hold only on Apple unified memory.
        let mut fit = crate::vram::fit_with(
            &crate::vram::FitInput::default(),
            None,
            &crate::vram::MemoryBudget {
                vram_bytes: 4 << 30,
                ram_bytes: 8 << 30,
                unified: false,
                shared_igpu: false,
                igpu_offload_faster: None,
                cuda: true,
                rocm: false,
            },
            &[],
        );
        fit.estimate.kv_bytes = 4 << 30;
        assert!(unified_fits(&fit, 32_768));
        fit.budget.unified = true;
        fit.budget.shared_igpu = true;
        assert!(unified_fits(&fit, 32_768), "an iGPU keeps its own rule");
        fit.budget.shared_igpu = false;
        assert!(!unified_fits(&fit, 32_768));
        // A 16 GB AMD card: 32k with f16.
        let (g, _) = default_load("Qwen3-4B-Instruct-2507-Q4_K_M.gguf", None, 16, false);
        assert_eq!(g.context, 32_768, "{g:?}");
        assert_eq!(g.cache_type_k, Some("f16"));
    }

    #[test]
    fn free_memory_of_an_8gb_mac_limits_the_default_load_further() {
        // Only 1.5 GiB free (+ margin): even 8k does not fit, the rule falls back instead of
        // starting a load the machine cannot hold.
        let tight = unified_load(8, Some(900 << 20));
        assert!(!tight.whole, "{tight:?}");
        let roomy = unified_load(8, Some(4 << 30));
        assert!(roomy.whole);
    }

    #[test]
    fn the_unified_reserve_is_2_5_gib_or_a_share_of_ram() {
        let gib = 1u64 << 30;
        assert_eq!(unified_os_reserve(8 * gib), 5 * gib / 2);
        assert_eq!(unified_os_reserve(16 * gib), 16 * gib / 10 * 3);
        assert_eq!(unified_os_reserve(64 * gib), 64 * gib / 20 * 3);
        assert_eq!(unified_kv_cap(8 * gib), Some(gib));
        assert_eq!(unified_kv_cap(32 * gib), None);
    }

    #[test]
    fn the_os_limit_on_apple_silicon_is_the_free_memory_minus_a_margin() {
        let mut hw = crate::hardware::detect_hardware();
        hw.unified_memory_budget_bytes = Some(5 << 30);
        hw.total_ram_bytes = 8 << 30;
        hw.available_ram_bytes = 3 << 30;
        assert_eq!(
            os_need_limit(&hw, 0),
            Some((3 << 30) - UNIFIED_AVAILABLE_MARGIN)
        );
    }
}
