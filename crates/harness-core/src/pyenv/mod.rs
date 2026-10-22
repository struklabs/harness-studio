//! Managed Python environments: folders Harness builds itself with a pinned uv, a uv-managed
//! Python and a virtual environment, so optional engines that need Python can run without
//! anything installed system-wide.
//!
//! Two kinds use this module:
//!
//! * the GGUF conversion environment ([`crate::convert::env`]), at `<data>/bin/convert-env`;
//! * named engine environments ([`envs`]), at `<data>/bin/pyenv`:
//!
//! ```text
//! uv/<version>/uv        astral-sh/uv release, SHA-256 pinned in this source
//! python/                python-build-standalone, fetched by uv, shared by every env
//! envs/<name>/venv/      the env's packages, installed from a hash-pinned lock
//! envs/<name>/manifest.json
//! ```
//!
//! Nothing here is on PATH, `pip` never runs outside these folders, every environment can be
//! deleted from Settings, and the processes started from them get [`python_env`] (no user
//! site-packages, offline Hugging Face Hub) and, on Linux, no network interfaces
//! ([`isolation`]; Unix sockets stay reachable).

pub mod envs;
pub mod isolation;
pub mod proc;

use crate::error_key::err;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;

/// Internal sentinel: a cancelled build ends as cancelled, never shown as an error.
pub(crate) const CANCELLED: &str = "convert.cancelled";

/// The pinned uv release.
pub const UV_VERSION: &str = "0.12.20";
/// The Python every environment uses (uv fetches the newest patch release it knows).
pub const PYTHON_SERIES: &str = "3.12";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Archive {
    Zip,
    TarGz,
}

/// The uv build of this platform.
#[derive(Debug, Clone, Copy)]
pub struct UvSpec {
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub archive: Archive,
    /// Path of the executable inside the archive.
    pub inner: &'static str,
}

#[cfg(all(windows, target_arch = "x86_64"))]
pub const UV: Option<UvSpec> = Some(UvSpec {
    url: "https://github.com/astral-sh/uv/releases/download/0.12.20/uv-x86_64-pc-windows-msvc.zip",
    size: 18_039_150,
    sha256: "95f9bc30fbb3574d276e28ac4a6de932d25153645853d13da8c21eec3bc88d06",
    archive: Archive::Zip,
    inner: "uv.exe",
});

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub const UV: Option<UvSpec> = Some(UvSpec {
    url: "https://github.com/astral-sh/uv/releases/download/0.12.20/uv-x86_64-unknown-linux-gnu.tar.gz",
    size: 19_827_214,
    sha256: "6590717592ace991ff83a63fef799e3ad9d33ecc8f96c5d6bdd732496e79337f",
    archive: Archive::TarGz,
    inner: "uv-x86_64-unknown-linux-gnu/uv",
});

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const UV: Option<UvSpec> = Some(UvSpec {
    url: "https://github.com/astral-sh/uv/releases/download/0.12.20/uv-aarch64-apple-darwin.tar.gz",
    size: 17_039_721,
    sha256: "848fdeb602ff1a1baacd4f6c8b7bdc6cf1ad026a6d9cf59475fda17c179743ca",
    archive: Archive::TarGz,
    inner: "uv-aarch64-apple-darwin/uv",
});

#[cfg(not(any(
    all(windows, target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64")
)))]
pub const UV: Option<UvSpec> = None;

/// Error keys of the environment that is being built: the conversion environment keeps its own
/// wording ("conversion environment"), engine environments say which engine.
#[derive(Debug, Clone, Copy)]
pub struct ErrorKeys {
    pub unsupported_platform: &'static str,
    pub spawn_failed: &'static str,
    pub env_failed: &'static str,
}

// --- progress and cancellation handles -------------------------------------------------------

/// A cancellation flag shared by a job and the processes it runs.
#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    pub fn set(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub(crate) fn flag(&self) -> Arc<AtomicBool> {
        self.0.clone()
    }
}

/// Progress of building an environment.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EnvProgress {
    /// `uv`, `source`, `python`, `packages`, `verify`.
    pub step: String,
    /// What is being fetched or the last line the tool printed.
    pub detail: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

/// Where environment progress goes.
#[derive(Clone)]
pub struct Progress {
    state: Arc<Mutex<EnvProgress>>,
    sink: Arc<dyn Fn(&EnvProgress) + Send + Sync>,
}

impl Progress {
    pub fn new(sink: impl Fn(&EnvProgress) + Send + Sync + 'static) -> Self {
        Self {
            state: Arc::default(),
            sink: Arc::new(sink),
        }
    }

