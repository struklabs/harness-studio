use super::*;
use std::path::Path;

const MIB_F: f64 = 1024.0 * 1024.0;

#[test]
fn s20_qwen3_measured_allocations_and_output() {
    let (size, meta) = shape_of("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
    let mut launch = input(size, 65536, "f16");
    launch.parallel_slots = Some(2);
    launch.kv_unified = Some(true);
    let f16 = estimate_raw(&launch, Some(&meta), true);
    // The old formula already returned 11.61 GiB, above the measured adapter
    // delta (14.1 - 4.3 = 9.8 GiB). The aggregate delta is not a process allocation
    // measurement: WDDM residency/spill cannot be inferred by subtracting it.
    assert_eq!(f16.vram_bytes, 12_465_044_576);
    assert!(f16.vram_bytes as f64 > (14.1 - 4.3) * GIB as f64);
    assert_eq!(f16.kv_gpu_bytes, 9 * GIB);
    assert_eq!(f16.compute_gpu_bytes, 140 * MIB);
    assert_eq!(f16.output_host_bytes, 151_936 * 512 * 4);
    launch.cache_type_k = Some("q8_0".into());
    launch.cache_type_v = Some("q8_0".into());
    let q8 = estimate_raw(&launch, Some(&meta), true);
    assert!(within(
        q8.vram_bytes,
        ((11.9 - 4.3) * GIB as f64) as u64,
        5.0
    ));
    launch.context_size = 40960;
    launch.parallel_slots = Some(1);
    launch.cache_type_k = Some("f16".into());
    launch.cache_type_v = Some("f16".into());
    let smaller = estimate_raw(&launch, Some(&meta), true);
    assert!(within(
        smaller.vram_bytes,
        ((12.7 - 4.3) * GIB as f64) as u64,
        5.0
    ));
    launch.ubatch = Some(256);
    let half_batch = estimate_raw(&launch, Some(&meta), true);
    assert_eq!(half_batch.output_host_bytes * 2, smaller.output_host_bytes);
    assert_eq!(half_batch.compute_gpu_bytes * 2, smaller.compute_gpu_bytes);
}

#[test]
fn gemma3_1b_estimate_uses_corrected_sliding_window() {
    let mut meta = GgufMetadata {
        architecture: Some("gemma3".into()),
        embedding_length: Some(1152),
        block_count: Some(26),
        head_count: Some(4),
        head_count_kv: Some(1),
        key_length: Some(256),
        value_length: Some(256),
        sliding_window: Some(1024),
        ..Default::default()
    };
    let input = input(1024 * 1024 * 1024, 8192, "f16");
    let bad_header = estimate_raw(&input, Some(&meta), true);
    assert_eq!(shape(input.model_size_bytes, Some(&meta)).n_swa, 512);
    meta.sliding_window = Some(512);
    let correct_header = estimate_raw(&input, Some(&meta), true);
    assert_eq!(bad_header.kv_gpu_bytes, correct_header.kv_gpu_bytes);
    meta.embedding_length = Some(2560);
    meta.sliding_window = Some(1024);
    assert_eq!(shape(input.model_size_bytes, Some(&meta)).n_swa, 1024);
    assert!(estimate_raw(&input, Some(&meta), true).kv_gpu_bytes > correct_header.kv_gpu_bytes);
    meta.architecture = Some("other".into());
    meta.embedding_length = Some(1152);
    assert_eq!(meta.known_metadata_fixes().sliding_window, Some(1024));
    assert!(meta.known_metadata_fixes().override_kv.is_none());
}

fn data(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name),
    )
    .unwrap()
}

fn lines(name: &str) -> Vec<String> {
    data(name).lines().map(str::to_string).collect()
}

/// GGUF header facts of the three real models (from `read_gguf_metadata`, see
/// `dump_real_shapes`) with their file sizes.
fn shapes() -> Vec<(String, u64, GgufMetadata)> {
    let v: serde_json::Value = serde_json::from_str(&data("gguf-vram-shapes.json")).unwrap();
    v.as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["file"].as_str().unwrap().to_string(),
                e["size_bytes"].as_u64().unwrap(),
                serde_json::from_value(e["meta"].clone()).unwrap(),
            )
        })
        .collect()
}

fn shape_of(file: &str) -> (u64, GgufMetadata) {
    shapes()
        .into_iter()
        .find(|(f, _, _)| f == file)
        .map(|(_, s, m)| (s, m))
        .unwrap()
}

/// A real load: fixture log, model, settings and the dedicated GPU memory Windows reported for
/// the process right after `/health` (PDH `GPU Process Memory`, RX 7900 GRE, Vulkan, b11236).
struct RealLoad {
    log: &'static str,
    file: &'static str,
    ctx: u32,
    cache: &'static str,
    process_mib: f64,
}

const REAL_LOADS: &[RealLoad] = &[
    RealLoad {
        log: "b11236-vulkan-qwen3-4b-c8k.load-log.txt",
        file: "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
        ctx: 8192,
        cache: "f16",
        process_mib: 3741.9,
    },
    RealLoad {
        log: "b11236-vulkan-qwen3-4b-c32k-q8.load-log.txt",
        file: "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
        ctx: 32768,
        cache: "q8_0",
        process_mib: 5064.5,
    },
    RealLoad {
        log: "b11236-vulkan-gpt-oss-20b-c8k.load-log.txt",
        file: "gpt-oss-20b-MXFP4.gguf",
        ctx: 8192,
        cache: "f16",
        process_mib: 11409.2,
    },
    RealLoad {
        log: "b11236-vulkan-gpt-oss-20b-c32k.load-log.txt",
        file: "gpt-oss-20b-MXFP4.gguf",
        ctx: 32768,
        cache: "f16",
        process_mib: 12010.2,
    },
    RealLoad {
        log: "b11236-vulkan-qwen2.5-coder-14b-c8k.load-log.txt",
        file: "qwen2.5-coder-14b-instruct-q4_k_m.gguf",
        ctx: 8192,
        cache: "f16",
        process_mib: 9937.6,
    },
];

