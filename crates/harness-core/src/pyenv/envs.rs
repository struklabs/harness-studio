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
    /// Oldest macOS major version its wheels run on (`None`: any the app runs on).
    pub min_macos: Option<u32>,
    /// The env this one is a GPU build of (`hf-cuda` of `hf-cpu`): one of the two serves the
    /// engine, the GPU build where there is an NVIDIA GPU ([`EnvStatus::preferred`]).
    pub gpu_build_of: Option<&'static str>,
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
    min_macos: None,
    gpu_build_of: None,
};

/// The NVIDIA CUDA build of the `python-hf` engine (Windows and Linux on x86-64): the same
/// packages as [`HF_CPU`] with PyTorch built for CUDA 12.6 (drivers from 560 on; on Linux the
/// CUDA libraries come as `nvidia-*` wheels, on Windows inside the torch wheel). The locks are
/// `uv pip compile --python-platform <x86_64-pc-windows-msvc | x86_64-unknown-linux-gnu>
/// --python-version 3.12 --generate-hashes --emit-index-url --index-url
/// https://download.pytorch.org/whl/cu126 --extra-index-url https://pypi.org/simple
/// --index-strategy unsafe-best-match python/hf-cuda.in`.
#[cfg(all(any(windows, target_os = "linux"), target_arch = "x86_64"))]
pub const HF_CUDA: EnvSpec = EnvSpec {
    name: "hf-cuda",
    #[cfg(windows)]
    lock: include_str!("../../python/hf-cuda-windows.lock"),
    #[cfg(target_os = "linux")]
    lock: include_str!("../../python/hf-cuda-linux.lock"),
    verify: "import torch, transformers, diffusers, sentence_transformers, timm, starlette, uvicorn, soundfile, PIL; assert torch.version.cuda, 'not a CUDA build of torch'",
    #[cfg(windows)]
    estimated_size_bytes: 2_900 * 1024 * 1024,
    #[cfg(target_os = "linux")]
    estimated_size_bytes: 4_600 * 1024 * 1024,
    min_macos: None,
    gpu_build_of: Some("hf-cpu"),
};

/// The `mlx` engine's environment: Apple's MLX and mlx-lm (its OpenAI-compatible server).
/// Apple Silicon only, and macOS 14 or later (the oldest the MLX wheels are built for; the app
/// itself runs on 13.3). `python/mlx.in` lists the requirement and how the lock is compiled.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const MLX: EnvSpec = EnvSpec {
    name: "mlx",
    lock: include_str!("../../python/mlx.lock"),
    verify: "import mlx.core, mlx_lm.server",
    estimated_size_bytes: 350 * 1024 * 1024,
    min_macos: Some(14),
    gpu_build_of: None,
};

/// The `mlx-vlm` engine's environment: mlx-vlm (vision-language models on MLX) and its
/// FastAPI server. Its own environment because it is heavier than `mlx` (transformers, OpenCV,
/// datasets, mlx-audio come along). `python/mlx-vlm.in` says how the lock is compiled.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const MLX_VLM: EnvSpec = EnvSpec {
    name: "mlx-vlm",
    lock: include_str!("../../python/mlx-vlm.lock"),
    verify: "import mlx.core, mlx_vlm.server.app, uvicorn",
    estimated_size_bytes: 780 * 1024 * 1024,
    min_macos: Some(14),
    gpu_build_of: None,
};

/// The `mflux` engine's environment: mflux (FLUX.2 [klein] image generation and editing on MLX)
/// and the worker's HTTP server. Its own environment because mflux pins its own MLX and brings
/// PyTorch and transformers along. `python/mflux.in` says how the lock is compiled.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const MFLUX: EnvSpec = EnvSpec {
    name: "mflux",
    lock: include_str!("../../python/mflux.lock"),
    verify: "import mlx.core, mflux.models.flux2.variants, uvicorn, PIL",
    estimated_size_bytes: 1_100 * 1024 * 1024,
    min_macos: Some(14),
    gpu_build_of: None,
};

/// The `transcribe.cpp` engine's environment: the transcribe-cpp binding and its default
/// native provider (CPU and Vulkan; Metal on macOS). The worker needs nothing else (the HTTP
/// server and WAV reading are the standard library's). `python/transcribe-cpp.in` says how the
/// lock is kept.
pub const TRANSCRIBE_CPP: EnvSpec = EnvSpec {
    name: "transcribe-cpp",
    lock: include_str!("../../python/transcribe-cpp.lock"),
    verify: "import transcribe_cpp; transcribe_cpp.native_version()",
    estimated_size_bytes: 60 * 1024 * 1024,
    min_macos: None,
    gpu_build_of: None,
};

