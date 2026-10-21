use crate::error_key::err;
use crate::hardware::AssetQuery;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[cfg(test)]
const RELEASE_DOWNLOAD_PREFIX: &str = "https://github.com/ggml-org/llama.cpp/releases/download/";

/// Error key: an archive has no usable SHA-256 digest on GitHub and the user hasn't confirmed
/// installing it unverified.
pub const ERR_INTEGRITY_UNVERIFIED: &str = "runtime.integrity_unverified";

/// Whether an asset can be verified before it is extracted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetIntegrity {
    /// GitHub publishes a SHA-256 digest; the download is checked against it.
    Verified,
    /// No digest on the release asset (older releases).
    #[default]
    Missing,
    /// A digest is published but it isn't a SHA-256 value this build can check.
    Invalid,
}

pub fn integrity_of(digest: Option<&str>) -> AssetIntegrity {
    match digest {
        None => AssetIntegrity::Missing,
        Some(d) if normalize_digest(d).is_some() => AssetIntegrity::Verified,
        Some(_) => AssetIntegrity::Invalid,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub download_url: String,
    pub size: u64,
    #[serde(default)]
    pub digest: Option<String>,
    #[serde(default)]
    pub integrity: AssetIntegrity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAssetInfo {
    pub tag_name: String,
    pub asset_name: String,
    pub download_url: String,
    pub size: u64,
    #[serde(default)]
    pub digest: Option<String>,
    /// Companion archives extracted alongside the main one (e.g. CUDA runtime DLLs).
    #[serde(default)]
    pub extra_assets: Vec<ReleaseAsset>,
    /// Integrity of the main archive (see `extra_assets[].integrity` for the companions).
    #[serde(default)]
    pub integrity: AssetIntegrity,
    /// Backend token of the chosen archive (`vulkan`, `rocm`, `cuda`, `sycl`, `cpu`...);
    /// differs from the requested one when the release had no such build (fallback).
    #[serde(default)]
    pub backend: Option<String>,
}

impl ReleaseAssetInfo {
    /// Main + companion archives.
    pub fn assets(&self) -> Vec<ReleaseAsset> {
        let mut out = vec![ReleaseAsset {
            name: self.asset_name.clone(),
            download_url: self.download_url.clone(),
            size: self.size,
            digest: self.digest.clone(),
            integrity: self.integrity,
        }];
        out.extend(self.extra_assets.iter().cloned());
        out
    }

    /// Archives that can't be verified (the UI asks before installing them; background
    /// auto-update never installs them).
    pub fn unverified_assets(&self) -> Vec<ReleaseAsset> {
        self.assets()
            .into_iter()
            .filter(|a| a.integrity != AssetIntegrity::Verified)
            .collect()
    }
}

/// How strictly a release listing is matched against the requested backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickMode {
    /// Only the requested backend. Updates of an installed build and explicit choices use it:
    /// a GPU build is never replaced by another backend (e.g. a CPU-only one) on its own.
    Exact,
    /// The requested backend, else Vulkan (the portable GPU build), else CPU. Only a first
    /// install that follows the hardware recommendation uses it.
    Fallback,
}

/// Picks a numbered build from a GitHub releases listing (newest first): the newest release
/// with an archive for `query`; with [`PickMode::Fallback`] and no such release, the newest
/// with its Vulkan build, then the newest with its CPU build.
///
/// Every candidate backend is looked up across all releases before the next one is tried:
/// GitHub lists a release while its archives are still being uploaded (alphabetically, so
/// `win-cpu` appears about 40 s before `win-vulkan`), and a half-uploaded newest release must
/// not turn a Vulkan/ROCm/CUDA request into a CPU build. The previous complete release is
/// picked instead.
pub fn pick_release(
    releases: &[serde_json::Value],
    query: &AssetQuery,
    mode: PickMode,
) -> Result<ReleaseAssetInfo, String> {
    pick_release_for(crate::engine::llama_cpp(), releases, query, mode)
}

/// [`pick_release`] for the release layout of any engine.
pub fn pick_release_for(
    engine: &dyn crate::engine::Engine,
    releases: &[serde_json::Value],
    query: &AssetQuery,
    mode: PickMode,
) -> Result<ReleaseAssetInfo, String> {
    let mut candidates = vec![query.clone()];
    if mode == PickMode::Fallback {
        candidates.extend(query.vulkan_fallback());
        let cpu = query.cpu_fallback();
        if !candidates.contains(&cpu) {
            candidates.push(cpu);
        }
    }
    for candidate in &candidates {
        if let Some(info) = releases
            .iter()
            .find_map(|release| engine.release_asset(release, candidate))
        {
            return Ok(info);
        }
    }
    if let Some(e) = cuda13_only_error(releases, query, windows_nvidia_driver().as_deref()) {
        return Err(e);
    }
    Err(err("runtime.no_matching_asset")
        .with("os", &query.os)
        .with("backend", query.backend.as_deref().unwrap_or("cpu"))
        .with("arch", &query.arch)
        .into())
}

/// The archive of one llama.cpp release that matches `query` exactly (plus the CUDA runtime
/// companion).
pub(crate) fn release_asset(
    release: &serde_json::Value,
    query: &AssetQuery,
) -> Option<ReleaseAssetInfo> {
    // Only numbered builds (`b1234`); the repo also publishes other tags (e.g. `v0.5.0`)
    // that would break version comparison.
    let tag = release
        .get("tag_name")
        .and_then(|t| t.as_str())
        .unwrap_or("");
    if !is_build_tag(tag) {
        return None;
    }
    let assets = release.get("assets").and_then(|a| a.as_array())?;
    let lookup = |name: &str| -> Option<ReleaseAsset> {
        let a = assets
            .iter()
            .find(|a| a.get("name").and_then(|v| v.as_str()) == Some(name))?;
        let digest = a
            .get("digest")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        Some(ReleaseAsset {
            name: name.to_string(),
            download_url: a.get("browser_download_url")?.as_str()?.to_string(),
            size: a.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
            integrity: integrity_of(digest.as_deref()),
            digest,
        })
    };
    let names: Vec<&str> = assets
        .iter()
        .filter_map(|a| a.get("name")?.as_str())
        .collect();

    let (asset_name, backend_version) = select_asset(&names, query)?;
    let main = lookup(asset_name)?;

    let mut extra_assets = Vec::new();
    if asset_name.contains("-cuda-") {
        if let Some(cudart) = backend_version
            .as_deref()
            .and_then(|ver| select_cudart(&names, query, ver))
            .and_then(lookup)
        {
            extra_assets.push(cudart);
        }
    }

    Some(ReleaseAssetInfo {
        tag_name: tag.to_string(),
        asset_name: main.name,
        download_url: main.download_url,
        size: main.size,
        digest: main.digest,
        extra_assets,
        integrity: main.integrity,
        backend: Some(query.backend.clone().unwrap_or_else(|| {
            if query.os == "macos" {
                "metal".into()
            } else {
                "cpu".into()
            }
        })),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimeDownloadProgress {
    pub current_asset: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomBinaryValidation {
    pub is_valid: bool,
    pub version: Option<String>,
    pub supports_metrics: bool,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct RuntimeManager {
    /// Overrides the runtime root (tests). `None` = `<data root>/bin`.
    pub base_bin_dir: Option<PathBuf>,
    /// Family whose builds this manager installs (llama.cpp unless made with
    /// [`RuntimeManager::for_engine`]).
    engine: crate::engine::EngineRef,
    progress: Mutex<Option<RuntimeDownloadProgress>>,
    /// Staging/activation is single-flight (manual UI flow vs. background auto-update).
    busy: std::sync::atomic::AtomicBool,
    /// Asset name / download URL → published digest (`None` = the release lists the asset
    /// without a digest).
    asset_digests: Mutex<std::collections::HashMap<String, Option<String>>>,
    /// GitHub requests made by [`Self::companion_digest`] (tests: a recorded digest needs none).
    companion_lookups: std::sync::atomic::AtomicUsize,
}

/// Holds the runtime's "busy" flag for the duration of a staging operation.
pub struct BusyGuard<'a>(&'a std::sync::atomic::AtomicBool);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

pub fn binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

/// Picks the release asset matching `query`. When several backend versions exist
/// (e.g. CUDA 12.4 and 13.4) the lowest, most driver-compatible one is chosen.
/// Returns `(asset_name, backend_version)`.
pub fn select_asset<'a>(
    names: &[&'a str],
    query: &AssetQuery,
) -> Option<(&'a str, Option<String>)> {
    let driver = (query.os == "win" && query.backend.as_deref() == Some("cuda"))
        .then(windows_nvidia_driver)
        .flatten();
    select_asset_for_driver(names, query, driver.as_deref())
}

/// NVIDIA driver version (`617.42`) used to filter Windows CUDA builds; cached for a minute
/// because `select_asset` runs once per release of a listing.
pub(crate) fn windows_nvidia_driver() -> Option<String> {
    static CACHE: Mutex<Option<(std::time::Instant, Option<String>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, v)) = cache.as_ref() {
        if at.elapsed() < std::time::Duration::from_secs(60) {
            return v.clone();
        }
    }
    let v = crate::hardware::detect_hardware()
        .driver_info
        .cuda_driver_version;
    *cache = Some((std::time::Instant::now(), v.clone()));
    v
}

/// Windows CUDA request that no listed release can serve because only 13.x (driver >= 580)
/// packages exist while the driver is older or unknown: the reason for the user.
fn cuda13_only_error(
    releases: &[serde_json::Value],
    query: &AssetQuery,
    driver: Option<&str>,
) -> Option<String> {
    if query.os != "win" || query.backend.as_deref() != Some("cuda") {
        return None;
    }
    let serves = |driver: &str| {
        releases.iter().any(|r| {
            r.get("assets")
                .and_then(|a| a.as_array())
                .is_some_and(|assets| {
                    let names: Vec<&str> = assets
                        .iter()
                        .filter_map(|a| a.get("name")?.as_str())
                        .collect();
                    select_asset_for_driver(&names, query, Some(driver)).is_some()
                })
        })
    };
    (!serves(driver.unwrap_or("0.0")) && serves("999.0")).then(|| {
        err("runtime.cuda_driver_too_old")
            .with("driver", driver.unwrap_or("?"))
            .into()
    })
}

/// [`select_asset`] with an explicit NVIDIA driver version for Windows CUDA builds.
pub fn select_asset_for_driver<'a>(
    names: &[&'a str],
    query: &AssetQuery,
    driver: Option<&str>,
) -> Option<(&'a str, Option<String>)> {
    let backend = match &query.backend {
        Some(b) => format!(r"{}(?:-(\d+(?:\.\d+)*))?-", regex::escape(b)),
        None => String::new(),
    };
    let re = Regex::new(&format!(
        r"^llama-[^-]+-bin-{}-{}{}\.(?:zip|tar\.gz)$",
        regex::escape(&query.os),
        backend,
        regex::escape(&query.arch)
    ))
    .ok()?;

    let mut matches: Vec<(&str, Option<String>)> = names
        .iter()
        .filter_map(|n| {
            re.captures(n)
                .map(|c| (*n, c.get(1).map(|m| m.as_str().to_string())))
        })
        .collect();
    // Windows CUDA majors by driver (NVIDIA CUDA release notes, Table 2 "Minimum Required Driver
    // Version for CUDA Minor Version Compatibility": 12.x >=528.33 per
    // https://docs.nvidia.com/cuda/archive/12.4.0/cuda-toolkit-release-notes/index.html, 13.x >=580
    // per https://docs.nvidia.com/cuda/archive/13.0.0/cuda-toolkit-release-notes/index.html):
    // driver >= 580 accepts 12.x and 13.x (the lowest, 12.4, still wins when both exist: it is
    // verified on the NVIDIA acceptance run b11457 with driver 617.42; 13.4 also ran on b11327,
    // 02.10); a known older or unknown driver gets 12.x only.
    if query.os == "win" && query.backend.as_deref() == Some("cuda") {
        let cuda13_ok = crate::hardware::windows_driver_at_least(driver, (580, 0)) == Some(true);
        matches.retain(|(_, ver)| {
            cuda13_ok
                || ver
                    .as_deref()
                    .is_none_or(|v| v.split('.').next() == Some("12"))
        });
    }
    matches.sort_by_key(|(_, ver)| {
        ver.as_deref()
            .map(|v| {
                v.split('.')
                    .map(|p| p.parse::<u32>().unwrap_or(0))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    matches.into_iter().next()
}

/// Backend of an installed llama.cpp build, from the ggml backend libraries next to the
/// binary (`ggml-hip.dll`, `libggml-vulkan.so`...): `cuda`, `rocm`, `sycl`, `vulkan`, `metal`,
/// `cpu`; `None` when the directory has no ggml libraries (static builds).
pub fn detect_backend_in_dir(dir: &Path) -> Option<String> {
    let names: Vec<String> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_lowercase())
        .collect();
    let has = |lib: &str| {
        names.iter().any(|n| {
            let n = n.strip_prefix("lib").unwrap_or(n);
            n.starts_with(&format!("ggml-{lib}.")) || n.starts_with(&format!("ggml-{lib}-"))
        })
    };
    [
        ("cuda", "cuda"),
        ("hip", "rocm"),
        ("sycl", "sycl"),
        ("vulkan", "vulkan"),
        ("metal", "metal"),
        ("cpu", "cpu"),
    ]
    .into_iter()
    .find(|(lib, _)| has(lib))
    .map(|(_, token)| token.to_string())
    .or_else(|| cfg!(target_os = "macos").then(|| "metal".to_string()))
}

/// DLLs the backend library in `dir` imports that cannot be found next to it, in the system
/// directory or on `PATH` (Windows only; empty elsewhere or when nothing is missing).
pub fn missing_backend_libraries(dir: &Path, backend: &str) -> Vec<String> {
    let lib = match backend {
        "rocm" => "ggml-hip.dll",
        "sycl" => "ggml-sycl.dll",
        _ => return Vec::new(),
    };
    if !cfg!(windows) {
        return Vec::new();
    }
    let Ok(bytes) = fs::read(dir.join(lib)) else {
        return Vec::new();
    };
    let mut search: Vec<PathBuf> = vec![dir.to_path_buf()];
    if let Some(root) = std::env::var_os("SystemRoot") {
        search.push(Path::new(&root).join("System32"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        search.extend(std::env::split_paths(&path));
    }
    let mut missing: Vec<String> = pe_imports(&bytes)
        .into_iter()
        .filter(|name| {
            let l = name.to_ascii_lowercase();
            !l.starts_with("api-ms-win-") && !l.starts_with("ext-ms-")
        })
        .filter(|name| !search.iter().any(|d| d.join(name).is_file()))
        .collect();
    missing.sort();
    missing.dedup();
    missing
}

/// Names of the DLLs a PE32/PE32+ image imports (its import directory); empty for anything
/// that does not parse.
fn pe_imports(b: &[u8]) -> Vec<String> {
    fn u16at(b: &[u8], o: usize) -> Option<usize> {
        Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?) as usize)
    }
    fn u32at(b: &[u8], o: usize) -> Option<usize> {
        Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?) as usize)
    }
    fn parse(b: &[u8]) -> Option<Vec<String>> {
        let pe = u32at(b, 0x3c)?;
        if b.get(pe..pe + 4)? != b"PE  " {
            return None;
        }
        let sections = u16at(b, pe + 6)?;
        let opt_size = u16at(b, pe + 20)?;
        let opt = pe + 24;
        // Data directory 1 (imports) follows 96 (PE32) or 112 (PE32+) bytes of header.
        let dir_off = match u16at(b, opt)? {
            0x10b => opt + 96,
            0x20b => opt + 112,
            _ => return None,
        } + 8;
        let import_rva = u32at(b, dir_off)?;
        if import_rva == 0 {
            return Some(Vec::new());
        }
        let table = opt + opt_size;
        let to_offset = |rva: usize| -> Option<usize> {
            (0..sections).find_map(|i| {
                let s = table + i * 40;
                let (virt, size, raw) = (
                    u32at(b, s + 12)?,
                    u32at(b, s + 8)?.max(u32at(b, s + 16)?),
                    u32at(b, s + 20)?,
                );
                (rva >= virt && rva < virt + size).then(|| rva - virt + raw)
            })
        };
        let mut out = Vec::new();
        let mut d = to_offset(import_rva)?;
        loop {
            let name_rva = u32at(b, d + 12)?;
            if name_rva == 0 {
                break;
            }
            let at = to_offset(name_rva)?;
            let end = b.get(at..)?.iter().position(|c| *c == 0)?;
            out.push(String::from_utf8_lossy(&b[at..at + end]).into_owned());
            d += 20;
        }
        Some(out)
    }
    parse(b).unwrap_or_default()
}

/// What an install or update of the managed runtime looks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallTarget {
    pub query: AssetQuery,
    pub mode: PickMode,
}

/// Release asset query for installing or updating the managed runtime. Precedence: an
/// explicit request (the user picked a backend), the recorded backend of the installed build
/// (`managed_runtime.backend`), the backend detected from the installed files (updates never
/// switch backends on their own), and last the hardware recommendation (first install).
///
/// Only the recommendation may fall back to another backend ([`PickMode::Fallback`]); every
/// other source is matched exactly.
pub fn install_query(
    hw: &crate::hardware::SystemHardwareInfo,
    configured: Option<&str>,
    installed: Option<&str>,
    requested: Option<&str>,
) -> InstallTarget {
    let pick = |t: Option<&str>| {
        t.map(str::trim)
            .filter(|t| !t.is_empty())
            .and_then(crate::hardware::RecommendedBackend::from_token)
    };
    let (backend, mode) = match pick(requested)
        .or_else(|| pick(configured))
        .or_else(|| pick(installed))
    {
        Some(b) => (b, PickMode::Exact),
        None => (hw.recommended_backend, PickMode::Fallback),
    };
    InstallTarget {
        query: backend.asset_query(&hw.os, &hw.arch),
        mode,
    }
}

/// Same backend token for comparisons: `hip` = `rocm`, `sycl-fp16` = `sycl`, `cuda12` = `cuda`.
pub fn normalize_backend(token: &str) -> String {
    crate::hardware::RecommendedBackend::from_token(token)
        .map(|b| b.as_str().to_string())
        .unwrap_or_else(|| token.trim().to_ascii_lowercase())
}

/// `--list-devices` id prefix of a GPU backend's devices (`None` for CPU/Metal builds).
fn gpu_device_prefix(backend: &str) -> Option<&'static str> {
    match normalize_backend(backend).as_str() {
        "vulkan" => Some("Vulkan"),
        "cuda" => Some("CUDA"),
        "rocm" => Some("ROCm"),
        "sycl" => Some("SYCL"),
        _ => None,
    }
}

/// Written into a build's folder when it is activated: which release and backend the folder
/// holds, so the backup slot says what "roll back" restores.
pub const RUNTIME_MANIFEST: &str = ".harness-runtime.json";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RuntimeManifest {
    /// Release tag (`b11298`); `None` when unknown.
    pub version: Option<String>,
    /// Backend token of the build's files (`vulkan`, `rocm`, `cpu`...).
    pub backend: Option<String>,
    /// SHA-256 of the CUDA runtime archive the folder's cudart/cuBLAS came from (the shared
    /// library store's key). Written at install; for older builds by the first
    /// [`crate::shared_libs::dedupe_installed`] that looked it up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub companion_sha256: Option<String>,
    /// Runtime family (`llama.cpp`); folders written before families existed have none
    /// and are llama.cpp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
}

impl RuntimeManifest {
    pub fn family(&self) -> &str {
        self.family.as_deref().unwrap_or(crate::engine::LLAMA_CPP)
    }
}

/// Records the companion archive digest in a build folder's manifest (keeps the other fields).
pub fn record_companion(dir: &Path, digest: &str) -> Result<(), String> {
    let mut m = read_manifest(dir);
    m.companion_sha256 = Some(digest.to_string());
    write_manifest(dir, &m)
}

/// The manifest of a build folder; for folders from before manifests existed the backend is
/// detected from the files and the version is unknown.
pub fn read_manifest(dir: &Path) -> RuntimeManifest {
    let mut m: RuntimeManifest = fs::read_to_string(dir.join(RUNTIME_MANIFEST))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if m.backend.is_none() {
        m.backend = detect_backend_in_dir(dir);
    }
    m
}

/// Records an activation in the settings: the new build is installed, the one it replaced is
/// the previous version, and the build's backend is kept so later updates stay on it.
pub fn record_activation(settings: &mut crate::config::AppSettings, activated: &RuntimeManifest) {
    let mr = &mut settings.managed_runtime;
    mr.previous_version = mr.installed_version.take();
    mr.installed_version = activated.version.clone();
    if activated.backend.is_some() {
        mr.backend = activated.backend.clone();
    }
}

/// Records a rollback: the restored build is installed again, the one it replaced is now the
/// previous version (the backup slot holds it), and the restored build's backend is kept.
pub fn record_rollback(settings: &mut crate::config::AppSettings, restored: &RuntimeManifest) {
    let mr = &mut settings.managed_runtime;
    let replaced = mr.installed_version.take();
    mr.installed_version = restored
        .version
        .clone()
        .or_else(|| mr.previous_version.clone());
    mr.previous_version = replaced;
    if restored.backend.is_some() {
        mr.backend = restored.backend.clone();
    }
}

/// Finds the CUDA runtime companion archive for a CUDA build.
pub fn select_cudart<'a>(
    names: &[&'a str],
    query: &AssetQuery,
    cuda_version: &str,
) -> Option<&'a str> {
    let needle = format!("bin-{}-cuda-{}-{}.", query.os, cuda_version, query.arch);
    names
        .iter()
        .copied()
        .find(|n| n.starts_with("cudart-") && n.contains(&needle))
}

impl RuntimeManager {
    /// Manager of another family's managed builds, under `bin/<family>/` (llama.cpp keeps
    /// `bin/` itself, where its builds have always been).
    pub fn for_engine(engine: &'static dyn crate::engine::Engine) -> Self {
        Self {
            engine: crate::engine::EngineRef(engine),
            ..Default::default()
        }
    }

    pub fn engine(&self) -> &'static dyn crate::engine::Engine {
        self.engine.0
    }

    /// File name of the family's executable.
    fn bin(&self) -> String {
        self.engine.release_source().binary_file()
    }

    fn base(&self) -> PathBuf {
        self.base_bin_dir.clone().unwrap_or_else(|| {
            let bin = crate::paths::bin_dir();
            match self.engine.family() {
                crate::engine::LLAMA_CPP => bin,
                family => bin.join(family),
            }
        })
    }

    pub fn managed_dir(&self) -> PathBuf {
        self.base().join("managed")
    }

    pub fn staging_dir(&self) -> PathBuf {
        self.base().join("staging")
    }

    pub fn backup_dir(&self) -> PathBuf {
        self.base().join("backup")
    }

    pub fn get_managed_binary_path(&self) -> Option<PathBuf> {
        let managed_exe = self.managed_dir().join(self.bin());
        managed_exe.exists().then_some(managed_exe)
    }

    /// Backend of the installed managed build (from its files).
    pub fn installed_backend(&self) -> Option<String> {
        self.get_managed_binary_path()
            .and_then(|_| detect_backend_in_dir(&self.managed_dir()))
    }

    pub fn has_backup(&self) -> bool {
        self.backup_dir().join(self.bin()).exists()
    }

    /// A download/activation holds the staging area right now.
    pub fn is_busy(&self) -> bool {
        self.busy.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Claims the staging area; fails if another download/activation is running.
    pub fn try_busy(&self) -> Result<BusyGuard<'_>, String> {
        self.busy
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .map(|_| BusyGuard(&self.busy))
            .map_err(|_| err("runtime.busy").into())
    }

    pub fn clear_download_progress(&self) {
        *self.progress.lock().unwrap() = None;
    }

    pub fn get_download_progress(&self) -> Option<RuntimeDownloadProgress> {
        self.progress.lock().unwrap().clone()
    }

    fn client() -> Result<reqwest::Client, String> {
        reqwest::Client::builder()
            .user_agent("HarnessStudio/0.2.0")
            .build()
            .map_err(|e| crate::http_error::transport(&e, "HTTP client"))
    }

    pub async fn check_latest_release(
        &self,
        query: &AssetQuery,
        mode: PickMode,
    ) -> Result<ReleaseAssetInfo, String> {
        let url = self.engine.release_source().releases_api(5);
        let url = url.as_str();
        let res = Self::client()?
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
        let releases = res
            .json::<Vec<serde_json::Value>>()
            .await
            .map_err(|e| crate::http_error::transport(&e, url))?;
        for release in &releases {
            self.ingest_release_digests(release);
        }
        pick_release_for(self.engine(), &releases, query, mode)
    }

    /// Remembers the digest (or its absence) of every asset of a GitHub release object.
    pub fn ingest_release_digests(&self, release: &serde_json::Value) {
        let Some(assets) = release.get("assets").and_then(|a| a.as_array()) else {
            return;
        };
        let mut cache = self.asset_digests.lock().unwrap();
        for a in assets {
            let digest = a
                .get("digest")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            for key in ["name", "browser_download_url"] {
                if let Some(k) = a.get(key).and_then(|v| v.as_str()) {
                    cache.insert(k.to_string(), digest.clone());
                }
            }
        }
    }

    pub fn register_asset_digest(&self, url_or_name: &str, digest: &str) {
        self.asset_digests
            .lock()
            .unwrap()
            .insert(url_or_name.to_string(), Some(digest.to_string()));
    }

    fn cached_digest(&self, url: &str, name: &str) -> Option<Option<String>> {
        let cache = self.asset_digests.lock().unwrap();
        cache.get(url).or_else(|| cache.get(name)).cloned()
    }

    /// Published digest of a release archive (`None` = none published or the release
    /// couldn't be read; both count as unverified).
    async fn get_or_fetch_digest(&self, url: &str, name: &str) -> Option<String> {
        if let Some(known) = self.cached_digest(url, name) {
            return known;
        }
        let source = self.engine.release_source();
        let tag = url
            .strip_prefix(&source.download_prefix())?
            .split('/')
            .next()?;
        let api_url = source.release_api(tag);
        let res = Self::client().ok()?.get(&api_url).send().await.ok()?;
        if !res.status().is_success() {
            return None;
        }
        let release = res.json::<serde_json::Value>().await.ok()?;
        self.ingest_release_digests(&release);
        self.cached_digest(url, name).flatten()
    }

    /// Number of [`Self::companion_digest`] calls (each one GitHub request).
    pub fn companion_lookups(&self) -> usize {
        self.companion_lookups
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Published digest of the CUDA runtime archive of release `tag` for CUDA `major` on this
    /// OS/arch (`cudart-llama-bin-win-cuda-12.4-x64.zip` for `12`). `None` offline or when the
    /// release has none.
    pub async fn companion_digest(&self, tag: &str, major: &str) -> Option<String> {
        self.companion_lookups
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let api_url = self.engine.release_source().release_api(tag);
        let res = Self::client().ok()?.get(&api_url).send().await.ok()?;
        if !res.status().is_success() {
            return None;
        }
        let release = res.json::<serde_json::Value>().await.ok()?;
        self.ingest_release_digests(&release);
        let (os, arch) = if cfg!(windows) {
            ("win", "x64")
        } else {
            (std::env::consts::OS, std::env::consts::ARCH)
        };
        let needle = format!("-bin-{os}-cuda-{major}.");
        let name = release
            .get("assets")?
            .as_array()?
            .iter()
            .filter_map(|a| a.get("name")?.as_str())
            .find(|n| {
                crate::shared_libs::is_companion(n) && n.contains(&needle) && n.contains(arch)
            })?
            .to_string();
        self.cached_digest(&name, &name)
            .flatten()
            .as_deref()
            .and_then(normalize_digest)
    }

    /// Archives among `urls` without a checkable SHA-256 digest.
    pub async fn unverified_urls(&self, urls: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for url in urls {
            let name = url.rsplit('/').next().unwrap_or("asset");
            let digest = self.get_or_fetch_digest(url, name).await;
            if integrity_of(digest.as_deref()) != AssetIntegrity::Verified {
                out.push(url.clone());
            }
        }
        out
    }

    /// Downloads the main archive plus companions (streamed to disk) and extracts
    /// them into the staging directory. Only official llama.cpp release URLs are accepted.
    ///
    /// Every archive must have a published SHA-256 digest; otherwise nothing is downloaded and
    /// [`ERR_INTEGRITY_UNVERIFIED`] is returned, unless `allow_unverified` (the user confirmed
    /// in the UI after seeing the asset name, size and source URL).
    pub async fn download_and_extract_staging(
        &self,
        urls: &[String],
        allow_unverified: bool,
    ) -> Result<(), String> {
        if urls.is_empty() {
            return Err(err("runtime.nothing_to_download").into());
        }
        let prefix = self.engine.release_source().download_prefix();
        for url in urls {
            if !url.starts_with(&prefix) || url.contains("..") {
                return Err(err("runtime.unofficial_url").with("url", url).into());
            }
        }
        if !allow_unverified && !self.unverified_urls(urls).await.is_empty() {
            return Err(ERR_INTEGRITY_UNVERIFIED.to_string());
        }

        let staging = self.staging_dir();
        if tokio::fs::try_exists(&staging).await.unwrap_or(false) {
            let _ = tokio::fs::remove_dir_all(&staging).await;
        }
        tokio::fs::create_dir_all(&staging)
            .await
            .map_err(|e| create_dir_failed(&staging, e))?;
        let downloads = self.base().join("downloads");
        tokio::fs::create_dir_all(&downloads)
            .await
            .map_err(|e| create_dir_failed(&downloads, e))?;

        let store = self.shared_store();
        let mut companion = None;
        let result = async {
            for url in urls {
                let name = url.rsplit('/').next().unwrap_or("asset").to_string();
                let archive = downloads.join(&name);
                let digest = self.get_or_fetch_digest(url, &name).await;
                // The CUDA runtime archive another build already brought: no download.
                let shared = digest
                    .as_deref()
                    .and_then(normalize_digest)
                    .filter(|_| crate::shared_libs::is_companion(&name));
                if shared.is_some() {
                    companion = shared.clone();
                }
                if let Some(key) = &shared {
                    let (s, k, d) = (store.clone(), key.clone(), staging.clone());
                    if tokio::task::spawn_blocking(move || s.restore(&k, &d))
                        .await
                        .unwrap_or(false)
                    {
                        continue;
                    }
                }
                self.download_to_file(url, &archive, &name, digest.as_deref())
                    .await?;
                let (a, d, s) = (archive.clone(), staging.clone(), store.clone());
                let extracted = tokio::task::spawn_blocking(move || match shared {
                    Some(key) if s.add(&key, &a, &d).is_ok() => Ok(()),
                    _ => extract_archive(&a, &d),
                })
                .await
                .map_err(|e| String::from(err("task.join_failed").with("error", e)))?;
                let _ = tokio::fs::remove_file(&archive).await;
                extracted?;
            }
            let (staging_clone, companion) = (staging.clone(), companion.clone());
            let binary = self.bin();
            tokio::task::spawn_blocking(move || {
                flatten_to_binary_dir_for(&staging_clone, &binary)?;
                // The shared library store's key, so later passes need no GitHub request.
                match companion {
                    Some(d) => record_companion(&staging_clone, &d),
                    None => Ok(()),
                }
            })
            .await
            .map_err(|e| String::from(err("task.join_failed").with("error", e)))?
        }
        .await;

        *self.progress.lock().unwrap() = None;
        if result.is_err() {
            let _ = tokio::fs::remove_dir_all(&staging).await;
            let _ = tokio::task::spawn_blocking(move || store.prune()).await;
        }
        result
    }

    /// Shared companion libraries of the builds under this base ([`crate::shared_libs`]).
    pub fn shared_store(&self) -> crate::shared_libs::SharedStore {
        crate::shared_libs::SharedStore::in_bin(&self.base())
    }

    pub async fn download_to_file(
        &self,
        url: &str,
        dest: &Path,
        label: &str,
        expected_digest: Option<&str>,
    ) -> Result<(), String> {
        use futures_util::StreamExt;
        use sha2::{Digest, Sha256};
        use tokio::io::AsyncWriteExt;

        let res = Self::client()?
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
        let total = res.content_length().unwrap_or(0);
        *self.progress.lock().unwrap() = Some(RuntimeDownloadProgress {
            current_asset: label.to_string(),
            downloaded_bytes: 0,
            total_bytes: total,
        });

        let write_failed = |e: std::io::Error| -> String {
            err("fs.write_failed")
                .with("path", dest.display())
                .with("error", e)
                .into()
        };
        let mut file = tokio::fs::File::create(dest).await.map_err(write_failed)?;
        let mut stream = res.bytes_stream();
        let mut downloaded = 0u64;
        let mut hasher = Sha256::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| crate::http_error::transport(&e, url))?;
            hasher.update(&chunk);
            if let Err(e) = file.write_all(&chunk).await {
                let _ = tokio::fs::remove_file(dest).await;
                return Err(write_failed(e));
            }
            downloaded += chunk.len() as u64;
            if let Some(p) = self.progress.lock().unwrap().as_mut() {
                p.downloaded_bytes = downloaded;
            }
        }
        if let Err(e) = file.flush().await {
            let _ = tokio::fs::remove_file(dest).await;
            return Err(write_failed(e));
        }
        drop(file);

        if total > 0 && downloaded != total {
            let _ = tokio::fs::remove_file(dest).await;
            return Err(err("runtime.download_incomplete")
                .num("downloaded", downloaded)
                .num("total", total)
                .into());
        }

        let computed_digest = crate::hex::lower(&hasher.finalize());
        if let Some(expected_raw) = expected_digest {
            if let Some(expected_hex) = normalize_digest(expected_raw) {
                if computed_digest != expected_hex {
                    let _ = tokio::fs::remove_file(dest).await;
                    return Err(err("runtime.digest_mismatch")
                        .with("expected", expected_hex)
                        .with("actual", computed_digest)
                        .into());
                }
            } else {
                eprintln!(
                    "[Runtime] Uyarı: '{}' için geçersiz digest formatı: '{}'; kullanıcı onayıyla doğrulanmadan kuruluyor.",
                    label, expected_raw
                );
            }
        } else {
            eprintln!(
                "[Runtime] Uyarı: '{}' için SHA-256 özeti (digest) yok; kullanıcı onayıyla doğrulanmadan kuruluyor.",
                label
            );
        }

        Ok(())
    }

    /// Aşama 1: Temel binary doğrulama
    /// Staging klasöründeki binary'yi `--version` ile test eder.
    /// Başarısız olursa staging'i siler, mevcut runtime'a dokunmaz!
    pub fn verify_stage1_binary(&self) -> Result<String, String> {
        let staging = self.staging_dir();
        let binary_path = staging.join(self.bin());

        if !binary_path.exists() {
            let _ = fs::remove_dir_all(&staging);
            return Err(err("runtime.package_missing_binary")
                .with("binary", self.bin())
                .into());
        }

        let mut cmd = crate::process::quiet_command(&binary_path);
        cmd.arg("--version").current_dir(&staging);
        let output = run_command_with_timeout(&mut cmd, PROBE_TIMEOUT, PROBE_MAX_BYTES);

        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                self.verify_stage1_devices()?;
                Ok(if !stdout.is_empty() { stdout } else { stderr })
            }
            Ok(out) => {
                let _ = fs::remove_dir_all(&staging);
                let stderr = String::from_utf8_lossy(&out.stderr);
                if let Some(library) = missing_shared_library(&stderr) {
                    return Err(err("runtime.verify_missing_library")
                        .with("packages", library_packages(&library))
                        .with("library", library)
                        .into());
                }
                Err(err("runtime.verify_exit").with("status", out.status).into())
            }
            Err(e) => {
                let _ = fs::remove_dir_all(&staging);
                Err(err("runtime.verify_run_failed").with("error", e).into())
            }
        }
    }

    /// Stage 1b for GPU builds that depend on the local driver stack (CUDA, ROCm/HIP, SYCL):
    /// the staged binary must list at least one device of its backend, otherwise staging is
    /// removed (`runtime.backend_no_device`) and the current runtime stays untouched.
    pub fn verify_stage1_devices(&self) -> Result<(), String> {
        let staging = self.staging_dir();
        let Some(backend) = detect_backend_in_dir(&staging) else {
            return Ok(());
        };
        let prefix = match backend.as_str() {
            "cuda" => "CUDA",
            "rocm" => "ROCm",
            "sycl" => "SYCL",
            _ => return Ok(()),
        };
        let devices = crate::devices::probe(&staging.join(self.bin())).unwrap_or_default();
        if devices.iter().any(|d| d.id.starts_with(prefix)) {
            Ok(())
        } else {
            // llama-server skips a backend library that cannot load without a word; name the
            // DLLs it needs that this machine does not have (the official Windows ROCm build
            // wants the HIP SDK's `hipblas.dll`, which is not in the archive).
            let missing = missing_backend_libraries(&staging, &backend);
            let _ = fs::remove_dir_all(&staging);
            if missing.is_empty() {
                Err(format!("runtime.backend_no_device: {backend}"))
            } else {
                Err(format!(
                    "runtime.backend_no_device: {backend} (missing: {})",
                    missing.join(", ")
                ))
            }
        }
    }

    /// Stage 1c for updates: the staged build must be the backend the installed one is
    /// (`expected`), and a GPU build must still see a device of its backend when the installed
    /// build does. Otherwise staging is removed and the installed build stays untouched
    /// (`runtime.update_backend_mismatch` / `runtime.update_no_device`).
    pub fn verify_staged_backend(&self, expected: &str) -> Result<(), String> {
        let staging = self.staging_dir();
        let fail = |e: String| -> Result<(), String> {
            let _ = fs::remove_dir_all(&staging);
            Err(e)
        };
        let expected_n = normalize_backend(expected);
        if let Some(found) = detect_backend_in_dir(&staging) {
            if normalize_backend(&found) != expected_n {
                return fail(
                    err("runtime.update_backend_mismatch")
                        .with("expected", &expected_n)
                        .with("found", found)
                        .into(),
                );
            }
        }
        if let Some(prefix) = gpu_device_prefix(&expected_n) {
            let sees_device = |binary: &Path| {
                crate::devices::probe(binary)
                    .map(|d| d.iter().any(|d| d.id.starts_with(prefix)))
                    .unwrap_or(false)
            };
            let installed_sees = self
                .get_managed_binary_path()
                .is_some_and(|b| sees_device(&b));
            if installed_sees && !sees_device(&staging.join(self.bin())) {
                return fail(
                    err("runtime.update_no_device")
                        .with("backend", &expected_n)
                        .into(),
                );
            }
        }
        Ok(())
    }

    /// Stage 2: activation. The staged build becomes `managed` and the build it replaces moves
    /// to the backup slot (what "roll back" restores). `version` is the release tag, recorded
    /// with the build's backend in the build's manifest ([`RUNTIME_MANIFEST`]), which is
    /// returned.
    ///
    /// Only a working build (one with the binary) is moved to the backup slot: an empty or
    /// broken `managed` folder never replaces the build a rollback would restore.
    pub fn activate_runtime(&self, version: Option<&str>) -> Result<RuntimeManifest, String> {
        let staging = self.staging_dir();
        let managed = self.managed_dir();
        let backup = self.backup_dir();

        if !staging.join(self.bin()).exists() {
            return Err(err("runtime.no_staging").into());
        }
        let mut manifest = read_manifest(&staging);
        if let Some(v) = version {
            manifest.version = Some(v.to_string());
        }
        manifest.family = Some(self.engine.family().to_string());
        write_manifest(&staging, &manifest)?;

        let moved_to_backup = if self.get_managed_binary_path().is_some() {
            if backup.exists() {
                fs::remove_dir_all(&backup).map_err(|e| remove_failed(&backup, e))?;
            }
            rename_dir(&managed, &backup)?;
            true
        } else {
            if managed.exists() {
                fs::remove_dir_all(&managed).map_err(|e| remove_failed(&managed, e))?;
            }
            false
        };

        if let Err(e) = rename_dir(&staging, &managed) {
            // Put the previous runtime back so we never end up without one.
            if moved_to_backup && !managed.exists() {
                let _ = rename_dir(&backup, &managed);
            }
            return Err(e);
        }
        // The build that left the backup slot may have held the last link of a library.
        self.shared_store().prune();
        Ok(manifest)
    }

    /// "Roll back": the backup build becomes `managed` again and the build it replaces takes
    /// its place in the backup slot (so the rollback can be undone). Returns the restored
    /// build's manifest. `runtime.no_backup` when the slot holds no build (the installed one is
    /// never removed for an empty backup).
    pub fn rollback_runtime(&self) -> Result<RuntimeManifest, String> {
        let managed = self.managed_dir();
        let backup = self.backup_dir();
        if !self.has_backup() {
            return Err(err("runtime.no_backup").into());
        }
        let restored = read_manifest(&backup);
        let parked = self.base().join("rollback");
        if parked.exists() {
            fs::remove_dir_all(&parked).map_err(|e| remove_failed(&parked, e))?;
        }
        let current_works = self.get_managed_binary_path().is_some();
        if managed.exists() {
            rename_dir(&managed, &parked)?;
        }
        if let Err(e) = rename_dir(&backup, &managed) {
            if parked.exists() && !managed.exists() {
                let _ = rename_dir(&parked, &managed);
            }
            return Err(e);
        }
        if parked.exists() {
            if current_works {
                let _ = rename_dir(&parked, &backup);
            } else {
                let _ = fs::remove_dir_all(&parked);
                self.shared_store().prune();
            }
        }
        Ok(restored)
    }

    /// Manifest of the build in the backup slot (`None` when the slot holds no build).
    pub fn backup_manifest(&self) -> Option<RuntimeManifest> {
        self.has_backup().then(|| read_manifest(&self.backup_dir()))
    }

    /// Custom binary minimum uyumluluk doğrulaması
    pub fn validate_custom_binary(path: &Path) -> CustomBinaryValidation {
        let invalid = |msg: crate::error_key::ErrorKey| CustomBinaryValidation {
            is_valid: false,
            version: None,
            supports_metrics: false,
            error: Some(msg.into()),
        };
        if !path.is_file() {
            return invalid(err("fs.not_a_file").with("path", path.display()));
        }

        let parent_dir = path.parent().unwrap_or(Path::new("."));

        let mut cmd_ver = crate::process::quiet_command(path);
        cmd_ver.arg("--version").current_dir(parent_dir);
        let version_str =
            match run_command_with_timeout(&mut cmd_ver, PROBE_TIMEOUT, PROBE_MAX_BYTES) {
                Ok(out) if out.status.success() => {
                    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    Some(if !stdout.is_empty() { stdout } else { stderr })
                }
                Ok(out) => return invalid(err("runtime.binary_exit").with("status", out.status)),
                Err(e) => {
                    return invalid(
                        err("process.spawn_failed")
                            .with("program", path.display())
                            .with("error", e),
                    )
                }
            };

        let mut cmd_help = crate::process::quiet_command(path);
        cmd_help.arg("--help").current_dir(parent_dir);
        let supports_metrics =
            run_command_with_timeout(&mut cmd_help, PROBE_TIMEOUT, PROBE_MAX_BYTES)
                .map(|out| {
                    let combined = format!(
                        "{}\n{}",
                        String::from_utf8_lossy(&out.stdout),
                        String::from_utf8_lossy(&out.stderr)
                    );
                    combined.contains("--metrics") || combined.contains("/metrics")
                })
                .unwrap_or(false);

        CustomBinaryValidation {
            is_valid: true,
            version: version_str,
            supports_metrics,
            error: None,
        }
    }

    /// [`detect_system_binary`](Self::detect_system_binary) remembered for 30 s (it runs
    /// `--version`/`--help` on every candidate; launches and capability lookups call it often).
    pub fn detect_system_binary_cached() -> Option<PathBuf> {
        use std::time::{Duration, Instant};
        static CACHE: Mutex<Option<(Instant, Option<PathBuf>)>> = Mutex::new(None);
        if let Some((at, hit)) = CACHE.lock().unwrap().as_ref() {
            if at.elapsed() < Duration::from_secs(30) && hit.as_ref().is_none_or(|p| p.is_file()) {
                return hit.clone();
            }
        }
        let found = Self::detect_system_binary();
        *CACHE.lock().unwrap() = Some((Instant::now(), found.clone()));
        found
    }

    /// Searches system PATH and common installation directories for a valid llama-server binary.
    pub fn detect_system_binary() -> Option<PathBuf> {
        let bin_name = binary_name();

        // 1. Check system PATH
        if let Some(paths) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&paths) {
                let candidate = dir.join(bin_name);
                if candidate.is_file() {
                    let v = Self::validate_custom_binary(&candidate);
                    if v.is_valid {
                        return Some(candidate);
                    }
                }
            }
        }

        // 2. Check common standard directories
        let mut candidates = Vec::new();
        #[cfg(windows)]
        {
            candidates.push(PathBuf::from(r"C:\llama.cpp").join(bin_name));
            candidates.push(PathBuf::from(r"C:\tools\llama.cpp").join(bin_name));
            candidates.push(PathBuf::from(r"C:\Program Files\llama.cpp").join(bin_name));
            if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
                candidates.push(
                    PathBuf::from(&local_app_data)
                        .join("llama.cpp")
                        .join(bin_name),
                );
                candidates.push(
                    PathBuf::from(&local_app_data)
                        .join("Programs")
                        .join("llama.cpp")
                        .join(bin_name),
                );
                candidates.push(
                    PathBuf::from(&local_app_data)
                        .join("Microsoft")
                        .join("WinGet")
                        .join("Links")
                        .join(bin_name),
                );
                let winget_pkgs = PathBuf::from(&local_app_data)
                    .join("Microsoft")
                    .join("WinGet")
                    .join("Packages");
                if let Ok(entries) = std::fs::read_dir(&winget_pkgs) {
                    for entry in entries.flatten() {
                        let n = entry.file_name().to_string_lossy().to_lowercase();
                        if n.contains("llamacpp") || n.contains("llama") {
                            candidates.push(entry.path().join(bin_name));
                        }
                    }
                }
            }
            if let Ok(user_profile) = std::env::var("USERPROFILE") {
                candidates.push(
                    PathBuf::from(&user_profile)
                        .join("llama.cpp")
                        .join(bin_name),
                );
                candidates.push(
                    PathBuf::from(&user_profile)
                        .join(".llama.cpp")
                        .join(bin_name),
                );
                candidates.push(
                    PathBuf::from(&user_profile)
                        .join("scoop")
                        .join("shims")
                        .join(bin_name),
                );
            }
        }
        #[cfg(not(windows))]
        {
            candidates.push(PathBuf::from("/usr/local/bin").join(bin_name));
            candidates.push(PathBuf::from("/opt/homebrew/bin").join(bin_name));
            candidates.push(PathBuf::from("/usr/bin").join(bin_name));
            if let Ok(home) = std::env::var("HOME") {
                candidates.push(
                    PathBuf::from(&home)
                        .join(".local")
                        .join("bin")
                        .join(bin_name),
                );
                candidates.push(PathBuf::from(&home).join("llama.cpp").join(bin_name));
            }
        }

        for candidate in candidates {
            if candidate.is_file() {
                let v = Self::validate_custom_binary(&candidate);
                if v.is_valid {
                    return Some(candidate);
                }
            }
        }

        None
    }
}

