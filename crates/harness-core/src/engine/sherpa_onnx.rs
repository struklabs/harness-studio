//! sherpa-onnx as an [`Engine`]: `sherpa-onnx-offline-tts`, the text-to-speech
//! command of the k2-fsa/sherpa-onnx releases. sherpa-onnx bundles its own ONNX Runtime, so no
//! system runtime is needed; the models (Kokoro, Piper/VITS) are plain files read per run.
//!
//! What the official releases ship (checked 2026-10-11 on `v1.13.8`, 287 assets): every OS and
//! arch has a `shared` archive with `bin/` (the CLI tools and `onnxruntime.dll`) next to `lib/`
//! (C API) and `include/`. Windows x64: `sherpa-onnx-v<v>-win-x64-shared-MD-Release.tar.bz2`
//! (20 MB; the `MT` / `static` / `Debug` / `RelWithDebInfo` / `no-tts` / `-lib` variants are
//! not the ones to run), Linux: `...-linux-x64-shared.tar.bz2` and
//! `...-linux-aarch64-shared-cpu.tar.bz2`, macOS: `...-osx-<arch>-shared.tar.bz2`. CUDA builds
//! exist for Windows and Linux x64 (0.5 GB with cuDNN) and are deliberately not picked: the
//! TTS models Harness uses run faster than real time on a CPU.
//!
//! There is no HTTP server in these releases (only an ASR websocket server), and the TTS tool
//! reads the model on every run, so this family is a [`JobEngine`]: one process per request
//! (see [`crate::tts`]), the gateway's `/v1/audio/speech` and `harness speak` drive it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use super::{Engine, JobEngine, JobProgress, ModelFormat, ReleaseSource, RuntimeKind};
use crate::capabilities::RuntimeCapabilities;
use crate::params::ParamCatalog;
use crate::runtime::ReleaseAsset;

/// Family id of sherpa-onnx builds.
pub const SHERPA_ONNX: &str = "sherpa-onnx";

/// `k2-fsa/sherpa-onnx`, the repository the managed runtime comes from.
pub const SHERPA_ONNX_REPO: &str = "k2-fsa/sherpa-onnx";

/// sherpa-onnx builds from the official releases (or a local folder).
#[derive(Debug, Clone, Copy, Default)]
pub struct SherpaOnnxEngine;

/// No catalog: a TTS run is configured by the request (voice, speed), not by launch parameters.
pub fn catalog() -> &'static ParamCatalog {
    static CATALOG: OnceLock<ParamCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| ParamCatalog {
        schema_version: 1,
        params: Vec::new(),
    })
}

impl Engine for SherpaOnnxEngine {
    fn family(&self) -> &'static str {
        SHERPA_ONNX
    }

    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Job
    }

    fn release_source(&self) -> ReleaseSource {
        ReleaseSource {
            repo: SHERPA_ONNX_REPO,
            binary: "sherpa-onnx-offline-tts",
        }
    }

    /// Like whisper.cpp, sherpa-onnx has no managed "official" runtime: its builds are
    /// registered runtimes (`runtimes.json`) installed from a release.
    fn release_asset(
        &self,
        _release: &serde_json::Value,
        _query: &crate::hardware::AssetQuery,
    ) -> Option<crate::runtime::ReleaseAssetInfo> {
        None
    }

    fn pick_release_assets(
        &self,
        assets: &[ReleaseAsset],
        backends: &[String],
    ) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
        pick_assets(assets, backends)
    }

    fn supports(&self, format: ModelFormat) -> bool {
        format == ModelFormat::Onnx
    }

    fn version_arg(&self) -> &'static str {
        // `--version` is an unknown flag; `--help` prints the usage and exits 0.
        "--help"
    }

    fn detect_capabilities(&self, binary: &Path) -> Arc<RuntimeCapabilities> {
        crate::capabilities::registry()
            .get_or_probe_with(binary, crate::capabilities::sherpa::probe_binary)
    }

    fn cached_capabilities(&self, binary: &Path) -> Option<Arc<RuntimeCapabilities>> {
        crate::capabilities::registry().cached(binary)
    }

    fn probe_capabilities(&self, binary: &Path) -> RuntimeCapabilities {
        crate::capabilities::sherpa::probe_binary(binary)
    }

    fn param_catalog(&self) -> &'static ParamCatalog {
        catalog()
    }

    fn as_job(&self) -> Option<&dyn JobEngine> {
        Some(self)
    }
}