/// The NVIDIA build of [`TRANSCRIBE_CPP`] (Windows and Linux on x86-64): the CUDA 12.9 native
/// provider (CUDA, Vulkan and CPU modules) and the NVIDIA runtime wheels it loads (cudart,
/// cuBLAS), so nothing of CUDA is needed on the system but the driver.
#[cfg(all(any(windows, target_os = "linux"), target_arch = "x86_64"))]
pub const TRANSCRIBE_CPP_CUDA: EnvSpec = EnvSpec {
    name: "transcribe-cpp-cuda",
    #[cfg(windows)]
    lock: include_str!("../../python/transcribe-cpp-cuda-windows.lock"),
    #[cfg(target_os = "linux")]
    lock: include_str!("../../python/transcribe-cpp-cuda-linux.lock"),
    verify: "import transcribe_cpp; transcribe_cpp.native_version()",
    estimated_size_bytes: 1_250 * 1024 * 1024,
    min_macos: None,
    gpu_build_of: Some("transcribe-cpp"),
};

/// The environments engines can ask for.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const ENVS: &[EnvSpec] = &[HF_CPU, MLX, MLX_VLM, MFLUX, TRANSCRIBE_CPP];
#[cfg(all(any(windows, target_os = "linux"), target_arch = "x86_64"))]
pub const ENVS: &[EnvSpec] = &[HF_CPU, HF_CUDA, TRANSCRIBE_CPP, TRANSCRIBE_CPP_CUDA];
#[cfg(not(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(any(windows, target_os = "linux"), target_arch = "x86_64")
)))]
pub const ENVS: &[EnvSpec] = &[HF_CPU, TRANSCRIBE_CPP];

/// Whether a CUDA build can run here: an NVIDIA GPU with a CUDA driver.
pub fn cuda_present() -> bool {
    crate::hardware::detect_hardware()
        .driver_info
        .cuda_supported
}

/// Whether `spec` is the one to install of its CPU / GPU pair on this machine: the GPU build
/// with CUDA, the CPU build without; an env with no pair always.
pub fn preferred(spec: &EnvSpec, cuda: bool) -> bool {
    if spec.gpu_build_of.is_some() {
        return cuda;
    }
    let has_gpu_build = ENVS.iter().any(|s| s.gpu_build_of == Some(spec.name));
    !(has_gpu_build && cuda)
}

/// The major version of the running macOS (`None` elsewhere, or when it can't be read).
pub fn macos_major() -> Option<u32> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    sysinfo::System::os_version()?
        .split('.')
        .next()?
        .parse()
        .ok()
}

/// Why this machine can't run the env's wheels (an error key), if it can't.
pub fn platform_refusal(spec: &EnvSpec) -> Option<String> {
    if UV.is_none() {
        return Some(
            err(KEYS.unsupported_platform)
                .with("os", std::env::consts::OS)
                .into(),
        );
    }
    let need = spec.min_macos?;
    let have = macos_major()?;
    (have < need).then(|| {
        err("pyenv.macos_too_old")
            .with("name", spec.name)
            .with("need", need)
            .with("have", have)
            .into()
    })
}

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
    /// This platform can build it (uv has a build for it, the OS is new enough).
    pub supported: bool,
    /// Why it can't (an error key with its values), when it can't.
    pub refusal: Option<String>,
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
    /// The CPU env this is a GPU build of ([`EnvSpec::gpu_build_of`]).
    pub gpu_build_of: Option<String>,
    /// The one of a CPU / GPU pair to install on this machine ([`preferred`]).
    pub preferred: bool,
}

pub fn status_in(root: &Path, spec: &EnvSpec) -> EnvStatus {
    let manifest = read_manifest_in(root, spec.name);
    let installed = manifest.is_some() && python_in(root, spec.name).is_file();
    let ready = installed
        && manifest
            .as_ref()
            .is_some_and(|m| m.lock_sha256 == lock_digest(spec) && m.uv_version == UV_VERSION);
    let dir = env_dir_in(root, spec.name);
    let refusal = platform_refusal(spec);
    EnvStatus {
        name: spec.name.to_string(),
        supported: refusal.is_none(),
        refusal,
        installed,
        ready,
        manifest,
        size_bytes: dir_size(&dir),
        dir,
        estimated_size_bytes: spec.estimated_size_bytes,
        progress: progress(spec.name),
        gpu_build_of: spec.gpu_build_of.map(str::to_string),
        preferred: spec.gpu_build_of.is_none(),
    }
}

