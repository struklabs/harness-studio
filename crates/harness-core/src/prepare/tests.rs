use super::*;
use std::path::Path;

fn data(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name),
    )
    .unwrap()
}

/// GGUF header facts and file sizes of the three real models (`gguf-vram-shapes.json`).
fn model(file: &str) -> (FitInput, GgufMetadata) {
    let v: serde_json::Value = serde_json::from_str(&data("gguf-vram-shapes.json")).unwrap();
    let e = v
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["file"] == file)
        .unwrap();
    let meta: GgufMetadata = serde_json::from_value(e["meta"].clone()).unwrap();
    let input = FitInput {
        model_size_bytes: e["size_bytes"].as_u64().unwrap(),
        ..FitInput::default()
    };
    (input, meta)
}

/// The RX 7900 GRE of the real `--list-devices` output (Vulkan0, 16368 MiB), 32 GiB of RAM.
fn gre() -> MemoryBudget {
    let devices = crate::devices::parse(&data("llama-server-b11236.list-devices.txt"));
    let d = devices
        .iter()
        .find(|d| d.name.contains("7900 GRE"))
        .expect("RX 7900 GRE in the device list");
    MemoryBudget {
        vram_bytes: d.total_bytes.unwrap(),
        ram_bytes: 32 * 1024 * 1024 * 1024,
        unified: false,
        shared_igpu: false,
        igpu_offload_faster: None,
        cuda: false,
        rocm: false,
    }
}

const QWEN3: &str = "Qwen3-4B-Instruct-2507-Q4_K_M.gguf";
const OSS: &str = "gpt-oss-20b-MXFP4.gguf";
const CODER14: &str = "qwen2.5-coder-14b-instruct-q4_k_m.gguf";

#[test]
fn playground_context_uses_model_limit_and_memory_instead_of_fixed_4k() {
    let p = plan_of(QWEN3, "playground", 0);
    let choice = chosen(&p);
    assert!(choice.context_size >= 16_384);
    assert!(u64::from(choice.context_size) <= p.model.trained);
    assert!(within_budget(choice, &gre()));
    let (input, mut meta) = model(QWEN3);
    meta.context_length = Some(8_192);
    let limited = plan(
        &input,
        Some(&meta),
        &gre(),
        &[],
        ToolNeed::for_tool("playground", 0),
    );
    assert!(chosen(&limited).context_size <= 8_192);
}

fn plan_of(file: &str, tool: &str, min: u32) -> PreparePlan {
    let (input, meta) = model(file);
    plan(
        &input,
        Some(&meta),
        &gre(),
        &[],
        ToolNeed::for_tool(tool, min),
    )
}

fn chosen(p: &PreparePlan) -> &PrepareOption {
    p.choice.as_ref().expect("a full-GPU choice")
}

fn within_budget(o: &PrepareOption, b: &MemoryBudget) -> bool {
    o.vram_bytes as f64 <= b.vram_bytes as f64 * VRAM_LIMIT
}

#[test]
fn model_limits_from_real_headers() {
    let (_, qwen3) = model(QWEN3);
    let l = model_limit(Some(&qwen3), 0);
    assert_eq!(
        (l.trained, l.max, l.extended_by_rope),
        (262_144, 262_144, false)
    );
    assert_eq!(l.rope, None);

    // gpt-oss declares yarn x32 over 4096 = 131072: the trained context already includes it.
    let (_, oss) = model(OSS);
    let l = model_limit(Some(&oss), 0);
    assert_eq!(
        (l.trained, l.max, l.extended_by_rope),
        (131_072, 131_072, false)
    );
    assert_eq!(l.rope.as_deref(), Some("yarn x32"));

    // No header: a conservative value, flagged unknown.
    let l = model_limit(None, 32_768);
    assert!(!l.known);
    assert_eq!(l.max, 32_768);
}

