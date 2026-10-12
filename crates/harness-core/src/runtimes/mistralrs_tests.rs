//! mistral.rs as a runtime family: a local folder only.

use super::*;

fn temp_registry(tag: &str) -> (RuntimeRegistry, PathBuf) {
    let base = std::env::temp_dir().join(format!(
        "{}{}-{}",
        crate::paths::test_dir_prefix("mistralrs"),
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

fn add(reg: &RuntimeRegistry, path: &Path) -> Result<RuntimeEntry, String> {
    reg.add_local(&AddLocal {
        path: path.to_path_buf(),
        name: String::new(),
        id: None,
        copy: false,
        family: crate::engine::MISTRALRS.into(),
    })
}

#[test]
fn the_family_is_known_and_has_no_release_download() {
    let e = crate::engine::engine(crate::engine::MISTRALRS).unwrap();
    let source = e.release_source();
    assert_eq!(source.binary, "mistralrs");
    assert!(source.repo.is_empty(), "no managed download");
    assert_eq!(
        source.binary_file(),
        if cfg!(windows) {
            "mistralrs.exe"
        } else {
            "mistralrs"
        }
    );
    assert!(e.pick_release_assets(&[], &["cuda".to_string()]).is_none());
    assert!(e.as_server().is_some());
}

#[test]
fn code_files_include_the_mistralrs_binary() {
    assert!(is_code_file("mistralrs.exe"));
    assert!(is_code_file("mistralrs"));
    assert!(!is_code_file("mistralrs.d"));
    assert!(!is_code_file("mistralrs.pdb"));
}

#[test]
fn version_text_comes_from_the_clap_line() {
    let e = crate::engine::engine(crate::engine::MISTRALRS).unwrap();
    assert_eq!(
        e.version_text("mistralrs 0.7.0\n").as_deref(),
        Some("mistralrs 0.7.0")
    );
    assert_eq!(e.version_text("garbage"), None);
    // The other families keep the release tag.
    let whisper = crate::engine::engine(crate::engine::WHISPER_CPP).unwrap();
    assert_eq!(whisper.version_text("mistralrs 0.7.0"), None);
}

#[test]
fn a_folder_without_the_binary_or_with_a_broken_one_is_refused() {
    let (reg, base) = temp_registry("refuse");
    let dir = base.join("target").join("release");
    std::fs::create_dir_all(&dir).unwrap();
    // No binary at all (a llama-server next to it does not count).
    std::fs::write(dir.join(binary_name()), b"x").unwrap();
    let e = add(&reg, &dir).unwrap_err();
    assert!(e.starts_with(ERR_NO_SERVER), "{e}");
    // A file with the right name that does not run.
    let bin = dir.join(
        crate::engine::engine(crate::engine::MISTRALRS)
            .unwrap()
            .release_source()
            .binary_file(),
    );
    std::fs::write(&bin, b"not a program").unwrap();
    let e = add(&reg, &dir).unwrap_err();
    assert!(e.starts_with(ERR_NOT_RUNNABLE), "{e}");
    // Picking the binary itself means its folder.
    let e = add(&reg, &bin).unwrap_err();
    assert!(e.starts_with(ERR_NOT_RUNNABLE), "{e}");
    assert!(reg.entries().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn launches_resolve_only_a_registered_mistralrs_runtime() {
    let (reg, base) = temp_registry("resolve");
    let e = resolve_family_binary_in(&reg, crate::engine::MISTRALRS, None).unwrap_err();
    assert!(e.starts_with(ERR_FAMILY_NOT_INSTALLED), "{e}");
    let dir = base.join("release");
    std::fs::create_dir_all(&dir).unwrap();
    let bin_name = crate::engine::engine(crate::engine::MISTRALRS)
        .unwrap()
        .release_source()
        .binary_file();
    std::fs::write(dir.join(&bin_name), b"bin").unwrap();
    let entry = RuntimeEntry {
        id: "mistralrs".into(),
        name: "mistral.rs".into(),
        family: crate::engine::MISTRALRS.into(),
        source: RuntimeSource::Local {
            path: dir.clone(),
            copied: false,
        },
        files: hash_manifest(&dir).unwrap(),
        dir: dir.clone(),
        version: Some("mistralrs 0.7.0".into()),
        build: None,
        commit: None,
        backend: None,
        trust: RuntimeTrust::LocalFiles,
        quant_types: Vec::new(),
        added_at: 1,
        verified_at: 1,
    };
    assert!(entry.files.iter().any(|f| f.name == bin_name));
    reg.save_entry(entry.clone()).unwrap();
    assert_eq!(entry.binary(), dir.join(&bin_name));
    assert_eq!(
        resolve_family_binary_in(&reg, crate::engine::MISTRALRS, None).unwrap(),
        dir.join(&bin_name)
    );
    assert_eq!(
        engine_for_in(&reg, Some("mistralrs")).unwrap().family(),
        crate::engine::MISTRALRS
    );
    // A mistral.rs runtime chosen for a whisper model is a mismatch.
    let e =
        resolve_family_binary_in(&reg, crate::engine::WHISPER_CPP, Some("mistralrs")).unwrap_err();
    assert!(e.starts_with(ERR_FAMILY_MISMATCH), "{e}");
    reg.remove("mistralrs").unwrap();
    assert!(dir.exists(), "a folder used in place is never deleted");
    let _ = std::fs::remove_dir_all(&base);
}

/// A stand-in binary that prints clap's version line registers with that version.
#[cfg(unix)]
#[test]
fn a_running_binary_registers_with_its_version() {
    use std::os::unix::fs::PermissionsExt;
    let (reg, base) = temp_registry("add");
    let dir = base.join("release");
    std::fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("mistralrs");
    std::fs::write(&bin, "#!/bin/sh\necho 'mistralrs 0.7.0'\n").unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let e = add(&reg, &dir).unwrap();
    assert_eq!(e.family, crate::engine::MISTRALRS);
    assert_eq!(e.name, "mistral.rs");
    assert_eq!(e.version.as_deref(), Some("mistralrs 0.7.0"));
    assert!(e.files.iter().any(|f| f.name == "mistralrs"));
    let report = reg.verify(&e.id, false).unwrap();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.version.as_deref(), Some("mistralrs 0.7.0"));
    let _ = std::fs::remove_dir_all(&base);
}