/// Extracts a `.zip`, `.tar.gz` or `.tar.bz2` archive into `dest`. Entries that would escape
/// `dest` are rejected for ZIP archives (tar uses `unpack`).
pub(crate) fn extract_archive(archive: &Path, dest: &Path) -> Result<(), String> {
    let name = archive.to_string_lossy().to_lowercase();
    let file = fs::File::open(archive).map_err(|e| {
        String::from(
            err("fs.read_failed")
                .with("path", archive.display())
                .with("error", e),
        )
    })?;
    let invalid = |e: &dyn std::fmt::Display| -> String {
        err("runtime.archive_invalid")
            .with("path", archive.display())
            .with("error", e)
            .into()
    };
    if name.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(file).map_err(|e| invalid(&e))?;
        crate::zip_safety::validate_archive(&mut zip).map_err(|e| invalid(&e))?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| invalid(&e))?;
            let rel = entry
                .enclosed_name()
                .ok_or_else(|| invalid(&"unsafe ZIP path"))?;
            let out = dest.join(rel);
            crate::zip_safety::reject_symlink_ancestors(&out, dest).map_err(|e| invalid(&e))?;
            if entry.is_dir() {
                fs::create_dir_all(&out).map_err(|e| create_dir_failed(&out, e))?;
                continue;
            }
            if let Some(p) = out.parent() {
                fs::create_dir_all(p).map_err(|e| create_dir_failed(p, e))?;
            }
            let write_failed = |e: std::io::Error| -> String {
                err("fs.write_failed")
                    .with("path", out.display())
                    .with("error", e)
                    .into()
            };
            // Never follow or overwrite an existing link (or hard link).
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&out)
                .map_err(write_failed)?;
            std::io::copy(&mut entry, &mut f).map_err(write_failed)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = entry.unix_mode() {
                    f.set_permissions(fs::Permissions::from_mode(mode & 0o777))
                        .map_err(write_failed)?;
                }
            }
        }
        Ok(())
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
        tar.set_preserve_permissions(true);
        tar.unpack(dest).map_err(|e| invalid(&e))
    } else if name.ends_with(".tar.bz2") || name.ends_with(".tbz2") {
        // sherpa-onnx releases and its TTS model archives.
        let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(file));
        tar.set_preserve_permissions(true);
        tar.unpack(dest).map_err(|e| invalid(&e))
    } else {
        Err(err("runtime.archive_unsupported")
            .with("path", archive.display())
            .into())
    }
}