#[test]
fn rope_scaling_extends_only_when_the_file_declares_it() {
    // The 14B header with the scaling a 32k-trained Qwen2.5 GGUF carries (yarn x4 over 32768).
    let (input, mut meta) = model(CODER14);
    meta.context_length = Some(32_768);
    assert_eq!(model_limit(Some(&meta), 0).max, 32_768);
    meta.rope_scaling_type = Some("yarn".into());
    meta.rope_scaling_factor = Some(4.0);
    meta.rope_scaling_original_ctx = Some(32_768);
    let l = model_limit(Some(&meta), 0);
    assert_eq!((l.max, l.extended_by_rope), (131_072, true));
    // A plan on it may use the extension (memory permitting) ...
    let big = MemoryBudget {
        vram_bytes: 48 * 1024 * 1024 * 1024,
        ram_bytes: 64 * 1024 * 1024 * 1024,
        unified: false,
        shared_igpu: false,
        igpu_offload_faster: None,
        cuda: false,
        rocm: false,
    };
    let p = plan(
        &input,
        Some(&meta),
        &big,
        &[],
        ToolNeed::for_tool("opendesign", 0),
    );
    assert_eq!(chosen(&p).context_size, 131_072);
    // ... a linear/none type without a factor does not.
    meta.rope_scaling_factor = None;
    assert_eq!(model_limit(Some(&meta), 0).max, 32_768);
    let p = plan(
        &input,
        Some(&meta),
        &big,
        &[],
        ToolNeed::for_tool("opendesign", 0),
    );
    assert_eq!(p.verdict, Verdict::BelowRecommended);
    assert_eq!(p.limit, Limit::Model);
    assert_eq!(chosen(&p).context_size, 32_768);
}

#[test]
fn qwen3_4b_gets_128k_for_open_design_and_never_exceeds_vram() {
    let b = gre();
    let p = plan_of(QWEN3, "opendesign", 0);
    assert_eq!(p.verdict, Verdict::Recommended);
    let c = chosen(&p);
    // 262k does not fit next to the weights; 131072 with a q8_0 KV cache does (f16 and
    // K-only q8_0 do not).
    assert_eq!((c.context_size, c.kv_level), (131_072, KvLevel::Balanced));
    assert_eq!(
        (c.cache_type_k.as_str(), c.cache_type_v.as_str()),
        ("q8_0", "q8_0")
    );
    assert_eq!(c.gpu_layers, None);
    assert!(within_budget(c, &b), "{c:?}");
    assert_eq!(p.limit, Limit::Memory);
    // f16 KV is offered at a smaller context.
    let f16 = p
        .alternatives
        .iter()
        .find(|a| a.kind == OptionKind::F16Kv)
        .unwrap();
    assert!(f16.context_size < c.context_size && within_budget(f16, &b));
    assert!(!p.suggest_smaller_model);
}

#[test]
fn qwen3_4b_agent_tools_stop_at_their_ceiling() {
    // OpenCode recommends 64k: it fits with the full-precision cache, which wins over a
    // larger context; 128k (its ceiling) with q8_0 is offered.
    let p = plan_of(QWEN3, "opencode", 0);
    assert_eq!(p.verdict, Verdict::Recommended);
    let c = chosen(&p);
    assert_eq!((c.context_size, c.kv_level), (65_536, KvLevel::Full));
    let larger = p
        .alternatives
        .iter()
        .find(|a| a.kind == OptionKind::LargerCtx)
        .expect("larger-context alternative");
    assert_eq!(
        (larger.context_size, larger.kv_level),
        (131_072, KvLevel::Balanced)
    );
    let p = plan_of(QWEN3, "aider", 0);
    assert_eq!(chosen(&p).context_size, 65_536);
    assert_eq!(p.limit, Limit::Ceiling);
    // 64k fits with the full-precision KV cache: nothing is quantized.
    assert_eq!(chosen(&p).kv_level, KvLevel::Full);
    assert!(p.alternatives.iter().all(|a| a.kind != OptionKind::F16Kv));
}

#[test]
fn gpt_oss_uses_its_sliding_window_and_is_limited_by_the_model() {
    let b = gre();
    let p = plan_of(OSS, "opendesign", 0);
    let c = chosen(&p);
    assert_eq!(p.verdict, Verdict::Recommended);
    // 128k is the model's own limit; the SWA layers keep the KV cache small enough to stay
    // f16 (gpt-oss is the most KV-sensitive model measured, see PERFORMANCE.md).
    assert_eq!((c.context_size, c.kv_level), (131_072, KvLevel::Full));
    assert_eq!(p.limit, Limit::Model);
    assert!(within_budget(c, &b));
}

