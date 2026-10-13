//! Named engine environments under `<data>/bin/pyenv`: one virtual environment per Python
//! engine, installed from a lock file compiled into this source (`uv pip compile
//! --generate-hashes` output), so every wheel is checked against a pinned SHA-256
//! (`--require-hashes`) and nothing outside the lock is installed (`--no-deps`).
//!
//! uv and the managed Python are shared by every env (`<root>/uv`, `<root>/python`); removing
//! an env deletes only `envs/<name>`. One build runs at a time.

use super::{
    create_dir, dir_size, ensure_uv, python_env, python_version, remove_dir, run_step, venv_python,
    write_json, Cancel, EnvProgress, ErrorKeys, Progress, Uv, CANCELLED, UV, UV_VERSION,
};
use crate::error_key::err;
use crate::paths;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Error keys of engine environments.
pub const KEYS: ErrorKeys = ErrorKeys {
    unsupported_platform: "pyenv.unsupported_platform",
    spawn_failed: "pyenv.spawn_failed",
    env_failed: "pyenv.env_failed",
};

/// One engine environment.
#[derive(Debug, Clone, Copy)]
pub struct EnvSpec {
    /// Folder and API name (`hf-cpu`).
    pub name: &'static str,
    /// The lock file: `uv pip compile --generate-hashes` output. Index options (`--index-url`,
    /// `--extra-index-url`) in it are honoured.
    pub lock: &'static str,
    /// Python code that must run without error once the packages are in (`import torch`).
    pub verify: &'static str,
    /// Rough size on disk (shown before it is built).
    pub estimated_size_bytes: u64,
}

/// The CPU build of the `python-hf` engine: PyTorch (CPU wheels; plain PyPI wheels on macOS,
/// which use Metal through MPS), transformers, diffusers, sentence-transformers, timm and the
/// worker's HTTP server. `python/hf-cpu.in` lists the direct requirements; the lock is
/// `uv pip compile --universal --python-version 3.12 --generate-hashes --emit-index-url
/// --index-url https://download.pytorch.org/whl/cpu --extra-index-url https://pypi.org/simple
/// --index-strategy unsafe-best-match python/hf-cpu.in`.
pub const HF_CPU: EnvSpec = EnvSpec {
    name: "hf-cpu",
    lock: include_str!("../../python/hf-cpu.lock"),
    verify: "import torch, transformers, diffusers, sentence_transformers, timm, starlette, uvicorn, soundfile, PIL",
    estimated_size_bytes: 1_300 * 1024 * 1024,
};

/// The environments engines can ask for.
pub const ENVS: &[EnvSpec] = &[HF_CPU];

pub fn spec(name: &str) -> Option<&'static EnvSpec> {
    ENVS.iter().find(|s| s.name == name)
}

/// `<data>/bin/pyenv`.
pub fn root() -> PathBuf {
    paths::bin_dir().join("pyenv")
}

pub fn env_dir_in(root: &Path, name: &str) -> PathBuf {
    root.join("envs").join(name)
}

/// The interpreter of the env `name`.
pub fn python_in(root: &Path, name: &str) -> PathBuf {
    venv_python(&env_dir_in(root, name).join("venv"))
}

/// `envs/<name>/manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Manifest {
    pub name: String,
    pub uv_version: String,
    /// `Python 3.12.13`.
    pub python: String,
    /// SHA-256 of the lock the packages were installed from (another lock means a rebuild).
    pub lock_sha256: String,
    pub installed_at: u64,
}

pub fn read_manifest_in(root: &Path, name: &str) -> Option<Manifest> {
    serde_json::from_slice(&std::fs::read(env_dir_in(root, name).join("manifest.json")).ok()?).ok()
}

pub fn lock_digest(spec: &EnvSpec) -> String {
    crate::hex::lower(&Sha256::digest(spec.lock.as_bytes()))
}

/// What Settings shows for one env.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EnvStatus {
    pub name: String,
    /// This platform can build it (uv has a build for it).
    pub supported: bool,
    /// A complete environment exists (it may be from an older lock).
    pub installed: bool,
    /// It matches this version's lock, so the engine can start at once.
    pub ready: bool,
    pub manifest: Option<Manifest>,
    pub size_bytes: u64,
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub dir: PathBuf,
    pub estimated_size_bytes: u64,
    /// Progress of the build running now.
    pub progress: Option<EnvProgress>,
}

pub fn status_in(root: &Path, spec: &EnvSpec) -> EnvStatus {
    let manifest = read_manifest_in(root, spec.name);
    let installed = manifest.is_some() && python_in(root, spec.name).is_file();
    let ready = installed
        && manifest
            .as_ref()
            .is_some_and(|m| m.lock_sha256 == lock_digest(spec) && m.uv_version == UV_VERSION);
    let dir = env_dir_in(root, spec.name);
    EnvStatus {
        name: spec.name.to_string(),
        supported: UV.is_some(),
        installed,
        ready,
        manifest,
        size_bytes: dir_size(&dir),
        dir,
        estimated_size_bytes: spec.estimated_size_bytes,
        progress: progress(spec.name),
    }
}

