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
    /// The whole model is on the GPU (a full offload) rather than the hybrid fallback at the
    /// floor context; only a full offload is admitted against [`crate::vram::full_offload_headroom`].
    #[serde(skip)]
    pub whole: bool,
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
    let (available, total) = crate::vram::os_available_vram(hw)?;
    Some(crate::vram::full_offload_need_limit(
        available,
        total,
        configured_mib * 1024 * 1024,
    ))
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
    let whole = |r: &crate::vram::FitResult| {
        u64::from(r.recommended_gpu_layers) > r.estimate.total_layers
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
            if whole(&r) {
                return Some(FullOffload {
                    context: ctx,
                    layers: r.recommended_gpu_layers,
                    // Only worth forcing when the automatic count did not fit.
                    slots,
                    whole: true,
                });
            }
        }
    }
    let floor = FIT_MIN_CONTEXT.min(context);
    let layers = at(floor, None).recommended_gpu_layers;
    (layers > 0).then_some(FullOffload {
        context: floor,
        layers,
        slots: None,
        whole: false,
    })
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

/// What the default load of `config` (context and `-ngl` automatic) starts with on this
/// machine: the same choice the launch makes, for forms that show it before Start. `None` when
/// the rule does not apply (set values, non-chat or router server, user cache types). Blocking.
pub fn automatic_choice(
    config: &crate::process::LaunchConfig,
    hw: &crate::hardware::SystemHardwareInfo,
) -> Option<FullOffload> {
    let context = default_context_for(config)?;
    full_offload_context(config, context, hw)
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
}