#[test]
fn coder_14b_falls_back_to_q4_kv_or_a_smaller_context() {
    let b = gre();
    // Claude Code (recommended 64k): q8_0 does not reach it, K q8_0 + V q4_0 does.
    let p = plan_of(CODER14, "claude", 0);
    assert_eq!(p.verdict, Verdict::Recommended);
    let c = chosen(&p);
    assert_eq!(c.kv_level, KvLevel::Saver);
    assert_eq!(
        (c.cache_type_k.as_str(), c.cache_type_v.as_str()),
        ("q8_0", "q4_0")
    );
    assert!(c.context_size >= 65_536 && within_budget(c, &b), "{c:?}");
    // The q8_0 KV cache at a smaller context is offered.
    let q8 = p
        .alternatives
        .iter()
        .find(|a| a.kind == OptionKind::Q8Kv)
        .expect("q8_0 alternative");
    assert!(q8.context_size < c.context_size && within_budget(q8, &b));

    // Open Design (128k): the 16 GB card cannot hold it fully on the GPU. The choice is
    // smaller than recommended, the reason and a CPU-offload alternative are given.
    let p = plan_of(CODER14, "opendesign", 0);
    assert_eq!(p.verdict, Verdict::BelowRecommended);
    assert_eq!(p.limit, Limit::Memory);
    assert!(p.suggest_smaller_model);
    let c = chosen(&p);
    assert!(c.context_size < 131_072 && c.context_size >= p.need.min);
    assert!(within_budget(c, &b));
    let off = p
        .alternatives
        .iter()
        .find(|a| a.kind == OptionKind::Offload)
        .expect("offload alternative");
    assert_eq!(off.context_size, 131_072);
    assert!(off.gpu_layers.is_some_and(|l| l > 0));
    assert!(within_budget(off, &b));
    assert!(off.ram_bytes > c.ram_bytes);
}

#[test]
fn minimum_that_cannot_fit_is_reported_with_options() {
    // A 6 GiB card: the 14B model (8.4 GB of weights) cannot be fully offloaded at any context.
    let (input, meta) = model(CODER14);
    let small = MemoryBudget {
        vram_bytes: 6 * 1024 * 1024 * 1024,
        ram_bytes: 32 * 1024 * 1024 * 1024,
        unified: false,
        shared_igpu: false,
        igpu_offload_faster: None,
        cuda: false,
        rocm: false,
    };
    let p = plan(
        &input,
        Some(&meta),
        &small,
        &[],
        ToolNeed::for_tool("claude", 0),
    );
    assert_eq!(p.verdict, Verdict::BelowMinimum);
    assert!(p.choice.is_none());
    assert_eq!(p.limit, Limit::Memory);
    assert!(p.suggest_smaller_model);
    let off = p
        .alternatives
        .iter()
        .find(|a| a.kind == OptionKind::Offload)
        .expect("offload alternative");
    assert!(off.gpu_layers.is_some_and(|l| l > 0));
    assert!(within_budget(off, &small));
}

#[test]
fn cpu_only_machine_is_judged_against_ram() {
    let (input, meta) = model(QWEN3);
    let cpu = MemoryBudget {
        vram_bytes: 0,
        ram_bytes: 16 * 1024 * 1024 * 1024,
        unified: false,
        shared_igpu: false,
        igpu_offload_faster: None,
        cuda: false,
        rocm: false,
    };
    let p = plan(
        &input,
        Some(&meta),
        &cpu,
        &[],
        ToolNeed::for_tool("claude", 0),
    );
    assert!(p.cpu_only);
    let c = chosen(&p);
    assert_eq!(c.gpu_layers, Some(0));
    assert!(c.ram_bytes as f64 <= cpu.ram_bytes as f64 * RAM_LIMIT);
}

