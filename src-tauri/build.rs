//! Builds the `harness` CLI and stages it as the Tauri sidecar (`bundle.externalBin`:
//! `binaries/harness-<target-triple>[.exe]`), together with `harness-hidden` (the launcher
//! the Windows logon task of `harness service install` starts; a plain pass-through
//! elsewhere) and `harness-onnx` (the embeddings / rerank server for Hugging Face ONNX
//! models; it loads the managed ONNX Runtime library at run time, see `harness_core::onnx_runtime`). `tauri_build` requires the file to exist for every
//! cargo build (and copies it next to the app executable), so it is produced here rather than by
//! a separate script: `cargo build`, `tauri dev` and `tauri build` all get a matching CLI.
//!
//! The CLI is built into its own target dir (`<target>/sidecar-build`) because the outer cargo
//! holds the lock on the main one. Re-runs when the CLI's crates change, and when the staged file
//! was replaced by a build of the other profile: dev and release stage to the same path, so a
//! `cargo clippy` after a release build used to leave the debug CLI there, and the next
//! `tauri build` (its build script not re-run) bundled that 58 MB debug binary.
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    // First: the staged copies are stamped just before this run (see `copy_if_changed`).
    let started = std::time::SystemTime::now();
    stage_sidecar(started);
    tauri_build::build()
}

fn stage_sidecar(started: std::time::SystemTime) {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.parent().unwrap().to_path_buf();
    for dir in [
        "crates/harness-cli/src",
        "crates/harness-core/src",
        "crates/harness-api/src",
        "crates/harness-gateway/src",
        "crates/harness-onnx/src",
    ] {
        println!("cargo:rerun-if-changed=../{dir}");
    }
    for f in [
        "Cargo.lock",
        "crates/harness-cli/Cargo.toml",
        "crates/harness-core/Cargo.toml",
        "crates/harness-onnx/Cargo.toml",
    ] {
        println!("cargo:rerun-if-changed=../{f}");
    }
    println!("cargo:rerun-if-env-changed=HARNESS_SKIP_SIDECAR_BUILD");

    let triple = std::env::var("TARGET").unwrap();
    let exe = if triple.contains("windows") {
        ".exe"
    } else {
        ""
    };
    const BINS: [&str; 3] = ["harness", "harness-hidden", "harness-onnx"];
    let dest = |bin: &str| {
        manifest
            .join("binaries")
            .join(format!("{bin}-{triple}{exe}"))
    };
    // The staged files themselves: replaced by the other profile's build, this one stages its
    // own again (`copy_if_changed` stamps the copy so that the swap is noticed, see there).
    for bin in BINS {
        println!("cargo:rerun-if-changed={}", dest(bin).display());
    }
    // Escape hatch for CI jobs that only lint: keep existing files, never fabricate them.
    if std::env::var_os("HARNESS_SKIP_SIDECAR_BUILD").is_some()
        && BINS.iter().all(|b| dest(b).is_file())
    {
        return;
    }

    let release = std::env::var("PROFILE").is_ok_and(|p| p == "release");
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"))
        .join("sidecar-build");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(&root)
        .args([
            "build",
            "-p",
            "harness-cli",
            "-p",
            "harness-onnx",
            "--target",
            &triple,
        ])
        .arg("--target-dir")
        .arg(&target_dir);
    if release {
        cmd.arg("--release");
    }
    // Inherited from the outer cargo/clippy run; the CLI must be a plain build.
    for var in [
        "RUSTC_WORKSPACE_WRAPPER",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_TARGET",
    ] {
        cmd.env_remove(var);
    }
    let status = cmd.status().expect("could not start cargo for the sidecar");
    assert!(status.success(), "building the harness sidecars failed");

    for bin in BINS {
        let built = target_dir
            .join(&triple)
            .join(if release { "release" } else { "debug" })
            .join(format!("{bin}{exe}"));
        copy_if_changed(&built, &dest(bin), started);
    }
}

/// Copies only when the content differs, so an unchanged CLI does not retrigger bundling. The copy
/// is stamped with a time just before this run started: `std::fs::copy` keeps the source's time on
/// Windows, which can be older than the other profile's last build-script run, so
/// `rerun-if-changed` would miss the swap; a time after this run's start would re-run this same
/// script once more (a full rebuild of the app crate) for nothing.
fn copy_if_changed(from: &Path, to: &Path, started: std::time::SystemTime) {
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    if let (Ok(a), Ok(b)) = (std::fs::read(from), std::fs::read(to)) {
        if a == b {
            return;
        }
    }
    std::fs::copy(from, to).unwrap_or_else(|e| panic!("copy {from:?} -> {to:?}: {e}"));
    if let Err(e) = std::fs::File::options()
        .write(true)
        .open(to)
        .and_then(|f| f.set_modified(started - std::time::Duration::from_secs(2)))
    {
        println!("cargo:warning=could not stamp {}: {e}", to.display());
    }
}
