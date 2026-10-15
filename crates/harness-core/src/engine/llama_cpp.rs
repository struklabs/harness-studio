//! llama.cpp as an [`Engine`]: delegates to the modules that implement it.

use std::path::Path;
use std::sync::Arc;

use super::{Engine, MetricsSource, ModelFormat, ReleaseSource, RuntimeKind, ServerEngine};
use crate::capabilities::RuntimeCapabilities;
use crate::gguf::GgufMetadata;
use crate::params::ParamCatalog;
use crate::process::{BuildOutput, LaunchConfig, ProcessManager, ServerCaps};
use crate::runtime::ReleaseAsset;
use crate::telemetry::Metrics;
use crate::vram::{FitInput, FitResult, Measurement, MemoryBudget};

/// llama-server builds: official (managed, custom, system) and forks.
#[derive(Debug, Clone, Copy, Default)]
pub struct LlamaCppEngine;

impl Engine for LlamaCppEngine {
    fn family(&self) -> &'static str {
        super::LLAMA_CPP
    }

    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Server
    }

    fn release_source(&self) -> ReleaseSource {
        ReleaseSource {
            repo: "ggml-org/llama.cpp",
            binary: "llama-server",
        }
    }

    fn release_asset(
        &self,
        release: &serde_json::Value,
        query: &crate::hardware::AssetQuery,
    ) -> Option<crate::runtime::ReleaseAssetInfo> {
        crate::runtime::release_asset(release, query)
    }

    fn pick_release_assets(
        &self,
        assets: &[ReleaseAsset],
        backends: &[String],
    ) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
        pick_assets(assets, backends)
    }

    fn supports(&self, format: ModelFormat) -> bool {
        format == ModelFormat::Gguf
    }

    fn detect_capabilities(&self, binary: &Path) -> Arc<RuntimeCapabilities> {
        crate::capabilities::registry().get_or_probe(binary)
    }

    fn cached_capabilities(&self, binary: &Path) -> Option<Arc<RuntimeCapabilities>> {
        crate::capabilities::registry().cached(binary)
    }

    fn probe_capabilities(&self, binary: &Path) -> RuntimeCapabilities {
        crate::capabilities::probe_binary(binary)
    }

    fn param_catalog(&self) -> &'static ParamCatalog {
        crate::params::catalog()
    }

    fn as_server(&self) -> Option<&dyn ServerEngine> {
        Some(self)
    }
}

impl ServerEngine for LlamaCppEngine {
    fn build_launch(
        &self,
        config: &LaunchConfig,
        caps: &ServerCaps,
        runtime: Option<&RuntimeCapabilities>,
    ) -> Result<BuildOutput, String> {
        if super::audio_cpp::is_model(&config.model_path) {
            return Err(crate::error_key::err("audio_cpp.llama_rejected").into());
        }
        ProcessManager::build_args_with(config, caps, runtime)
    }

    fn health_path(&self) -> &'static str {
        "/health"
    }

    fn health(&self, status: u16, body: &str) -> bool {
        crate::process::health_ready(status, body)
    }

    fn openai_base(&self, host_url: &str) -> String {
        format!("{}/v1", host_url.trim_end_matches('/'))
    }

    fn metrics_source(&self) -> MetricsSource {
        MetricsSource {
            metrics_path: "/metrics",
            slots_path: Some("/slots"),
        }
    }

    fn parse_metrics(&self, text: &str) -> Metrics {
        crate::telemetry::parse_prometheus(text)
    }

    fn estimate_memory(
        &self,
        input: &FitInput,
        meta: Option<&GgufMetadata>,
        budget: &MemoryBudget,
        measured: &[Measurement],
    ) -> FitResult {
        crate::vram::fit_with(input, meta, budget, measured)
    }
}

// --- release archive naming ---
//
// Release archives follow `llama-<tag>-bin-<os>-<backend>[-<ver>]-<arch>` (`.zip` on
// Windows, `.tar.gz` elsewhere); forks such as PrismML's use the same scheme with their own
// tags. A CUDA build brings a `cudart-*` companion archive.

fn os_tokens() -> &'static [&'static str] {
    if cfg!(windows) {
        &["win"]
    } else if cfg!(target_os = "macos") {
        &["macos"]
    } else {
        &["ubuntu", "linux"]
    }
}

fn arch_token() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    }
}

/// Backend spellings used in archive names (upstream `rocm`, the fork's `hip-radeon`...).
/// `None` = the plain build without a backend token.
fn backend_tokens(backend: &str) -> Vec<Option<&'static str>> {
    match backend {
        "vulkan" => vec![Some("vulkan")],
        "cuda" => vec![Some("cuda")],
        "rocm" | "hip" => vec![Some("rocm"), Some("hip-radeon"), Some("hip")],
        "sycl" => vec![Some("sycl"), Some("sycl-fp16")],
        "metal" => vec![None],
        // Windows CPU builds are named `cpu`; Linux/macOS CPU builds have no token.
        _ => {
            if cfg!(windows) {
                vec![Some("cpu")]
            } else {
                vec![None]
            }
        }
    }
}

/// `(main + companions, backend)` of the first backend that has an archive for this OS/arch.
fn pick_assets(
    assets: &[ReleaseAsset],
    backends: &[String],
) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
    pick_assets_for(assets, backends, cuda13_allowed())
}

/// Whether CUDA 13 builds can run here: on Windows they need NVIDIA driver 580 or newer, the
/// rule of the official install path ([`crate::runtime::select_asset_for_driver`]); a known
/// older or unknown driver gets 12.x only. Elsewhere every CUDA build is a candidate.
fn cuda13_allowed() -> bool {
    !cfg!(windows)
        || crate::hardware::windows_driver_at_least(
            crate::runtime::windows_nvidia_driver().as_deref(),
            (580, 0),
        ) == Some(true)
}