    fn emit(&self, f: impl FnOnce(&mut EnvProgress)) {
        let snapshot = {
            let mut s = self.state.lock().unwrap();
            f(&mut s);
            s.clone()
        };
        (self.sink)(&snapshot);
    }

    /// A new step starts.
    pub fn set(&self, step: &str, detail: String, total: u64) {
        self.emit(|s| {
            *s = EnvProgress {
                step: step.to_string(),
                detail,
                downloaded_bytes: 0,
                total_bytes: total,
            }
        });
    }

    /// Bytes of the current download so far.
    pub fn bytes(&self, n: u64) {
        self.emit(|s| s.downloaded_bytes = n);
    }

    /// A line the tool of the current step printed.
    pub fn detail(&self, step: &str, line: &str) {
        self.emit(|s| {
            s.step = step.to_string();
            s.detail = line.chars().take(200).collect();
        });
    }
}

// --- paths --------------------------------------------------------------------------------------

/// `<dir>/uv/<version>/uv`.
pub fn uv_bin_in(dir: &Path) -> PathBuf {
    dir.join("uv")
        .join(UV_VERSION)
        .join(if cfg!(windows) { "uv.exe" } else { "uv" })
}

/// The interpreter of the virtual environment `venv`.
pub fn venv_python(venv: &Path) -> PathBuf {
    if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    }
}

pub fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    for e in rd.flatten() {
        let Ok(md) = e.metadata() else { continue };
        if md.is_dir() {
            total += dir_size(&e.path());
        } else {
            total += md.len();
        }
    }
    total
}

/// Deletes `dir` and everything in it (missing is fine).
pub fn remove_dir(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| {
            String::from(
                err("fs.remove_failed")
                    .with("path", dir.display())
                    .with("error", e),
            )
        })?;
    }
    Ok(())
}

pub(crate) fn create_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| {
        String::from(
            err("fs.create_dir_failed")
                .with("path", dir.display())
                .with("error", e),
        )
    })
}

pub(crate) fn write_json(file: &Path, value: &impl Serialize) -> Result<(), String> {
    std::fs::write(
        file,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| {
        String::from(
            err("fs.write_failed")
                .with("path", file.display())
                .with("error", e),
        )
    })
}

// --- downloads ----------------------------------------------------------------------------------

pub(crate) fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("HarnessStudio/0.1.2")
        .build()
        .map_err(|e| crate::http_error::transport(&e, "HTTP client"))
}

/// Streams `url` into `dest`, returning the SHA-256 of what arrived.
pub(crate) async fn download(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    cancel: &Cancel,
    mut on_bytes: impl FnMut(u64),
) -> Result<String, String> {
    let res = client
        .get(url)
        .send()
        .await
        .map_err(|e| crate::http_error::transport(&e, url))?;
    if !res.status().is_success() {
        return Err(err("http.status")
            .with("status", res.status())
            .with("url", url)
            .into());
    }
    if let Some(parent) = dest.parent() {
        create_dir(parent)?;
    }
    let write_err = |e: std::io::Error| {
        String::from(
            err("fs.write_failed")
                .with("path", dest.display())
                .with("error", e),
        )
    };
    let mut file = tokio::fs::File::create(dest).await.map_err(write_err)?;
    let mut hasher = Sha256::new();
    let mut stream = res.bytes_stream();
    let mut got = 0u64;
    while let Some(chunk) = stream.next().await {
        if cancel.is_set() {
            return Err(CANCELLED.to_string());
        }
        let chunk = chunk.map_err(|e| crate::http_error::transport(&e, url))?;
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(write_err)?;
        got += chunk.len() as u64;
        on_bytes(got);
    }
    file.flush().await.map_err(write_err)?;
    Ok(crate::hex::lower(&hasher.finalize()))
}

/// uv itself under `dir` (`<dir>/uv/<version>/uv`): downloaded once, checked against the pinned
/// SHA-256.
pub async fn ensure_uv(
    dir: &Path,
    keys: &ErrorKeys,
    cancel: &Cancel,
    report: &Progress,
) -> Result<PathBuf, String> {
    let spec = UV.ok_or_else(|| {
        String::from(err(keys.unsupported_platform).with("os", std::env::consts::OS))
    })?;
    let bin = uv_bin_in(dir);
    if bin.is_file() {
        return Ok(bin);
    }
    report.set("uv", format!("uv {UV_VERSION}"), spec.size);
    let staging = dir.join(".staging-uv");
    let _ = std::fs::remove_dir_all(&staging);
    let archive = staging.join("uv.archive");
    let client = client()?;
    let r = report.clone();
    let digest = download(&client, spec.url, &archive, cancel, move |n| r.bytes(n)).await?;
    if digest != spec.sha256 {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(err("runtime.digest_mismatch")
            .with("file", spec.url)
            .with("expected", spec.sha256)
            .with("actual", digest)
            .into());
    }
    let (a, inner, target) = (archive.clone(), spec.inner, bin.clone());
    let format = spec.archive;
    tokio::task::spawn_blocking(move || extract_single(&a, format, inner, &target))
        .await
        .map_err(|e| e.to_string())??;
    let _ = std::fs::remove_dir_all(&staging);
    Ok(bin)
}

