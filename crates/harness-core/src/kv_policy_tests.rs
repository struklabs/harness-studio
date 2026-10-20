use super::*;
use crate::vram::FitInput;
use std::path::Path;

const GIB: u64 = 1024 * 1024 * 1024;

/// GGUF header facts of a real model (`tests/data/gguf-vram-shapes.json`) and its file size.
fn shape(file: &str) -> (u64, GgufMetadata) {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/gguf-vram-shapes.json"),
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

/// The RX 7900 GRE machine: 16 GiB VRAM, 32 GiB RAM.
fn budget() -> MemoryBudget {
    MemoryBudget {
        vram_bytes: 16 * GIB,
        ram_bytes: 32 * GIB,
        unified: false,
        shared_igpu: false,
        igpu_offload_faster: None,
        cuda: false,
        rocm: false,
    }
}

fn input(size: u64, ctx: u32) -> FitInput {
    FitInput {
        model_size_bytes: size,
        context_size: ctx,
        gpu_layers: 999,
        parallel_slots: Some(1),
        kv_auto: true,
        ..Default::default()
    }
}

fn level_at(file: &str, ctx: u32, v_quant_ok: bool) -> (KvLevel, KvReason) {
    let (size, meta) = shape(file);
    let (c, _) = choose(&input(size, ctx), Some(&meta), &budget(), &[], v_quant_ok);
    (c.level, c.reason)
}

const QWEN3: &str = "Qwen3-4B-Instruct-2507-Q4_K_M.gguf";

#[test]
fn level_names_map_to_types_and_back() {
    for l in AUTO_LADDER
        .into_iter()
        .chain(CUDA_LADDER)
        .chain(NO_FA_LADDER)
    {
        let (k, v) = l.types().unwrap();
        assert_eq!(KvLevel::of(Some(k), Some(v)), l);
    }
    assert_eq!(KvLevel::of(None, Some("")), KvLevel::Full);
    assert_eq!(KvLevel::of(Some("Q8_0"), Some(" q8_0 ")), KvLevel::Balanced);
    assert_eq!(KvLevel::of(Some("q4_0"), Some("q4_0")), KvLevel::Compact);
    assert_eq!(KvLevel::of(Some("q4_0"), Some("q8_0")), KvLevel::Custom);
    assert!(KvLevel::Custom.types().is_none());
    assert!(is_quantized(Some("q4_0")) && !is_quantized(Some("bf16")) && !is_quantized(None));
}

#[test]
fn auto_keeps_f16_while_it_fits_and_steps_down_one_rung_at_a_time() {
    // Qwen3-4B, one slot: f16 KV is 144 KiB per token.
    assert_eq!(
        level_at(QWEN3, 32_768, true),
        (KvLevel::Full, KvReason::Fits)
    );
    assert_eq!(
        level_at(QWEN3, 65_536, true),
        (KvLevel::Full, KvReason::Fits)
    );
    assert_eq!(
        level_at(QWEN3, 98_304, true),
        (KvLevel::Balanced, KvReason::ToFit)
    );
    assert_eq!(
        level_at(QWEN3, 131_072, true),
        (KvLevel::Balanced, KvReason::ToFit)
    );
    // 256k does not fit even with the most compact rung; the weights do.
    assert_eq!(
        level_at(QWEN3, 262_144, true),
        (KvLevel::Saver, KvReason::Tightest)
    );
}

#[test]
fn without_flash_attention_v_is_never_quantized() {
    assert_eq!(
        level_at(QWEN3, 98_304, false),
        (KvLevel::KeysOnly, KvReason::ToFit)
    );
    assert_eq!(
        level_at(QWEN3, 131_072, false),
        (KvLevel::KeysOnly, KvReason::Tightest)
    );
}

#[test]
fn weights_that_do_not_fit_keep_f16() {
    let (size, meta) = shape("gpt-oss-20b-MXFP4.gguf");
    let small = MemoryBudget {
        vram_bytes: 8 * GIB,
        ram_bytes: 8 * GIB,
        unified: false,
        shared_igpu: false,
        igpu_offload_faster: None,
        cuda: false,
        rocm: false,
    };
    let (c, _) = choose(&input(size, 131_072), Some(&meta), &small, &[], true);
    assert_eq!((c.level, c.reason), (KvLevel::Full, KvReason::TooBig));
}

#[test]
fn the_estimate_with_kv_auto_is_the_estimate_of_the_chosen_types() {
    let (size, meta) = shape(QWEN3);
    let auto = vram::fit_with(&input(size, 131_072), Some(&meta), &budget(), &[]);
    let kv = auto.kv.clone().unwrap();
    assert_eq!(
        (kv.cache_type_k.as_str(), kv.cache_type_v.as_str()),
        ("q8_0", "q8_0")
    );
    let explicit = vram::fit_with(
        &FitInput {
            cache_type_k: Some("q8_0".into()),
            cache_type_v: Some("q8_0".into()),
            kv_auto: false,
            ..input(size, 131_072)
        },
        Some(&meta),
        &budget(),
        &[],
    );
    assert_eq!(auto.estimate, explicit.estimate);
    assert!(explicit.kv.is_none());
    // q8_0 is 34 bytes per 32 elements: 53% of f16.
    assert_eq!(kv.kv_bytes * 64 / kv.kv_bytes_f16, 34);
}

#[test]
fn custom_argument_parsing() {
    assert!(custom_args_set_cache_type(Some("--verbose -ctk q8_0")));
    assert!(custom_args_set_cache_type(Some("--cache-type-v=q4_0")));
    assert!(!custom_args_set_cache_type(Some("-ctkd q8_0 --verbose")));
    assert!(!custom_args_set_cache_type(None));
    assert!(custom_args_disable_flash_attn(Some("-fa off")));
    assert!(custom_args_disable_flash_attn(Some("--flash-attn=0")));
    assert!(custom_args_disable_flash_attn(Some(
        "--verbose --flash-attn false"
    )));
    assert!(!custom_args_disable_flash_attn(Some("-fa on")));
    assert!(!custom_args_disable_flash_attn(Some("-fa auto")));
    assert!(!custom_args_disable_flash_attn(Some("-fa")));
}

fn launch(ctx: u32) -> (LaunchConfig, GgufMetadata) {
    let (_, meta) = shape(QWEN3);
    let c = LaunchConfig {
        model_path: "missing/Qwen3-4B-Instruct-2507-Q4_K_M.gguf".into(),
        gpu_layers: 999,
        context_size: ctx,
        parallel_slots: Some(1),
        ..Default::default()
    };
    (c, meta)
}

#[test]
fn apply_respects_explicit_types_and_custom_args() {
    // Explicit q8_0 V without flash attention set: forced on, types unchanged.
    let (mut c, meta) = launch(8192);
    c.cache_type_k = Some("q8_0".into());
    c.cache_type_v = Some("q8_0".into());
    let a = apply(&mut c, Some(&meta), &budget(), &[], true);
    assert!(a.choice.is_none() && a.flash_attn_forced && c.flash_attn);
    assert_eq!(c.cache_type_v.as_deref(), Some("q8_0"));

    // The user's own -fa off: V stays f16, K is kept.
    let (mut c, meta) = launch(8192);
    c.cache_type_k = Some("q8_0".into());
    c.cache_type_v = Some("q4_0".into());
    c.custom_args = Some("-fa off".into());
    let a = apply(&mut c, Some(&meta), &budget(), &[], true);
    assert!(a.v_kept_f16 && !a.flash_attn_forced);
    assert_eq!(
        (c.cache_type_k.as_deref(), c.cache_type_v.as_deref()),
        (Some("q8_0"), None)
    );

    // The user's own -ctk: nothing is chosen or changed.
    let (mut c, meta) = launch(262_144);
    c.custom_args = Some("-ctk q4_0 -ctv q4_0".into());
    let before = (c.cache_type_k.clone(), c.cache_type_v.clone(), c.flash_attn);
    assert_eq!(
        apply(&mut c, Some(&meta), &budget(), &[], true),
        Applied::default()
    );
    assert_eq!((c.cache_type_k, c.cache_type_v, c.flash_attn), before);

    // Router mode: no single model to estimate.
    let (mut c, meta) = launch(262_144);
    c.router_mode = true;
    assert_eq!(
        apply(&mut c, Some(&meta), &budget(), &[], true),
        Applied::default()
    );
}

#[test]
fn apply_auto_fills_types_only_when_needed() {
    let (mut c, meta) = launch(32_768);
    let a = apply(&mut c, Some(&meta), &budget(), &[], true);
    assert_eq!(a.choice.as_ref().map(|c| c.level), Some(KvLevel::Full));
    assert!(c.cache_type_k.is_none() && c.cache_type_v.is_none() && !c.flash_attn);
    assert!(a.notes("tr").is_empty());

    // The model file is missing in tests: give the estimate its size.
    let (mut c, meta) = launch(131_072);
    let (size, _) = shape(QWEN3);
    let input = FitInput {
        model_size_bytes: size,
        ..vram::input_from_launch(&c)
    };
    assert!(input.kv_auto);
    let (choice, _) = choose(&input, Some(&meta), &budget(), &[], true);
    assert_eq!(choice.level, KvLevel::Balanced);

    // With `-fa off` in the user's arguments the automatic choice never quantizes V.
    c.custom_args = Some("-fa off".into());
    let a = apply(&mut c, Some(&meta), &budget(), &[], true);
    assert!(!is_quantized(c.cache_type_v.as_deref()));
    assert!(!a.flash_attn_forced);
}

#[test]
fn notes_explain_the_downgrade_in_both_languages() {
    let a = Applied {
        choice: Some(KvChoice {
            level: KvLevel::Balanced,
            cache_type_k: "q8_0".into(),
            cache_type_v: "q8_0".into(),
            reason: KvReason::ToFit,
            kv_bytes_f16: 18 * GIB,
            kv_bytes: 18 * GIB * 34 / 64,
            prompt_slowdown_pct: None,
        }),
        flash_attn_forced: true,
        v_kept_f16: false,
    };
    let tr = a.notes("tr");
    assert_eq!(tr.len(), 2);
    assert!(tr[0].contains("-ctk q8_0 -ctv q8_0") && tr[0].contains("%47"));
    assert!(a.notes("en")[1].contains("--flash-attn on"));
}

// ---------------------------------------------------------------------------------------
// Real llama-server

/// Automatic KV precision end to end on the real Qwen3-4B: the policy picks the cache for a
/// 128k context on this machine's GPU, the managed llama-server is started with the arguments
/// Harness builds (port 19760+, `-lv 4` for the buffer sizes), and the load log must show the
/// chosen K/V types and a KV buffer within 3% of the estimate. Stops the server it started.
/// `cargo test -p harness-core kv_policy_e2e -- --ignored --nocapture`
#[test]
#[ignore]
fn kv_policy_e2e_qwen3_4b_128k() {
    use crate::process::{ProcessManager, ServerCaps};
    use std::io::{BufRead, BufReader};
    let bin = std::env::var("HS_LLAMA_SERVER").unwrap_or_else(|_| {
        format!(
            r"{}\com.harnessstudio.app\bin\managed\llama-server.exe",
            std::env::var("APPDATA").expect("APPDATA")
        )
    });
    let model = std::env::var("HS_TEST_MODEL").unwrap_or_else(|_| {
        r"D:\Models\unsloth\Qwen3-4B-Instruct-2507-GGUF\Qwen3-4B-Instruct-2507-Q4_K_M.gguf".into()
    });
    let devices = crate::devices::probe(Path::new(&bin)).expect("--list-devices");
    let gpu = devices
        .iter()
        .filter(|d| !d.integrated)
        .max_by_key(|d| d.total_bytes)
        .expect("a discrete GPU");
    let budget = MemoryBudget {
        vram_bytes: gpu.total_bytes.unwrap(),
        ram_bytes: 32 * GIB,
        unified: false,
        shared_igpu: false,
        igpu_offload_faster: None,
        cuda: false,
        rocm: false,
    };
    let meta = crate::gguf::read_gguf_metadata(Path::new(&model)).unwrap();
    let port = (19_760u16..19_800)
        .find(|p| std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok())
        .expect("a free port in 19760..19800");
    let mut config = LaunchConfig {
        binary_path: bin.clone().into(),
        model_path: model.clone().into(),
        gpu_layers: 999,
        context_size: 131_072,
        host: "127.0.0.1".into(),
        port,
        parallel_slots: Some(1),
        custom_args: Some("-lv 4".into()),
        ..Default::default()
    };
    let applied = apply(&mut config, Some(&meta), &budget, &[], true);
    let choice = applied.choice.clone().expect("an automatic choice");
    println!(
        "choice: {choice:?}, flash attention forced: {}",
        applied.flash_attn_forced
    );
    assert_ne!(
        choice.level,
        KvLevel::Full,
        "f16 at 128k should not fit a 16 GB card"
    );
    let args = ProcessManager::build_args(&config, &ServerCaps::default()).unwrap();
    println!("args: {}", args.join(" "));

    let mut child = std::process::Command::new(&bin)
        .args(&args)
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    struct Kill(std::process::Child);
    impl Drop for Kill {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let server = Kill(child);
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    let mut log = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    let mut healthy = false;
    while std::time::Instant::now() < deadline && !healthy {
        while let Ok(l) = rx.try_recv() {
            healthy |= l.contains("server is listening") || l.contains("listening on");
            log.push(l);
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    std::thread::sleep(std::time::Duration::from_secs(1));
    while let Ok(l) = rx.try_recv() {
        log.push(l);
    }
    let process = crate::gpu::process_vram_bytes(server.0.id());
    drop(server);
    assert!(healthy, "server did not start:\n{}", log.join("\n"));
    let want = format!("K ({}):", choice.cache_type_k);
    let kv_line = log
        .iter()
        .find(|l| l.contains("llama_kv_cache: size ="))
        .expect("KV size line at -lv 4");
    println!(
        "{kv_line}\nprocess VRAM: {:?} MiB",
        process.map(|b| b >> 20)
    );
    assert!(kv_line.contains(&want), "{kv_line}");
    assert!(
        kv_line.contains(&format!("V ({}):", choice.cache_type_v)),
        "{kv_line}"
    );
    let parsed = vram::parse_load_log(&log);
    let measured = parsed.kv_gpu() + parsed.kv_host();
    let err = (measured as f64 - choice.kv_bytes as f64).abs() / measured as f64;
    println!(
        "KV measured {} MiB, estimated {} MiB (f16 would be {} MiB)",
        measured >> 20,
        choice.kv_bytes >> 20,
        choice.kv_bytes_f16 >> 20
    );
    assert!(err < 0.03, "KV estimate off by {:.1}%", err * 100.0);
}

#[test]
fn prompt_slowdown_is_known_for_measured_backends_only() {
    use crate::hardware::RecommendedBackend as B;
    assert_eq!(prompt_slowdown_pct(KvLevel::Full, B::Cuda12), Some(0));
    assert_eq!(prompt_slowdown_pct(KvLevel::Saver, B::Vulkan), Some(14));
    // ROCm (b11320, RX 7900 GRE): at most 5 % in prompt processing.
    for level in [
        KvLevel::KeysOnly,
        KvLevel::Balanced,
        KvLevel::Saver,
        KvLevel::Compact,
    ] {
        assert_eq!(prompt_slowdown_pct(level, B::Rocm), Some(5));
    }
    // CUDA (b11327, RTX 3050 Ti Laptop): at most 6 % in prompt processing.
    assert_eq!(prompt_slowdown_pct(KvLevel::Balanced, B::Cuda12), Some(6));
    assert_eq!(prompt_slowdown_pct(KvLevel::Compact, B::Cuda12), Some(5));
    assert_eq!(prompt_slowdown_pct(KvLevel::Compact, B::Vulkan), None);
    // Metal (b11344, M1): the mixed pairs fall to a slow path, the matching pairs do not.
    assert_eq!(prompt_slowdown_pct(KvLevel::Saver, B::Metal), Some(90));
    assert_eq!(prompt_slowdown_pct(KvLevel::KeysOnly, B::Metal), Some(87));
    assert_eq!(prompt_slowdown_pct(KvLevel::Balanced, B::Metal), Some(0));
    assert_eq!(prompt_slowdown_pct(KvLevel::Compact, B::Metal), Some(0));
    assert_eq!(prompt_slowdown_pct(KvLevel::Saver, B::Cpu), None);
}

/// An 8 GB M1: Metal's share of unified memory is 2/3 of it.
fn metal_8gb() -> MemoryBudget {
    MemoryBudget {
        vram_bytes: 8 * GIB * 2 / 3,
        ram_bytes: 8 * GIB,
        unified: true,
        ..budget()
    }
}

#[test]
fn metal_ladder_has_equal_pairs_only() {
    // Whatever the context, the automatic choice on Apple Silicon is never a mixed pair nor
    // q4_0 / q4_0; an integrated GPU with the same unified pool (Vulkan) keeps the mixed
    // q8_0/q4_0 rung.
    let (size, meta) = shape(QWEN3);
    let igpu = MemoryBudget {
        shared_igpu: true,
        ..metal_8gb()
    };
    let mut igpu_saver = false;
    for ctx in [8_192, 16_384, 32_768, 65_536, 131_072, 262_144] {
        let (c, _) = choose(&input(size, ctx), Some(&meta), &metal_8gb(), &[], true);
        assert!(
            matches!(c.level, KvLevel::Full | KvLevel::Balanced),
            "ctx {ctx}: {:?}",
            c.level
        );
        assert!(!is_mixed_pair(Some(&c.cache_type_k), Some(&c.cache_type_v)));
        igpu_saver |= choose(&input(size, ctx), Some(&meta), &igpu, &[], true)
            .0
            .level
            == KvLevel::Saver;
    }
    assert!(igpu_saver, "an integrated GPU keeps the mixed rung");
}

#[test]
fn shared_igpu_counts_gpu_buffers_against_ram() {
    // 4 GiB machine, integrated GPU with half of it as shared memory. The GPU-side buffers come
    // out of the same RAM: a partial offload that fits "VRAM" and RAM separately (f16 at 4k with
    // 12 layers: 2.4 GB + 1.1 GB) does not fit the one pool, so a smaller KV type is chosen.
    let (size, meta) = shape(QWEN3);
    let ram = 4 * GIB;
    let shared = MemoryBudget {
        vram_bytes: ram / 2,
        ram_bytes: ram,
        unified: true,
        shared_igpu: true,
        igpu_offload_faster: Some(false),
        cuda: false,
        rocm: false,
    };
    let separate = MemoryBudget {
        shared_igpu: false,
        unified: false,
        ..shared
    };
    let mut stricter = false;
    for ctx in [4096u32, 8192, 16_384, 32_768, 65_536, 131_072] {
        for layers in [0, 12, 24, 999] {
            let i = FitInput {
                gpu_layers: layers,
                ..input(size, ctx)
            };
            let (c, r) = choose(&i, Some(&meta), &shared, &[], true);
            if matches!(c.reason, KvReason::Fits | KvReason::ToFit) {
                let e = &r.estimate;
                assert!(
                    (e.ram_bytes + e.vram_bytes) as f64 <= ram as f64 * RAM_LIMIT,
                    "ctx {ctx} ngl {layers}: {c:?}"
                );
            }
            let (s, _) = choose(&i, Some(&meta), &separate, &[], true);
            let rung = |l: KvLevel| AUTO_LADDER.iter().position(|x| *x == l).unwrap();
            // `TooBig` means nothing fits and the policy keeps the default level.
            if c.reason != KvReason::TooBig {
                assert!(
                    rung(c.level) >= rung(s.level),
                    "shared memory is never more generous"
                );
            }
            stricter |= c.level != s.level;
        }
    }
    assert!(
        stricter,
        "some context needs a smaller KV type on shared memory"
    );
}

/// The RTX 3050 Ti Laptop: 4 GiB VRAM on CUDA, 16 GiB RAM.
fn cuda_4gb() -> MemoryBudget {
    MemoryBudget {
        vram_bytes: 4 * GIB,
        ram_bytes: 16 * GIB,
        cuda: true,
        rocm: false,
        ..budget()
    }
}

fn cuda_level(ctx: u32, slots: u32) -> KvLevel {
    let (size, meta) = shape(QWEN3);
    let i = FitInput {
        parallel_slots: Some(slots),
        ..input(size, ctx)
    };
    choose(&i, Some(&meta), &cuda_4gb(), &[], true).0.level
}

#[test]
fn cuda_ladder_has_equal_pairs_only() {
    assert_eq!(
        CUDA_LADDER.map(|l| l.types().unwrap()),
        [("f16", "f16"), ("q8_0", "q8_0")]
    );
    for l in CUDA_LADDER {
        let (k, v) = l.types().unwrap();
        assert!(!is_mixed_pair(Some(k), Some(v)));
    }
    // Whatever the context, the automatic choice on CUDA is never a mixed pair; the same
    // contexts reach the mixed q8_0/q4_0 rung on other backends.
    let (size, meta) = shape(QWEN3);
    let mut other_backend_saver = false;
    for ctx in [8_192, 16_384, 32_768, 65_536, 131_072, 262_144, 524_288] {
        for b in [
            budget(),
            cuda_4gb(),
            MemoryBudget {
                cuda: true,
                rocm: false,
                ..budget()
            },
        ] {
            let (c, _) = choose(&input(size, ctx), Some(&meta), &b, &[], true);
            if b.cuda {
                assert!(
                    matches!(c.level, KvLevel::Full | KvLevel::Balanced),
                    "ctx {ctx}: {:?}",
                    c.level
                );
                assert!(!is_mixed_pair(Some(&c.cache_type_k), Some(&c.cache_type_v)));
            } else {
                other_backend_saver |= c.level == KvLevel::Saver;
            }
        }
    }
    assert!(other_backend_saver, "Vulkan keeps the mixed rung");
}

#[test]
fn cuda_without_flash_attention_keeps_keys_only_step() {
    let (size, meta) = shape(QWEN3);
    let (c, _) = choose(&input(size, 98_304), Some(&meta), &cuda_4gb(), &[], false);
    assert_eq!(c.level, KvLevel::KeysOnly);
}

#[test]
fn rocm_uses_equal_pairs_in_estimates_and_launches_but_vulkan_keeps_saver() {
    let (size, meta) = shape(QWEN3);
    let rocm = MemoryBudget {
        rocm: true,
        ..budget()
    };
    for ctx in [32_768, 98_304, 131_072, 196_608, 262_144, 524_288] {
        let i = input(size, ctx);
        let (choice, fit) = choose(&i, Some(&meta), &rocm, &[], true);
        assert_eq!(choice.cache_type_k, choice.cache_type_v);
        assert_ne!(choice.level, KvLevel::Compact, "ctx {ctx}");
        assert_eq!(
            vram::fit_with(&i, Some(&meta), &rocm, &[]).kv.unwrap(),
            choice
        );
        let mut launch = LaunchConfig {
            context_size: ctx,
            gpu_layers: 999,
            parallel_slots: Some(1),
            ..Default::default()
        };
        let applied = apply(&mut launch, Some(&meta), &rocm, &[], true);
        let launched = applied.choice.unwrap();
        assert_eq!(launched.cache_type_k, launched.cache_type_v);
        assert_eq!(fit.estimate.kv_bytes, choice.kv_bytes);
    }
    assert_eq!(
        choose(&input(size, 262_144), Some(&meta), &budget(), &[], true)
            .0
            .level,
        KvLevel::Saver
    );
    assert_eq!(
        choose(&input(size, 262_144), Some(&meta), &rocm, &[], false)
            .0
            .level,
        KvLevel::KeysOnly
    );
    let mut explicit = LaunchConfig {
        cache_type_k: Some("q8_0".into()),
        cache_type_v: Some("q4_0".into()),
        ..Default::default()
    };
    assert!(apply(&mut explicit, Some(&meta), &rocm, &[], true)
        .choice
        .is_none());
    assert_eq!(explicit.cache_type_k.as_deref(), Some("q8_0"));
    assert_eq!(explicit.cache_type_v.as_deref(), Some("q4_0"));
}

#[test]
fn hardware_budget_distinguishes_rocm_from_vulkan_and_old_budgets_default_to_vulkan() {
    use crate::hardware::{GpuInfo, GpuVendor, RecommendedBackend as B};
    let mut hw = crate::hardware::detect_hardware();
    hw.unified_memory_budget_bytes = None;
    hw.total_ram_bytes = 32 * GIB;
    hw.gpus = vec![GpuInfo {
        index: 0,
        name: "Synthetic AMD GPU".into(),
        vendor: GpuVendor::Amd,
        vram_bytes: Some(16 * GIB),
        driver_version: None,
        is_discrete: true,
        is_default: true,
    }];
    hw.recommended_backend = B::Rocm;
    assert!(vram::budget(&hw).rocm);
    assert!(!vram::budget(&hw).cuda);
    hw.recommended_backend = B::Vulkan;
    assert!(!vram::budget(&hw).rocm);
    let mut old = serde_json::to_value(budget()).unwrap();
    old.as_object_mut().unwrap().remove("rocm");
    let old: MemoryBudget = serde_json::from_value(old).unwrap();
    assert!(!old.rocm);
}

#[test]
fn cuda_4gb_card_avoids_the_overflowing_combinations() {
    // Measured on the RTX 3050 Ti Laptop (private hs-internal performance notes "CUDA"): model 2.32 GiB; f16 at
    // 16k and q8_0 at 32k spill into system memory (generation 9.9 and 25-36 t/s against
    // 55 and 51), q8_0 at 16k and q4_0 at 32k fit.
    let (size, meta) = shape(QWEN3);
    let used = |ctx: u32, k: &str, v: &str, slots: u32| {
        let i = FitInput {
            cache_type_k: Some(k.into()),
            cache_type_v: Some(v.into()),
            kv_auto: false,
            parallel_slots: Some(slots),
            ..input(size, ctx)
        };
        vram::fit_with(&i, Some(&meta), &cuda_4gb(), &[])
            .estimate
            .vram_bytes
    };
    // The estimate is within ~1 % of the measured use (3813 and 3905 MiB for q8_0 16k and
    // q4_0 32k) and puts both overflowing cases far above the card.
    let card = 4 * GIB;
    assert!(used(16_384, "f16", "f16", 1) > card, "f16 16k overflows");
    assert!(used(32_768, "q8_0", "q8_0", 1) > card, "q8_0 32k overflows");
    assert!(used(16_384, "q8_0", "q8_0", 1) < card, "q8_0 16k fits");
    assert!(used(32_768, "q4_0", "q4_0", 1) < card, "q4_0 32k fits");
    // Full-attention layers cost the same for any slot count (one pool of -c tokens).
    assert_eq!(
        used(32_768, "q4_0", "q4_0", 4),
        used(32_768, "q4_0", "q4_0", 1)
    );
    // The automatic choice never picks q4_0 / q4_0 (it broke Qwen3-0.6B on CUDA): at a fixed
    // context that q8_0 does not fit it stays q8_0 (the tightest rung); the gateway shrinks the
    // context instead (scheduler `fit_to_memory`).
    assert_eq!(cuda_level(16_384, 1), KvLevel::Balanced);
    assert_eq!(cuda_level(32_768, 1), KvLevel::Balanced);
    assert_eq!(cuda_level(32_768, 4), KvLevel::Balanced);
    let first = crate::prepare::first_ladder_fit(&CUDA_LADDER, 32_768, 4_096, |level, ctx| {
        let (k, v) = level.types().unwrap();
        used(ctx, k, v, 1) as f64 <= card as f64 * VRAM_LIMIT
    })
    .unwrap();
    assert_eq!(first.level, KvLevel::Balanced);
    assert!(first.context_size < 32_768, "{first:?}");
}

#[test]
fn mixed_pair_on_cuda_counts_the_conversion_buffer() {
    let (size, meta) = shape(QWEN3);
    let est = |k: &str, v: &str, b: &MemoryBudget| {
        let i = FitInput {
            cache_type_k: Some(k.into()),
            cache_type_v: Some(v.into()),
            kv_auto: false,
            ..input(size, 16_384)
        };
        vram::fit_with(&i, Some(&meta), b, &[]).estimate
    };
    let plain = MemoryBudget {
        cuda: false,
        rocm: false,
        ..cuda_4gb()
    };
    let cuda = cuda_4gb();
    // Qwen3-4B: 8 KV heads x 128 = 1024 elements per token for K and for V; q8_0/q4_0 convert
    // both sides, one layer at a time: 16384 x 2048 x 2 bytes = 64 MiB.
    let (m, p) = (est("q8_0", "q4_0", &cuda), est("q8_0", "q4_0", &plain));
    assert_eq!(m.vram_bytes - p.vram_bytes, 64 << 20);
    assert_eq!(m.compute_gpu_bytes - p.compute_gpu_bytes, 64 << 20);
    // q8_0/f16 converts K only.
    assert_eq!(
        est("q8_0", "f16", &cuda).vram_bytes - est("q8_0", "f16", &plain).vram_bytes,
        32 << 20
    );
    // Equal pairs have no conversion.
    for (k, v) in [("f16", "f16"), ("q8_0", "q8_0"), ("q4_0", "q4_0")] {
        assert_eq!(est(k, v, &cuda), est(k, v, &plain));
    }
}

/// Qwen3.5-2B header (hybrid, head width 256) and Qwen2.5-3B-like (head width 128).
fn heads(width: u64) -> GgufMetadata {
    GgufMetadata {
        architecture: Some("qwen35".into()),
        block_count: Some(24),
        embedding_length: Some(2048),
        head_count: Some(8),
        head_count_kv: Some(2),
        key_length: Some(width),
        value_length: Some(width),
        full_attention_interval: Some(4),
        ssm_conv_kernel: Some(4),
        ssm_state_size: Some(128),
        ssm_inner_size: Some(2048),
        ssm_group_count: Some(16),
        feed_forward_length: Some(6144),
        ..Default::default()
    }
}

/// Real CUDA loads (RTX 3050 Ti Laptop, b11456, `-lv 4`): with 256-wide heads a quantized
/// equal K/V pair still makes the flash-attention kernel convert one layer of K and V to f16
/// (compute buffer 224 MiB at 65536 cells q8_0 against 116 MiB with f16; 800 MiB at 262144
/// against 308); 128-wide heads read the quantized cache directly (148 against 144 MiB).
#[test]
fn wide_heads_convert_equal_quantized_pairs_on_cuda() {
    let input = |ctx: u32, k: &str, flash: Option<bool>| FitInput {
        model_size_bytes: 1_280_835_840,
        context_size: ctx,
        gpu_layers: 99,
        cache_type_k: Some(k.into()),
        cache_type_v: Some(k.into()),
        flash_attn: flash,
        ..Default::default()
    };
    let mib = |b: u64| b as f64 / (1024.0 * 1024.0);
    let wide = heads(256);
    // One layer of K and V in f16: padded context x (512 + 512) x 2 bytes.
    for (ctx, base_compute, measured) in [(65_536, 112.0, 224.29), (262_144, 304.0, 800.29)] {
        let extra = mib(cuda_convert_buffer_bytes(
            &input(ctx, "q8_0", None),
            Some(&wide),
        ));
        assert_eq!(extra, f64::from(ctx) / 512.0, "{ctx} cells");
        let est = base_compute + extra;
        assert!(
            (est - measured).abs() / measured < 0.08,
            "{ctx}: {est} vs {measured}"
        );
    }
    let q4 = cuda_convert_buffer_bytes(&input(65_536, "q4_0", Some(true)), Some(&wide));
    assert_eq!(mib(q4), 128.0);
    assert_eq!(
        cuda_convert_buffer_bytes(&input(65_536, "f16", None), Some(&wide)),
        0
    );
    assert_eq!(
        cuda_convert_buffer_bytes(&input(65_536, "q8_0", Some(false)), Some(&wide)),
        0
    );
    assert_eq!(
        cuda_convert_buffer_bytes(&input(65_536, "q8_0", None), Some(&heads(128))),
        0
    );
    // A mixed pair is converted whatever the head width (existing rule).
    let mixed = FitInput {
        cache_type_v: Some("f16".into()),
        ..input(65_536, "q8_0", None)
    };
    assert!(cuda_convert_buffer_bytes(&mixed, Some(&heads(128))) > 0);
}

/// Vulkan and ROCm read a quantized cache with 256-wide heads directly: no conversion buffer
/// in the estimate. Measured on an RX 7900 GRE (Vulkan, b11445, `-lv 4`): Gemma-3-4B (head
/// width 256), 65536 cells, flash attention, compute buffer 150.52 MiB with f16 and 151.05 MiB
/// with q8_0.
#[test]
fn wide_heads_add_no_conversion_buffer_off_cuda() {
    let wide = heads(256);
    let est = |k: &str, b: &MemoryBudget| {
        let i = FitInput {
            model_size_bytes: 1_280_835_840,
            context_size: 65_536,
            gpu_layers: 99,
            cache_type_k: Some(k.into()),
            cache_type_v: Some(k.into()),
            kv_auto: false,
            ..Default::default()
        };
        vram::fit_with(&i, Some(&wide), b, &[]).estimate
    };
    let vulkan = MemoryBudget {
        cuda: false,
        rocm: false,
        ..cuda_4gb()
    };
    let rocm = MemoryBudget {
        rocm: true,
        ..vulkan
    };
    let cuda = cuda_4gb();
    for b in [&vulkan, &rocm] {
        let (q8, f16) = (est("q8_0", b), est("f16", b));
        assert_eq!(q8.compute_gpu_bytes, f16.compute_gpu_bytes);
    }
    assert!(est("q8_0", &cuda).compute_gpu_bytes > est("q8_0", &vulkan).compute_gpu_bytes);
}

/// A header with a zero head count and no key length (broken or unusual GGUF) gives no head
/// width instead of dividing by zero.
#[test]
fn zero_head_count_has_no_head_width() {
    let meta = GgufMetadata {
        embedding_length: Some(2048),
        head_count: Some(0),
        ..Default::default()
    };
    let input = FitInput {
        model_size_bytes: 1 << 30,
        context_size: 4096,
        gpu_layers: 99,
        cache_type_k: Some("q8_0".into()),
        cache_type_v: Some("q8_0".into()),
        ..Default::default()
    };
    assert_eq!(cuda_convert_buffer_bytes(&input, Some(&meta)), 0);
}