/// Every env this version knows.
pub fn status_all() -> Vec<EnvStatus> {
    let root = root();
    let cuda = cuda_present();
    ENVS.iter()
        .map(|s| EnvStatus {
            preferred: preferred(s, cuda),
            ..status_in(&root, s)
        })
        .collect()
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
    if let Some(refusal) = platform_refusal(spec) {
        return Err(refusal);
    }
    let _one_at_a_time = BUILD_LOCK.lock().await;
    if status_in(root, spec).ready {
        return Ok(());
    }
    let dir = env_dir_in(root, spec.name);
    // Before anything is downloaded: a data folder too deep for Windows' path limit would
    // only fail at the end, with a raw traceback.
    if let Some(len) = long_path_refusal(&dir, windows_long_paths_enabled()) {
        return Err(path_too_long(len));
    }
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
    run_step(cmd, "packages", &KEYS, cancel, report)
        .await
        .map_err(|e| remap_path_failure(e, &dir))?;
    // The wheels are installed; keeping uv's copy of them would double the size.
    let _ = std::fs::remove_dir_all(&uv.cache_dir);

    report.set("verify", spec.name.to_string(), 0);
    let mut cmd = tokio::process::Command::new(&python);
    cmd.current_dir(&dir)
        .args(["-c", spec.verify])
        .envs(python_env(&dir));
    run_step(cmd, "verify", &KEYS, cancel, report)
        .await
        .map_err(|e| remap_path_failure(e, &dir))?;

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

/// Windows' classic path limit (`MAX_PATH`, counting the terminating NUL).
const WINDOWS_MAX_PATH: usize = 260;

/// A deep file the packages put in the env (relative to the env folder): the longest path
/// known to be created by the locks. Used to project the longest path before building.
const DEEPEST_ENV_FILE: &str = "venv\\Lib\\site-packages\\transformers\\models\\audio_spectrogram_transformer\\configuration_audio_spectrogram_transformer.py";

/// Length (in characters) of the longest known path under the env folder `env_dir`.
pub fn projected_longest_path_len(env_dir: &Path) -> usize {
    env_dir.display().to_string().chars().count() + 1 + DEEPEST_ENV_FILE.chars().count()
}

/// The projected path length when it would not fit (`long_paths` is Windows' `LongPathsEnabled`
/// policy; `None`: unknown, treated as off). `None` when it fits or long paths are on.
pub fn long_path_refusal(env_dir: &Path, long_paths: Option<bool>) -> Option<usize> {
    if long_paths == Some(true) {
        return None;
    }
    let len = projected_longest_path_len(env_dir);
    (len >= WINDOWS_MAX_PATH).then_some(len)
}

/// `HKLM\SYSTEM\CurrentControlSet\Control\FileSystem\LongPathsEnabled` (read only); `None`
/// off Windows or when it can't be read. Off Windows no limit applies, so it reports `true`.
fn windows_long_paths_enabled() -> Option<bool> {
    #[cfg(not(windows))]
    {
        Some(true)
    }
    #[cfg(windows)]
    {
        use windows::core::HSTRING;
        use windows::Win32::Foundation::ERROR_SUCCESS;
        use windows::Win32::System::Registry::{
            RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD,
        };
        let sub = HSTRING::from(r"SYSTEM\CurrentControlSet\Control\FileSystem");
        let name = HSTRING::from("LongPathsEnabled");
        let mut data = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let r = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                &sub,
                &name,
                RRF_RT_REG_DWORD,
                None,
                Some((&mut data as *mut u32).cast()),
                Some(&mut size),
            )
        };
        (r == ERROR_SUCCESS).then_some(data == 1)
    }
}

fn path_too_long(len: usize) -> String {
    err("pyenv.path_too_long").with("len", len).into()
}

/// Whether a step's output looks like a file that couldn't be opened for its path length.
fn is_path_failure_log(log: &str) -> bool {
    (log.contains("FileNotFoundError") && log.contains("site-packages"))
        || log.contains("os error 206")
        || log.contains("filename or extension is too long")
}