/// One file out of a `.zip` / `.tar.gz`.
fn extract_single(
    archive: &Path,
    format: Archive,
    inner: &str,
    target: &Path,
) -> Result<(), String> {
    use std::io::{Read, Write};
    crate::zip_safety::reject_symlink_ancestors(target, target.parent().unwrap_or(target))?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut bytes = Vec::new();
    match format {
        Archive::Zip => {
            let mut zip =
                zip::ZipArchive::new(std::fs::File::open(archive).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            crate::zip_safety::validate_archive(&mut zip)?;
            let mut f = zip.by_name(inner).map_err(|e| format!("{inner}: {e}"))?;
            f.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        }
        Archive::TarGz => {
            let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
            let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
            let mut found = false;
            for entry in tar.entries().map_err(|e| e.to_string())? {
                let mut entry = entry.map_err(|e| e.to_string())?;
                let path = entry
                    .path()
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/");
                if path == inner {
                    entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(format!("{inner} is not in the archive"));
            }
        }
    }
    let write_failed = |e: std::io::Error| {
        String::from(
            err("fs.write_failed")
                .with("path", target.display())
                .with("error", e),
        )
    };
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(write_failed)?;
    file.write_all(&bytes).map_err(write_failed)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o755))
            .map_err(write_failed)?;
    }
    Ok(())
}

// --- uv and Python runs -------------------------------------------------------------------------

/// A uv binary with the folders it keeps its Python installs and cache in; its commands read no
/// user or system configuration.
#[derive(Debug, Clone)]
pub struct Uv {
    pub bin: PathBuf,
    pub cwd: PathBuf,
    pub python_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl Uv {
    pub fn command(&self) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.bin);
        crate::appimage::foreign_tokio(&mut cmd);
        cmd.current_dir(&self.cwd)
            .env("UV_PYTHON_INSTALL_DIR", &self.python_dir)
            .env("UV_CACHE_DIR", &self.cache_dir)
            .env("UV_NO_CONFIG", "1")
            .env("UV_NO_PROGRESS", "1")
            .env("NO_COLOR", "1");
        cmd
    }

    /// `uv venv` at `venv` with the managed Python [`PYTHON_SERIES`] (installed into
    /// `python_dir` the first time).
    pub async fn create_venv(
        &self,
        venv: &Path,
        keys: &ErrorKeys,
        cancel: &Cancel,
        report: &Progress,
    ) -> Result<(), String> {
        report.set("python", format!("Python {PYTHON_SERIES}"), 0);
        let mut cmd = self.command();
        cmd.args([
            "venv",
            "--python",
            PYTHON_SERIES,
            "--managed-python",
            "--no-config",
        ])
        .arg(venv);
        let first = run_step(cmd, "python", keys, cancel, report).await;
        let Err(error) = first else {
            return Ok(());
        };
        // uv points `cpython-3.12-…` at the patch install with an NTFS junction and checks it at
        // once. On some Windows 11 machines (25H2) nothing that stats by name (uv, Node) follows a
        // junction under %APPDATA%, so this step failed every time although the Python was
        // unpacked: the venv is then made from that python.exe directly.
        if !error.contains(MINOR_LINK_ERROR) {
            return Err(error);
        }
        let Some(python) = unpacked_python(&self.python_dir) else {
            return Err(error);
        };
        let mut cmd = self.command();
        cmd.args(["venv", "--no-config", "--python"])
            .arg(&python)
            .arg(venv)
            .env("UV_PYTHON_DOWNLOADS", "never");
        run_step(cmd, "python", keys, cancel, report)
            .await
            .map_err(|_| error)
    }
}

/// uv's error when the junction of a minor version (`cpython-3.12-…`) cannot be followed.
const MINOR_LINK_ERROR: &str = "Missing expected target directory for Python minor version link";

/// The newest unpacked [`PYTHON_SERIES`] install in `python_dir` (`cpython-3.12.<patch>-…`, not
/// the minor version's link), as its interpreter path.
pub fn unpacked_python(python_dir: &Path) -> Option<PathBuf> {
    let prefix = format!("cpython-{PYTHON_SERIES}.");
    std::fs::read_dir(python_dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let patch: u32 = name
                .strip_prefix(&prefix)?
                .split('-')
                .next()?
                .parse()
                .ok()?;
            let dir = entry.path();
            let python = if cfg!(windows) {
                dir.join("python.exe")
            } else {
                dir.join("bin").join("python3")
            };
            python.is_file().then_some((patch, python))
        })
        .max_by_key(|(patch, _)| *patch)
        .map(|(_, python)| python)
}