#[test]
fn older_intel_igpu_keeps_the_weights_on_the_cpu() {
    let (input, meta) = model(QWEN3);
    let ram = 16 * 1024 * 1024 * 1024u64;
    let igpu = |faster: Option<bool>| MemoryBudget {
        vram_bytes: ram / 2,
        ram_bytes: ram,
        unified: true,
        shared_igpu: true,
        igpu_offload_faster: faster,
        cuda: false,
        rocm: false,
    };
    let need = || ToolNeed::for_tool("opencode", 0);
    // HD 6000 class: `-ngl 0`, judged against RAM (weights and buffers share it).
    let p = plan(&input, Some(&meta), &igpu(Some(false)), &[], need());
    assert!(p.cpu_only && p.shared_igpu);
    let c = chosen(&p);
    assert_eq!(c.gpu_layers, Some(0));
    assert!((c.ram_bytes + c.vram_bytes) as f64 <= ram as f64 * RAM_LIMIT);
    // RAM is not filled beyond what the tool recommends.
    assert_eq!(c.context_size, p.need.recommended);
    // A newer iGPU (or one measured faster) still gets the weights offloaded.
    let p = plan(&input, Some(&meta), &igpu(None), &[], need());
    assert!(!p.cpu_only && p.shared_igpu);
    assert_eq!(chosen(&p).gpu_layers, None);
}

#[test]
fn tool_needs() {
    let od = ToolNeed::for_tool("opendesign", 32_768);
    assert_eq!((od.min, od.recommended), (32_768, 131_072));
    let cc = ToolNeed::for_tool("claude", 32_768);
    assert_eq!((cc.min, cc.recommended), (32_768, 65_536));
    let other = ToolNeed::for_tool("goose", 0);
    assert!(other.min <= other.recommended && other.recommended <= other.ceiling);
}

fn available_plan(file: &str, free_mib: u64) -> PreparePlan {
    let (input, meta) = model(file);
    plan_with_available_memory(
        &input,
        Some(&meta),
        &gre(),
        &[],
        ToolNeed::for_tool("opencode", 0),
        Some(AvailableMemory {
            total_vram_bytes: gre().vram_bytes,
            available_vram_bytes: free_mib * 1024 * 1024,
            headroom_bytes: 512 * 1024 * 1024,
        }),
    )
}

#[test]
fn available_vram_limits_qwen3_and_every_alternative() {
    let p = available_plan(QWEN3, 6875);
    assert_eq!(gre().vram_bytes, 16368 * 1024 * 1024);
    let c = chosen(&p);
    assert_ne!((c.context_size, c.kv_level), (65_536, KvLevel::Full));
    assert_eq!(p.limit, Limit::Available);
    let available = p.available_memory.unwrap();
    for o in p.choice.iter().chain(&p.alternatives) {
        assert!(available.fits(o.vram_bytes), "{o:?}");
        assert!(within_budget(o, &gre()));
    }
    let roomy = available_plan(QWEN3, 6875 + 6000);
    assert_eq!(chosen(&roomy).kv_level, KvLevel::Full);
    assert!(chosen(&roomy).vram_bytes > c.vram_bytes);
}

#[test]
fn zero_available_vram_is_not_cpu_only_and_unknown_keeps_capacity_behavior() {
    let p = available_plan(QWEN3, 0);
    assert!(!p.cpu_only);
    assert!(p.choice.is_none() && p.alternatives.is_empty());
    assert_eq!(p.limit, Limit::Available);
    let (input, meta) = model(QWEN3);
    let p = plan_with_available_memory(
        &input,
        Some(&meta),
        &gre(),
        &[],
        ToolNeed::for_tool("opencode", 0),
        None,
    );
    assert_eq!(p, plan_of(QWEN3, "opencode", 0));
}

#[test]
fn partial_offload_layers_follow_available_memory() {
    let small = available_plan(CODER14, 4000);
    let large = available_plan(CODER14, 6875);
    let offload = |p: &PreparePlan| {
        p.alternatives
            .iter()
            .find(|a| a.kind == OptionKind::Offload)
            .unwrap()
            .clone()
    };
    let s = offload(&small);
    let l = offload(&large);
    assert!(s.gpu_layers < l.gpu_layers);
    for p in [&small, &large] {
        for o in p.choice.iter().chain(&p.alternatives) {
            assert!(p.available_memory.unwrap().fits(o.vram_bytes), "{o:?}");
        }
    }
}