fn pick_assets_for(
    assets: &[ReleaseAsset],
    backends: &[String],
    cuda13_ok: bool,
) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
    let arch = regex::escape(arch_token());
    let os = os_tokens()
        .iter()
        .map(|o| regex::escape(o))
        .collect::<Vec<_>>()
        .join("|");
    for backend in backends {
        for token in backend_tokens(backend) {
            let middle = match token {
                Some(t) => format!(r"{}(?:-(\d+(?:\.\d+)*))?-", regex::escape(t)),
                None => String::new(),
            };
            let Ok(re) = regex::Regex::new(&format!(
                r"^llama-.+-bin-(?:{})-{}{}\.(?:zip|tar\.gz)$",
                os, middle, arch
            )) else {
                continue;
            };
            let mut hits: Vec<(&ReleaseAsset, Vec<u32>)> = assets
                .iter()
                .filter_map(|a| {
                    let c = re.captures(&a.name)?;
                    let ver = c
                        .get(1)
                        .map(|m| {
                            m.as_str()
                                .split('.')
                                .map(|p| p.parse::<u32>().unwrap_or(0))
                                .collect()
                        })
                        .unwrap_or_default();
                    Some((a, ver))
                })
                .collect();
            // A CUDA 13 build on a driver that cannot run it would load no GPU at all.
            if token == Some("cuda") && !cuda13_ok {
                hits.retain(|(_, ver)| ver.first().is_none_or(|major| *major == 12));
            }
            // Several CUDA versions: the lowest is the most driver-compatible.
            hits.sort_by(|a, b| a.1.cmp(&b.1));
            let Some((main, ver)) = hits.first() else {
                continue;
            };
            let mut out = vec![(*main).clone()];
            if token == Some("cuda") && !ver.is_empty() {
                // The companion is named after the build: `cudart-llama-bin-<os>-cuda-<ver>-<arch>`
                // (the OS too: upstream ships the same CUDA version for Windows and Ubuntu).
                let build = main
                    .name
                    .split_once("-bin-")
                    .map(|(_, rest)| rest)
                    .and_then(|rest| {
                        rest.strip_suffix(".zip")
                            .or_else(|| rest.strip_suffix(".tar.gz"))
                    });
                if let Some(build) = build {
                    let needle = format!("-bin-{build}.");
                    if let Some(rt) = assets
                        .iter()
                        .find(|a| a.name.starts_with("cudart-") && a.name.contains(&needle))
                    {
                        out.push(rt.clone());
                    }
                }
            }
            return Some((out, Some(backend.clone())));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> ReleaseAsset {
        ReleaseAsset {
            name: name.into(),
            download_url: format!("https://example.invalid/{name}"),
            size: 1,
            digest: None,
            integrity: Default::default(),
        }
    }

    /// Archive names of this OS and of another one, with this OS's extension.
    fn names() -> (&'static str, &'static str, &'static str) {
        if cfg!(windows) {
            ("win", "ubuntu", ".zip")
        } else {
            ("ubuntu", "win", ".tar.gz")
        }
    }

    #[test]
    fn fork_cuda_13_needs_a_driver_that_runs_it() {
        if cfg!(target_os = "macos") {
            return; // no CUDA builds
        }
        let (os, _, ext) = names();
        let arch = arch_token();
        let assets = vec![
            asset(&format!("llama-f1-bin-{os}-cuda-13.3-{arch}{ext}")),
            asset(&format!("llama-f1-bin-{os}-vulkan-{arch}{ext}")),
        ];
        let backends = ["cuda".to_string(), "vulkan".to_string()];
        // Old driver: the CUDA 13 build is skipped and the next backend is used.
        let (a, b) = pick_assets_for(&assets, &backends, false).unwrap();
        assert_eq!(b.as_deref(), Some("vulkan"));
        assert!(a[0].name.contains("vulkan"));
        // Driver 580+: the CUDA 13 build is used.
        let (a, b) = pick_assets_for(&assets, &backends, true).unwrap();
        assert_eq!(b.as_deref(), Some("cuda"));
        assert!(a[0].name.contains("cuda-13.3"));
        // CUDA 12 stays a candidate on an old driver, and the lowest version wins.
        let mut both = assets.clone();
        both.push(asset(&format!("llama-f1-bin-{os}-cuda-12.4-{arch}{ext}")));
        let (a, _) = pick_assets_for(&both, &backends, false).unwrap();
        assert!(a[0].name.contains("cuda-12.4"));
    }

    #[test]
    fn fork_cudart_matches_the_build_os() {
        if cfg!(target_os = "macos") {
            return;
        }
        let (os, other, ext) = names();
        let other_ext = if ext == ".zip" { ".tar.gz" } else { ".zip" };
        let arch = arch_token();
        let assets = vec![
            asset(&format!("llama-f1-bin-{os}-cuda-13.4-{arch}{ext}")),
            // Same CUDA version for the other OS, listed first.
            asset(&format!(
                "cudart-llama-bin-{other}-cuda-13.4-{arch}{other_ext}"
            )),
            asset(&format!("cudart-llama-bin-{os}-cuda-13.4-{arch}{ext}")),
        ];
        let (a, _) = pick_assets_for(&assets, &["cuda".to_string()], true).unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(
            a[1].name,
            format!("cudart-llama-bin-{os}-cuda-13.4-{arch}{ext}")
        );
        // No companion for this OS: the build alone.
        let (a, _) = pick_assets_for(&assets[..2], &["cuda".to_string()], true).unwrap();
        assert_eq!(a.len(), 1);
    }
}