/// Every env this version knows.
pub fn status_all() -> Vec<EnvStatus> {
    let root = root();
    ENVS.iter().map(|s| status_in(&root, s)).collect()
}

// --- builds -------------------------------------------------------------------------------------

static PROGRESS: Mutex<Option<HashMap<String, EnvProgress>>> = Mutex::new(None);
static CANCELS: Mutex<Option<HashMap<String, Cancel>>> = Mutex::new(None);
static BUILD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn progress(name: &str) -> Option<EnvProgress> {
    PROGRESS.lock().unwrap().as_ref()?.get(name).cloned()
}

fn set_progress(name: &str, p: Option<EnvProgress>) {
    let mut map = PROGRESS.lock().unwrap();
    let map = map.get_or_insert_with(HashMap::new);
    match p {
        Some(p) => map.insert(name.to_string(), p),
        None => map.remove(name),
    };
}

/// Builds whatever of the env is missing or out of date. Safe to call when it is ready.
pub async fn ensure_in(
    root: &Path,
    spec: &EnvSpec,
    cancel: &Cancel,
    report: &Progress,
) -> Result<(), String> {
    let _one_at_a_time = BUILD_LOCK.lock().await;
    if status_in(root, spec).ready {
        return Ok(());
    }
    let dir = env_dir_in(root, spec.name);
    create_dir(&dir)?;
    let uv = Uv {
        bin: ensure_uv(root, &KEYS, cancel, report).await?,
        cwd: dir.clone(),
        python_dir: root.join("python"),
        cache_dir: root.join("cache"),
    };
    // Another lock (or a half-built env): start the venv again; the Python itself stays.
    let venv = dir.join("venv");
    let _ = std::fs::remove_dir_all(&venv);
    let _ = std::fs::remove_file(dir.join("manifest.json"));
    let lock_file = dir.join("requirements.lock");
    std::fs::write(&lock_file, spec.lock).map_err(|e| {
        String::from(
            err("fs.write_failed")
                .with("path", lock_file.display())
                .with("error", e),
        )
    })?;
    uv.create_venv(&venv, &KEYS, cancel, report).await?;

    let python = venv_python(&venv);
    report.set("packages", spec.name.to_string(), 0);
    let mut cmd = uv.command();
    cmd.args(["pip", "install", "--no-config", "--python"])
        .arg(&python)
        // Every file is pinned by hash, so taking a version from whichever index has it (the
        // CPU PyTorch index next to PyPI) cannot swap in another file. Wheels only: a source
        // distribution would run its build script here, outside the isolation, with network.
        .args([
            "--require-hashes",
            "--no-deps",
            "--only-binary",
            ":all:",
            "--index-strategy",
            "unsafe-best-match",
            "-r",
        ])
        .arg(&lock_file);
    run_step(cmd, "packages", &KEYS, cancel, report).await?;
    // The wheels are installed; keeping uv's copy of them would double the size.
    let _ = std::fs::remove_dir_all(&uv.cache_dir);

    report.set("verify", spec.name.to_string(), 0);
    let mut cmd = tokio::process::Command::new(&python);
    cmd.current_dir(&dir)
        .args(["-c", spec.verify])
        .envs(python_env(&dir));
    run_step(cmd, "verify", &KEYS, cancel, report).await?;

    write_json(
        &dir.join("manifest.json"),
        &Manifest {
            name: spec.name.to_string(),
            uv_version: UV_VERSION.to_string(),
            python: python_version(&python).await,
            lock_sha256: lock_digest(spec),
            installed_at: crate::capabilities::now_secs(),
        },
    )
}

/// Registers the install of `name`. A second install of the same env while one runs (or waits
/// for the build lock) would take over its progress and cancel handle: it is refused.
fn begin(name: &str) -> Result<Cancel, String> {
    let mut cancels = CANCELS.lock().unwrap();
    let cancels = cancels.get_or_insert_with(HashMap::new);
    if cancels.contains_key(name) {
        return Err(err("pyenv.env_busy").with("name", name).into());
    }
    let cancel = Cancel::default();
    cancels.insert(name.to_string(), cancel.clone());
    Ok(cancel)
}

/// Builds the env `name` with progress Settings can poll ([`status_all`]).
pub async fn install(name: &str) -> Result<EnvStatus, String> {
    let spec =
        spec(name).ok_or_else(|| String::from(err("pyenv.unknown_env").with("name", name)))?;
    let cancel = begin(name)?;
    set_progress(name, Some(EnvProgress::default()));
    let key = name.to_string();
    let report = Progress::new(move |p| set_progress(&key, Some(p.clone())));
    let root = root();
    let result = ensure_in(&root, spec, &cancel, &report).await;
    set_progress(name, None);
    if let Some(map) = CANCELS.lock().unwrap().as_mut() {
        map.remove(name);
    }
    match result {
        Ok(()) => Ok(status_in(&root, spec)),
        Err(e) if e == CANCELLED => Err(err("pyenv.cancelled").into()),
        Err(e) => Err(e),
    }
}

/// Stops the build of `name` that [`install`] started.
pub fn cancel_install(name: &str) {
    if let Some(c) = CANCELS.lock().unwrap().as_ref().and_then(|m| m.get(name)) {
        c.set();
    }
}

