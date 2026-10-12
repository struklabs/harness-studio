use super::*;
use std::io::{Seek, Write};

fn temp_registry(tag: &str) -> (RuntimeRegistry, PathBuf) {
    let base = std::env::temp_dir().join(format!(
        "{}{}-{}",
        crate::paths::test_dir_prefix("runtimes"),
        tag,
        now_secs()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    (
        RuntimeRegistry::with_paths(base.join("config/runtimes.json"), base.join("bin/runtimes")),
        base,
    )
}

fn entry(reg: &RuntimeRegistry, id: &str, dir: PathBuf, source: RuntimeSource) -> RuntimeEntry {
    let e = RuntimeEntry {
        id: id.into(),
        name: id.into(),
        family: crate::engine::default_family(),
        source,
        files: hash_manifest(&dir).unwrap(),
        dir,
        version: None,
        build: None,
        commit: None,
        backend: None,
        trust: RuntimeTrust::LocalFiles,
        quant_types: Vec::new(),
        added_at: 1,
        verified_at: 1,
    };
    reg.save_entry(e.clone()).unwrap();
    e
}

fn legacy_q2_gguf(path: &Path) {
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(b"GGUF").unwrap();
    f.write_all(&3u32.to_le_bytes()).unwrap();
    f.write_all(&2u64.to_le_bytes()).unwrap();
    f.write_all(&1u64.to_le_bytes()).unwrap();
    let key = "general.architecture";
    f.write_all(&(key.len() as u64).to_le_bytes()).unwrap();
    f.write_all(key.as_bytes()).unwrap();
    f.write_all(&8u32.to_le_bytes()).unwrap();
    f.write_all(&5u64.to_le_bytes()).unwrap();
    f.write_all(b"llama").unwrap();
    for (name, elements, ggml_type, offset) in [
        ("blk.0.attn_q.weight", 4096u64, 42u32, 0u64),
        ("output_norm.weight", 8, 0, 1088),
    ] {
        f.write_all(&(name.len() as u64).to_le_bytes()).unwrap();
        f.write_all(name.as_bytes()).unwrap();
        f.write_all(&1u32.to_le_bytes()).unwrap();
        f.write_all(&elements.to_le_bytes()).unwrap();
        f.write_all(&ggml_type.to_le_bytes()).unwrap();
        f.write_all(&offset.to_le_bytes()).unwrap();
    }
    let pos = f.stream_position().unwrap();
    let data = pos.div_ceil(32) * 32;
    f.write_all(&vec![0; (data - pos + 1120) as usize]).unwrap();
}

#[test]
fn advice_from_synthetic_header_requires_prism_b9601() {
    let (reg, base) = temp_registry("legacy-q2");
    let model = base.join("Legacy-Q2_0.gguf");
    legacy_q2_gguf(&model);
    let mut manager = RuntimeManager::default();
    manager.base_bin_dir = Some(base.join("bin"));
    let settings = AppSettings {
        runtime_mode: crate::config::RuntimeMode::Custom,
        custom_binary: crate::config::CustomBinaryConfig::default(),
        ..Default::default()
    };

    let advice = advise_in(&reg, &manager, &settings, &model, None).unwrap();
    assert!(advice.official_unsupported);
    assert_eq!(
        advice.special_types,
        [quant::TensorType::of(quant::LEGACY_PRISM_Q2_0)]
    );
    assert_eq!(advice.fork.as_ref().unwrap().id, "prismml-legacy");

    let legacy = RuntimeEntry {
        id: "prism-v5".into(),
        name: "Prism v5".into(),
        family: crate::engine::default_family(),
        source: RuntimeSource::Github {
            repo: "PrismML-Eng/llama.cpp".into(),
            tag: "prism-b9601".into(),
            assets: Vec::new(),
        },
        dir: base.join("legacy"),
        version: Some("prism b9601".into()),
        build: Some(9601),
        commit: None,
        backend: Some("cpu".into()),
        trust: RuntimeTrust::Verified,
        files: Vec::new(),
        quant_types: vec!["Q2_0".into()],
        added_at: 1,
        verified_at: 1,
    };
    reg.save_entry(legacy).unwrap();
    let advice = advise_in(&reg, &manager, &settings, &model, None).unwrap();
    assert_eq!(advice.suggested_runtime.as_deref(), Some("prism-v5"));
    assert!(advice.fork.is_none());
}

#[test]
fn ids_and_slugs() {
    assert_eq!(
        slug("PrismML llama.cpp (Bonsai)"),
        "prismml-llama-cpp-bonsai"
    );
    assert_eq!(slug("  "), "runtime");
    assert_eq!(slug("Official"), "runtime");
    assert_eq!(slug("System"), "runtime");
    assert!(validate_id("prismml").is_ok());
    assert!(validate_id("b11026-winget").is_ok());
    for bad in [
        "",
        "official",
        "system",
        "-x",
        "A",
        "a b",
        "a/b",
        &"x".repeat(41),
    ] {
        assert!(validate_id(bad).is_err(), "{bad}");
    }
    assert!(is_official(None));
    assert!(is_official(Some("")));
    assert!(is_official(Some("official")));
    assert!(!is_official(Some("prismml")));
}

#[test]
fn manifest_detects_changes_and_remove_keeps_foreign_folders() {
    let (reg, base) = temp_registry("manifest");
    // A local folder used in place (never deleted by Harness).
    let local = base.join("my-llama");
    std::fs::create_dir_all(&local).unwrap();
    std::fs::write(local.join(binary_name()), b"not really a binary").unwrap();
    std::fs::write(local.join("ggml-base.dll"), b"lib").unwrap();
    std::fs::write(local.join("README.md"), b"docs are not hashed").unwrap();
    let e = entry(
        &reg,
        "mine",
        local.clone(),
        RuntimeSource::Local {
            path: local.clone(),
            copied: false,
        },
    );
    let names: Vec<&str> = e.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["ggml-base.dll", binary_name()]);
    assert!(changed_files(&e).is_empty());
    assert_eq!(
        reg.launch_binary("mine").unwrap(),
        local.join(binary_name())
    );

    // A changed library blocks the launch until the runtime is verified again.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(local.join("ggml-base.dll"), b"tampered library").unwrap();
    let err = reg.launch_binary("mine").unwrap_err();
    assert!(err.starts_with(ERR_CHANGED), "{err}");
    // verify reports it (the fake binary doesn't run, so nothing is accepted).
    let report = reg.verify("mine", true).unwrap();
    assert!(!report.ok);
    assert_eq!(report.changed, ["ggml-base.dll"]);
    assert!(report.error.is_some());

    // Ids are unique; the official id is reserved.
    assert!(reg
        .unique_id(Some("mine"), "x")
        .unwrap_err()
        .starts_with(ERR_ID_TAKEN));
    assert_eq!(reg.unique_id(None, "mine").unwrap(), "mine-2");
    assert!(reg.remove(OFFICIAL_ID).is_err());

    // Removing an in-place runtime keeps the user's folder.
    reg.remove("mine").unwrap();
    assert!(local.join(binary_name()).is_file());
    assert!(reg.get("mine").unwrap_err().starts_with(ERR_NOT_FOUND));

    // A folder Harness created (copy/download under the root) is deleted with it.
    let owned = reg.root().join("copied");
    std::fs::create_dir_all(&owned).unwrap();
    std::fs::write(owned.join(binary_name()), b"x").unwrap();
    entry(
        &reg,
        "copied",
        owned.clone(),
        RuntimeSource::Local {
            path: local.clone(),
            copied: true,
        },
    );
    reg.remove("copied").unwrap();
    assert!(!owned.exists());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn add_local_refuses_folders_without_a_server() {
    let (reg, base) = temp_registry("nosrv");
    let err = reg
        .add_local(&AddLocal {
            path: base.clone(),
            name: "x".into(),
            id: None,
            copy: false,
            family: crate::engine::default_family(),
        })
        .unwrap_err();
    assert!(err.starts_with(ERR_NO_SERVER), "{err}");
    assert!(reg.entries().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn launch_layer_carries_the_runtime() {
    use crate::domain::launch::{apply_layer, config_layer, RUNTIME};
    let mut c = crate::process::LaunchConfig {
        runtime: Some("prismml".into()),
        ..Default::default()
    };
    let layer = config_layer(&c);
    assert_eq!(
        layer.get(RUNTIME),
        Some(&crate::params::ParamValue::Text("prismml".into()))
    );
    // "official" is the default and never stored.
    c.runtime = Some(OFFICIAL_ID.into());
    assert!(!config_layer(&c).contains_key(RUNTIME));
    // An unset marker (a profile going back to the official runtime) clears it.
    let mut back = crate::process::LaunchConfig::default();
    apply_layer(&mut back, &layer).unwrap();
    assert_eq!(back.runtime.as_deref(), Some("prismml"));
    let mut clear = crate::params::ParamValues::new();
    clear.insert(
        RUNTIME.into(),
        crate::params::ParamValue::Text(String::new()),
    );
    apply_layer(&mut back, &clear).unwrap();
    assert_eq!(back.runtime, None);
}

/// Real builds. Run with
/// `HS_RUNTIME_DIRS="<dir A>;<dir B>" cargo test -p harness-core runtimes::tests::real -- --ignored`
/// (read-only: each folder is copied into a temp registry).
#[test]
#[ignore]
fn real_local_runtimes_register_and_differ() {
    let Ok(dirs) = std::env::var("HS_RUNTIME_DIRS") else {
        eprintln!("HS_RUNTIME_DIRS not set, skipping");
        return;
    };
    let (reg, base) = temp_registry("real");
    let mut builds = Vec::new();
    for (i, dir) in dirs.split(';').filter(|d| !d.is_empty()).enumerate() {
        let e = reg
            .add_local(&AddLocal {
                path: dir.into(),
                name: format!("rt {}", i),
                id: None,
                copy: true,
                family: crate::engine::default_family(),
            })
            .unwrap();
        assert!(reg.owns_dir(&e));
        assert!(e.build.is_some(), "{:?}", e.version);
        assert!(e.files.iter().any(|f| f.name == binary_name()));
        let report = reg.verify(&e.id, false).unwrap();
        assert!(report.ok, "{report:?}");
        builds.push((e.id.clone(), e.build, e.quant_types.clone()));
        eprintln!(
            "{} -> {:?} backend {:?} quants {}",
            e.id,
            e.version,
            e.backend,
            e.quant_types.len()
        );
    }
    assert!(builds.len() >= 2);
    assert_ne!(builds[0].1, builds[1].1, "two different builds expected");
    for (id, _, _) in &builds {
        reg.remove(id).unwrap();
    }
    let _ = std::fs::remove_dir_all(&base);
}

/// The quant check against a real Bonsai 2 file and real builds. Run with
/// `HS_BONSAI_MODEL=<Ternary-Bonsai-2-27B-PQ2_0.gguf> HS_LLAMA_SERVER=<official llama-server>
/// HS_FORK_DIR=<PrismML build folder> cargo test -p harness-core runtimes::tests::real_bonsai -- --ignored`
#[test]
#[ignore]
fn real_bonsai_needs_the_fork() {
    let (Ok(model), Ok(official), Ok(fork)) = (
        std::env::var("HS_BONSAI_MODEL"),
        std::env::var("HS_LLAMA_SERVER"),
        std::env::var("HS_FORK_DIR"),
    ) else {
        eprintln!("HS_BONSAI_MODEL / HS_LLAMA_SERVER / HS_FORK_DIR not set, skipping");
        return;
    };
    let meta = crate::gguf::read_gguf_metadata(Path::new(&model)).unwrap();
    eprintln!("tensor types: {:?}", meta.tensor_types);
    let special = quant::special_types(&meta.tensor_types);
    assert!(
        special
            .iter()
            .any(|t| t.name.as_deref() == Some("PQ2_0") || t.name.as_deref() == Some("PTQ1_0")),
        "{special:?}"
    );

    let official_info = quant::RuntimeQuantInfo {
        quant_types: quant::detect_quant_types(Path::new(&official)),
        official: true,
        legacy_prism_q2_0: false,
    };
    assert_eq!(
        quant::model_support(&official_info, &meta.tensor_types).0,
        quant::Support::Unsupported
    );
    let fork_types = quant::detect_quant_types(&Path::new(&fork).join(binary_name())).unwrap();
    assert!(fork_types.iter().any(|t| t == "PQ2_0"), "{fork_types:?}");
    let fork_info = quant::RuntimeQuantInfo {
        quant_types: Some(fork_types),
        official: false,
        legacy_prism_q2_0: false,
    };
    assert_eq!(
        quant::model_support(&fork_info, &meta.tensor_types).0,
        quant::Support::Supported
    );

    // Advice with an empty registry points at the fork; with the fork registered it names it.
    let (reg, base) = temp_registry("bonsai");
    let settings = AppSettings {
        runtime_mode: crate::config::RuntimeMode::Custom,
        custom_binary: crate::config::CustomBinaryConfig {
            binary_path: Some(official.clone().into()),
            is_valid: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mgr = RuntimeManager::default();
    let advice = advise_in(&reg, &mgr, &settings, Path::new(&model), None).unwrap();
    assert!(advice.official_unsupported);
    assert!(advice.suggested_runtime.is_none());
    assert_eq!(advice.fork.as_ref().unwrap().repo, "PrismML-Eng/llama.cpp");
    let err = check_model_support(&reg, &mgr, &settings, None, Path::new(&model)).unwrap_err();
    assert!(err.starts_with(ERR_QUANT_UNSUPPORTED), "{err}");
    eprintln!("launch guard: {err}");

    let e = reg
        .add_local(&AddLocal {
            path: fork.clone().into(),
            name: "PrismML llama.cpp (Bonsai)".into(),
            id: Some("prismml".into()),
            copy: false,
            family: crate::engine::default_family(),
        })
        .unwrap();
    assert!(!reg.owns_dir(&e));
    let advice = advise_in(&reg, &mgr, &settings, Path::new(&model), None).unwrap();
    assert_eq!(advice.suggested_runtime.as_deref(), Some("prismml"));
    assert!(advice.fork.is_none());
    check_model_support(&reg, &mgr, &settings, Some("prismml"), Path::new(&model)).unwrap();
    reg.remove("prismml").unwrap();
    assert!(
        Path::new(&fork).join(binary_name()).is_file(),
        "in-place folder kept"
    );
    let _ = std::fs::remove_dir_all(&base);
}

/// Per-server runtime choice against real builds: three servers of the same model, each
/// resolved through definition -> profile layering to a different llama-server build, run
/// side by side on CPU (`-ngl 0`). Run with
/// `HS_RUNTIME_A=<dir> HS_RUNTIME_B=<dir> HS_FORK_DIR=<dir> HS_TEST_MODEL=<gguf>
/// cargo test -p harness-core runtimes::tests::e2e_per_server -- --ignored --nocapture`
/// (the folders are only read; ports 19901-19903).
#[tokio::test]
#[ignore]
async fn e2e_per_server_runtime_launch() {
    use crate::domain::launch::{resolve_launch, LaunchDefaults, RUNTIME};
    use crate::domain::{ModelDefinition, ModelRef, ServerProfile};
    use crate::params::{ParamValue, ParamValues};
    use crate::process::ServerStatus;
    let (Ok(a), Ok(b), Ok(fork), Ok(model)) = (
        std::env::var("HS_RUNTIME_A"),
        std::env::var("HS_RUNTIME_B"),
        std::env::var("HS_FORK_DIR"),
        std::env::var("HS_TEST_MODEL"),
    ) else {
        eprintln!("HS_RUNTIME_A / HS_RUNTIME_B / HS_FORK_DIR / HS_TEST_MODEL not set, skipping");
        return;
    };
    let reg = RuntimeRegistry::global();
    let mut builds = std::collections::BTreeMap::new();
    for (id, dir) in [("e2e-a", &a), ("e2e-b", &b), ("e2e-fork", &fork)] {
        let _ = reg.remove(id);
        let e = reg
            .add_local(&AddLocal {
                path: dir.into(),
                name: id.into(),
                id: Some(id.into()),
                copy: false,
                family: crate::engine::default_family(),
            })
            .unwrap();
        eprintln!("{id}: {:?} ({:?})", e.version, e.backend);
        builds.insert(id, e.build.unwrap());
    }

    // Capability gating follows the binary: the catalog parameters each build lacks.
    let mgr = RuntimeManager::default();
    let settings = AppSettings::load();
    let mut hidden = std::collections::BTreeMap::new();
    for id in ["e2e-a", "e2e-b", "e2e-fork"] {
        let v = view_in(&reg, &mgr, &settings, id).unwrap();
        assert!(v.available && v.help_parsed, "{v:?}");
        eprintln!(
            "{id}: {} flags, hidden params {:?}",
            v.flag_count, v.unsupported_params
        );
        hidden.insert(id, v.unsupported_params);
    }
    // The catalog parameters exist in all three builds; the fork (b10709) lacks newer flags
    // (the "all flags" explorer and custom-argument checks use these per-binary lists) and
    // differs in enum values the catalog reads from --help.
    let caps_of =
        |id: &str| crate::capabilities::registry().get_or_probe(&reg.get(id).unwrap().binary());
    let (official, forked, older) = (caps_of("e2e-a"), caps_of("e2e-fork"), caps_of("e2e-b"));
    for flag in ["--kv-unified-per-slot", "--n-cpu-ffn", "--lazy-mode"] {
        assert!(official.has_flag(flag), "{flag}");
        assert!(!forked.has_flag(flag), "{flag}");
    }
    assert!(forked.has_flag("--kv-mean-center") && !official.has_flag("--kv-mean-center"));
    let names = |c: &crate::capabilities::RuntimeCapabilities| {
        c.flags
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
    };
    eprintln!(
        "only in b11236 vs fork: {:?}",
        names(&official)
            .difference(&names(&forked))
            .collect::<Vec<_>>()
    );
    eprintln!(
        "only in b11236 vs b11026: {:?}",
        names(&official)
            .difference(&names(&older))
            .collect::<Vec<_>>()
    );
    for spec in crate::params::catalog()
        .params
        .iter()
        .filter(|p| p.values_from_help)
    {
        let vals = |c: &crate::capabilities::RuntimeCapabilities| {
            spec.flags
                .iter()
                .find_map(|f| c.flag(f))
                .map(|i| i.enum_values.clone())
                .unwrap_or_default()
        };
        if vals(&official) != vals(&forked) {
            eprintln!(
                "{}: b11236 {:?} / fork {:?}",
                spec.id,
                vals(&official),
                vals(&forked)
            );
        }
    }

    // Definition picks runtime B; profile 1 overrides it with A, profile 2 keeps B, profile 3
    // overrides it with the fork.
    let mut def = ModelDefinition::new(
        "qwen3-4b-rt",
        ModelRef::Local {
            path: model.clone().into(),
        },
    );
    def.id = "md-rt".into();
    def.load
        .insert(RUNTIME.into(), ParamValue::Text("e2e-b".into()));
    let defaults = LaunchDefaults {
        gpu_layers: 0,
        context_size: 1024,
        host: "127.0.0.1".into(),
        port: 19900,
        models_dir: std::env::temp_dir(),
    };
    let profile = |id: &str, port: u16, runtime: Option<&str>| {
        let mut overrides = ParamValues::new();
        if let Some(r) = runtime {
            overrides.insert(RUNTIME.into(), ParamValue::Text(r.into()));
        }
        ServerProfile {
            id: id.into(),
            name: id.into(),
            purpose: String::new(),
            definition_id: Some(def.id.clone()),
            router: None,
            host: "127.0.0.1".into(),
            port,
            port_auto: false,
            auto_start: false,
            api_key_ref: None,
            serve_overrides: overrides,
            created_at: 0,
            updated_at: 0,
        }
    };
    let servers = [
        (profile("rt-srv-1", 19901, Some("e2e-a")), "e2e-a"),
        (profile("rt-srv-2", 19902, None), "e2e-b"),
        (profile("rt-srv-3", 19903, Some("e2e-fork")), "e2e-fork"),
    ];
    let registry = crate::servers::ServerRegistry::default();
    let gate = crate::operation_gate::OperationGate::default();
    for (p, want) in &servers {
        let config = resolve_launch(&defaults, Some(&def), Some(p), &ParamValues::new()).unwrap();
        assert_eq!(config.runtime.as_deref(), Some(*want));
        crate::orchestrator::launch(&registry, &mgr, &gate, p.id.clone(), config)
            .await
            .unwrap();
    }
    let t = std::time::Instant::now();
    loop {
        let running = servers
            .iter()
            .filter(|(p, _)| registry.get(&p.id).unwrap().get_status() == ServerStatus::Running)
            .count();
        if running == servers.len() {
            break;
        }
        for (p, _) in &servers {
            if let ServerStatus::Error(e) = registry.get(&p.id).unwrap().get_status() {
                registry.stop_all().await;
                panic!("{}: {}", p.id, e);
            }
        }
        assert!(t.elapsed() < std::time::Duration::from_secs(180));
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    let http = reqwest::Client::new();
    for (p, want) in &servers {
        let pm = registry.get(&p.id).unwrap();
        let used = pm.launch_config().unwrap().binary_path;
        assert_eq!(used, reg.get(want).unwrap().binary(), "{}", p.id);
        let props: serde_json::Value = http
            .get(format!("http://127.0.0.1:{}/props", p.port))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let build_info = props["build_info"].as_str().unwrap_or_default().to_string();
        eprintln!(
            "{} :{} -> {} ({})",
            p.id,
            p.port,
            build_info,
            used.display()
        );
        assert!(
            build_info.contains(&format!("b{}", builds[want])),
            "{}: {build_info}",
            p.id
        );
    }
    registry.stop_all().await;
    for id in ["e2e-a", "e2e-b", "e2e-fork"] {
        reg.remove(id).unwrap();
    }
}

/// The official runtime reports what it really runs: with "managed" configured but no managed
/// build installed it falls back to a system llama-server (PATH/WinGet) and says so; a staged
/// managed build is reported and, once activated, used. Run with
/// `HS_STAGING_DIR=<a staged llama.cpp folder> cargo test -p harness-core
/// runtimes::tests::e2e_official_state -- --ignored --nocapture` (the folder is copied).
#[test]
#[ignore]
fn e2e_official_state_reports_fallback_and_staged() {
    use crate::orchestrator::BinarySource;
    let Ok(staging_src) = std::env::var("HS_STAGING_DIR") else {
        eprintln!("HS_STAGING_DIR not set, skipping");
        return;
    };
    let (_, base) = temp_registry("official");
    let mut mgr = RuntimeManager::default();
    mgr.base_bin_dir = Some(base.join("bin"));
    copy_top_level(Path::new(&staging_src), &mgr.staging_dir()).unwrap();
    let settings = AppSettings::default(); // runtime_mode: Managed, nothing installed
    let before = official_state(&mgr, &settings);
    eprintln!("before: {before:?}");
    assert_eq!(before.configured_mode, "managed");
    assert!(!before.managed_installed);
    let staged_build = before.staged.as_ref().and_then(|v| v.build);
    assert!(staged_build.is_some(), "{before:?}");
    match RuntimeManager::detect_system_binary_cached() {
        Some(sys) => {
            assert_eq!(before.source, Some(BinarySource::System));
            assert!(before.fallback, "managed configured, system used");
            eprintln!("fallback binary: {}", sys.display());
        }
        None => assert_eq!(before.source, None),
    }

    // Activate the staged build (what Settings > Runtime's button does).
    mgr.verify_stage1_binary().unwrap();
    mgr.activate_runtime(None).unwrap();
    let after = official_state(&mgr, &settings);
    eprintln!("after: {after:?}");
    assert_eq!(after.source, Some(BinarySource::Managed));
    assert!(!after.fallback);
    assert!(after.managed_installed);
    assert!(after.staged.is_none());
    let resolved = crate::orchestrator::resolve_server_binary(&mgr, &settings).unwrap();
    assert_eq!(resolved, mgr.managed_dir().join(binary_name()));
    let _ = std::fs::remove_dir_all(&base);
}

/// Two real CUDA builds from GitHub share one CUDA runtime archive; removing one keeps the
/// other working, removing both frees the store. Downloads ~1 GB.
/// `HS_CUDA_TAGS=b11327,b11320 cargo test -p harness-core --lib e2e_cuda_builds_share -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn e2e_cuda_builds_share_the_cuda_runtime() {
    let Ok(tags) = std::env::var("HS_CUDA_TAGS") else {
        eprintln!("HS_CUDA_TAGS not set, skipping");
        return;
    };
    let tags: Vec<&str> = tags.split(',').map(str::trim).collect();
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let reg = RuntimeRegistry::with_paths(tmp.path().join("runtimes.json"), bin.join("runtimes"));
    let store = reg.shared_store();
    let mut entries = Vec::new();
    for tag in &tags[..2] {
        let started = std::time::Instant::now();
        let e = super::github::install(
            &reg,
            &super::github::GithubInstall {
                query: super::github::ReleaseQuery {
                    family: crate::engine::default_family(),
                    repo: "ggml-org/llama.cpp".into(),
                    tag: Some(tag.to_string()),
                    asset: None,
                    backend: Some("cuda".into()),
                },
                name: None,
                id: Some(format!("cuda-{tag}")),
                allow_unverified: false,
            },
        )
        .await
        .unwrap();
        eprintln!("{tag}: {:?} in {:.1?}", e.backend, started.elapsed());
        entries.push(e);
    }
    let archives: Vec<_> = std::fs::read_dir(store.root())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    assert_eq!(archives.len(), 1, "{archives:?}");
    for f in std::fs::read_dir(&archives[0]).unwrap().flatten() {
        let links = crate::shared_libs::link_count(&f.path());
        eprintln!(
            "  {:?}: {} MB, {links:?} links",
            f.file_name(),
            f.metadata().unwrap().len() >> 20
        );
        assert_eq!(links, Some(3));
    }

    reg.remove(&entries[0].id).unwrap();
    assert!(archives[0].exists());
    let out = std::process::Command::new(reg.launch_binary(&entries[1].id).unwrap())
        .arg("--list-devices")
        .output()
        .unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    eprintln!("{text}");
    assert!(text.contains("CUDA0"), "{text}");

    reg.remove(&entries[1].id).unwrap();
    assert!(!archives[0].exists());
}

/// `family` is optional in older files (llama.cpp) and an entry of a family this build does
/// not know is kept as it is, not dropped or rewritten.
#[test]
fn families_default_to_llama_and_unknown_ones_survive() {
    let (reg, base) = temp_registry("family");
    let file = base.join("config/runtimes.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let whisper_dir = base.join("whisper");
    std::fs::create_dir_all(base.join("old")).unwrap();
    std::fs::write(
        &file,
        serde_json::json!({
            "schema_version": 1,
            "runtimes": [
                {
                    "id": "old", "name": "old",
                    "source": { "kind": "local", "path": base.join("old"), "copied": false },
                    "dir": base.join("old"), "trust": "local_files"
                },
                {
                    "id": "w", "name": "sd", "family": "future-engine.cpp",
                    "source": { "kind": "local", "path": whisper_dir, "copied": false },
                    "dir": whisper_dir, "trust": "local_files"
                }
            ]
        })
        .to_string(),
    )
    .unwrap();
    let entries = reg.entries().unwrap();
    assert_eq!(entries[0].family, "llama.cpp");
    assert_eq!(entries[0].engine().unwrap().family(), "llama.cpp");
    assert_eq!(entries[1].family, "future-engine.cpp");
    assert!(entries[1].engine().is_none());

    // Saving another entry rewrites the file; the unknown family is still there.
    entry(&reg, "x", base.join("old"), entries[0].source.clone());
    let again = reg.get("w").unwrap();
    assert_eq!(again.family, "future-engine.cpp");
    let raw = std::fs::read_to_string(&file).unwrap();
    assert!(raw.contains("\"family\": \"llama.cpp\"") || raw.contains("\"family\":\"llama.cpp\""));

    let err = reg
        .add_local(&AddLocal {
            path: base.clone(),
            name: "x".into(),
            id: None,
            copy: false,
            family: "nope.cpp".into(),
        })
        .unwrap_err();
    assert!(err.starts_with(ERR_UNKNOWN_FAMILY), "{err}");
}