/// Some archives (tar.gz) nest the binaries in a sub-folder. Moves the contents of
/// the folder that contains `llama-server` up to the staging root.
#[cfg(test)]
pub(crate) fn flatten_to_binary_dir(staging: &Path) -> Result<(), String> {
    flatten_to_binary_dir_for(staging, binary_name())
}

/// [`flatten_to_binary_dir`] for the executable `binary` of any family.
pub(crate) fn flatten_to_binary_dir_for(staging: &Path, binary: &str) -> Result<(), String> {
    if staging.join(binary).exists() {
        return Ok(());
    }
    fn find(dir: &Path, binary: &str) -> Option<PathBuf> {
        for entry in fs::read_dir(dir).ok()?.flatten() {
            let p = entry.path();
            if p.is_dir() {
                if p.join(binary).exists() {
                    return Some(p);
                }
                if let Some(found) = find(&p, binary) {
                    return Some(found);
                }
            }
        }
        None
    }
    let bin_dir = find(staging, binary).ok_or_else(|| {
        String::from(err("runtime.package_missing_binary").with("binary", binary))
    })?;
    let entries = fs::read_dir(&bin_dir).map_err(|e| {
        String::from(
            err("fs.read_dir_failed")
                .with("path", bin_dir.display())
                .with("error", e),
        )
    })?;
    for entry in entries.flatten() {
        let target = staging.join(entry.file_name());
        if !target.exists() {
            fs::rename(entry.path(), &target)
                .map_err(|e| rename_failed(&entry.path(), &target, e))?;
        }
    }
    // A `bin/` + `lib/` package (sherpa-onnx on macOS and Linux) finds its shared libraries
    // through `@loader_path/../lib` / `$ORIGIN/../lib`, which no longer resolves once the
    // binaries sit at the staging root; its other rpath, `@loader_path` / `$ORIGIN`, does.
    if let Some(lib_dir) = bin_dir.parent().map(|p| p.join("lib")) {
        if let Ok(libs) = fs::read_dir(&lib_dir) {
            for entry in libs.flatten() {
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(true);
                let target = staging.join(entry.file_name());
                if !is_dir && fs::symlink_metadata(&target).is_err() {
                    fs::rename(entry.path(), &target)
                        .map_err(|e| rename_failed(&entry.path(), &target, e))?;
                }
            }
        }
    }
    // The emptied package folders (`crispasr-windows-x86_64-vulkan/`...) are removed; a
    // folder that still holds anything is left alone (`remove_dir` only removes empty ones).
    let mut leftover = Some(bin_dir.as_path());
    while let Some(dir) = leftover.filter(|d| *d != staging && d.starts_with(staging)) {
        if fs::remove_dir(dir).is_err() {
            break;
        }
        leftover = dir.parent();
    }
    Ok(())
}