fn input(size: u64, ctx: u32, cache: &str) -> FitInput {
    FitInput {
        model_size_bytes: size,
        context_size: ctx,
        gpu_layers: 99,
        cache_type_k: Some(cache.into()),
        cache_type_v: Some(cache.into()),
        ..Default::default()
    }
}

fn mib(b: u64) -> f64 {
    b as f64 / MIB_F
}

fn within(est: u64, measured: u64, pct: f64) -> bool {
    let (e, m) = (est as f64, measured as f64);
    m > 0.0 && ((e - m) / m).abs() * 100.0 <= pct
}

#[test]
fn parses_real_load_logs() {
    let l = parse_load_log(&lines("b11236-vulkan-qwen3-4b-c8k.load-log.txt"));
    assert_eq!(l.model_buffers["Vulkan0"], (2375.91 * MIB_F).round() as u64);
    assert_eq!(
        l.model_buffers["CPU_Mapped"],
        (304.28 * MIB_F).round() as u64
    );
    assert_eq!(l.kv_gpu(), 1152 * 1024 * 1024);
    assert_eq!(l.compute_gpu(), (83.01 * MIB_F).round() as u64);
    assert_eq!(
        l.compute_buffers["Vulkan_Host"],
        (18.01 * MIB_F).round() as u64
    );
    assert_eq!(l.kv_caches, vec![(8192, 36)]);
    assert_eq!((l.offloaded_layers, l.total_layers), (Some(37), Some(37)));
    assert_eq!((l.n_ctx, l.n_seq_max), (Some(8192), Some(4)));

    // gpt-oss: interleaved SWA -> two KV caches (full 8192 cells, SWA 1024 cells).
    let l = parse_load_log(&lines("b11236-vulkan-gpt-oss-20b-c8k.load-log.txt"));
    assert_eq!(l.kv_caches, vec![(8192, 12), (1024, 12)]);
    assert_eq!(l.kv_gpu(), (192 + 24) * 1024 * 1024);
    assert_eq!(l.model_host(), (586.82 * MIB_F).round() as u64);

    // Lines as Harness stores them (`[stderr] ` prefix) parse the same.
    let prefixed: Vec<String> = lines("b11236-vulkan-qwen3-4b-c8k.load-log.txt")
        .into_iter()
        .map(|l| format!("[stderr] {l}"))
        .collect();
    assert_eq!(
        parse_load_log(&prefixed),
        parse_load_log(&lines("b11236-vulkan-qwen3-4b-c8k.load-log.txt"))
    );
    // Default verbosity (the Harness-captured log of a b11236 run) prints no buffer sizes.
    assert!(parse_load_log(&lines("b11236-qwen3-4b.server-log.txt")).is_empty());
}

#[test]
fn only_the_last_load_counts() {
    let mut all = lines("b11236-vulkan-gpt-oss-20b-c8k.load-log.txt");
    all.extend(lines("b11236-vulkan-qwen3-4b-c8k.load-log.txt"));
    let l = parse_load_log(&all);
    assert_eq!(l.kv_caches, vec![(8192, 36)]);
}