/// Runs a uv (or verification) step to its end: output lines go to `report`, a failure carries
/// the end of the output.
pub async fn run_step(
    cmd: tokio::process::Command,
    step: &'static str,
    keys: &ErrorKeys,
    cancel: &Cancel,
    report: &Progress,
) -> Result<(), String> {
    let outcome = proc::run(cmd, &cancel.flag(), |line| {
        if !line.trim().is_empty() {
            report.detail(step, line.trim());
        }
    })
    .await
    .map_err(|e| String::from(err(keys.spawn_failed).with("step", step).with("error", e)))?;
    if outcome.cancelled {
        return Err(CANCELLED.to_string());
    }
    if !outcome.success {
        return Err(err(keys.env_failed)
            .with("step", step)
            .with("log", outcome.log())
            .into());
    }
    Ok(())
}

/// `Python 3.12.13` of an interpreter (empty when it does not start).
pub async fn python_version(python: &Path) -> String {
    let mut cmd = tokio::process::Command::new(python);
    crate::appimage::foreign_tokio(&mut cmd);
    cmd.arg("--version").kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    match cmd.output().await {
        Ok(o) => format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        )
        .trim()
        .to_string(),
        Err(_) => String::new(),
    }
}

/// Environment variables of every Python run: nothing from the user's site-packages, UTF-8
/// everywhere, no network for the Hub (models are local folders), and a private Hugging Face
/// cache under `hf_home_parent`. Matplotlib draws off screen (`Agg`): its macOS backend would
/// otherwise put a Python icon in the Dock.
pub fn python_env(hf_home_parent: &Path) -> Vec<(&'static str, std::ffi::OsString)> {
    vec![
        ("PYTHONNOUSERSITE", "1".into()),
        ("PYTHONUTF8", "1".into()),
        ("PYTHONIOENCODING", "utf-8".into()),
        ("PYTHONUNBUFFERED", "1".into()),
        ("HF_HUB_OFFLINE", "1".into()),
        ("TRANSFORMERS_OFFLINE", "1".into()),
        ("HF_HUB_DISABLE_TELEMETRY", "1".into()),
        ("HF_HOME", hf_home_parent.join("hf-home").into()),
        ("TOKENIZERS_PARALLELISM", "false".into()),
        ("NO_COLOR", "1".into()),
        ("MPLBACKEND", "Agg".into()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_runs_draw_matplotlib_off_screen() {
        let env = python_env(Path::new("/x"));
        assert!(env.iter().any(|(k, v)| *k == "MPLBACKEND" && v == "Agg"));
    }

    #[test]
    fn zip_binary_extraction_rejects_symlinks() {
        use std::io::{Cursor, Write};
        for symlink in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let archive = dir.path().join("uv.zip");
            let target = dir.path().join("uv");
            let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let options = zip::write::SimpleFileOptions::default();
            if symlink {
                writer.add_symlink("uv", "../../outside", options).unwrap();
            } else {
                writer.start_file("uv", options).unwrap();
                writer.write_all(b"binary").unwrap();
            }
            std::fs::write(&archive, writer.finish().unwrap().into_inner()).unwrap();
            let result = extract_single(&archive, Archive::Zip, "uv", &target);
            if symlink {
                assert!(result.is_err());
                assert!(!target.exists());
            } else {
                result.unwrap();
                assert_eq!(std::fs::read(target).unwrap(), b"binary");
            }
        }
    }

    #[test]
    fn the_newest_unpacked_python_of_the_series_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let make = |name: &str| {
            let d = dir.path().join(name);
            let exe = if cfg!(windows) {
                d.join("python.exe")
            } else {
                d.join("bin").join("python3")
            };
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(&exe, b"").unwrap();
            exe
        };
        assert_eq!(unpacked_python(dir.path()), None);
        make("cpython-3.12.9-windows-x86_64-none");
        let newest = make("cpython-3.12.14-windows-x86_64-none");
        // Another series, the minor version's link name and an empty folder are not installs.
        make("cpython-3.13.1-windows-x86_64-none");
        make(&format!("cpython-{PYTHON_SERIES}-windows-x86_64-none"));
        std::fs::create_dir_all(dir.path().join("cpython-3.12.20-windows-x86_64-none")).unwrap();
        assert_eq!(unpacked_python(dir.path()), Some(newest));
    }
}
