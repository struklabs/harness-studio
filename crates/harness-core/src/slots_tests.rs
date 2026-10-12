use super::*;
use crate::vram::{estimate_raw, fit_with, parse_load_log};
use std::path::Path;

const MIB: u64 = 1024 * 1024;

fn data(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name),
    )
    .unwrap()
}

/// Real GGUF headers (`tests/data/gguf-vram-shapes.json`, see `vram_tests::dump_real_shapes`).
fn shape_of(file: &str) -> (u64, GgufMetadata) {
    let v: serde_json::Value = serde_json::from_str(&data("gguf-vram-shapes.json")).unwrap();
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

/// The measurement machine: RX 7900 GRE (16368 MiB, Vulkan) and 31831 MiB of RAM.
fn budget() -> MemoryBudget {
    MemoryBudget {
        vram_bytes: 16368 * MIB,
        ram_bytes: 31831 * MIB,
        unified: false,
        shared_igpu: false,
        igpu_offload_faster: None,
        cuda: false,
        rocm: false,
    }
}

fn input(file: &str, ctx: u32, origin: SlotOrigin) -> (FitInput, GgufMetadata) {
    let (size, meta) = shape_of(file);
    (
        FitInput {
            model_size_bytes: size,
            context_size: ctx,
            gpu_layers: 999,
            flash_attn: Some(true),
            origin,
            ..Default::default()
        },
        meta,
    )
}

fn decide_for(file: &str, ctx: u32, origin: SlotOrigin) -> Decision {
    let (i, meta) = input(file, ctx, origin);
    decide(&i, Some(&meta), &budget(), &[], origin)
}

const QWEN3_4B: &str = "Qwen3-4B-Instruct-2507-Q4_K_M.gguf";
const GPT_OSS: &str = "gpt-oss-20b-MXFP4.gguf";
const GEMMA4_12B: &str = "gemma-4-12B-it-Q4_K_M.gguf";
const QWEN36_27B: &str = "Qwen3.6-27B-Q4_K_M.gguf";

/// The load logs of b11312 (Vulkan, RX 7900 GRE, `-c 32768 -fa on`): Gemma 4 12B with
/// llama-server's automatic 4 slots and with `-np 1` (docs/en/PERFORMANCE.md).
#[test]
fn gemma4_swa_cache_matches_real_loads() {
    let (size, meta) = shape_of(GEMMA4_12B);
    for (log, slots) in [
        ("b11312-vulkan-gemma4-12b-c32k-np4.load-log.txt", 4u32),
        ("b11312-vulkan-gemma4-12b-c32k-np1.load-log.txt", 1),
    ] {
        let lines: Vec<String> = data(log).lines().map(str::to_string).collect();
        let parsed = parse_load_log(&lines);
        let est = estimate_raw(
            &with_slots(
                &FitInput {
                    model_size_bytes: size,
                    context_size: 32768,
                    gpu_layers: 999,
                    ..Default::default()
                },
                slots,
            ),
            Some(&meta),
            true,
        );
        // Two caches: full attention (32768 cells) and the sliding window.
        let swa_cells = parsed.kv_caches.iter().map(|c| c.0).min().unwrap();
        assert_eq!(est.swa_cells, swa_cells, "{log}");
        let diff = est.kv_gpu_bytes.abs_diff(parsed.kv_gpu());
        assert!(
            diff < 2 * MIB,
            "{log}: {} vs {}",
            est.kv_gpu_bytes,
            parsed.kv_gpu()
        );
    }
}

#[test]
fn per_slot_cost_follows_the_architecture() {
    // Full attention only: one unified pool, an extra slot is free.
    assert!(decide_for(QWEN3_4B, 32768, SlotOrigin::User).per_slot_bytes <= CHEAP_SLOT_BYTES);
    // gpt-oss: a 128-token window, a few MiB per slot.
    assert!(decide_for(GPT_OSS, 32768, SlotOrigin::User).per_slot_bytes <= CHEAP_SLOT_BYTES);
    // Gemma 4: 40 window layers x 1024 cells x 8 KiB = 320 MiB of VRAM per slot, plus up to 4
    // checkpoints of the window in RAM.
    let gemma = decide_for(GEMMA4_12B, 32768, SlotOrigin::User);
    assert!(
        gemma.per_slot_bytes >= 320 * MIB,
        "{}",
        gemma.per_slot_bytes
    );
    // Qwen3.6: 48 gated-delta-net layers x ~3.1 MiB of state per slot.
    let qwen36 = decide_for(QWEN36_27B, 32768, SlotOrigin::User);
    assert!(
        qwen36.per_slot_bytes >= 140 * MIB,
        "{}",
        qwen36.per_slot_bytes
    );
}

#[test]
fn cheap_slots_are_limited_by_the_shared_context() {
    for (ctx, origin, slots, reason) in [
        (32768, SlotOrigin::User, 4, "slots.reason.shared_pool"),
        (16384, SlotOrigin::User, 2, "slots.reason.small_context"),
        (8192, SlotOrigin::User, 1, "slots.reason.small_context"),
        (32768, SlotOrigin::Agents, 1, "slots.reason.small_context"),
        // The gateway holds requests against the shared pool: four streams at the default 32k.
        (32768, SlotOrigin::Gateway, 4, "slots.reason.shared_pool"),
        (16384, SlotOrigin::Gateway, 2, "slots.reason.small_context"),
        (65536, SlotOrigin::Agents, 2, "slots.reason.small_context"),
        (131072, SlotOrigin::Agents, 4, "slots.reason.shared_pool"),
    ] {
        let d = decide_for(QWEN3_4B, ctx, origin);
        assert_eq!((d.slots, d.reason), (slots, reason), "c={ctx} {origin:?}");
        assert_eq!(d.ctx_per_slot, ctx as u64 / slots as u64);
    }
    assert_eq!(decide_for(GPT_OSS, 32768, SlotOrigin::User).slots, 4);
}

#[test]
fn costly_slots_one_for_chat_two_for_agents_while_it_fits() {
    let chat = decide_for(GEMMA4_12B, 32768, SlotOrigin::User);
    assert_eq!((chat.slots, chat.reason), (1, "slots.reason.single_user"));
    let agents = decide_for(GEMMA4_12B, 65536, SlotOrigin::Agents);
    assert_eq!(
        (agents.slots, agents.reason),
        (2, "slots.reason.agents_costly")
    );
    // The gateway's loads get the same two at the default 32k: its admission holds the requests.
    let gateway = decide_for(GEMMA4_12B, 32768, SlotOrigin::Gateway);
    assert_eq!(
        (gateway.slots, gateway.reason),
        (2, "slots.reason.agents_costly")
    );
    // Qwen3.6-27B Q4_K_M (16 GB) does not fit the 16 GB card fully: every slot counts.
    let tight = decide_for(QWEN36_27B, 65536, SlotOrigin::Agents);
    assert_eq!(
        (tight.slots, tight.reason),
        (1, "slots.reason.memory_tight")
    );
}

#[test]
fn advice_keeps_a_set_value_and_still_recommends() {
    let (mut i, meta) = input(GEMMA4_12B, 32768, SlotOrigin::User);
    let auto = advise(&i, Some(&meta), &budget(), &[]);
    assert!(auto.auto);
    assert_eq!((auto.slots, auto.recommended), (1, 1));
    assert_eq!(auto.recommended_agents, 1); // 32k cannot hold two agent requests
    i.parallel_slots = Some(4);
    let set = advise(&i, Some(&meta), &budget(), &[]);
    assert!(!set.auto);
    assert_eq!((set.slots, set.recommended), (4, 1));
    // The estimate follows the slots: 3 more windows of 320 MiB.
    let one = fit_with(&with_slots(&i, 1), Some(&meta), &budget(), &[]);
    let four = fit_with(&with_slots(&i, 4), Some(&meta), &budget(), &[]);
    let extra = four.estimate.vram_bytes - one.estimate.vram_bytes;
    assert!((900 * MIB..1000 * MIB).contains(&extra), "{}", extra / MIB);
    assert_eq!(four.estimate.slots, 4);
    assert!(four.estimate.checkpoint_bytes > one.estimate.checkpoint_bytes * 3);
}

#[test]
fn custom_arguments_set_the_slots() {
    assert_eq!(custom_slots(Some("--verbose -np 2")), Some(2));
    assert_eq!(custom_slots(Some("--parallel=8 --no-warmup")), Some(8));
    assert_eq!(custom_slots(Some("-np -1")), Some(AUTO_MAX));
    assert_eq!(custom_slots(Some("--alias np")), None);
    assert_eq!(custom_slots(None), None);
    assert!(custom_sets_kv_unified(Some("-np 2 -no-kvu")));
    assert!(!custom_sets_kv_unified(Some("-np 2")));
}

#[test]
fn origin_is_not_serialized_for_user_launches() {
    let c = crate::process::LaunchConfig::default();
    let v = serde_json::to_value(&c).unwrap();
    assert!(v.get("launched_by").is_none());
    let agents = crate::process::LaunchConfig {
        launched_by: SlotOrigin::Agents,
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(&agents).unwrap()["launched_by"],
        "agents"
    );
}

#[test]
fn custom_np_replaces_the_typed_value() {
    use crate::process::{ProcessManager, ServerCaps};
    let mut c = crate::process::LaunchConfig {
        host: "127.0.0.1".into(),
        port: 18990,
        context_size: 4096,
        parallel_slots: Some(2),
        ..Default::default()
    };
    let args = ProcessManager::build_args(&c, &ServerCaps::default()).unwrap();
    assert_eq!(args.iter().filter(|a| *a == "-np").count(), 1);
    c.custom_args = Some("-np 3".into());
    let args = ProcessManager::build_args(&c, &ServerCaps::default()).unwrap();
    let at = args.iter().position(|a| a == "-np").unwrap();
    assert_eq!(args[at + 1], "3");
    assert_eq!(args.iter().filter(|a| *a == "-np").count(), 1);
}

#[test]
fn slots_are_decided_before_the_kv_precision() {
    // Automatic cache types: the slot rule judges at f16, the KV policy then picks the types
    // for the chosen slots (the estimate carries both).
    let (mut i, meta) = input(GEMMA4_12B, 32768, SlotOrigin::User);
    i.kv_auto = true;
    let advice = advise(&i, Some(&meta), &budget(), &[]);
    let at_f16 = advise(
        &FitInput {
            kv_auto: false,
            ..i.clone()
        },
        Some(&meta),
        &budget(),
        &[],
    );
    assert_eq!(advice, at_f16);
    let r = fit_with(&with_slots(&i, advice.slots), Some(&meta), &budget(), &[]);
    assert!(r.kv.is_some());
    assert_eq!(r.estimate.slots, u64::from(advice.slots));
}

#[test]
fn log_line_is_plain_language_not_a_key() {
    let mut a = SlotAdvice {
        slots: 1,
        auto: false,
        recommended: 4,
        recommended_agents: 4,
        reason: "slots.reason.shared_pool".into(),
        per_slot_bytes: 0,
        ctx_per_slot: 8192,
    };
    let tr = a.log_line("tr");
    assert!(!tr.contains("slots.reason"), "{tr}");
    assert!(tr.contains("elle seçildi; önerilen 4"), "{tr}");
    a.auto = true;
    a.slots = 4;
    let en = a.log_line("en");
    assert!(
        en.contains("Concurrent requests: 4 (automatic; extra requests share"),
        "{en}"
    );
    for key in [
        "slots.reason.small_context",
        "slots.reason.single_user",
        "slots.reason.agents_costly",
        "slots.reason.memory_tight",
        "slots.reason.runtime_default",
    ] {
        a.reason = key.into();
        for lang in ["tr", "en"] {
            assert!(
                !a.reason_text(lang).contains("slots.reason"),
                "{key} {lang}"
            );
        }
    }
}

#[test]
fn dashboard_fit_and_launch_plan_use_the_same_slots_on_discrete_and_unified_gpus() {
    use crate::hardware::RecommendedBackend;
    use crate::process::LaunchConfig;

    let (base, meta) = input(QWEN3_4B, 16384, SlotOrigin::User);
    let config = LaunchConfig {
        context_size: 16384,
        gpu_layers: 33,
        parallel_slots: None,
        ..Default::default()
    };
    let igpu = MemoryBudget {
        vram_bytes: 8 * 1024 * MIB,
        ram_bytes: 16 * 1024 * MIB,
        unified: true,
        shared_igpu: true,
        igpu_offload_faster: Some(true),
        cuda: false,
        rocm: false,
    };

    for (name, machine, backend) in [
        ("dGPU", budget(), RecommendedBackend::Vulkan),
        ("iGPU/unified", igpu, RecommendedBackend::Vulkan),
    ] {
        let launch = plan_for_launch_with(&config, &base, Some(&meta), &machine, &[], true)
            .expect("single-model launch");
        let form =
            crate::vram::fit_launch_with(&config, &base, Some(&meta), &machine, &[], true, backend);
        assert_eq!(
            form.slots.as_ref().map(|a| a.slots),
            Some(launch.advice.slots),
            "{name}"
        );
        assert_eq!(
            form.estimate.slots,
            u64::from(launch.advice.slots),
            "{name}"
        );
        assert_eq!(
            launch.advice.slots, 2,
            "real Qwen3 4B metadata at 16k: {name}"
        );
        assert_eq!(launch.kv_unified, Some(true), "{name}");
    }
}

/// The draft model takes memory from the slots' budget, but its KV cache does not grow per
/// slot: llama.cpp b11333 gives the one draft context the target's total context and slot
/// count (`common_speculative_init_result`: `cparams.n_ctx = llama_n_ctx(ctx_tgt)`, every slot
/// a sequence of it). The form and the launch take the same path (`plan_for_launch_with`).
#[test]
fn draft_counts_against_the_slots_but_not_per_slot() {
    // Gemma 4 12B, 64k, agents on the 16 GB card: 2 slots without a draft (each slot costs
    // 1.6 GB of sliding-window cache), 1 with a 1.5 GB draft.
    let card = budget();
    let (target, meta) = input(GEMMA4_12B, 65536, SlotOrigin::Agents);
    let drafted = FitInput {
        draft: Some(crate::vram::DraftFit {
            model_size_bytes: 1500 * MIB,
            gpu_layers: Some(99),
            ..Default::default()
        }),
        ..target.clone()
    };
    let without = decide(&target, Some(&meta), &card, &[], SlotOrigin::Agents);
    let with = decide(&drafted, Some(&meta), &card, &[], SlotOrigin::Agents);
    assert_eq!(
        (without.slots, without.reason),
        (2, "slots.reason.agents_costly")
    );
    assert_eq!((with.slots, with.reason), (1, "slots.reason.memory_tight"));
    assert_eq!(with.per_slot_bytes, without.per_slot_bytes);

    let config = crate::process::LaunchConfig {
        model_path: GEMMA4_12B.into(),
        context_size: 65536,
        draft_model_path: Some("draft.gguf".into()),
        draft_gpu_layers: Some(99),
        launched_by: SlotOrigin::Agents,
        ..Default::default()
    };
    let plan = plan_for_launch_with(&config, &drafted, Some(&meta), &card, &[], true).unwrap();
    assert_eq!(plan.advice.slots, with.slots);
    let fit = crate::vram::fit_launch_with(
        &config,
        &drafted,
        Some(&meta),
        &card,
        &[],
        true,
        crate::hardware::RecommendedBackend::Vulkan,
    );
    assert!(fit.estimate.draft_gpu_bytes > 1500 * MIB);
    assert_eq!(fit.slots.unwrap().slots, with.slots);
}

#[test]
fn unified_pool_detection_follows_what_llama_server_will_run() {
    use crate::params::ParamValue;
    let plan = |slots: u32, np: Option<u32>, unified: Option<bool>| SlotPlan {
        advice: SlotAdvice {
            slots,
            auto: true,
            recommended: slots,
            recommended_agents: slots,
            reason: String::new(),
            per_slot_bytes: 0,
            ctx_per_slot: 0,
        },
        np,
        kv_unified: unified,
    };
    let config = |custom: Option<&str>, kv: Option<bool>| {
        let mut c = crate::process::LaunchConfig {
            custom_args: custom.map(str::to_string),
            ..Default::default()
        };
        if let Some(on) = kv {
            c.params.insert("kv_unified".into(), ParamValue::Bool(on));
        }
        c
    };
    // One slot never shares a pool.
    assert!(!runs_unified_pool(
        &plan(1, Some(1), None),
        &config(None, None)
    ));
    // llama-server's own automatic 4 (no -np) is unified; so is Harness's `-np 2 --kv-unified`.
    assert!(runs_unified_pool(&plan(4, None, None), &config(None, None)));
    assert!(runs_unified_pool(
        &plan(2, Some(2), Some(true)),
        &config(None, Some(true))
    ));
    // The user switched the pool off, in the form or in the arguments.
    assert!(!runs_unified_pool(
        &plan(4, None, None),
        &config(None, Some(false))
    ));
    assert!(!runs_unified_pool(
        &plan(4, None, None),
        &config(Some("--no-kv-unified"), None)
    ));
    // A custom -np alone gives every slot its own cache (llama-server's rule) ...
    assert!(!runs_unified_pool(
        &plan(4, None, None),
        &config(Some("-np 4"), None)
    ));
    // ... unless the user asks for the pool too.
    assert!(runs_unified_pool(
        &plan(4, None, None),
        &config(Some("-np 4 --kv-unified"), None)
    ));
}