fn create_dir_failed(path: &Path, e: std::io::Error) -> String {
    err("fs.create_dir_failed")
        .with("path", path.display())
        .with("error", e)
        .into()
}

fn remove_failed(path: &Path, e: std::io::Error) -> String {
    err("fs.remove_failed")
        .with("path", path.display())
        .with("error", e)
        .into()
}

fn write_manifest(dir: &Path, manifest: &RuntimeManifest) -> Result<(), String> {
    let path = dir.join(RUNTIME_MANIFEST);
    let text = serde_json::to_string_pretty(manifest).unwrap_or_default();
    fs::write(&path, text).map_err(|e| {
        err("fs.write_failed")
            .with("path", path.display())
            .with("error", e)
            .into()
    })
}

/// Renames a runtime folder. On Windows a folder whose files were just written (extraction)
/// or just run (`--version`, `--list-devices`) can be locked for a moment by virus scanners;
/// "access denied" is retried for up to about 3 s.
fn rename_dir(from: &Path, to: &Path) -> Result<(), String> {
    let mut attempt = 0;
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && attempt < 15 => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(e) => return Err(rename_failed(from, to, e)),
        }
    }
}

fn rename_failed(from: &Path, to: &Path, e: std::io::Error) -> String {
    err("fs.rename_failed")
        .with("from", from.display())
        .with("to", to.display())
        .with("error", e)
        .into()
}

