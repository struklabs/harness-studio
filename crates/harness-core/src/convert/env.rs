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

use super::{Cancel, Progress};
use crate::error_key::err;
use crate::paths;
use crate::pyenv::{self, ErrorKeys, Uv};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub use crate::pyenv::{dir_size, PYTHON_SERIES, UV, UV_VERSION};

/// Error keys of this environment (it keeps its "conversion environment" wording).
const KEYS: ErrorKeys = ErrorKeys {
    unsupported_platform: "convert.unsupported_platform",
    spawn_failed: "convert.spawn_failed",
    env_failed: "convert.env_failed",
};
/// Extra packages the llama.cpp `gguf` package needs besides the converter's own requirements.
const EXTRA_PACKAGES: &[&str] = &["tqdm", "pyyaml", "requests"];
/// Rough size of the whole environment on disk (shown before it is built).
pub const ESTIMATED_SIZE_BYTES: u64 = 700 * 1024 * 1024;

/// `<data>/bin/convert-env`.
pub fn root() -> PathBuf {
    paths::bin_dir().join("convert-env")
}

pub fn uv_bin_in(root: &Path) -> PathBuf {
    pyenv::uv_bin_in(root)
}

pub fn python_bin_in(root: &Path) -> PathBuf {
    pyenv::venv_python(&root.join("venv"))
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
    pyenv::remove_dir(root)
}

/// Whether a path of the llama.cpp source archive belongs to the converter (relative to the
/// archive's top folder).
pub fn is_converter_path(rel: &str) -> bool {
    rel == "convert_hf_to_gguf.py"
        || rel == "convert_lora_to_gguf.py"
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
    if dir.join("convert_hf_to_gguf.py").is_file()
        && dir.join("convert_lora_to_gguf.py").is_file()
        && dir.join("requirements").is_dir()
    {
        return Ok(dir);
    }
    report.set("source", format!("llama.cpp {tag}"), 0);
    let url = format!("https://github.com/ggml-org/llama.cpp/archive/refs/tags/{tag}.tar.gz");
    let staging = root.join(".staging-src");
    let _ = std::fs::remove_dir_all(&staging);
    let archive = staging.join("src.tar.gz");
    let client = pyenv::client()?;
    let r = report.clone();
    pyenv::download(&client, &url, &archive, cancel, move |n| r.bytes(n)).await?;
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

/// The environment for the runtime tag `tag`: builds whatever is missing. Safe to call when it
/// is complete (returns quickly).
pub async fn ensure_in(
    root: &Path,
    tag: &str,
    cancel: &Cancel,
    report: &Progress,
) -> Result<(), String> {
    let _one_at_a_time = super::ENV_LOCK.lock().await;
    pyenv::create_dir(root)?;
    let uv = Uv {
        bin: pyenv::ensure_uv(root, &KEYS, cancel, report).await?,
        cwd: root.to_path_buf(),
        python_dir: root.join("python"),
        cache_dir: root.join("cache"),
    };
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

        uv.create_venv(&venv, &KEYS, cancel, report).await?;

        report.set("packages", "torch, transformers".to_string(), 0);
        let mut cmd = uv.command();
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
        pyenv::run_step(cmd, "packages", &KEYS, cancel, report).await?;
        // The wheels are installed; keeping uv's copy of them would double the size.
        let _ = std::fs::remove_dir_all(&uv.cache_dir);

        report.set("verify", "convert_hf_to_gguf.py".to_string(), 0);
        let mut cmd = crate::process::hidden_tokio_command(&python);
        cmd.current_dir(&src)
            .args(["convert_hf_to_gguf.py", "--help"])
            .envs(python_env(root));
        pyenv::run_step(cmd, "verify", &KEYS, cancel, &Progress::new(|_| {})).await?;
        let manifest = Manifest {
            uv_version: UV_VERSION.to_string(),
            python: pyenv::python_version(&python).await,
            tag: tag.to_string(),
            requirements_sha256: digest,
            installed_at: crate::capabilities::now_secs(),
        };
        pyenv::write_json(&root.join("manifest.json"), &manifest)?;
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

/// Environment variables of every Python run ([`pyenv::python_env`], Hugging Face cache in this
/// environment's folder).
pub fn python_env(root: &Path) -> Vec<(&'static str, std::ffi::OsString)> {
    pyenv::python_env(root)
}
