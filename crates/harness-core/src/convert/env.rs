//! The managed conversion environment: a throwaway Python virtual environment that Harness
//! builds itself, only to run llama.cpp's official `convert_hf_to_gguf.py`.
//!
//! Harness needs no Python to run models. Converting a Hugging Face `safetensors` repo to GGUF
//! is the one job only the official Python converter does (it reads every architecture's
//! tensor layout and tokenizer), so it runs here, in a folder Harness owns and the user can
//! delete from Settings at any time (`<data>/bin/convert-env`):
//!
//! ```text
//! uv/<version>/uv        astral-sh/uv release, SHA-256 pinned in this source
//! python/                python-build-standalone, fetched by uv (uv checks its own digests)
//! venv/                  torch (CPU wheel), transformers, numpy, sentencepiece, protobuf...
//! src/<tag>/             convert_hf_to_gguf.py + conversion/ + gguf-py/ of llama.cpp <tag>
//! manifest.json          what was installed
//! ```
//!
//! The converter source is the one of the **same llama.cpp tag as the managed runtime**, so the
//! GGUF it writes is one that runtime loads. The package requirements are read from that tag's
//! own `requirements/requirements-convert_hf_to_gguf.txt`. Nothing here is on the machine's
//! PATH, nothing is installed system-wide, no `pip` runs outside this folder, and the Python
//! side never serves anything: it converts one folder and exits.

use super::{proc, Cancel, Progress};
use crate::error_key::err;
use crate::paths;
use futures_util::StreamExt;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

/// The pinned uv release.
pub const UV_VERSION: &str = "0.12.20";
/// The Python the environment uses (uv fetches the newest patch release it knows).
pub const PYTHON_SERIES: &str = "3.12";
/// Extra packages the llama.cpp `gguf` package needs besides the converter's own requirements.
const EXTRA_PACKAGES: &[&str] = &["tqdm", "pyyaml", "requests"];
/// Rough size of the whole environment on disk (shown before it is built).
pub const ESTIMATED_SIZE_BYTES: u64 = 700 * 1024 * 1024;

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

/// `<data>/bin/convert-env`.
pub fn root() -> PathBuf {
    paths::bin_dir().join("convert-env")
}

pub fn uv_bin_in(root: &Path) -> PathBuf {
    root.join("uv")
        .join(UV_VERSION)
        .join(if cfg!(windows) { "uv.exe" } else { "uv" })
}

pub fn python_bin_in(root: &Path) -> PathBuf {
    if cfg!(windows) {
        root.join("venv").join("Scripts").join("python.exe")
    } else {
        root.join("venv").join("bin").join("python")
    }
}

pub fn source_dir_in(root: &Path, tag: &str) -> PathBuf {
    root.join("src").join(tag)
}

/// A llama.cpp release tag (`b11559`): the only form that goes into a URL or a path here.
pub fn valid_tag(tag: &str) -> bool {
    Regex::new(r"^b[0-9]{3,7}$")
        .expect("static regex")
        .is_match(tag)
}

/// `manifest.json` of the environment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Manifest {
    pub uv_version: String,
    /// `Python 3.12.13`.
    pub python: String,
    /// llama.cpp tag of the converter source the packages were installed for.
    pub tag: String,
    /// SHA-256 over the requirement files of that tag (a different set means a rebuild).
    pub requirements_sha256: String,
    pub installed_at: u64,
}

pub fn read_manifest_in(root: &Path) -> Option<Manifest> {
    serde_json::from_slice(&std::fs::read(root.join("manifest.json")).ok()?).ok()
}

/// The requirement files the converter of `src` reads (the file named and the legacy one it
/// includes), plus the extra packages: the digest decides whether the venv matches.
pub fn requirements_digest(src: &Path) -> Result<String, String> {
    let mut hasher = Sha256::new();
    for name in [
        "requirements-convert_hf_to_gguf.txt",
        "requirements-convert_legacy_llama.txt",
    ] {
        let path = src.join("requirements").join(name);
        let bytes = std::fs::read(&path).map_err(|e| {
            String::from(
                err("fs.read_failed")
                    .with("path", path.display())
                    .with("error", e),
            )
        })?;
        hasher.update(&bytes);
    }
    hasher.update(EXTRA_PACKAGES.join(",").as_bytes());
    Ok(crate::hex::lower(&hasher.finalize()))
}