pub const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
pub const PROBE_MAX_BYTES: usize = 64 * 1024;

pub fn normalize_digest(raw: &str) -> Option<String> {
    let hex = raw
        .strip_prefix("sha256:")
        .or_else(|| raw.strip_prefix("sha256-"))
        .unwrap_or(raw)
        .trim();
    if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(hex.to_ascii_lowercase())
    } else {
        None
    }
}

pub fn run_command_with_timeout(
    cmd: &mut std::process::Command,
    timeout: std::time::Duration,
    max_bytes: usize,
) -> Result<std::process::Output, std::io::Error> {
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn()?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let stdout_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut stream) = stdout {
            use std::io::Read;
            let _ = stream.by_ref().take(max_bytes as u64).read_to_end(&mut buf);
            let mut sink = std::io::sink();
            let _ = std::io::copy(&mut stream, &mut sink);
        }
        buf
    });

    let stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut stream) = stderr {
            use std::io::Read;
            let _ = stream.by_ref().take(max_bytes as u64).read_to_end(&mut buf);
            let mut sink = std::io::sink();
            let _ = std::io::copy(&mut stream, &mut sink);
        }
        buf
    });

    let start = std::time::Instant::now();
    let poll_interval = std::time::Duration::from_millis(20);
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_handle.join();
                    let _ = stderr_handle.join();
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "command timed out",
                    ));
                }
                std::thread::sleep(poll_interval);
            }
        }
    };

    let stdout_bytes = stdout_handle.join().unwrap_or_default();
    let stderr_bytes = stderr_handle.join().unwrap_or_default();

    Ok(std::process::Output {
        status,
        stdout: stdout_bytes,
        stderr: stderr_bytes,
    })
}

