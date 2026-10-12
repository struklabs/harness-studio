//! stable-diffusion.cpp as a runtime family: the real release flow.

use super::*;

fn temp_registry(tag: &str) -> (RuntimeRegistry, PathBuf) {
    let base = std::env::temp_dir().join(format!(
        "{}{}-{}",
        crate::paths::test_dir_prefix("sdcpp"),
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
fn versions_come_from_the_master_tag() {
    assert_eq!(
        version_from_tag("master-956-1b0ba10"),
        (Some("master-956-1b0ba10".to_string()), Some(956))
    );
}

#[test]
fn code_files_include_the_sd_tools() {
    for name in [
        "sd-server.exe",
        "sd-cli.exe",
        "stable-diffusion.dll",
        "ggml.dll",
    ] {
        assert!(is_code_file(name), "{name}");
    }
    assert!(is_code_file("sd-server"));
    assert!(!is_code_file("README.md"));
}

/// The real stable-diffusion.cpp release flow: preview, SHA-256-verified install of the Vulkan
/// build, capability probe, verify, tamper detection, remove. Downloads ~30 MB.
/// `cargo test -p harness-core --lib e2e_sdcpp_install -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn e2e_sdcpp_install() {
    let (reg, base) = temp_registry("install");
    let query = super::github::ReleaseQuery {
        family: crate::engine::SD_CPP.into(),
        repo: "leejet/stable-diffusion.cpp".into(),
        tag: None,
        asset: None,
        backend: Some("vulkan".into()),
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
    assert_eq!(pick.family, "stable-diffusion.cpp");
    assert!(pick.unverified.is_empty(), "GitHub publishes digests");
    let started = std::time::Instant::now();
    let e = super::github::install(
        &reg,
        &super::github::GithubInstall {
            query: super::github::ReleaseQuery {
                tag: Some(pick.tag.clone()),
                ..query
            },
            name: Some("stable-diffusion.cpp test".into()),
            id: Some("sdcpp-test".into()),
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
    assert_eq!(e.family, "stable-diffusion.cpp");
    assert_eq!(e.backend.as_deref(), Some("vulkan"));
    assert_eq!(e.trust, RuntimeTrust::Verified);
    assert_eq!(e.version, version_from_tag(&pick.tag).0);
    assert!(e.binary().is_file(), "{}", e.binary().display());
    assert!(e.files.iter().any(|f| f.name.starts_with("sd-server")));

    let engine = e.engine().unwrap();
    let caps = engine.detect_capabilities(&e.binary());
    assert!(
        caps.help_parsed && caps.has_flag("--diffusion-model") && caps.has_flag("--listen-port")
    );
    eprintln!("capabilities: {} flags", caps.flags.len());

    let report = reg.verify(&e.id, false).unwrap();
    assert!(report.ok && report.changed.is_empty() && report.missing.is_empty());

    // A changed file is reported and refused at launch.
    let dll = e.dir.join("ggml.dll");
    let original = std::fs::read(&dll).unwrap();
    let mut changed = original.clone();
    changed[original.len() / 2] ^= 0xff;
    std::fs::write(&dll, changed).unwrap();
    let report = reg.verify(&e.id, false).unwrap();
    assert!(
        report.changed.contains(&"ggml.dll".to_string()),
        "{report:?}"
    );
    assert!(reg.launch_binary(&e.id).is_err());
    std::fs::write(&dll, original).unwrap();

    reg.remove(&e.id).unwrap();
    assert!(!e.dir.exists());
    let _ = std::fs::remove_dir_all(&base);
}
