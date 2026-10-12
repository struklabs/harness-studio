//! whisper.cpp as a runtime family: the real release flow.

use super::*;

fn temp_registry(tag: &str) -> (RuntimeRegistry, PathBuf) {
    let base = std::env::temp_dir().join(format!(
        "{}{}-{}",
        crate::paths::test_dir_prefix("whisper"),
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

#[test]
fn versions_come_from_the_release_tag() {
    assert_eq!(
        version_from_tag("b5130"),
        (Some("b5130".to_string()), Some(5130))
    );
    assert_eq!(
        version_from_tag("v1.9.2"),
        (Some("1.9.2".to_string()), None)
    );
    assert_eq!(version_from_tag(" "), (None, None));
}

#[test]
fn code_files_include_the_whisper_tools() {
    for name in [
        "whisper-server.exe",
        "whisper.dll",
        "ggml-cpu-haswell.dll",
        "SDL2.dll",
    ] {
        assert!(is_code_file(name), "{name}");
    }
    // Linux archives: executables have no extension.
    assert!(is_code_file("whisper-server"));
    assert!(!is_code_file("README.md"));
}

/// The real whisper.cpp release flow: preview, SHA-256-verified install of the CPU build (the
/// build for a machine without an NVIDIA GPU: the official releases have no Vulkan),
/// capability probe, verify, tamper detection, remove. Downloads ~8 MB.
/// `cargo test -p harness-core --lib e2e_whisper_cpp_install -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn e2e_whisper_cpp_install() {
    let (reg, base) = temp_registry("install");
    let query = super::github::ReleaseQuery {
        family: crate::engine::WHISPER_CPP.into(),
        repo: "ggml-org/whisper.cpp".into(),
        tag: None,
        asset: None,
        // Vulkan and ROCm have no official whisper.cpp build: the picker falls through to CPU
        // (and takes CUDA on an NVIDIA machine).
        backend: None,
    };
    let pick = super::github::find_release(&query).await.unwrap();
    eprintln!(
        "preview: {} {} backend {:?} assets {:?}",
        pick.repo,
        pick.tag,
        pick.backend,
        pick.assets
            .iter()
            .map(|a| (&a.name, a.size))
            .collect::<Vec<_>>()
    );
    assert_eq!(pick.family, "whisper.cpp");
    assert!(pick.unverified.is_empty(), "GitHub publishes digests");
    let started = std::time::Instant::now();
    let e = super::github::install(
        &reg,
        &super::github::GithubInstall {
            query: super::github::ReleaseQuery {
                // The tag the preview resolved, and the CPU build explicitly.
                tag: Some(pick.tag.clone()),
                backend: Some("cpu".into()),
                ..query
            },
            name: Some("whisper.cpp test".into()),
            id: Some("whisper-test".into()),
            allow_unverified: false,
        },
    )
    .await
    .unwrap();
    eprintln!(
        "installed in {:.1?}: id {} family {} version {:?} build {:?} backend {:?} trust {:?} files {}",
        started.elapsed(),
        e.id,
        e.family,
        e.version,
        e.build,
        e.backend,
        e.trust,
        e.files.len()
    );
    assert_eq!(e.family, "whisper.cpp");
    assert_eq!(e.backend.as_deref(), Some("cpu"));
    assert_eq!(e.trust, RuntimeTrust::Verified);
    assert_eq!(e.version, version_from_tag(&pick.tag).0);
    assert!(e.binary().is_file(), "{}", e.binary().display());
    assert!(e.files.iter().any(|f| f.name.starts_with("whisper-server")));

    let engine = e.engine().unwrap();
    let caps = engine.detect_capabilities(&e.binary());
    assert!(caps.help_parsed && caps.has_flag("--threads") && caps.has_flag("--vad"));
    eprintln!("capabilities: {} flags", caps.flags.len());

    let report = reg.verify(&e.id, false).unwrap();
    eprintln!("verify: {report:?}");
    assert!(report.ok && report.changed.is_empty() && report.missing.is_empty());
    assert_eq!(report.version, e.version);

    // A changed file is reported and refused at launch.
    let dll = e.dir.join("whisper.dll");
    let original = std::fs::read(&dll).unwrap();
    let mut changed = original.clone();
    changed[original.len() / 2] ^= 0xff;
    std::fs::write(&dll, changed).unwrap();
    let report = reg.verify(&e.id, false).unwrap();
    assert!(
        report.changed.contains(&"whisper.dll".to_string()),
        "{report:?}"
    );
    assert!(reg.launch_binary(&e.id).is_err());
    std::fs::write(&dll, original).unwrap();

    reg.remove(&e.id).unwrap();
    assert!(!e.dir.exists());
    let _ = std::fs::remove_dir_all(&base);
}