#[test]
fn only_other_managed_ready_idle_models_are_evictable() {
    let slot: crate::gateway::scheduler::ModelSlot = serde_json::from_value(serde_json::json!({
        "definition_id": "other", "alias": "other", "state": "ready", "server_id": "gw-other",
        "port": null, "managed": true, "pinned": false, "ttl_secs": null,
        "in_flight": 0, "queued": 0, "concurrency": 1, "last_used": null, "idle_secs": 10,
    }))
    .unwrap();
    assert!(evictable_model(&slot, "target"));
    assert!(!evictable_model(&slot, "other"));
    assert_eq!(
        available_after_eviction(6875, 16368, "target", [(&slot, Some(6000))]),
        12875
    );
    assert_eq!(
        available_after_eviction(6875, 16368, "target", [(&slot, None)]),
        6875
    );
    assert_eq!(
        available_after_eviction(6875, 16368, "other", [(&slot, Some(6000))]),
        6875
    );
    assert_eq!(
        available_after_eviction(6875, 16368, "target", [(&slot, Some(u64::MAX))]),
        16368
    );
    let mut s = slot.clone();
    s.pinned = true;
    assert!(!evictable_model(&s, "target"));
    assert_eq!(
        available_after_eviction(6875, 16368, "target", [(&s, Some(6000))]),
        6875
    );
    s = slot.clone();
    s.in_flight = 1;
    assert!(!evictable_model(&s, "target"));
    s = slot.clone();
    s.queued = 1;
    assert!(!evictable_model(&s, "target"));
    s = slot.clone();
    s.managed = false;
    assert!(!evictable_model(&s, "target"));
    s = slot.clone();
    s.state = crate::gateway::scheduler::SlotState::Loading;
    assert!(!evictable_model(&s, "target"));
    s = slot;
    s.server_id = None;
    assert!(!evictable_model(&s, "target"));
}

fn memory_report() -> crate::devices::DeviceReport {
    crate::devices::DeviceReport {
        devices: crate::devices::parse(
            "Available devices:\nVulkan0: Test GPU (100 MiB, 20 MiB free)",
        ),
        ..Default::default()
    }
}

fn memory_slot() -> crate::gateway::scheduler::ModelSlot {
    serde_json::from_value(serde_json::json!({
        "definition_id": "other", "alias": "other", "state": "ready", "server_id": "idle",
        "port": null, "managed": true, "pinned": false, "ttl_secs": null,
        "in_flight": 0, "queued": 0, "concurrency": 1, "last_used": null, "idle_secs": 10
    }))
    .unwrap()
}

fn footprint(server: &str, adapter: u32, bytes: u64) -> ServerMemory {
    ServerMemory {
        server_id: server.into(),
        adapter,
        bytes,
    }
}

#[test]
fn prepare_memory_reclaims_target_and_passes_headroom() {
    let report = memory_report();
    let free = report.devices[0].available_bytes.unwrap();
    let mut slot = memory_slot();
    slot.definition_id = "target".into();
    slot.server_id = Some("target-server".into());
    slot.pinned = true;
    slot.in_flight = 1;
    let result = available_memory(
        &report,
        (0, 0),
        123,
        "target",
        &[slot],
        &[footprint("target-server", 0, 1000)],
        &["target-server".into()],
    )
    .unwrap();
    assert_eq!(result.available_vram_bytes, free + 1000);
    // An explicit reserve is used as given.
    assert_eq!(result.headroom_bytes, 123);
    let auto = available_memory(&report, (0, 0), 0, "target", &[], &[], &[]).unwrap();
    assert_eq!(
        auto.headroom_bytes,
        crate::vram::memory_headroom(report.devices[0].total_bytes.unwrap(), 0)
    );
    assert!(auto.headroom_bytes >= crate::vram::MIN_VRAM_HEADROOM);
    let missing = available_memory(
        &report,
        (0, 0),
        123,
        "target",
        &[],
        &[],
        &["target-server".into()],
    )
    .unwrap();
    assert_eq!(missing.available_vram_bytes, free);
}