#[test]
fn real_shapes_have_swa_and_tensor_sizes() {
    let (_, oss) = shape_of("gpt-oss-20b-MXFP4.gguf");
    assert_eq!(oss.sliding_window, Some(128));
    assert_eq!(oss.expert_feed_forward_length, Some(2880));
    assert!(
        oss.output_bytes.is_some(),
        "gpt-oss has its own output tensor"
    );
    let (_, q3) = shape_of("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
    assert_eq!(q3.sliding_window, None);
    assert_eq!(q3.output_bytes, None, "Qwen3-4B ties output and embeddings");
    // token_embd (Q6_K, 151936 x 2560) = 304.28 MiB, the CPU_Mapped buffer of the log.
    assert!((mib(q3.token_embd_bytes.unwrap()) - 304.28).abs() < 0.1);
}

/// The estimate against every real load: weights within 1%, KV exact (SWA included),
/// compute buffer within 15%, total within 3% of what Windows reported for the process.
#[test]
fn estimate_matches_real_loads() {
    let mut table = String::from(
        "\n| load | weights est/meas MiB | KV est/meas | compute est/meas | total est / process MiB |\n",
    );
    for load in REAL_LOADS {
        let (size, meta) = shape_of(load.file);
        let est = estimate_raw(&input(size, load.ctx, load.cache), Some(&meta), true);
        let log = parse_load_log(&lines(load.log));
        let process = (load.process_mib * MIB_F) as u64;
        table += &format!(
            "| {} | {:.0} / {:.0} | {:.0} / {:.0} | {:.0} / {:.0} | {:.0} / {:.0} |\n",
            load.log,
            mib(est.model_gpu_bytes),
            mib(log.model_gpu()),
            mib(est.kv_gpu_bytes),
            mib(log.kv_gpu()),
            mib(est.compute_gpu_bytes),
            mib(log.compute_gpu()),
            mib(est.vram_bytes),
            load.process_mib
        );
        assert!(within(est.model_gpu_bytes, log.model_gpu(), 1.0), "{table}");
        assert!(within(est.kv_gpu_bytes, log.kv_gpu(), 0.5), "{table}");
        assert!(
            within(est.compute_gpu_bytes, log.compute_gpu(), 15.0),
            "{table}"
        );
        assert!(within(est.vram_bytes, process, 3.0), "{table}");
        assert!(
            within(est.model_host_bytes, log.model_host(), 2.0),
            "{table}"
        );
    }
    println!("{table}");
}

#[test]
fn swa_cache_rules() {
    let (size, meta) = shape_of("gpt-oss-20b-MXFP4.gguf");
    let base = input(size, 32768, "f16");
    let e = estimate_raw(&base, Some(&meta), true);
    assert_eq!((e.swa_layers, e.swa_cells, e.kv_cells), (12, 1024, 32768));
    // --swa-full keeps the whole context in the SWA layers too.
    let full = estimate_raw(
        &FitInput {
            swa_full: true,
            ..base.clone()
        },
        Some(&meta),
        true,
    );
    assert_eq!(full.swa_cells, 32768);
    assert_eq!(full.kv_bytes, 2 * 768 * 1024 * 1024);
    // One slot: 128 + 512 -> 640 -> padded to 768 cells.
    let one = estimate_raw(
        &FitInput {
            parallel_slots: Some(1),
            ..base
        },
        Some(&meta),
        true,
    );
    assert_eq!(one.swa_cells, 768);
}

#[test]
fn partial_offload_and_cpu_only() {
    let (size, meta) = shape_of("qwen2.5-coder-14b-instruct-q4_k_m.gguf");
    let full = estimate_raw(&input(size, 8192, "f16"), Some(&meta), true);
    let half = estimate_raw(
        &FitInput {
            gpu_layers: 24,
            ..input(size, 8192, "f16")
        },
        Some(&meta),
        true,
    );
    assert_eq!(half.offloaded_layers, 24);
    assert!(half.model_gpu_bytes < full.model_gpu_bytes / 2 + 64 * 1024 * 1024);
    assert_eq!(half.kv_gpu_bytes, full.kv_bytes / 2);
    // Weights never vanish: host + device = file (+ the tied copy).
    assert!(half.model_gpu_bytes + half.model_host_bytes >= size);
    let cpu = estimate_raw(&input(size, 8192, "f16"), Some(&meta), false);
    assert_eq!(cpu.vram_bytes, 0);
    assert_eq!(cpu.offloaded_layers, 0);
    assert!(cpu.ram_bytes > size);
}

#[test]
fn mmproj_and_no_flash_attention() {
    let (size, meta) = shape_of("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
    let base = estimate_raw(&input(size, 8192, "f16"), Some(&meta), true);
    let with_mmproj = estimate_raw(
        &FitInput {
            mmproj_size_bytes: 800 * 1024 * 1024,
            ..input(size, 8192, "f16")
        },
        Some(&meta),
        true,
    );
    assert_eq!(with_mmproj.vram_bytes - base.vram_bytes, 800 * 1024 * 1024);
    let no_fa = estimate_raw(
        &FitInput {
            flash_attn: Some(false),
            ..input(size, 8192, "f16")
        },
        Some(&meta),
        true,
    );
    // f32 KQ: 512 x 8192 x 32 heads x 4 bytes = 512 MiB more.
    assert_eq!(
        no_fa.compute_gpu_bytes - base.compute_gpu_bytes,
        512 * 1024 * 1024
    );
}

fn measurement(load: &RealLoad, with_log: bool) -> Measurement {
    let (size, meta) = shape_of(load.file);
    let i = input(size, load.ctx, load.cache);
    Measurement {
        model_key: load.file.into(),
        model_size_bytes: size,
        settings: MeasuredSettings::from_input(&i, meta.block_count.unwrap()),
        log: with_log.then(|| parse_load_log(&lines(load.log))),
        process_vram_bytes: Some((load.process_mib * MIB_F) as u64),
        build: Some(11236),
        measured_at: 1_000,
    }
}

#[test]
fn measurement_corrects_later_estimates() {
    let budget = MemoryBudget {
        vram_bytes: 16 * 1024 * 1024 * 1024,
        ram_bytes: 32 * 1024 * 1024 * 1024,
        unified: false,
        shared_igpu: false,
        igpu_offload_faster: None,
        cuda: false,
        rocm: false,
    };
    let load = &REAL_LOADS[2]; // gpt-oss 8k
    let (size, meta) = shape_of(load.file);
    let m = measurement(load, true);

    // Same settings: the stored numbers come back exactly.
    let same = fit_with(
        &input(size, 8192, "f16"),
        Some(&meta),
        &budget,
        std::slice::from_ref(&m),
    );
    assert_eq!(same.source, EstimateSource::Measured);
    let log = m.log.clone().unwrap();
    assert_eq!(same.estimate.model_gpu_bytes, log.model_gpu());
    assert_eq!(same.estimate.kv_gpu_bytes, log.kv_gpu());
    assert_eq!(same.estimate.compute_gpu_bytes, log.compute_gpu());
    assert_eq!(same.estimate.vram_bytes, m.process_vram_bytes.unwrap());
    assert_eq!(same.measured_at, Some(1_000));

    // Other context: corrected with the measured ratios; closer to the real 32k load than
    // the plain estimate or at least as close.
    let other = fit_with(
        &input(size, 32768, "f16"),
        Some(&meta),
        &budget,
        std::slice::from_ref(&m),
    );
    assert_eq!(other.source, EstimateSource::Corrected);
    let real_32k = (REAL_LOADS[3].process_mib * MIB_F) as u64;
    let raw = estimate_raw(&input(size, 32768, "f16"), Some(&meta), true);
    let err = |v: u64| (v as i64 - real_32k as i64).unsigned_abs();
    assert!(err(other.estimate.vram_bytes) <= err(raw.vram_bytes));
    assert!(within(other.estimate.vram_bytes, real_32k, 1.0));

    // Process total only (default log verbosity): the difference becomes overhead.
    let total_only = measurement(load, false);
    let t = fit_with(
        &input(size, 8192, "f16"),
        Some(&meta),
        &budget,
        &[total_only],
    );
    assert_eq!(t.estimate.vram_bytes, m.process_vram_bytes.unwrap());

    // A replaced file (other size) is ignored.
    let other_file = fit_with(&input(size + 1, 8192, "f16"), Some(&meta), &budget, &[m]);
    assert_eq!(other_file.source, EstimateSource::Estimate);
}

#[test]
fn categories_and_recommended_layers() {
    const GIB: u64 = 1024 * 1024 * 1024;
    let (size, meta) = shape_of("qwen2.5-coder-14b-instruct-q4_k_m.gguf");
    let big = MemoryBudget {
        vram_bytes: 16 * GIB,
        ram_bytes: 32 * GIB,
        unified: false,
        shared_igpu: false,
        igpu_offload_faster: None,
        cuda: false,
        rocm: false,
    };
    let r = fit_with(&input(size, 8192, "f16"), Some(&meta), &big, &[]);
    assert_eq!(r.category, FitCategory::FullGpu);
    assert_eq!(r.recommended_gpu_layers, 49);
    assert_eq!(r.source, EstimateSource::Estimate);

    let small = MemoryBudget {
        vram_bytes: 6 * GIB,
        ..big
    };
    let r = fit_with(&input(size, 8192, "f16"), Some(&meta), &small, &[]);
    assert_eq!(r.category, FitCategory::OomRisk);
    let ngl = r.recommended_gpu_layers;
    assert!(ngl > 0 && ngl < 48, "{ngl}");
    let at = fit_with(
        &FitInput {
            gpu_layers: ngl,
            ..input(size, 8192, "f16")
        },
        Some(&meta),
        &small,
        &[],
    );
    assert!(at.estimate.vram_bytes as f64 <= 6.0 * GIB as f64 * 0.9);
    assert_eq!(at.category, FitCategory::Hybrid);

    let cpu = MemoryBudget {
        vram_bytes: 0,
        ..big
    };
    let r = fit_with(&input(size, 8192, "f16"), Some(&meta), &cpu, &[]);
    assert_eq!(r.category, FitCategory::CpuHeavy);
    assert_eq!(r.recommended_gpu_layers, 0);
}

#[test]
fn unknown_model_falls_back_to_size_guess() {
    let r = estimate_raw(&input(5 * 1024 * 1024 * 1024, 4096, "f16"), None, true);
    assert!(!r.exact);
    assert_eq!(r.total_layers, 32);
    assert!(r.vram_bytes > 5 * 1024 * 1024 * 1024);
}

#[test]
fn settings_key_normalizes_full_offload() {
    let a = MeasuredSettings::from_input(
        &FitInput {
            gpu_layers: 99,
            ..input(1, 4096, "F16")
        },
        36,
    );
    let b = MeasuredSettings::from_input(
        &FitInput {
            gpu_layers: 999,
            cache_type_k: None,
            cache_type_v: None,
            ..input(1, 4096, "f16")
        },
        36,
    );
    assert_eq!(a.key(), b.key());
    assert_eq!(a.gpu_layers, 37);
}

/// Writes `tests/data/gguf-vram-shapes.json` from real files:
/// `HS_VRAM_MODELS="a.gguf;b.gguf" cargo test -p harness-core dump_real_shapes -- --ignored`
#[test]
#[ignore]
fn dump_real_shapes() {
    let list = std::env::var("HS_VRAM_MODELS").expect("HS_VRAM_MODELS");
    let mut out = Vec::new();
    for p in list.split(';').filter(|p| !p.is_empty()) {
        let path = Path::new(p);
        let meta = crate::gguf::read_gguf_metadata(path).unwrap();
        out.push(serde_json::json!({
            "file": path.file_name().unwrap().to_string_lossy(),
            "size_bytes": std::fs::metadata(path).unwrap().len(),
            "meta": meta,
        }));
    }
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/gguf-vram-shapes.json");
    std::fs::write(target, serde_json::to_string_pretty(&out).unwrap() + "\n").unwrap();
}

/// Real llama-server: loads `HS_TEST_MODEL` at `-lv 4` on port 18600+, reads the buffer sizes
/// and the process's GPU memory, and compares them with the estimate.
/// `HS_LLAMA_SERVER=... HS_TEST_MODEL=... cargo test -p harness-core real_load_vs_estimate -- --ignored --nocapture`
#[test]
#[ignore]
fn real_load_vs_estimate() {
    use std::io::{BufRead, BufReader};
    let bin = std::env::var("HS_LLAMA_SERVER").expect("HS_LLAMA_SERVER");
    let model = std::env::var("HS_TEST_MODEL").expect("HS_TEST_MODEL");
    let port = std::env::var("HS_TEST_PORT").unwrap_or_else(|_| "18610".into());
    let ctx: u32 = std::env::var("HS_TEST_CTX")
        .ok()
        .and_then(|c| c.parse().ok())
        .unwrap_or(8192);
    let mut child = std::process::Command::new(&bin)
        .args([
            "-m",
            &model,
            "--host",
            "127.0.0.1",
            "--port",
            &port,
            "-c",
            &ctx.to_string(),
            "-ngl",
            "99",
            "-lv",
            "4",
        ])
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    let mut log = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(240);
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok(l) => {
                let done = l.contains("listening on");
                log.push(l);
                if done {
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => break,
        }
    }
    std::thread::sleep(std::time::Duration::from_secs(1));
    let process = crate::gpu::process_vram_bytes(child.id());
    let _ = child.kill();
    let _ = child.wait();

    let parsed = parse_load_log(&log);
    assert!(!parsed.is_empty(), "no buffer lines:\n{}", log.join("\n"));
    let meta = crate::gguf::read_gguf_metadata(Path::new(&model)).unwrap();
    let size = std::fs::metadata(&model).unwrap().len();
    let est = estimate_raw(&input(size, ctx, "f16"), Some(&meta), true);
    println!(
        "{model} c={ctx}: weights {:.0}/{:.0} MiB, KV {:.0}/{:.0}, compute {:.0}/{:.0}, total {:.0} / process {:?} MiB",
        mib(est.model_gpu_bytes),
        mib(parsed.model_gpu()),
        mib(est.kv_gpu_bytes),
        mib(parsed.kv_gpu()),
        mib(est.compute_gpu_bytes),
        mib(parsed.compute_gpu()),
        mib(est.vram_bytes),
        process.map(mib)
    );
    assert!(within(est.model_gpu_bytes, parsed.model_gpu(), 2.0));
    assert!(within(est.kv_gpu_bytes, parsed.kv_gpu(), 1.0));
    if let Some(p) = process {
        assert!(within(est.vram_bytes, p, 5.0));
    }
}

#[test]
fn integrated_gpu_budget_is_shared_ram_and_weights_stay_on_the_cpu() {
    use crate::hardware::{GpuInfo, GpuVendor, RecommendedBackend};
    let mut hw = crate::hardware::detect_hardware();
    hw.total_ram_bytes = 4 * GIB;
    hw.recommended_backend = RecommendedBackend::Vulkan;
    hw.unified_memory_budget_bytes = None;
    let gpu = |discrete: bool, vram: Option<u64>| GpuInfo {
        index: 0,
        name: "GPU".into(),
        vendor: GpuVendor::Intel,
        vram_bytes: vram,
        driver_version: None,
        is_discrete: discrete,
        is_default: true,
    };
    hw.gpus = vec![gpu(false, None)];
    let b = budget(&hw);
    assert!(b.shared_igpu && b.unified);
    assert_eq!(b.vram_bytes, 2 * GIB);
    assert_eq!(
        b.igpu_offload_faster, None,
        "name 'GPU' is not a known older Intel iGPU"
    );
    hw.gpus[0].name = "Intel(R) HD Graphics 6000 (BDW GT3)".into();
    assert_eq!(budget(&hw).igpu_offload_faster, Some(false));
    // A discrete card, or an integrated one with a large pool of its own, is unchanged.
    hw.gpus = vec![gpu(true, Some(8 * GIB))];
    assert!(!budget(&hw).shared_igpu);
    hw.gpus = vec![gpu(false, Some(16 * GIB))];
    assert_eq!(budget(&hw).vram_bytes, 16 * GIB);
    assert!(!budget(&hw).shared_igpu);
    // An APU's small BIOS carve-out (Renoir: 512 MiB) adds to the shared half of RAM.
    hw.gpus = vec![gpu(false, Some(GIB / 2))];
    let b = budget(&hw);
    assert!(b.shared_igpu && b.unified);
    assert_eq!(b.vram_bytes, GIB / 2 + 2 * GIB);

    let shared = MemoryBudget {
        vram_bytes: 2 * GIB,
        ram_bytes: 4 * GIB,
        unified: true,
        shared_igpu: true,
        igpu_offload_faster: None,
        cuda: false,
        rocm: false,
    };
    let input = |ngl: u32, size: u64| FitInput {
        model_size_bytes: size,
        context_size: 4096,
        gpu_layers: ngl,
        ..Default::default()
    };
    // Older Intel iGPU (as `budget` sets it): weights on the CPU; the small model fits.
    let shared = MemoryBudget {
        igpu_offload_faster: Some(false),
        cuda: false,
        rocm: false,
        ..shared
    };
    let small = fit_with(&input(0, 500 * MIB), None, &shared, &[]);
    assert_eq!(small.recommended_gpu_layers, 0);
    assert_eq!(small.category, FitCategory::CpuHeavy);
    // GPU and CPU parts come out of the same RAM: 3.5 GB on 4 GB does not fit.
    let big = fit_with(&input(99, 3 * GIB + GIB / 2), None, &shared, &[]);
    assert_eq!(big.category, FitCategory::OomRisk);
}

#[test]
fn only_older_intel_igpus_default_to_cpu_weights() {
    use crate::hardware::{GpuInfo, GpuVendor};
    let g = |vendor: GpuVendor, name: &str| GpuInfo {
        index: 0,
        name: name.into(),
        vendor,
        vram_bytes: None,
        driver_version: None,
        is_discrete: false,
        is_default: true,
    };
    assert!(is_legacy_intel_igpu(&g(
        GpuVendor::Intel,
        "Intel(R) HD Graphics 6000 (BDW GT3)"
    )));
    assert!(is_legacy_intel_igpu(&g(
        GpuVendor::Intel,
        "Intel(R) UHD Graphics 620"
    )));
    assert!(is_legacy_intel_igpu(&g(
        GpuVendor::Intel,
        "Intel(R) Iris(R) Plus Graphics"
    )));
    assert!(!is_legacy_intel_igpu(&g(
        GpuVendor::Intel,
        "Intel(R) Iris(R) Xe Graphics"
    )));
    assert!(!is_legacy_intel_igpu(&g(
        GpuVendor::Intel,
        "Intel(R) Arc(TM) Graphics"
    )));
    assert!(!is_legacy_intel_igpu(&g(GpuVendor::Amd, "AMD Radeon 780M")));

    // Unknown (newer) iGPU: the memory budget decides; a measurement can overrule either way.
    let mut b = MemoryBudget {
        vram_bytes: 8 * GIB,
        ram_bytes: 16 * GIB,
        unified: true,
        shared_igpu: true,
        igpu_offload_faster: None,
        cuda: false,
        rocm: false,
    };
    let input = FitInput {
        model_size_bytes: 500 * MIB,
        context_size: 4096,
        ..Default::default()
    };
    assert!(fit_with(&input, None, &b, &[]).recommended_gpu_layers > 0);
    b.igpu_offload_faster = Some(false);
    assert_eq!(fit_with(&input, None, &b, &[]).recommended_gpu_layers, 0);
}

#[test]
fn integrated_gpu_with_automatic_kv_and_slots() {
    use crate::hardware::{GpuInfo, GpuVendor, RecommendedBackend};
    // The Debian test machine: 4 GB RAM, Intel HD 6000 (shared memory, weights on the CPU).
    let mut hw = crate::hardware::detect_hardware();
    hw.total_ram_bytes = 4 * GIB;
    hw.recommended_backend = RecommendedBackend::Vulkan;
    hw.unified_memory_budget_bytes = None;
    hw.gpus = vec![GpuInfo {
        index: 0,
        name: "Intel(R) HD Graphics 6000 (BDW GT3)".into(),
        vendor: GpuVendor::Intel,
        vram_bytes: None,
        driver_version: None,
        is_discrete: false,
        is_default: true,
    }];
    let b = budget(&hw);
    let (size, meta) = shape_of("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
    let auto = |ctx: u32, ngl: u32| FitInput {
        model_size_bytes: size,
        context_size: ctx,
        gpu_layers: ngl,
        kv_auto: true,
        parallel_slots: None,
        flash_attn: Some(true),
        ..Default::default()
    };
    let r = fit_auto(&auto(4096, 0), Some(&meta), &b, &[], &hw);
    assert_eq!(r.recommended_gpu_layers, 0);
    assert!(r.budget.shared_igpu);
    let kv = r.kv.as_ref().expect("automatic KV choice");
    let slots = r.slots.as_ref().expect("automatic slots");
    assert!(slots.auto && slots.slots >= 1);
    // The chosen types fit the one pool (weights, KV and the GPU's share together).
    if matches!(
        kv.reason,
        crate::kv_policy::KvReason::Fits | crate::kv_policy::KvReason::ToFit
    ) {
        let e = &r.estimate;
        assert!(
            (e.ram_bytes + e.vram_bytes) as f64 <= b.ram_bytes as f64 * crate::kv_policy::RAM_LIMIT
        );
    }
    // A full offload of a long context into shared memory is an OOM risk, whatever the KV type.
    let big = fit_auto(&auto(131_072, 999), Some(&meta), &b, &[], &hw);
    assert_eq!(big.category, FitCategory::OomRisk);
}

#[test]
fn measured_full_offload_needs_no_estimate_margin() {
    // Real laptop (RTX 3050 Ti Laptop, 3962 MiB dedicated, WDDM, b11327 CUDA 12.4): Qwen3-4B
    // 16k q8_0 with 2 slots ran fully on the GPU at 3801 MiB (no spill to shared memory). The
    // log had no buffer sizes (default verbosity), so only the process total is known.
    let (size, meta) = shape_of("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
    let budget = MemoryBudget {
        vram_bytes: 3962 * 1024 * 1024,
        ram_bytes: 16 * 1024 * 1024 * 1024,
        unified: false,
        shared_igpu: false,
        igpu_offload_faster: None,
        cuda: true,
        rocm: false,
    };
    let at = |ngl: u32| FitInput {
        gpu_layers: ngl,
        parallel_slots: Some(2),
        kv_unified: Some(true),
        ..input(size, 16384, "q8_0")
    };
    let total = meta.block_count.unwrap();
    let run = |ngl: u32, mib: f64, at_ms: i64| Measurement {
        model_key: "qwen".into(),
        model_size_bytes: size,
        settings: MeasuredSettings::from_input(&at(ngl), total),
        log: None,
        process_vram_bytes: Some((mib * MIB_F) as u64),
        build: Some(11327),
        measured_at: at_ms,
    };
    let full = total as u32 + 1;
    let partial = run(33, 3475.0, 1);
    let fitted = run(99, 3801.0, 2);

    // Only the -ngl 33 run: the 90% rule keeps part of the model on the CPU.
    let r = fit_with(
        &at(33),
        Some(&meta),
        &budget,
        std::slice::from_ref(&partial),
    );
    assert!(
        r.recommended_gpu_layers < full,
        "{}",
        r.recommended_gpu_layers
    );
    // The full run measured: full offload, whatever -ngl the form holds now.
    for ngl in [20, 33, 99] {
        let r = fit_with(
            &at(ngl),
            Some(&meta),
            &budget,
            &[partial.clone(), fitted.clone()],
        );
        assert_eq!(r.recommended_gpu_layers, full, "-ngl {ngl}");
    }
    // The form's automatic slots (1, not unified) against the 2-slot launch: same context and
    // cache types, the correction carries the rest.
    let one_slot = FitInput {
        parallel_slots: Some(1),
        kv_unified: None,
        ..at(33)
    };
    let r = fit_with(
        &one_slot,
        Some(&meta),
        &budget,
        &[partial.clone(), fitted.clone()],
    );
    assert_eq!(r.recommended_gpu_layers, full);
    // Above 97% of the card (WDDM would spill): back to the estimate rule.
    let tight = run(99, 3900.0, 3);
    let r = fit_with(&at(99), Some(&meta), &budget, &[partial, tight]);
    assert!(r.recommended_gpu_layers < full);
    // Other cache types or context do not borrow the q8_0 16k measurement.
    let q4 = FitInput {
        cache_type_k: Some("q4_0".into()),
        cache_type_v: Some("q4_0".into()),
        ..at(33)
    };
    let c32k = FitInput {
        context_size: 32768,
        ..at(33)
    };
    for other in [q4, c32k] {
        assert!(measured_full_offload(&other, Some(&meta), total, &budget, &[&fitted]).is_none());
    }
}

fn gib4() -> MemoryBudget {
    MemoryBudget {
        vram_bytes: 3962 * 1024 * 1024,
        ram_bytes: 16 * 1024 * 1024 * 1024,
        unified: false,
        shared_igpu: false,
        igpu_offload_faster: None,
        cuda: true,
        rocm: false,
    }
}

#[test]
fn draft_model_adds_its_weights_and_kv_and_leaves_fewer_target_layers() {
    let budget = gib4();
    let target = FitInput {
        parallel_slots: Some(1),
        ..input(2_500 * 1024 * 1024, 8192, "f16")
    };
    let alone = fit_with(&target, None, &budget, &[]);
    let draft = |gpu_layers| FitInput {
        draft: Some(DraftFit {
            model_size_bytes: 610 * 1024 * 1024,
            gpu_layers,
            ..Default::default()
        }),
        ..target.clone()
    };
    let with = fit_with(&draft(None), None, &budget, &[]);
    let d = with.estimate.draft_gpu_bytes;
    // Weights plus a KV cache of the target's context.
    assert!(d > 610 * 1024 * 1024, "{}", mib(d));
    assert_eq!(with.estimate.vram_bytes, alone.estimate.vram_bytes + d);
    assert_eq!(with.estimate.draft_host_bytes, 0);
    assert!(with.recommended_gpu_layers < alone.recommended_gpu_layers);
    // A longer context grows the draft's cache too.
    let long = fit_with(
        &FitInput {
            context_size: 16384,
            ..draft(None)
        },
        None,
        &budget,
        &[],
    );
    assert!(long.estimate.draft_gpu_bytes > d);
    // Draft on the CPU (-ngld 0): nothing on the device, all in RAM.
    let cpu = fit_with(&draft(Some(0)), None, &budget, &[]);
    assert_eq!(cpu.estimate.draft_gpu_bytes, 0);
    assert!(cpu.estimate.draft_host_bytes > 610 * 1024 * 1024);
    assert_eq!(cpu.recommended_gpu_layers, alone.recommended_gpu_layers);
}

/// Real files: the draft's share next to what the OS measured (RTX 3050 Ti Laptop, b11333,
/// Qwen3-4B-Instruct-2507 Q4_K_M + Qwen3-0.6B Q8_0, -np 1, f16, all on the GPU): +862 MiB at
/// 2k, +1098 MiB at 4k (dedicated + shared, process counter, draft minus no draft).
/// `HS_TEST_MODEL=... HS_TEST_DRAFT_MODEL=... cargo test -p harness-core --lib real_draft_estimate -- --ignored --nocapture`
#[test]
#[ignore]
fn real_draft_estimate() {
    let (Ok(model), Ok(draft)) = (
        std::env::var("HS_TEST_MODEL"),
        std::env::var("HS_TEST_DRAFT_MODEL"),
    ) else {
        eprintln!("HS_TEST_MODEL / HS_TEST_DRAFT_MODEL not set, skipping");
        return;
    };
    let meta = crate::models::cached_metadata(std::path::Path::new(&model));
    for (ctx, measured_mib) in [(2048u32, 862.0), (4096, 1098.0)] {
        let i = complete_input(FitInput {
            model_path: Some(model.clone()),
            parallel_slots: Some(1),
            draft: Some(DraftFit {
                model_path: Some(draft.clone()),
                gpu_layers: Some(99),
                ..Default::default()
            }),
            ..input(0, ctx, "f16")
        });
        let r = fit_with(&i, meta.as_ref(), &gib4(), &[]);
        let est = mib(r.estimate.draft_gpu_bytes);
        eprintln!(
            "ctx {ctx}: draft estimate {est:.0} MiB, measured {measured_mib} MiB; target -ngl {}",
            r.recommended_gpu_layers
        );
        assert!((est - measured_mib).abs() / measured_mib < 0.15);
    }
}

/// GGUF header of unsloth Qwen3.5-2B / 4B (hybrid: gated delta net + every 4th layer attention)
/// and what llama.cpp b11456 CUDA really allocated (RTX 3050 Ti Laptop, `-lv 4`, 07.10.2026):
/// `llama_kv_cache` / `llama_memory_recurrent` buffer sizes and the `context checkpoint` size.
fn qwen35(layers: u64, embd: u64, heads: u64, kv_heads: u64, inner: u64) -> GgufMetadata {
    GgufMetadata {
        architecture: Some("qwen35".into()),
        context_length: Some(262_144),
        block_count: Some(layers),
        embedding_length: Some(embd),
        head_count: Some(heads),
        head_count_kv: Some(kv_heads),
        key_length: Some(256),
        value_length: Some(256),
        full_attention_interval: Some(4),
        ssm_conv_kernel: Some(4),
        ssm_state_size: Some(128),
        ssm_inner_size: Some(inner),
        ssm_group_count: Some(16),
        feed_forward_length: Some(embd * 3),
        ..Default::default()
    }
}

#[test]
fn hybrid_recurrent_state_and_kv_match_real_cuda_buffers() {
    // (model, layers, embd, heads, kv heads, ssm inner, np, ctx, KV MiB, RS MiB) measured.
    let cases = [
        ("2B", 24, 2048, 8, 2, 2048, 1, 16_384, 192.0, 19.27),
        ("2B", 24, 2048, 8, 2, 2048, 4, 16_384, 192.0, 77.06),
        ("2B", 24, 2048, 8, 2, 2048, 4, 32_768, 384.0, 77.06),
        ("4B", 32, 2560, 16, 4, 4096, 1, 16_384, 512.0, 50.25),
        ("4B", 32, 2560, 16, 4, 4096, 2, 32_768, 1024.0, 100.5),
        ("4B", 32, 2560, 16, 4, 4096, 4, 16_384, 512.0, 201.0),
    ];
    for (name, layers, embd, heads, kv_heads, inner, np, ctx, kv_mib, rs_mib) in cases {
        let meta = qwen35(layers, embd, heads, kv_heads, inner);
        let size = if name == "2B" {
            1_280_835_840
        } else {
            2_740_937_888
        };
        let est = estimate_raw(
            &FitInput {
                parallel_slots: Some(np),
                kv_unified: Some(np > 1),
                flash_attn: Some(true),
                ..input(size, ctx, "f16")
            },
            Some(&meta),
            true,
        );
        let kv = mib(est.kv_bytes - est.recurrent_bytes);
        let rs = mib(est.recurrent_bytes);
        assert!(
            (kv - kv_mib).abs() < 0.5,
            "{name} np{np} {ctx}: KV {kv:.2} vs {kv_mib}"
        );
        assert!(
            (rs - rs_mib).abs() < 0.1,
            "{name} np{np} {ctx}: RS {rs:.2} vs {rs_mib}"
        );
        // One context checkpoint is one slot's recurrent state (19.266 / 50.251 MiB measured);
        // the estimate keeps `--ctx-checkpoints` (4) of them per slot.
        let one = mib(est.checkpoint_bytes) / (np as f64 * 4.0);
        let measured = if name == "2B" { 19.266 } else { 50.251 };
        assert!((one - measured).abs() < 0.05, "{name}: checkpoint {one:.3}");
    }
}

#[test]
fn headroom_scales_with_the_card_and_never_drops_below_the_floor() {
    // 16 GiB card (S20): 8% = 1.28 GiB, above the old 512 MiB setting.
    assert_eq!(
        headroom_with(16 * GIB, 0, 0),
        16 * GIB / 100 * VRAM_HEADROOM_PERCENT
    );
    // Small card: the 1 GiB floor applies.
    assert_eq!(headroom_with(6 * GIB, 0, 0), MIN_VRAM_HEADROOM);
    // An explicit setting is used as given, larger or smaller.
    assert_eq!(headroom_with(16 * GIB, 3 * GIB, 0), 3 * GIB);
    assert_eq!(headroom_with(16 * GIB, 256 * MIB, 0), 256 * MIB);
    // The machine's own value is never below the generic one.
    assert!(memory_headroom(16 * GIB, 0) >= headroom_with(16 * GIB, 0, 0));
}

#[test]
fn amd_windows_driver_reserve_lifts_the_automatic_headroom_only() {
    // RX 7900 GRE 16 GB: the driver spilled once all processes held more than the
    // card minus ~2.1 GiB, though 8% (1.28 GiB) was left free.
    let reserve = AMD_WINDOWS_DRIVER_RESERVE;
    assert_eq!(headroom_with(16 * GIB, 0, reserve), reserve);
    assert!(reserve > 16 * GIB / 100 * VRAM_HEADROOM_PERCENT);
    // A big card keeps its percentage; an explicit setting still wins.
    assert_eq!(
        headroom_with(48 * GIB, 0, reserve),
        48 * GIB / 100 * VRAM_HEADROOM_PERCENT
    );
    assert_eq!(headroom_with(16 * GIB, GIB, reserve), GIB);
}

#[test]
fn fixed_footprint_servers_get_half_the_reserve_but_never_below_the_driver_reserve() {
    // 4 GB NVIDIA: 512 MiB instead of the 1 GiB floor.
    assert_eq!(fixed_footprint_headroom_with(4 * GIB, 0, 0), GIB / 2);
    // 16 GB AMD on Windows: the spill threshold stays whole.
    let amd = AMD_WINDOWS_DRIVER_RESERVE;
    assert_eq!(fixed_footprint_headroom_with(16 * GIB, 0, amd), amd);
    // A configured reserve is the user's.
    assert_eq!(fixed_footprint_headroom_with(4 * GIB, 2 * GIB, 0), 2 * GIB);
}

#[test]
fn a_full_offload_keeps_the_rules_own_margin_not_the_automatic_reserve() {
    // 4 GB card: the rule's 90% limit leaves 10% (about 410 MiB), not the 1 GiB floor on top.
    let h4 = full_offload_headroom_with(4096 * MIB, 0, 0);
    assert_eq!(h4 / MIB, 409);
    assert!(h4 < headroom_with(4096 * MIB, 0, 0));
    // Qwen3-4B at 12k / 37 layers (3537 MiB) is within what nvidia-smi leaves (3962 MiB free).
    assert_eq!(
        full_offload_need_limit_with(3962 * MIB, 4096 * MIB, 0, 0) / MIB,
        3962 - 410
    );
    assert!(3537 * MIB <= full_offload_need_limit_with(3962 * MIB, 4096 * MIB, 0, 0));
    assert!(3537 * MIB > full_offload_need_limit_with(3295 * MIB, 4096 * MIB, 0, 0));
    // 16 GB AMD on Windows: the driver reserve stays (2200 MiB, same as the automatic one).
    let amd = AMD_WINDOWS_DRIVER_RESERVE;
    assert_eq!(full_offload_headroom_with(16 * GIB, 0, amd), amd);
    assert_eq!(
        full_offload_headroom_with(16 * GIB, 0, amd),
        headroom_with(16 * GIB, 0, amd)
    );
    // 16 GB without a reserve: the 10% margin (1.6 GiB).
    assert_eq!(full_offload_headroom_with(16 * GIB, 0, 0) / MIB, 1638);
    // A configured headroom wins.
    assert_eq!(full_offload_headroom_with(4096 * MIB, GIB, 0), GIB);
    assert_eq!(
        full_offload_headroom_with(16 * GIB, 512 * MIB, amd),
        512 * MIB
    );
}