/// Maps a failed build step to the path-too-long error when its output says so (Windows only).
fn remap_path_failure(error: String, env_dir: &Path) -> String {
    if cfg!(windows) && is_path_failure_log(&error) {
        if let Some(len) = long_path_refusal(env_dir, windows_long_paths_enabled()) {
            return path_too_long(len);
        }
    }
    error
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
pub(crate) mod tests {
    use super::*;

    const SPEC: EnvSpec = EnvSpec {
        name: "test-env",
        lock: "six==1.17.0 \\\n    --hash=sha256:4721f391ed90541fddacab5acf947aa0d3dc7d27b2e1e8eda2be8970586c3274\n",
        verify: "import six",
        estimated_size_bytes: 1,
        min_macos: None,
        gpu_build_of: None,
    };

    /// Writes a ready env of `spec` under `root` (the interpreter and a manifest of its lock).
    #[cfg(all(any(windows, target_os = "linux"), target_arch = "x86_64"))]
    pub(crate) fn fake_ready(root: &Path, spec: &EnvSpec) {
        let python = python_in(root, spec.name);
        std::fs::create_dir_all(python.parent().unwrap()).unwrap();
        std::fs::write(&python, "").unwrap();
        let m = Manifest {
            name: spec.name.into(),
            uv_version: UV_VERSION.into(),
            python: "Python 3.12.13".into(),
            lock_sha256: lock_digest(spec),
            installed_at: 1,
        };
        write_json(&env_dir_in(root, spec.name).join("manifest.json"), &m).unwrap();
    }

    #[test]
    fn the_gpu_build_is_preferred_only_with_cuda() {
        assert!(preferred(&HF_CPU, false));
        assert!(
            preferred(&SPEC, true),
            "an env without a pair is always the one"
        );
        #[cfg(all(any(windows, target_os = "linux"), target_arch = "x86_64"))]
        {
            assert!(!preferred(&HF_CPU, true));
            assert!(preferred(&HF_CUDA, true));
            assert!(!preferred(&HF_CUDA, false));
        }
    }

    #[cfg(all(any(windows, target_os = "linux"), target_arch = "x86_64"))]
    #[test]
    fn the_cuda_lock_pins_a_cuda_torch_with_hashes() {
        let lock = HF_CUDA.lock;
        assert!(lock.contains("--index-url https://download.pytorch.org/whl/cu126"));
        assert!(lock.contains("torch==2.14.1+cu126"));
        assert!(!lock.contains("+cpu"));
        // Every requirement line carries a hash (uv installs with --require-hashes).
        let pins = lock
            .lines()
            .filter(|l| l.contains("==") && !l.trim_start().starts_with('#'))
            .count();
        assert!(pins > 50 && lock.matches("--hash=sha256:").count() >= pins);
        assert_eq!(HF_CUDA.gpu_build_of, Some(HF_CPU.name));
        assert!(spec("hf-cuda").is_some());
    }

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
    fn a_deep_data_folder_is_refused_unless_long_paths_are_on() {
        let deep = Path::new(
            r"C:\Users\u\AppData\Local\Temp\build-cache\C--Users-u-Documents-Projects-Harness-Studio\96f0aa61-2797-41d5-8909-421a066a6cd6\workspace\test-data\bin\pyenv\envs\hf-cpu",
        );
        let len = projected_longest_path_len(deep);
        assert!(len >= 260);
        assert_eq!(long_path_refusal(deep, Some(false)), Some(len));
        assert_eq!(long_path_refusal(deep, None), Some(len));
        assert_eq!(long_path_refusal(deep, Some(true)), None);
        let short =
            Path::new(r"C:\Users\u\AppData\Roaming\com.harnessstudio.app\bin\pyenv\envs\hf-cpu");
        assert!(projected_longest_path_len(short) < 260);
        assert_eq!(long_path_refusal(short, Some(false)), None);
    }

    #[test]
    fn path_failures_are_recognised_in_the_output() {
        assert!(is_path_failure_log(
            r"FileNotFoundError: [Errno 2] No such file: 'x\site-packages\y.py'"
        ));
        assert!(is_path_failure_log("failed (os error 206)"));
        assert!(!is_path_failure_log(
            "ModuleNotFoundError: No module named 'torch'"
        ));
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

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn mlx_lock_pins_every_wheel_for_macos_14() {
        assert_eq!(spec("mlx").unwrap().name, "mlx");
        assert_eq!(MLX.min_macos, Some(14));
        let pinned: Vec<&str> = MLX
            .lock
            .lines()
            .filter(|l| l.contains("==") && !l.trim_start().starts_with('#'))
            .collect();
        for line in &pinned {
            assert!(line.trim_end().ends_with('\\'), "no hashes after {line}");
        }
        for pkg in ["mlx==", "mlx-lm==", "mlx-metal=="] {
            assert!(pinned.iter().any(|l| l.starts_with(pkg)), "{pkg}");
        }
        // No PyTorch: the env is small.
        assert!(!pinned.iter().any(|l| l.starts_with("torch==")));
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn mlx_vlm_lock_pins_every_wheel_for_macos_14() {
        assert_eq!(spec("mlx-vlm").unwrap().name, "mlx-vlm");
        assert_eq!(MLX_VLM.min_macos, Some(14));
        let pinned: Vec<&str> = MLX_VLM
            .lock
            .lines()
            .filter(|l| l.contains("==") && !l.trim_start().starts_with('#'))
            .collect();
        for line in &pinned {
            assert!(line.trim_end().ends_with('\\'), "no hashes after {line}");
        }
        for pkg in [
            "mlx==",
            "mlx-vlm==",
            "mlx-metal==",
            "fastapi==",
            "uvicorn==",
        ] {
            assert!(pinned.iter().any(|l| l.starts_with(pkg)), "{pkg}");
        }
        assert!(!pinned.iter().any(|l| l.starts_with("torch==")));
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn mflux_lock_pins_every_wheel_for_macos_14() {
        assert_eq!(spec("mflux").unwrap().name, "mflux");
        assert_eq!(MFLUX.min_macos, Some(14));
        let pinned: Vec<&str> = MFLUX
            .lock
            .lines()
            .filter(|l| l.contains("==") && !l.trim_start().starts_with('#'))
            .collect();
        for line in &pinned {
            assert!(line.trim_end().ends_with('\\'), "no hashes after {line}");
        }
        for pkg in [
            "mflux==0.22.0",
            "mlx==",
            "mlx-metal==",
            "uvicorn==",
            "pillow==",
        ] {
            assert!(pinned.iter().any(|l| l.starts_with(pkg)), "{pkg}");
        }
    }

    #[test]
    fn an_env_for_a_newer_macos_is_refused_with_its_versions() {
        let newer = EnvSpec {
            min_macos: Some(999),
            ..SPEC
        };
        if cfg!(target_os = "macos") && UV.is_some() {
            let e = platform_refusal(&newer).expect("refused");
            assert!(e.contains("pyenv.macos_too_old"), "{e}");
            assert!(e.contains("999"), "{e}");
        }
        assert!(platform_refusal(&EnvSpec {
            min_macos: Some(1),
            ..SPEC
        })
        .is_none_or(|e| e.contains("unsupported_platform")));
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
        for line in lock
            .lines()
            .filter(|l| l.contains("==") && !l.trim_start().starts_with('#'))
        {
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
    fn transcribe_cpp_locks_pin_every_file_of_one_version() {
        #[cfg(all(any(windows, target_os = "linux"), target_arch = "x86_64"))]
        let locks = {
            assert_eq!(TRANSCRIBE_CPP_CUDA.gpu_build_of, Some(TRANSCRIBE_CPP.name));
            assert!(preferred(&TRANSCRIBE_CPP_CUDA, true));
            assert!(!preferred(&TRANSCRIBE_CPP, true));
            let cuda = TRANSCRIBE_CPP_CUDA.lock;
            // The CUDA provider is a GitHub release wheel of the same version as the binding.
            assert!(cuda.contains(
                "transcribe-cpp-native-cu12 @ https://github.com/handy-computer/transcribe.cpp/releases/download/v0.4.0/"
            ));
            assert!(
                cuda.contains("nvidia-cublas-cu12==12.9.")
                    && cuda.contains("nvidia-cuda-runtime-cu12==12.9.")
            );
            assert!(!cuda.contains("transcribe-cpp-native=="));
            vec![TRANSCRIBE_CPP.lock, cuda]
        };
        #[cfg(not(all(any(windows, target_os = "linux"), target_arch = "x86_64")))]
        let locks = vec![TRANSCRIBE_CPP.lock];
        for lock in locks {
            assert!(lock.contains("transcribe-cpp==0.4.0 \\"));
            let requirements: Vec<&str> = lock
                .lines()
                .filter(|l| !l.starts_with(' ') && !l.starts_with('#') && !l.trim().is_empty())
                .collect();
            assert!(!requirements.is_empty());
            for line in requirements {
                assert!(
                    line.contains("==") || line.contains(" @ https://"),
                    "{line}"
                );
                assert!(line.trim_end().ends_with('\\'), "no hashes after {line}");
            }
        }
        assert!(spec("transcribe-cpp").is_some());
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