/// Deletes the env `name` (refused while it is being built).
pub fn remove_in(root: &Path, name: &str) -> Result<(), String> {
    let installing = CANCELS
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|m| m.contains_key(name));
    if installing || progress(name).is_some() {
        return Err(err("pyenv.env_in_use").with("name", name).into());
    }
    remove_dir(&env_dir_in(root, name))
}

pub fn remove(name: &str) -> Result<(), String> {
    spec(name).ok_or_else(|| String::from(err("pyenv.unknown_env").with("name", name)))?;
    remove_in(&root(), name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: EnvSpec = EnvSpec {
        name: "test-env",
        lock: "six==1.17.0 \\\n    --hash=sha256:4721f391ed90541fddacab5acf947aa0d3dc7d27b2e1e8eda2be8970586c3274\n",
        verify: "import six",
        estimated_size_bytes: 1,
    };

    #[test]
    fn status_follows_the_manifest_and_the_lock() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let none = status_in(root, &SPEC);
        assert!(!none.installed && !none.ready && none.manifest.is_none());

        let python = python_in(root, SPEC.name);
        std::fs::create_dir_all(python.parent().unwrap()).unwrap();
        std::fs::write(&python, "").unwrap();
        let mut m = Manifest {
            name: SPEC.name.into(),
            uv_version: UV_VERSION.into(),
            python: "Python 3.12.13".into(),
            lock_sha256: lock_digest(&SPEC),
            installed_at: 1,
        };
        write_json(&env_dir_in(root, SPEC.name).join("manifest.json"), &m).unwrap();
        let ready = status_in(root, &SPEC);
        assert!(ready.installed && ready.ready);

        // A new lock in a later version: installed, but rebuilt before use.
        m.lock_sha256 = "old".into();
        write_json(&env_dir_in(root, SPEC.name).join("manifest.json"), &m).unwrap();
        let stale = status_in(root, &SPEC);
        assert!(stale.installed && !stale.ready);

        remove_in(root, SPEC.name).unwrap();
        assert!(!env_dir_in(root, SPEC.name).exists());
        // uv and the shared Python are not part of an env.
        assert!(root.exists());
    }

    #[test]
    fn lock_digest_is_the_lock_text() {
        let other = EnvSpec {
            lock: "six==1.16.0\n",
            ..SPEC
        };
        assert_eq!(lock_digest(&SPEC), lock_digest(&SPEC));
        assert_ne!(lock_digest(&SPEC), lock_digest(&other));
    }

    #[test]
    fn unknown_envs_are_refused() {
        assert!(remove("no-such-env").is_err());
    }

    #[test]
    fn hf_cpu_lock_pins_every_wheel_and_a_safe_torch() {
        assert_eq!(spec("hf-cpu").unwrap().name, "hf-cpu");
        let lock = HF_CPU.lock;
        // Every requirement line carries hashes (uv also refuses otherwise, but fail early).
        for line in lock.lines().filter(|l| l.contains("==") && !l.trim_start().starts_with('#')) {
            assert!(line.trim_end().ends_with('\\'), "no hashes after {line}");
        }
        // torch.load(weights_only=True) is safe from 2.6 on (CVE-2025-32434): .bin weights rely
        // on it.
        let torch = lock
            .lines()
            .find(|l| l.starts_with("torch=="))
            .expect("torch is pinned");
        let ver: Vec<u32> = torch["torch==".len()..]
            .split(|c: char| !c.is_ascii_digit())
            .take(2)
            .map(|p| p.parse().unwrap())
            .collect();
        assert!(ver >= vec![2, 6], "{torch}");
    }

    #[test]
    fn a_second_install_of_the_same_env_is_refused_and_blocks_removal() {
        let dir = tempfile::tempdir().unwrap();
        begin("busy-env").unwrap();
        assert!(begin("busy-env").err().unwrap().contains("pyenv.env_busy"));
        assert!(begin("other-env").is_ok());
        assert!(remove_in(dir.path(), "busy-env").is_err());
        for name in ["busy-env", "other-env"] {
            CANCELS.lock().unwrap().as_mut().unwrap().remove(name);
        }
        assert!(remove_in(dir.path(), "busy-env").is_ok());
    }

    /// Builds a real env (downloads uv, Python and one wheel): `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn builds_an_env_from_a_hashed_lock() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        ensure_in(root, &SPEC, &Cancel::default(), &Progress::new(|_| {}))
            .await
            .unwrap();
        let s = status_in(root, &SPEC);
        assert!(s.ready, "{s:?}");
        assert!(s.manifest.unwrap().python.starts_with("Python 3.12"));
        // A wheel whose hash is not the pinned one is refused.
        let bad = EnvSpec {
            name: "bad-env",
            lock: "six==1.17.0 \\\n    --hash=sha256:0000000000000000000000000000000000000000000000000000000000000000\n",
            ..SPEC
        };
        assert!(
            ensure_in(root, &bad, &Cancel::default(), &Progress::new(|_| {}))
                .await
                .is_err()
        );
        assert!(!status_in(root, &bad).ready);
    }
}