#[test]
fn prepare_memory_counts_the_dashboard_loaded_target_itself() {
    // NVIDIA 4 GB: the target loaded from the Dashboard (no gateway slot, not managed) is
    // reloaded by the preparation, so its own footprint is free; without it only 0.2 GB showed.
    let report = memory_report();
    let free = report.devices[0].available_bytes.unwrap();
    let measured = [footprint("default", 0, 3000)];
    let result = available_memory(
        &report,
        (0, 0),
        0,
        "target",
        &[],
        &measured,
        &["default".into()],
    )
    .unwrap();
    assert_eq!(
        result.available_vram_bytes,
        (free + 3000).min(result.total_vram_bytes)
    );
    // Not being reloaded: it stays used.
    let result = available_memory(&report, (0, 0), 0, "target", &[], &measured, &[]).unwrap();
    assert_eq!(result.available_vram_bytes, free);
}

#[test]
fn prepare_memory_ignores_other_adapters_and_non_evictable_models() {
    let report = memory_report();
    let free = report.devices[0].available_bytes.unwrap();
    let slot = memory_slot();
    let measured = [
        footprint("target-server", 1, 1000),
        footprint("idle", 0, 2000),
    ];
    let stopping = ["target-server".into()];
    for blocked in 0..5 {
        let mut slot = slot.clone();
        match blocked {
            0 => slot.pinned = true,
            1 => slot.managed = false,
            2 => slot.in_flight = 1,
            3 => slot.queued = 1,
            _ => slot.state = crate::gateway::scheduler::SlotState::Loading,
        }
        let result =
            available_memory(&report, (0, 0), 0, "target", &[slot], &measured, &stopping).unwrap();
        assert_eq!(result.available_vram_bytes, free);
    }
    let result =
        available_memory(&report, (0, 0), 0, "target", &[slot], &measured, &stopping).unwrap();
    assert_eq!(result.available_vram_bytes, free + 2000);
}

#[test]
fn prepare_memory_caps_total_and_counts_each_server_once() {
    let report = memory_report();
    let slot = memory_slot();
    let result = available_memory(
        &report,
        (0, 0),
        0,
        "target",
        &[slot.clone(), slot.clone()],
        &[footprint("idle", 0, 1000), footprint("idle", 0, 1000)],
        &["idle".into()],
    )
    .unwrap();
    assert_eq!(
        result.available_vram_bytes,
        report.devices[0].available_bytes.unwrap() + 1000
    );
    let result = available_memory(
        &report,
        (0, 0),
        0,
        "target",
        &[slot],
        &[footprint("idle", 0, u64::MAX)],
        &[],
    )
    .unwrap();
    assert_eq!(
        result.available_vram_bytes,
        report.devices[0].total_bytes.unwrap()
    );
}

#[test]
fn prepare_main_device_maps_the_configured_adapter() {
    let mut report = memory_report();
    let mut second = report.devices[0].clone();
    second.index = 1;
    second.name = "Other GPU".into();
    report.devices.push(second);
    let adapters = [(7, "Other GPU".into()), (3, "Test GPU".into())];
    assert_eq!(main_device(&report, &adapters, Some(7)), Some((1, 7)));
    assert_eq!(main_device(&report, &adapters, None), Some((0, 3)));
    report.error = Some("probe failed".into());
    assert!(available_memory(&report, (0, 3), 0, "target", &[], &[], &[]).is_none());
}

// ---------------------------------------------------------------------------------------
// Real llama-server