/// What Settings shows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EnvStatus {
    /// This platform can build the environment (uv has a build for it).
    pub supported: bool,
    /// A complete environment exists (it may be for another llama.cpp tag).
    pub installed: bool,
    /// It matches the managed runtime's tag, so a conversion can start at once.
    pub ready: bool,
    /// Tag of the managed llama.cpp runtime (`None` when it is not installed).
    pub runtime_tag: Option<String>,
    pub manifest: Option<Manifest>,
    pub size_bytes: u64,
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub dir: PathBuf,
    /// Rough size a first build adds.
    pub estimated_size_bytes: u64,
    /// Progress of the build running now.
    pub progress: Option<super::EnvProgress>,
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

/// Environment state under `root` for the runtime tag `runtime_tag`.
pub fn status_in(root: &Path, runtime_tag: Option<&str>) -> EnvStatus {
    let manifest = read_manifest_in(root);
    let installed = manifest.is_some() && python_bin_in(root).is_file();
    let ready = installed
        && runtime_tag.is_some_and(|tag| {
            manifest.as_ref().is_some_and(|m| m.tag == tag)
                && source_dir_in(root, tag)
                    .join("convert_hf_to_gguf.py")
                    .is_file()
        });
    EnvStatus {
        supported: UV.is_some(),
        installed,
        ready,
        runtime_tag: runtime_tag.map(String::from),
        manifest,
        size_bytes: dir_size(root),
        dir: root.to_path_buf(),
        estimated_size_bytes: ESTIMATED_SIZE_BYTES,
        progress: super::env_progress(),
    }
}

pub fn status() -> EnvStatus {
    status_in(&root(), super::runtime_tag().as_deref())
}

/// Deletes the whole environment (nothing else of Harness depends on it).
pub fn remove_in(root: &Path) -> Result<(), String> {
    if root.exists() {
        std::fs::remove_dir_all(root).map_err(|e| {
            String::from(
                err("fs.remove_failed")
                    .with("path", root.display())
                    .with("error", e),
            )
        })?;
    }
    Ok(())
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("HarnessStudio/0.1.0")
        .build()
        .map_err(|e| crate::http_error::transport(&e, "HTTP client"))
}