impl JobEngine for SherpaOnnxEngine {
    /// The registered runtime's binary is the TTS tool itself.
    fn job_binary(&self, server_binary: &Path) -> Result<PathBuf, String> {
        if server_binary.is_file() {
            Ok(server_binary.to_path_buf())
        } else {
            Err(crate::error_key::err("tts.binary_not_found")
                .with("path", server_binary.display())
                .into())
        }
    }

    /// `sample=59541, progress=0.500000` lines of the TTS tool.
    fn parse_progress(&self, line: &str) -> Option<JobProgress> {
        let fraction: f64 = line
            .trim()
            .split(',')
            .find_map(|p| p.trim().strip_prefix("progress="))?
            .parse()
            .ok()?;
        Some(JobProgress {
            done: (fraction.clamp(0.0, 1.0) * 100.0).round() as u64,
            total: Some(100),
            phase: None,
        })
    }
}

fn os_arch_patterns() -> Vec<&'static str> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        vec![r"^sherpa-onnx-v(\d+(?:\.\d+)*)-win-x64-shared-MD-Release\.tar\.bz2$"]
    } else if cfg!(all(windows, target_arch = "aarch64")) {
        vec![r"^sherpa-onnx-v(\d+(?:\.\d+)*)-win-arm64-shared-MD-Release\.tar\.bz2$"]
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        vec![r"^sherpa-onnx-v(\d+(?:\.\d+)*)-linux-x64-shared\.tar\.bz2$"]
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        vec![r"^sherpa-onnx-v(\d+(?:\.\d+)*)-linux-aarch64-shared-cpu\.tar\.bz2$"]
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        vec![
            r"^sherpa-onnx-v(\d+(?:\.\d+)*)-osx-arm64-shared\.tar\.bz2$",
            r"^sherpa-onnx-v(\d+(?:\.\d+)*)-osx-universal2-shared\.tar\.bz2$",
        ]
    } else if cfg!(target_os = "macos") {
        vec![
            r"^sherpa-onnx-v(\d+(?:\.\d+)*)-osx-x64-shared\.tar\.bz2$",
            r"^sherpa-onnx-v(\d+(?:\.\d+)*)-osx-universal2-shared\.tar\.bz2$",
        ]
    } else {
        Vec::new()
    }
}