struct Server(std::process::Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn http_get(port: u16, path: &str) -> Option<String> {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok()?;
    write!(s, "GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n").ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    out.split_once("\r\n\r\n").map(|(_, b)| b.to_string())
}

/// Prepares Open Design on the real Qwen3-4B for this machine's GPU, starts the staged
/// llama-server on port 19700+ with the chosen values and checks that it loads and serves the
/// planned context. Stops the server it started. The GPU may be shared: a failed load is
/// retried (out-of-memory caused by another process is not this plan's fault).
/// `cargo test -p harness-core prepare_e2e -- --ignored --nocapture`
#[test]
#[ignore]
fn prepare_e2e_open_design_qwen3_4b() {
    use std::io::{BufRead, BufReader};
    let bin = std::env::var("HS_LLAMA_SERVER").unwrap_or_else(|_| {
        format!(
            r"{}\com.harnessstudio.app\bin\staging\llama-server.exe",
            std::env::var("APPDATA").expect("APPDATA")
        )
    });
    let model = std::env::var("HS_TEST_MODEL").unwrap_or_else(|_| {
        r"D:\Models\unsloth\Qwen3-4B-Instruct-2507-GGUF\Qwen3-4B-Instruct-2507-Q4_K_M.gguf".into()
    });
    // Budget: the real device list of this runtime (discrete GPU with the most memory).
    let devices = crate::devices::probe(Path::new(&bin)).expect("--list-devices");
    let gpu = devices
        .iter()
        .filter(|d| !d.integrated)
        .max_by_key(|d| d.total_bytes)
        .expect("a discrete GPU");
    println!(
        "device {} total {:?} free {:?} available {:?}",
        gpu.id, gpu.total_bytes, gpu.free_bytes, gpu.available_bytes
    );
    let budget = MemoryBudget {
        vram_bytes: gpu.total_bytes.unwrap(),
        ram_bytes: 32 * 1024 * 1024 * 1024,
        unified: false,
        shared_igpu: false,
        igpu_offload_faster: None,
        cuda: false,
        rocm: false,
    };
    let size = std::fs::metadata(&model).unwrap().len();
    let meta = crate::gguf::read_gguf_metadata(Path::new(&model)).unwrap();
    let base = FitInput {
        model_path: Some(model.clone()),
        model_size_bytes: size,
        ..FitInput::default()
    };
    let p = plan(
        &base,
        Some(&meta),
        &budget,
        &[],
        ToolNeed::for_tool("opendesign", 32_768),
    );
    let c = chosen(&p).clone();
    println!("plan: {c:?} ({:?}, limit {:?})", p.verdict, p.limit);
    assert_eq!(p.verdict, Verdict::Recommended);

    let port = (19_700u16..19_760)
        .find(|p| std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok())
        .expect("a free port in 19700..19760");
    let mut last = String::new();
    for attempt in 1..=3 {
        let mut child = std::process::Command::new(&bin)
            .args([
                "-m",
                &model,
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
            ])
            .args([
                "-c",
                &c.context_size.to_string(),
                "-ngl",
                "999",
                "-fa",
                "on",
            ])
            .args(["-ctk", &c.cache_type_k, "-ctv", &c.cache_type_v, "-np", "1"])
            .stderr(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let stderr = child.stderr.take().unwrap();
        let server = Server(child);
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let _ = tx.send(line);
            }
        });
        let mut log = Vec::new();
        let mut healthy = false;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
        while std::time::Instant::now() < deadline {
            while let Ok(l) = rx.try_recv() {
                log.push(l);
            }
            if http_get(port, "/health").is_some_and(|b| b.contains("\"ok\"")) {
                healthy = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        while let Ok(l) = rx.try_recv() {
            log.push(l);
        }
        if healthy {
            let props = http_get(port, "/props").expect("/props");
            let v: serde_json::Value = serde_json::from_str(&props).unwrap();
            let n_ctx = v["default_generation_settings"]["n_ctx"].as_u64().unwrap();
            let process = crate::gpu::process_vram_bytes(server.0.id());
            println!(
                "attempt {attempt}: loaded, n_ctx {n_ctx}, process VRAM {:?} MiB (estimate {:.0} MiB, budget {:.0} MiB)",
                process.map(|b| b / (1024 * 1024)),
                c.vram_bytes as f64 / 1048576.0,
                budget.vram_bytes as f64 / 1048576.0
            );
            assert_eq!(n_ctx, u64::from(c.context_size));
            if let Some(pr) = process {
                assert!(pr <= budget.vram_bytes, "process VRAM above the budget");
            }
            drop(server);
            return;
        }
        last = log
            .iter()
            .rev()
            .take(15)
            .rev()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        println!("attempt {attempt} failed:\n{last}");
        drop(server);
        std::thread::sleep(std::time::Duration::from_secs(15));
    }
    panic!("llama-server did not load with the prepared settings:\n{last}");
}