/// llama.cpp build tags look like `b1234`.
fn is_build_tag(tag: &str) -> bool {
    tag.strip_prefix('b')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The shared library the dynamic loader could not find (`error while loading shared libraries:
/// libgomp.so.1: cannot open shared object file`): the managed builds link the system's OpenMP
/// and Vulkan loader, which minimal Linux installs may lack.
fn missing_shared_library(stderr: &str) -> Option<String> {
    let rest = stderr
        .split("error while loading shared libraries: ")
        .nth(1)?;
    let name = rest.split(':').next()?.trim();
    (!name.is_empty() && name.len() <= 128).then(|| name.to_string())
}

/// Package names that provide `library` on the common distributions, for the error's hint.
fn library_packages(library: &str) -> String {
    let base = library.split(".so").next().unwrap_or(library);
    match base {
        "libgomp" => "Debian/Ubuntu: libgomp1, Fedora: libgomp, Arch: gcc-libs".into(),
        "libvulkan" => {
            "Debian/Ubuntu: libvulkan1, Fedora: vulkan-loader, Arch: vulkan-icd-loader".into()
        }
        "libcurl" => "Debian/Ubuntu: libcurl4, Fedora: libcurl, Arch: curl".into(),
        "libssl" | "libcrypto" => {
            "Debian/Ubuntu: libssl3, Fedora: openssl-libs, Arch: openssl".into()
        }
        _ => library.into(),
    }
}

#[cfg(test)]
mod tests {

    #[cfg(windows)]
    #[test]
    fn pe_imports_reads_a_real_executable() {
        let exe = std::env::current_exe().unwrap();
        let imports = pe_imports(&std::fs::read(exe).unwrap());
        assert!(
            imports
                .iter()
                .any(|n| n.eq_ignore_ascii_case("kernel32.dll")),
            "{imports:?}"
        );
        assert!(pe_imports(b"not a pe").is_empty());
    }
    use super::*;

    #[test]
    fn a_missing_shared_library_is_named_with_its_packages() {
        let stderr = "./llama-server: error while loading shared libraries: libgomp.so.1: cannot open shared object file: No such file or directory";
        let lib = missing_shared_library(stderr).unwrap();
        assert_eq!(lib, "libgomp.so.1");
        assert!(library_packages(&lib).contains("libgomp1"));
        assert!(library_packages("libvulkan.so.1").contains("vulkan-loader"));
        assert_eq!(library_packages("libfoo.so.2"), "libfoo.so.2");
        assert_eq!(missing_shared_library("Segmentation fault"), None);
    }

    #[test]
    fn build_tags_only() {
        assert!(is_build_tag("b11255"));
        assert!(!is_build_tag("v0.5.0"));
        assert!(!is_build_tag("b"));
        assert!(!is_build_tag("b12x"));
    }

    fn q(os: &str, backend: Option<&str>, arch: &str) -> AssetQuery {
        AssetQuery {
            os: os.into(),
            arch: arch.into(),
            backend: backend.map(String::from),
        }
    }

    const NAMES: &[&str] = &[
        "cudart-llama-bin-win-cuda-12.4-x64.zip",
        "cudart-llama-bin-win-cuda-13.4-x64.zip",
        "cudart-llama-b1-bin-ubuntu-cuda-12.8-x64.tar.gz",
        "llama-b1-bin-macos-arm64.tar.gz",
        "llama-b1-bin-ubuntu-cuda-12.8-x64.tar.gz",
        "llama-b1-bin-ubuntu-cuda-13.4-x64.tar.gz",
        "llama-b1-bin-ubuntu-vulkan-x64.tar.gz",
        "llama-b1-bin-ubuntu-x64.tar.gz",
        "llama-b1-bin-ubuntu-arm64.tar.gz",
        "llama-b1-bin-win-cpu-x64.zip",
        "llama-b1-bin-win-cuda-12.4-x64.zip",
        "llama-b1-bin-win-cuda-13.4-x64.zip",
        "llama-b1-bin-win-vulkan-x64.zip",
    ];

    #[test]
    fn test_select_asset() {
        let cuda = q("win", Some("cuda"), "x64");
        let only13 = ["llama-b1-bin-win-cuda-13.4-x64.zip"];
        let both = [
            "llama-b1-bin-win-cuda-13.4-x64.zip",
            "llama-b1-bin-win-cuda-12.4-x64.zip",
        ];
        let pick = |names: &[&'static str], driver| {
            select_asset_for_driver(names, &cuda, driver).map(|(n, _)| n)
        };
        assert_eq!(pick(&only13, Some("617.42")), Some(only13[0]));
        assert_eq!(pick(&only13, Some("32.0.16.1742")), Some(only13[0]));
        assert_eq!(pick(&only13, Some("551.61")), None);
        assert_eq!(pick(&only13, None), None);
        assert_eq!(pick(&only13, Some("broken")), None);
        for driver in [Some("617.42"), Some("551.61"), Some("528.33"), None] {
            assert_eq!(pick(&both, driver), Some(both[1]), "{driver:?}");
        }
        let release = |names: &[&str]| {
            serde_json::json!({"tag_name": "b1", "assets": names.iter()
                .map(|n| serde_json::json!({"name": n})).collect::<Vec<_>>()})
        };
        let (only13_release, both_release) = ([release(&only13)], [release(&both)]);
        assert!(cuda13_only_error(&only13_release, &cuda, Some("617.42")).is_none());
        for driver in [Some("551.61"), None] {
            let error = cuda13_only_error(&only13_release, &cuda, driver).unwrap();
            assert!(error.contains("runtime.cuda_driver_too_old"), "{error}");
            assert!(cuda13_only_error(&both_release, &cuda, driver).is_none());
        }
        assert_eq!(
            select_asset(NAMES, &q("win", Some("vulkan"), "x64"))
                .unwrap()
                .0,
            "llama-b1-bin-win-vulkan-x64.zip"
        );
        let (cuda, ver) = select_asset(NAMES, &q("win", Some("cuda"), "x64")).unwrap();
        assert_eq!(cuda, "llama-b1-bin-win-cuda-12.4-x64.zip");
        assert_eq!(ver.as_deref(), Some("12.4"));
        assert_eq!(
            select_cudart(NAMES, &q("win", Some("cuda"), "x64"), "12.4"),
            Some("cudart-llama-bin-win-cuda-12.4-x64.zip")
        );
        assert_eq!(
            select_asset(NAMES, &q("ubuntu", None, "x64")).unwrap().0,
            "llama-b1-bin-ubuntu-x64.tar.gz"
        );
        assert_eq!(
            select_asset(NAMES, &q("ubuntu", Some("cuda"), "x64"))
                .unwrap()
                .0,
            "llama-b1-bin-ubuntu-cuda-12.8-x64.tar.gz"
        );
        assert_eq!(
            select_asset(NAMES, &q("macos", None, "arm64")).unwrap().0,
            "llama-b1-bin-macos-arm64.tar.gz"
        );
        assert!(select_asset(NAMES, &q("win", Some("rocm"), "x64")).is_none());
        assert_eq!(
            select_asset(NAMES, &q("win", Some("rocm"), "x64").cpu_fallback())
                .unwrap()
                .0,
            "llama-b1-bin-win-cpu-x64.zip"
        );
    }

    #[test]
    fn test_custom_binary_nonexistent() {
        let res = RuntimeManager::validate_custom_binary(Path::new("non_existent_binary_xyz.exe"));
        assert!(!res.is_valid);
        assert!(res.error.is_some());
    }

    /// A fake build folder: the binary plus a ggml backend library (names only).
    fn write_build(dir: &Path, version: &str, backend_lib: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(binary_name()), b"exe").unwrap();
        fs::write(dir.join(backend_lib), b"lib").unwrap();
        fs::write(dir.join("version.txt"), version).unwrap();
    }

    fn temp_manager() -> (tempfile::TempDir, RuntimeManager) {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = RuntimeManager {
            base_bin_dir: Some(temp_dir.path().to_path_buf()),
            ..Default::default()
        };
        (temp_dir, manager)
    }

    fn version_in(dir: &Path) -> String {
        fs::read_to_string(dir.join("version.txt")).unwrap()
    }

    #[test]
    fn test_activation_and_rollback() {
        let (_t, manager) = temp_manager();
        write_build(&manager.managed_dir(), "b1", "ggml-vulkan.dll");
        write_build(&manager.staging_dir(), "b2", "ggml-vulkan.dll");

        let m = manager.activate_runtime(Some("b2")).unwrap();
        assert_eq!(m.version.as_deref(), Some("b2"));
        assert_eq!(m.backend.as_deref(), Some("vulkan"));
        assert_eq!(version_in(&manager.managed_dir()), "b2");
        // The backup slot holds the replaced build, not an empty folder.
        assert!(manager.has_backup());
        assert_eq!(version_in(&manager.backup_dir()), "b1");
        assert_eq!(read_manifest(&manager.managed_dir()), m);
        assert_eq!(m.family.as_deref(), Some("llama.cpp"));

        // Rolling back restores b1 and keeps b2 in the slot (the rollback can be undone).
        let restored = manager.rollback_runtime().unwrap();
        assert_eq!(restored.version, None); // b1 predates manifests
        assert_eq!(restored.backend.as_deref(), Some("vulkan"));
        assert_eq!(version_in(&manager.managed_dir()), "b1");
        assert_eq!(version_in(&manager.backup_dir()), "b2");
        assert_eq!(
            manager.backup_manifest().unwrap().version.as_deref(),
            Some("b2")
        );
        let again = manager.rollback_runtime().unwrap();
        assert_eq!(again.version.as_deref(), Some("b2"));
        assert_eq!(version_in(&manager.managed_dir()), "b2");
    }

    /// Every family installs through the same manager; llama.cpp keeps the folders it has
    /// always had and its release URLs are unchanged.
    #[test]
    fn release_source_and_folders_per_family() {
        let llama = crate::engine::llama_cpp().release_source();
        assert_eq!(llama.download_prefix(), RELEASE_DOWNLOAD_PREFIX);
        assert_eq!(
            llama.releases_api(5),
            "https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=5"
        );
        assert_eq!(
            llama.release_api("b11337"),
            "https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/b11337"
        );
        assert_eq!(llama.binary_file(), binary_name());
        let m = RuntimeManager::default();
        assert_eq!(m.engine().family(), "llama.cpp");
        assert_eq!(m.managed_dir(), crate::paths::bin_dir().join("managed"));
        let m = RuntimeManager::for_engine(crate::engine::llama_cpp());
        assert_eq!(m.staging_dir(), crate::paths::bin_dir().join("staging"));
        // Manifests written before families are llama.cpp.
        let old: RuntimeManifest =
            serde_json::from_str(r#"{"version":"b11337","backend":"cuda"}"#).unwrap();
        assert_eq!(old.family(), "llama.cpp");
    }

    /// An empty backup folder (or none) is never "restored": the installed build stays.
    #[test]
    fn test_rollback_with_empty_backup_keeps_installed_build() {
        let (_t, manager) = temp_manager();
        write_build(&manager.managed_dir(), "b2", "ggml-vulkan.dll");
        assert!(manager.rollback_runtime().is_err());
        fs::create_dir_all(manager.backup_dir()).unwrap();
        assert!(!manager.has_backup());
        let e = manager.rollback_runtime().unwrap_err();
        assert!(e.starts_with("runtime.no_backup"), "{e}");
        assert_eq!(version_in(&manager.managed_dir()), "b2");
    }

    /// A `managed` folder without the binary never replaces a working backup.
    #[test]
    fn test_activation_keeps_backup_when_managed_is_broken() {
        let (_t, manager) = temp_manager();
        write_build(&manager.backup_dir(), "b1", "ggml-vulkan.dll");
        fs::create_dir_all(manager.managed_dir()).unwrap();
        write_build(&manager.staging_dir(), "b3", "ggml-vulkan.dll");
        manager.activate_runtime(Some("b3")).unwrap();
        assert_eq!(version_in(&manager.managed_dir()), "b3");
        assert_eq!(version_in(&manager.backup_dir()), "b1");
        // Nothing staged (or staging without the binary): nothing changes.
        assert!(manager.activate_runtime(Some("b4")).is_err());
        fs::create_dir_all(manager.staging_dir()).unwrap();
        assert!(manager.activate_runtime(Some("b4")).is_err());
        assert_eq!(version_in(&manager.managed_dir()), "b3");
    }

    /// Settings follow the builds: the backend of an activated build is recorded (so updates
    /// keep it) and a rollback restores the backup's version and backend.
    #[test]
    fn test_record_activation_and_rollback() {
        let mut settings = crate::config::AppSettings::default();
        settings.managed_runtime.installed_version = Some("b11293".into());
        record_activation(
            &mut settings,
            &RuntimeManifest {
                version: Some("b11298".into()),
                backend: Some("vulkan".into()),
                ..Default::default()
            },
        );
        let mr = &settings.managed_runtime;
        assert_eq!(mr.installed_version.as_deref(), Some("b11298"));
        assert_eq!(mr.previous_version.as_deref(), Some("b11293"));
        assert_eq!(mr.backend.as_deref(), Some("vulkan"));

        record_rollback(
            &mut settings,
            &RuntimeManifest {
                version: None,
                backend: Some("rocm".into()),
                ..Default::default()
            },
        );
        let mr = &settings.managed_runtime;
        assert_eq!(mr.installed_version.as_deref(), Some("b11293"));
        assert_eq!(mr.previous_version.as_deref(), Some("b11298"));
        assert_eq!(mr.backend.as_deref(), Some("rocm"));
    }

    #[test]
    fn cuda_without_devices_refuses_stage_and_keeps_installed_runtime() {
        let (_temp, manager) = temp_manager();
        let staging = manager.staging_dir();
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("ggml-cuda.dll"), []).unwrap();
        let installed = manager.managed_dir();
        fs::create_dir_all(&installed).unwrap();
        fs::write(installed.join(manager.bin()), b"existing runtime").unwrap();
        assert!(manager
            .verify_stage1_devices()
            .unwrap_err()
            .contains("runtime.backend_no_device: cuda"));
        assert!(!staging.exists());
        assert_eq!(
            fs::read(installed.join(manager.bin())).unwrap(),
            b"existing runtime"
        );
    }

    /// Stage 1c: a staged build of another backend is refused and staging removed.
    #[test]
    fn test_verify_staged_backend_refuses_other_backend() {
        let (_t, manager) = temp_manager();
        let cpu = [
            "ggml-base.dll",
            "ggml-cpu-zen4.dll",
            "ggml-cpu-x64.dll",
            "llama.dll",
        ];
        fs::create_dir_all(manager.staging_dir()).unwrap();
        fs::write(manager.staging_dir().join(binary_name()), b"exe").unwrap();
        for f in cpu {
            fs::write(manager.staging_dir().join(f), b"").unwrap();
        }
        let e = manager.verify_staged_backend("vulkan").unwrap_err();
        let (key, params) = crate::error_key::parse(&e).unwrap();
        assert_eq!(key, "runtime.update_backend_mismatch");
        assert_eq!(params["expected"], "vulkan");
        assert_eq!(params["found"], "cpu");
        assert!(!manager.staging_dir().exists());

        // Same backend (and no installed build to compare devices with): accepted.
        write_build(&manager.staging_dir(), "b2", "ggml-hip.dll");
        manager.verify_staged_backend("hip").unwrap();
        assert!(manager.staging_dir().exists());
    }

    #[test]
    fn test_flatten_nested_tarball_layout() {
        let temp_dir = tempfile::tempdir().unwrap();
        let nested = temp_dir.path().join("build").join("bin");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join(binary_name()), b"x").unwrap();
        fs::write(nested.join("libggml.so"), b"x").unwrap();
        flatten_to_binary_dir(temp_dir.path()).unwrap();
        assert!(temp_dir.path().join(binary_name()).exists());
        assert!(temp_dir.path().join("libggml.so").exists());
        // The emptied package folders are gone.
        assert!(!temp_dir.path().join("build").exists());
    }

    #[test]
    fn test_flatten_moves_sibling_lib_folder_next_to_the_binary() {
        let temp_dir = tempfile::tempdir().unwrap();
        let pkg = temp_dir.path().join("sherpa-onnx-v1.13.8-osx-arm64-shared");
        fs::create_dir_all(pkg.join("bin")).unwrap();
        fs::create_dir_all(pkg.join("lib").join("pkgconfig")).unwrap();
        fs::write(pkg.join("bin").join("sherpa-onnx-offline-tts"), b"x").unwrap();
        fs::write(pkg.join("lib").join("libonnxruntime.dylib"), b"x").unwrap();
        flatten_to_binary_dir_for(temp_dir.path(), "sherpa-onnx-offline-tts").unwrap();
        assert!(temp_dir.path().join("sherpa-onnx-offline-tts").exists());
        assert!(temp_dir.path().join("libonnxruntime.dylib").exists());
        assert!(!temp_dir.path().join("pkgconfig").exists());
    }

    #[tokio::test]
    #[ignore] // network: cargo test -- --ignored
    async fn test_check_latest_release_api() {
        let manager = RuntimeManager::default();
        let info = manager
            .check_latest_release(&q("win", Some("cuda"), "x64"), PickMode::Exact)
            .await
            .unwrap();
        assert!(info.asset_name.contains("cuda"));
        assert_eq!(info.extra_assets.len(), 1);
    }

    #[tokio::test]
    #[ignore] // network + large download: cargo test -- --ignored
    async fn test_download_and_verify_stage1() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = RuntimeManager {
            base_bin_dir: Some(temp_dir.path().to_path_buf()),
            ..Default::default()
        };
        // This machine's archive: the staged binary must run here (`--version`).
        let hw = crate::hardware::detect_hardware();
        let target = install_query(&hw, Some("vulkan"), None, None);
        let release = manager
            .check_latest_release(&target.query, target.mode)
            .await
            .unwrap();
        manager
            .download_and_extract_staging(&[release.download_url], false)
            .await
            .unwrap();
        assert!(!manager.verify_stage1_binary().unwrap().is_empty());
    }

    #[test]
    fn test_normalize_digest() {
        let valid = "a".repeat(64);
        assert_eq!(
            normalize_digest(&format!("sha256:{}", valid)),
            Some(valid.clone())
        );
        assert_eq!(
            normalize_digest(&format!("sha256-{}", valid)),
            Some(valid.clone())
        );
        assert_eq!(normalize_digest(&valid.to_uppercase()), Some(valid.clone()));
        assert_eq!(normalize_digest("short"), None);
        assert_eq!(normalize_digest("sha256:xyz"), None);
    }

    #[tokio::test]
    async fn test_download_digest_verification() {
        use sha2::{Digest, Sha256};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let file_data = b"llama-runtime-test-payload-12345";
        let expected_sha = crate::hex::lower(&Sha256::digest(file_data));

        tokio::spawn(async move {
            loop {
                if let Ok((mut socket, _)) = listener.accept().await {
                    let mut req_buf = [0u8; 1024];
                    let _ = socket.read(&mut req_buf).await;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        file_data.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.write_all(file_data).await;
                }
            }
        });

        let temp_dir = tempfile::tempdir().unwrap();
        let manager = RuntimeManager {
            base_bin_dir: Some(temp_dir.path().to_path_buf()),
            ..Default::default()
        };

        let url = format!("http://{}:{}/test.zip", addr.ip(), addr.port());

        // 1. Mismatching digest should fail with runtime.digest_mismatch and delete the file
        let bad_dest = temp_dir.path().join("bad.zip");
        let err = manager
            .download_to_file(
                &url,
                &bad_dest,
                "test.zip",
                Some("sha256:0000000000000000000000000000000000000000000000000000000000000000"),
            )
            .await
            .unwrap_err();
        let (key, params) = crate::error_key::parse(&err).unwrap();
        assert_eq!(key, "runtime.digest_mismatch", "{err}");
        assert!(
            params["actual"].as_str().is_some_and(|d| d.len() == 64),
            "{err}"
        );
        assert!(!bad_dest.exists(), "Dosya silinmiş olmalı");

        // 2. Matching digest should succeed
        let good_dest = temp_dir.path().join("good.zip");
        manager
            .download_to_file(
                &url,
                &good_dest,
                "test.zip",
                Some(&format!("sha256:{}", expected_sha)),
            )
            .await
            .unwrap();
        assert!(good_dest.exists());
        assert_eq!(std::fs::read(&good_dest).unwrap(), file_data);

        // 3. No digest should proceed
        let no_digest_dest = temp_dir.path().join("no_digest.zip");
        manager
            .download_to_file(&url, &no_digest_dest, "test.zip", None)
            .await
            .unwrap();
        assert!(no_digest_dest.exists());
        assert_eq!(std::fs::read(&no_digest_dest).unwrap(), file_data);
    }

    fn fixture(name: &str) -> serde_json::Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name);
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn win_vulkan() -> AssetQuery {
        AssetQuery {
            os: "win".into(),
            arch: "x64".into(),
            backend: Some("vulkan".into()),
        }
    }

    /// Real GitHub API listing (b11263 was still being uploaded: only one asset, skipped).
    #[test]
    fn test_pick_release_from_real_listing_is_verified() {
        let releases = fixture("github-releases-b11263.json");
        let releases = releases.as_array().unwrap();
        let info = pick_release(releases, &win_vulkan(), PickMode::Exact).unwrap();
        assert_eq!(info.tag_name, "b11262");
        assert_eq!(info.asset_name, "llama-b11262-bin-win-vulkan-x64.zip");
        assert!(info.download_url.starts_with(RELEASE_DOWNLOAD_PREFIX));
        assert!(info.size > 0);
        assert_eq!(info.integrity, AssetIntegrity::Verified);
        assert!(info.unverified_assets().is_empty());

        // CUDA picks the lowest CUDA build plus its runtime companion, both verified.
        let cuda = AssetQuery {
            backend: Some("cuda".into()),
            ..win_vulkan()
        };
        let info = pick_release(releases, &cuda, PickMode::Exact).unwrap();
        assert_eq!(info.asset_name, "llama-b11262-bin-win-cuda-12.4-x64.zip");
        assert_eq!(info.extra_assets.len(), 1);
        assert_eq!(
            info.extra_assets[0].name,
            "cudart-llama-bin-win-cuda-12.4-x64.zip"
        );
        assert!(info.unverified_assets().is_empty());
    }

    /// Real release from before GitHub published asset digests (`"digest": null`).
    #[test]
    fn test_pick_release_without_digest_is_unverified() {
        let release = fixture("github-release-b4000.json");
        let info = pick_release(
            std::slice::from_ref(&release),
            &win_vulkan(),
            PickMode::Exact,
        )
        .unwrap();
        assert_eq!(info.asset_name, "llama-b4000-bin-win-vulkan-x64.zip");
        assert_eq!(info.integrity, AssetIntegrity::Missing);
        let unverified = info.unverified_assets();
        assert_eq!(unverified.len(), 1);
        assert_eq!(unverified[0].name, info.asset_name);
        assert_eq!(unverified[0].size, info.size);
        assert_eq!(unverified[0].download_url, info.download_url);
    }

    #[test]
    fn test_integrity_of() {
        let hex = "a".repeat(64);
        assert_eq!(
            integrity_of(Some(&format!("sha256:{hex}"))),
            AssetIntegrity::Verified
        );
        assert_eq!(integrity_of(None), AssetIntegrity::Missing);
        assert_eq!(integrity_of(Some("sha1:abcd")), AssetIntegrity::Invalid);
        assert_eq!(integrity_of(Some("")), AssetIntegrity::Invalid);
    }

    /// Missing digest: staging refuses before downloading or touching the staging dir, and
    /// reports the needs-confirmation key; a verified asset is not reported.
    #[tokio::test]
    async fn test_staging_without_digest_needs_confirmation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = RuntimeManager {
            base_bin_dir: Some(temp_dir.path().to_path_buf()),
            ..Default::default()
        };
        manager.ingest_release_digests(&fixture("github-release-b4000.json"));
        let url = format!(
            "{}b4000/llama-b4000-bin-win-vulkan-x64.zip",
            RELEASE_DOWNLOAD_PREFIX
        );
        let err = manager
            .download_and_extract_staging(std::slice::from_ref(&url), false)
            .await
            .unwrap_err();
        assert_eq!(err, ERR_INTEGRITY_UNVERIFIED);
        assert!(!manager.staging_dir().exists());
        assert!(manager.get_download_progress().is_none());

        let listing = fixture("github-releases-b11263.json");
        for r in listing.as_array().unwrap() {
            manager.ingest_release_digests(r);
        }
        let verified = format!(
            "{}b11262/llama-b11262-bin-win-vulkan-x64.zip",
            RELEASE_DOWNLOAD_PREFIX
        );
        assert_eq!(
            manager.unverified_urls(&[verified, url.clone()]).await,
            vec![url]
        );
    }

    /// Real listing: the ROCm/HIP build of b11262 is picked for RDNA3 on Windows; a release
    /// without a ROCm build (b4000) falls back to Vulkan, never to a silent CPU build.
    #[test]
    fn test_pick_release_rocm_and_fallback() {
        let releases = fixture("github-releases-b11263.json");
        let releases = releases.as_array().unwrap();
        let rocm = crate::hardware::RecommendedBackend::Rocm.asset_query("windows", "x86_64");
        let info = pick_release(releases, &rocm, PickMode::Fallback).unwrap();
        assert_eq!(info.asset_name, "llama-b11262-bin-win-rocm-10.0-x64.zip");
        assert_eq!(info.backend.as_deref(), Some("rocm"));
        assert!(info.extra_assets.is_empty());
        let sycl = crate::hardware::RecommendedBackend::Sycl.asset_query("linux", "x86_64");
        let info = pick_release(releases, &sycl, PickMode::Fallback).unwrap();
        assert_eq!(
            info.asset_name,
            "llama-b11262-bin-ubuntu-sycl-fp16-x64.tar.gz"
        );

        let old = fixture("github-release-b4000.json");
        let info = pick_release(std::slice::from_ref(&old), &rocm, PickMode::Fallback).unwrap();
        assert_eq!(info.asset_name, "llama-b4000-bin-win-vulkan-x64.zip");
        assert_eq!(info.backend.as_deref(), Some("vulkan"));
        // An explicit ROCm request (or an update of a ROCm build) never falls back.
        assert!(pick_release(&[old], &rocm, PickMode::Exact).is_err());
    }

    /// The real listing of b11298 as GitHub served it while the release was still being
    /// uploaded: archives appear alphabetically, so for about 36 s `win-cpu-x64` was listed and
    /// `win-vulkan-x64` was not. Older versions picked the CPU build of b11298 for an installed
    /// Vulkan build (what turned a Radeon RX 7900 GRE machine CPU-only). Rebuilt from the real
    /// release JSON by keeping the assets whose upload had finished (`updated_at`).
    fn listing_at(time: &str) -> Vec<serde_json::Value> {
        let mut releases = fixture("github-releases-b11298.json")
            .as_array()
            .unwrap()
            .clone();
        for r in &mut releases {
            let assets = r["assets"].as_array().unwrap().clone();
            r["assets"] = serde_json::Value::Array(
                assets
                    .into_iter()
                    .filter(|a| a["updated_at"].as_str().unwrap() <= time)
                    .collect(),
            );
        }
        releases
    }

    #[test]
    fn test_half_uploaded_release_never_downgrades_to_cpu() {
        let during_upload = listing_at("2026-09-30T20:22:20Z");
        let names = |r: &serde_json::Value| -> Vec<String> {
            r["assets"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a["name"].as_str().unwrap().to_string())
                .collect()
        };
        let newest = names(&during_upload[0]);
        assert!(newest.contains(&"llama-b11298-bin-win-cpu-x64.zip".to_string()));
        assert!(!newest.contains(&"llama-b11298-bin-win-vulkan-x64.zip".to_string()));

        // Update of a Vulkan build: the complete previous release, never b11298's CPU build.
        let info = pick_release(&during_upload, &win_vulkan(), PickMode::Exact).unwrap();
        assert_eq!(info.asset_name, "llama-b11297-bin-win-vulkan-x64.zip");
        assert_eq!(info.backend.as_deref(), Some("vulkan"));
        // Even the first-install fallback prefers the previous release's GPU build.
        let rocm = crate::hardware::RecommendedBackend::Rocm.asset_query("windows", "x86_64");
        let info = pick_release(&during_upload, &rocm, PickMode::Fallback).unwrap();
        assert_eq!(info.asset_name, "llama-b11297-bin-win-rocm-10.0-x64.zip");
        let info = pick_release(&during_upload, &win_vulkan(), PickMode::Fallback).unwrap();
        assert_eq!(info.asset_name, "llama-b11297-bin-win-vulkan-x64.zip");

        // Once the upload finished: b11298's Vulkan build.
        let done = listing_at("2026-09-30T20:30:00Z");
        let info = pick_release(&done, &win_vulkan(), PickMode::Exact).unwrap();
        assert_eq!(info.asset_name, "llama-b11298-bin-win-vulkan-x64.zip");
        assert_eq!(info.integrity, AssetIntegrity::Verified);

        // No release has the requested build at all: an update fails instead of switching.
        let only_cpu: Vec<serde_json::Value> = listing_at("2026-09-30T20:22:20Z")
            .into_iter()
            .map(|mut r| {
                let assets = r["assets"].as_array().unwrap().clone();
                r["assets"] = serde_json::Value::Array(
                    assets
                        .into_iter()
                        .filter(|a| !a["name"].as_str().unwrap().contains("vulkan"))
                        .collect(),
                );
                r
            })
            .collect();
        let e = pick_release(&only_cpu, &win_vulkan(), PickMode::Exact).unwrap_err();
        assert!(e.starts_with("runtime.no_matching_asset"), "{e}");
    }

    #[test]
    fn test_detect_backend_in_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            detect_backend_in_dir(dir.path()).as_deref(),
            if cfg!(target_os = "macos") {
                Some("metal")
            } else {
                None
            }
        );
        for (files, expected) in [
            (
                &["ggml-base.dll", "ggml-cpu-zen4.dll", "ggml-vulkan.dll"][..],
                "vulkan",
            ),
            (
                &["ggml-base.dll", "ggml-cpu-x64.dll", "ggml-hip.dll"][..],
                "rocm",
            ),
            (
                &[
                    "libggml-base.so",
                    "libggml-cpu-haswell.so",
                    "libggml-cuda.so",
                ][..],
                "cuda",
            ),
            (&["ggml-base.dll", "ggml-sycl.dll"][..], "sycl"),
            (&["ggml-base.dll", "ggml-cpu-x64.dll"][..], "cpu"),
        ] {
            let d = tempfile::tempdir().unwrap();
            for f in files {
                std::fs::write(d.path().join(f), b"").unwrap();
            }
            assert_eq!(detect_backend_in_dir(d.path()).as_deref(), Some(expected));
        }
    }

    #[test]
    fn test_install_query_never_switches_installed_backend() {
        let mut hw = crate::hardware::detect_hardware();
        hw.os = "windows".into();
        hw.arch = "x86_64".into();
        hw.recommended_backend = crate::hardware::RecommendedBackend::Rocm;
        let q = |c, i, r| install_query(&hw, c, i, r).query.backend;
        let mode = |c, i, r| install_query(&hw, c, i, r).mode;
        // Only the recommendation may fall back to another backend.
        assert_eq!(mode(None, None, None), PickMode::Fallback);
        assert_eq!(mode(None, Some("vulkan"), None), PickMode::Exact);
        assert_eq!(mode(Some("vulkan"), None, None), PickMode::Exact);
        assert_eq!(mode(None, None, Some("vulkan")), PickMode::Exact);
        // First install: the recommendation.
        assert_eq!(q(None, None, None).as_deref(), Some("rocm"));
        // Installed Vulkan build: updates stay on Vulkan.
        assert_eq!(q(None, Some("vulkan"), None).as_deref(), Some("vulkan"));
        // Configured override beats the installed build; an explicit request beats both.
        assert_eq!(q(Some("cpu"), Some("vulkan"), None).as_deref(), Some("cpu"));
        assert_eq!(
            q(Some("cpu"), Some("vulkan"), Some("rocm")).as_deref(),
            Some("rocm")
        );
        assert_eq!(q(Some(" "), None, None).as_deref(), Some("rocm"));
    }

    #[test]
    fn test_run_command_with_timeout_kills_child() {
        let mut cmd = if cfg!(windows) {
            let mut c = std::process::Command::new("powershell");
            c.args(["-Command", "Start-Sleep -Seconds 30"]);
            c
        } else {
            let mut c = std::process::Command::new("sleep");
            c.arg("30");
            c
        };
        let start = std::time::Instant::now();
        let res = run_command_with_timeout(&mut cmd, std::time::Duration::from_millis(150), 1024);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        // Far below the child's 30 s: the kill worked. The margin absorbs a slow PowerShell
        // start and teardown on a loaded machine (several parallel builds failed a 3 s limit).
        assert!(start.elapsed() < std::time::Duration::from_secs(15));
    }

    #[test]
    fn test_run_command_with_timeout_caps_output() {
        let mut cmd = if cfg!(windows) {
            let mut c = std::process::Command::new("powershell");
            c.args(["-Command", "[Console]::Out.Write('A' * 100000)"]);
            c
        } else {
            let mut c = std::process::Command::new("sh");
            c.args(["-c", "head -c 100000 /dev/zero | tr '\\0' 'A'"]);
            c
        };
        let max_bytes = 4096;
        // Generous timeout: a cold PowerShell start alone can take longer than 5 s.
        let res = run_command_with_timeout(&mut cmd, std::time::Duration::from_secs(30), max_bytes);
        assert!(res.is_ok(), "{:?}", res.as_ref().err());
        let out = res.unwrap();
        assert_eq!(out.stdout.len(), max_bytes);
    }
}