/// The CPU `shared` archive of this OS / arch. Only the `cpu` backend matches (the default
/// order of an install ends with it, so `cuda` / `vulkan` fall through to it); an explicit
/// request for any other backend finds nothing.
fn pick_assets(
    assets: &[ReleaseAsset],
    backends: &[String],
) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
    if !backends.iter().any(|b| b == "cpu") {
        return None;
    }
    for pattern in os_arch_patterns() {
        let re = regex::Regex::new(pattern).ok()?;
        if let Some(asset) = assets.iter().find(|a| re.is_match(&a.name)) {
            return Some((vec![asset.clone()], Some("cpu".to_string())));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::AssetIntegrity;

    /// Release `v1.13.8` of k2-fsa/sherpa-onnx as the GitHub API returned it on 2026-10-11.
    fn assets() -> Vec<ReleaseAsset> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/github-releases-sherpa-onnx-v1.13.8.json");
        let releases: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        releases[0]["assets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| ReleaseAsset {
                name: a["name"].as_str().unwrap().into(),
                download_url: a["browser_download_url"].as_str().unwrap().into(),
                size: a["size"].as_u64().unwrap(),
                digest: a["digest"].as_str().map(str::to_string),
                integrity: AssetIntegrity::Verified,
            })
            .collect()
    }

    fn cpu() -> Vec<String> {
        vec!["cpu".to_string()]
    }

    #[test]
    fn engine_identity() {
        let e = SherpaOnnxEngine;
        assert_eq!(e.family(), "sherpa-onnx");
        assert_eq!(e.kind(), RuntimeKind::Job);
        assert_eq!(e.release_source().repo, "k2-fsa/sherpa-onnx");
        assert_eq!(e.release_source().binary, "sherpa-onnx-offline-tts");
        assert!(e.supports(ModelFormat::Onnx));
        assert!(!e.supports(ModelFormat::Gguf) && !e.supports(ModelFormat::GgmlBin));
        assert!(e.as_job().is_some() && e.as_server().is_none());
        assert_eq!(e.version_arg(), "--help");
        assert_eq!(
            crate::engine::engine("sherpa-onnx").unwrap().family(),
            SHERPA_ONNX
        );
        // An ONNX file served over HTTP is an encoder of `harness-onnx`; sherpa-onnx runs jobs.
        assert_eq!(
            crate::engine::server_for_model(Path::new("m/model.onnx")).map(|e| e.family()),
            Some(crate::engine::ONNX_FAMILY)
        );
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    fn windows_x64_picks_the_small_shared_release_build() {
        let (picked, backend) = pick_assets(&assets(), &cpu()).unwrap();
        assert_eq!(picked.len(), 1);
        assert_eq!(
            picked[0].name,
            "sherpa-onnx-v1.13.8-win-x64-shared-MD-Release.tar.bz2"
        );
        assert_eq!(backend.as_deref(), Some("cpu"));
        // The digest GitHub publishes is what the download is checked against.
        assert_eq!(
            picked[0].digest.as_deref(),
            Some("sha256:3e971a04b2e0ba4dfa53d381a006367ce8c9f5f09b4ae00043e9845c2baded22")
        );
        assert!(picked[0].size < 25 * 1024 * 1024);
    }

    #[test]
    fn only_the_cpu_backend_matches() {
        let default_order = vec!["cuda".to_string(), "vulkan".to_string(), "cpu".to_string()];
        let from_default = pick_assets(&assets(), &default_order);
        assert_eq!(from_default.map(|(_, b)| b), Some(Some("cpu".to_string())));
        // An explicit CUDA request is not served (0.5 GB cuDNN archives).
        assert!(pick_assets(&assets(), &["cuda".to_string()]).is_none());
        assert!(pick_assets(&[], &cpu()).is_none());
        // A release of another layout (a `tauri` tag, a jar) matches nothing.
        let other = vec![ReleaseAsset {
            name: "sherpa-onnx-native-lib-win-x64-1.13.8.jar".into(),
            download_url: String::new(),
            size: 1,
            digest: None,
            integrity: AssetIntegrity::Verified,
        }];
        assert!(pick_assets(&other, &cpu()).is_none());
    }

    #[test]
    fn progress_lines_of_the_tts_tool() {
        let e = SherpaOnnxEngine;
        assert_eq!(
            e.parse_progress("sample=59541, progress=1.000000"),
            Some(JobProgress {
                done: 100,
                total: Some(100),
                phase: None
            })
        );
        assert_eq!(
            e.parse_progress("sample=100, progress=0.254").unwrap().done,
            25
        );
        assert_eq!(e.parse_progress("Elapsed seconds: 2.441 s"), None);
        assert_eq!(e.parse_progress("progress=abc"), None);
    }

    #[test]
    fn job_binary_must_exist() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir
            .path()
            .join(SherpaOnnxEngine.release_source().binary_file());
        let err = SherpaOnnxEngine.job_binary(&bin).unwrap_err();
        assert!(err.contains("tts.binary_not_found"), "{err}");
        std::fs::write(&bin, b"x").unwrap();
        assert_eq!(SherpaOnnxEngine.job_binary(&bin).unwrap(), bin);
    }
}
