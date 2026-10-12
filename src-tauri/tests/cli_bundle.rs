//! The `harness` CLI ships with the app: `tauri.conf.json` lists it as a sidecar
//! (`bundle.externalBin`), `build.rs` stages a matching build for every cargo build, and the
//! UI finds it next to the app executable (`cli_install::locate`). This checks the chain on
//! a dev build: config, staged file, the copy Tauri puts beside the app binary, and that the
//! lookup the UI uses finds and runs it. No installer is built.

use std::path::{Path, PathBuf};

use harness_core::integrations::cli_install::{locate_in, version, CliSource};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn app() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_harness-studio"))
}

fn external_bins() -> Vec<String> {
    let conf: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(manifest_dir().join("tauri.conf.json")).unwrap(),
    )
    .unwrap();
    conf["bundle"]["externalBin"]
        .as_array()
        .expect("bundle.externalBin")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn installer_config_lists_the_cli_sidecar() {
    let bins = external_bins();
    assert!(bins.contains(&"binaries/harness".to_string()), "{bins:?}");
    // The macOS override must not replace the list (arrays do not merge in Tauri configs).
    let mac = std::fs::read_to_string(manifest_dir().join("tauri.macos.conf.json")).unwrap();
    assert!(
        !mac.contains("externalBin"),
        "tauri.macos.conf.json would drop the sidecar"
    );
}

#[test]
fn every_sidecar_is_staged_for_this_target() {
    for bin in external_bins() {
        let name = bin.rsplit('/').next().unwrap();
        let staged: Vec<_> = std::fs::read_dir(manifest_dir().join("binaries"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|f| {
                f.strip_prefix(&format!("{name}-")).is_some_and(|rest| {
                    ["x86_64", "aarch64", "i686", "armv7"]
                        .iter()
                        .any(|a| rest.starts_with(a))
                })
            })
            .collect();
        assert!(
            !staged.is_empty(),
            "build.rs did not stage binaries/{name}-<triple>"
        );
    }
}

#[test]
fn the_ui_finds_and_runs_the_cli_next_to_the_app() {
    // `tauri_build` copies the sidecar (without the target triple) beside the app binary.
    let found = locate_in(Some(app()), None, None)
        .unwrap_or_else(|| panic!("no harness next to {}", app().display()));
    assert_eq!(found.source, CliSource::App);
    assert_eq!(found.path.parent(), app().parent());
    let v = version(&found.path).expect("the sidecar runs");
    assert!(v.starts_with("harness "), "{v}");
    eprintln!("{} -> {v}", found.path.display());
}
