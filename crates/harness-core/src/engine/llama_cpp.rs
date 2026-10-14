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
            // Several CUDA versions: the lowest is the most driver-compatible.
            hits.sort_by(|a, b| a.1.cmp(&b.1));
            let Some((main, ver)) = hits.first() else {
                continue;
            };
            let mut out = vec![(*main).clone()];
            if token == Some("cuda") && !ver.is_empty() {
                let v = ver
                    .iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(".");
                let needle = format!("-cuda-{}-{}.", v, arch_token());
                if let Some(rt) = assets
                    .iter()
                    .find(|a| a.name.starts_with("cudart-") && a.name.contains(&needle))
                {
                    out.push(rt.clone());
                }
            }
            return Some((out, Some(backend.clone())));
        }
    }
    None
}