/// Streams `url` into `dest`, returning the SHA-256 of what arrived.
async fn download(
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
        std::fs::create_dir_all(parent).map_err(|e| {
            String::from(
                err("fs.create_dir_failed")
                    .with("path", parent.display())
                    .with("error", e),
            )
        })?;
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
            return Err(super::CANCELLED.to_string());
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

/// uv itself: downloaded once, checked against the pinned SHA-256.
async fn ensure_uv(root: &Path, cancel: &Cancel, report: &Progress) -> Result<PathBuf, String> {
    let spec = UV.ok_or_else(|| {
        String::from(err("convert.unsupported_platform").with("os", std::env::consts::OS))
    })?;
    let bin = uv_bin_in(root);
    if bin.is_file() {
        return Ok(bin);
    }
    report.set("uv", format!("uv {UV_VERSION}"), spec.size);
    let staging = root.join(".staging-uv");
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
    use std::io::Read;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut bytes = Vec::new();
    match format {
        Archive::Zip => {
            let mut zip =
                zip::ZipArchive::new(std::fs::File::open(archive).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
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
    std::fs::write(target, &bytes).map_err(|e| {
        String::from(
            err("fs.write_failed")
                .with("path", target.display())
                .with("error", e),
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// Whether a path of the llama.cpp source archive belongs to the converter (relative to the
/// archive's top folder).
pub fn is_converter_path(rel: &str) -> bool {
    rel == "convert_hf_to_gguf.py"
        || rel.starts_with("conversion/")
        || rel.starts_with("gguf-py/")
        || rel.starts_with("requirements/")
}

/// Keeps only the converter's files out of a llama.cpp source archive (`.tar.gz`).
pub fn extract_converter(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut count = 0;
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let full = entry
            .path()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        // Drop the archive's top folder (`llama.cpp-b11559/`).
        let Some((_, rel)) = full.split_once('/') else {
            continue;
        };
        if !is_converter_path(rel) || rel.split('/').any(|c| c.is_empty() || c == "..") {
            continue;
        }
        let out = dest.join(rel);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        entry.unpack(&out).map_err(|e| e.to_string())?;
        count += 1;
    }
    if !dest.join("convert_hf_to_gguf.py").is_file() || count == 0 {
        return Err("convert_hf_to_gguf.py is not in the archive".into());
    }
    Ok(())
}

/// The converter source of llama.cpp `tag` (the GitHub tag archive, filtered to the converter).
pub async fn ensure_source(
    root: &Path,
    tag: &str,
    cancel: &Cancel,
    report: &Progress,
) -> Result<PathBuf, String> {
    if !valid_tag(tag) {
        return Err(err("convert.runtime_missing").into());
    }
    let dir = source_dir_in(root, tag);
    if dir.join("convert_hf_to_gguf.py").is_file() && dir.join("requirements").is_dir() {
        return Ok(dir);
    }
    report.set("source", format!("llama.cpp {tag}"), 0);
    let url = format!("https://github.com/ggml-org/llama.cpp/archive/refs/tags/{tag}.tar.gz");
    let staging = root.join(".staging-src");
    let _ = std::fs::remove_dir_all(&staging);
    let archive = staging.join("src.tar.gz");
    let client = client()?;
    let r = report.clone();
    download(&client, &url, &archive, cancel, move |n| r.bytes(n)).await?;
    let (a, target) = (archive.clone(), staging.join("out"));
    tokio::task::spawn_blocking(move || extract_converter(&a, &target))
        .await
        .map_err(|e| e.to_string())??;
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(staging.join("out"), &dir).map_err(|e| {
        String::from(
            err("fs.rename_failed")
                .with("from", staging.display())
                .with("to", dir.display())
                .with("error", e),
        )
    })?;
    let _ = std::fs::remove_dir_all(&staging);
    Ok(dir)
}

fn uv_command(uv: &Path, root: &Path) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(uv);
    cmd.current_dir(root)
        .env("UV_PYTHON_INSTALL_DIR", root.join("python"))
        .env("UV_CACHE_DIR", root.join("cache"))
        .env("UV_NO_CONFIG", "1")
        .env("UV_NO_PROGRESS", "1")
        .env("NO_COLOR", "1");
    cmd
}

async fn run_uv(
    cmd: tokio::process::Command,
    step: &'static str,
    cancel: &Cancel,
    report: &Progress,
) -> Result<(), String> {
    let outcome = proc::run(cmd, &cancel.flag(), |line| {
        if !line.trim().is_empty() {
            report.detail(step, line.trim());
        }
    })
    .await
    .map_err(|e| {
        String::from(
            err("convert.spawn_failed")
                .with("step", step)
                .with("error", e),
        )
    })?;
    if outcome.cancelled {
        return Err(super::CANCELLED.to_string());
    }
    if !outcome.success {
        return Err(err("convert.env_failed")
            .with("step", step)
            .with("log", outcome.log())
            .into());
    }
    Ok(())
}

/// The environment for the runtime tag `tag`: builds whatever is missing. Safe to call when it
/// is complete (returns quickly).
pub async fn ensure_in(
    root: &Path,
    tag: &str,
    cancel: &Cancel,
    report: &Progress,
) -> Result<(), String> {
    let _one_at_a_time = super::ENV_LOCK.lock().await;
    std::fs::create_dir_all(root).map_err(|e| {
        String::from(
            err("fs.create_dir_failed")
                .with("path", root.display())
                .with("error", e),
        )
    })?;
    let uv = ensure_uv(root, cancel, report).await?;
    let src = ensure_source(root, tag, cancel, report).await?;
    let digest = requirements_digest(&src)?;
    let current = read_manifest_in(root);
    let python = python_bin_in(root);
    let complete = current.as_ref().is_some_and(|m| {
        m.requirements_sha256 == digest && m.uv_version == UV_VERSION && m.tag == tag
    }) && python.is_file();
    if !complete {
        // A different set of packages: start the venv again (the Python itself stays).
        let venv = root.join("venv");
        let _ = std::fs::remove_dir_all(&venv);
        let _ = std::fs::remove_file(root.join("manifest.json"));

        report.set("python", format!("Python {PYTHON_SERIES}"), 0);
        let mut cmd = uv_command(&uv, root);
        cmd.args([
            "venv",
            "--python",
            PYTHON_SERIES,
            "--managed-python",
            "--no-config",
        ])
        .arg(&venv);
        run_uv(cmd, "python", cancel, report).await?;

        report.set("packages", "torch, transformers".to_string(), 0);
        let mut cmd = uv_command(&uv, root);
        cmd.args(["pip", "install", "--no-config", "--python"])
            .arg(&python)
            // The requirements file names the CPU-only PyTorch index next to PyPI; the best
            // version across both is `torch==X+cpu`, not the CUDA build PyPI ships on Linux.
            .args(["--index-strategy", "unsafe-best-match", "-r"])
            .arg(
                src.join("requirements")
                    .join("requirements-convert_hf_to_gguf.txt"),
            )
            .args(EXTRA_PACKAGES);
        run_uv(cmd, "packages", cancel, report).await?;
        // The wheels are installed; keeping uv's copy of them would double the size.
        let _ = std::fs::remove_dir_all(root.join("cache"));

        report.set("verify", "convert_hf_to_gguf.py".to_string(), 0);
        let mut cmd = tokio::process::Command::new(&python);
        cmd.current_dir(&src)
            .args(["convert_hf_to_gguf.py", "--help"])
            .envs(python_env(root));
        let outcome = proc::run(cmd, &cancel.flag(), |_| {}).await.map_err(|e| {
            String::from(
                err("convert.spawn_failed")
                    .with("step", "verify")
                    .with("error", e),
            )
        })?;
        if outcome.cancelled {
            return Err(super::CANCELLED.to_string());
        }
        if !outcome.success {
            return Err(err("convert.env_failed")
                .with("step", "verify")
                .with("log", outcome.log())
                .into());
        }
        let manifest = Manifest {
            uv_version: UV_VERSION.to_string(),
            python: python_version(&python).await,
            tag: tag.to_string(),
            requirements_sha256: digest,
            installed_at: crate::capabilities::now_secs(),
        };
        let file = root.join("manifest.json");
        std::fs::write(
            &file,
            serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
        )
        .map_err(|e| {
            String::from(
                err("fs.write_failed")
                    .with("path", file.display())
                    .with("error", e),
            )
        })?;
    }
    // Sources of older tags are of no use any more.
    if let Ok(rd) = std::fs::read_dir(root.join("src")) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy() != tag {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    Ok(())
}

async fn python_version(python: &Path) -> String {
    let mut cmd = tokio::process::Command::new(python);
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
/// everywhere, no network (the model is a local folder), and a private Hugging Face cache.
pub fn python_env(root: &Path) -> Vec<(&'static str, std::ffi::OsString)> {
    vec![
        ("PYTHONNOUSERSITE", "1".into()),
        ("PYTHONUTF8", "1".into()),
        ("PYTHONIOENCODING", "utf-8".into()),
        ("PYTHONUNBUFFERED", "1".into()),
        ("HF_HUB_OFFLINE", "1".into()),
        ("TRANSFORMERS_OFFLINE", "1".into()),
        ("HF_HUB_DISABLE_TELEMETRY", "1".into()),
        ("HF_HOME", root.join("hf-home").into()),
        ("TOKENIZERS_PARALLELISM", "false".into()),
        ("NO_COLOR", "1".into()),
    ]
}
