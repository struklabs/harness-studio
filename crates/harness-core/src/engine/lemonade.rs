//! Lemonade as an [`Engine`]: `lemond`, the server of `lemonade-sdk/lemonade`.
//!
//! What the releases ship (checked 2026-10-12 on `v2026.41.1`): per release one
//! `lemonade-embeddable-<version>-<os>` archive that holds only `lemond`, the `lemonade` CLI,
//! `LICENSE` and `resources/` (5-7 MB): `windows-x64.zip`, `ubuntu-x64.tar.gz`,
//! `ubuntu-arm64.tar.gz` and `macos-arm64.tar.gz`. The inference backends (llama.cpp,
//! whisper.cpp, FastFlowLM...) are not in it: `lemond` downloads them on first use. The
//! releases also carry `candidate-v<next>` pre-releases with the same asset names; those are
//! skipped ([`Engine::accepts_release`]), so "newest" means the newest stable tag.
//!
//! `lemond` is OpenAI-compatible (`/v1/chat/completions`, `/v1/embeddings`, `/v1/rerank`,
//! `/v1/audio/transcriptions`) and loads a model on its first request, so the family is a
//! [`ServerEngine`] that is not tied to one model file: it supports no [`ModelFormat`], and
//! [`crate::lemonade`] drives it (own port, key and lifecycle) instead of the generic
//! `ProcessManager` launch.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use super::{Engine, MetricsSource, ModelFormat, ReleaseSource, RuntimeKind, ServerEngine};
use crate::capabilities::RuntimeCapabilities;
use crate::error_key::err;
use crate::gguf::GgufMetadata;
use crate::lemonade::LEMONADE_FAMILY;
use crate::params::ParamCatalog;
use crate::process::{BuildOutput, LaunchConfig, ServerCaps};
use crate::runtime::ReleaseAsset;
use crate::telemetry::Metrics;
use crate::vram::{
    EstimateSource, FitCategory, FitInput, FitResult, Measurement, MemoryBudget, MemoryEstimate,
};

/// Lemonade builds from the official releases.
#[derive(Debug, Clone, Copy, Default)]
pub struct LemonadeEngine;

/// No catalog: `lemond` has no per-model launch parameters; its settings live in its
/// `config.json`, which [`crate::lemonade`] owns.
pub fn catalog() -> &'static ParamCatalog {
    static CATALOG: OnceLock<ParamCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| ParamCatalog {
        schema_version: 1,
        params: Vec::new(),
    })
}

/// A stable release tag: `v2026.41.1` (the pre-releases are `candidate-v2026.42.0`).
pub fn is_stable_tag(tag: &str) -> bool {
    let Some(rest) = tag.strip_prefix('v') else {
        return false;
    };
    let parts: Vec<&str> = rest.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

fn asset_pattern() -> &'static str {
    if cfg!(all(windows, target_arch = "x86_64")) {
        r"^lemonade-embeddable-\d+(?:\.\d+)*-windows-x64\.zip$"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        r"^lemonade-embeddable-\d+(?:\.\d+)*-ubuntu-x64\.tar\.gz$"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        r"^lemonade-embeddable-\d+(?:\.\d+)*-ubuntu-arm64\.tar\.gz$"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        r"^lemonade-embeddable-\d+(?:\.\d+)*-macos-arm64\.tar\.gz$"
    } else {
        // No embeddable build for this platform: matches nothing.
        r"^$"
    }
}

/// The embeddable archive of a release for this OS and CPU.
pub fn pick_assets(assets: &[ReleaseAsset]) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
    let re = regex::Regex::new(asset_pattern()).ok()?;
    let asset = assets.iter().find(|a| re.is_match(&a.name))?;
    Some((vec![asset.clone()], None))
}

impl Engine for LemonadeEngine {
    fn family(&self) -> &'static str {
        LEMONADE_FAMILY
    }

    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Server
    }

    fn release_source(&self) -> ReleaseSource {
        ReleaseSource {
            repo: crate::lemonade::LEMONADE_REPO,
            binary: "lemond",
        }
    }

    /// Like whisper.cpp, Lemonade has no managed "official" runtime: its builds are
    /// registered runtimes installed from a release.
    fn release_asset(
        &self,
        _release: &serde_json::Value,
        _query: &crate::hardware::AssetQuery,
    ) -> Option<crate::runtime::ReleaseAssetInfo> {
        None
    }

    fn accepts_release(&self, release: &serde_json::Value) -> bool {
        let prerelease = release
            .get("prerelease")
            .and_then(|p| p.as_bool())
            .unwrap_or(false);
        let tag = release
            .get("tag_name")
            .and_then(|t| t.as_str())
            .unwrap_or_default();
        !prerelease && is_stable_tag(tag)
    }

    fn pick_release_assets(
        &self,
        assets: &[ReleaseAsset],
        _backends: &[String],
    ) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
        pick_assets(assets)
    }

    /// Lemonade is not a runner of model files Harness holds: it has its own model registry.
    fn supports(&self, _format: ModelFormat) -> bool {
        false
    }

    fn detect_capabilities(&self, binary: &Path) -> Arc<RuntimeCapabilities> {
        crate::capabilities::registry()
            .get_or_probe_with(binary, crate::capabilities::lemonade::probe_binary)
    }

    fn cached_capabilities(&self, binary: &Path) -> Option<Arc<RuntimeCapabilities>> {
        crate::capabilities::registry().cached(binary)
    }

    fn probe_capabilities(&self, binary: &Path) -> RuntimeCapabilities {
        crate::capabilities::lemonade::probe_binary(binary)
    }

    fn param_catalog(&self) -> &'static ParamCatalog {
        catalog()
    }

    fn as_server(&self) -> Option<&dyn ServerEngine> {
        Some(self)
    }
}

impl ServerEngine for LemonadeEngine {
    fn build_launch(
        &self,
        config: &LaunchConfig,
        _caps: &ServerCaps,
        _runtime: Option<&RuntimeCapabilities>,
    ) -> Result<BuildOutput, String> {
        // The API key protects the routes, but Harness still keeps it on this machine.
        if !crate::network::is_loopback_host(&config.host) {
            return Err(err("process.lemonade_loopback_only")
                .with("host", &config.host)
                .into());
        }
        Ok(BuildOutput {
            args: crate::lemonade::launch_args(
                &config.host,
                config.port,
                &crate::lemonade::root().join("cache"),
                &crate::lemonade::root().join("config"),
            ),
            skipped: Vec::new(),
        })
    }

    /// Answers without a key as soon as the process listens.
    fn health_path(&self) -> &'static str {
        "/live"
    }

    fn health(&self, status: u16, body: &str) -> bool {
        status == 200
            && serde_json::from_str::<serde_json::Value>(body)
                .ok()
                .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(|s| s == "ok"))
                .unwrap_or(false)
    }

    fn openai_base(&self, host_url: &str) -> String {
        format!("{}/v1", host_url.trim_end_matches('/'))
    }

    /// Lemonade's `/metrics` is its own (`lemonade_*` series), not llama.cpp's.
    fn metrics_source(&self) -> MetricsSource {
        MetricsSource {
            metrics_path: "",
            slots_path: None,
        }
    }

    fn parse_metrics(&self, _text: &str) -> Metrics {
        Metrics::default()
    }

    /// The model is Lemonade's (its size is not known to a launch): nothing to estimate.
    fn estimate_memory(
        &self,
        _input: &FitInput,
        _meta: Option<&GgufMetadata>,
        budget: &MemoryBudget,
        _measured: &[Measurement],
    ) -> FitResult {
        FitResult {
            category: FitCategory::FullGpu,
            vram_percentage: 0,
            ram_percentage: 0,
            estimate: MemoryEstimate::default(),
            budget: *budget,
            source: EstimateSource::Estimate,
            measured_at: None,
            recommended_gpu_layers: 0,
            slots: None,
            kv: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::AssetIntegrity;

    /// The releases of lemonade-sdk/lemonade as the GitHub API returned them on 2026-10-12
    /// (the embeddable assets of `v2026.41.1` and two `candidate-` pre-releases).
    fn releases() -> Vec<serde_json::Value> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/github-releases-lemonade-v2026.41.1.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn assets_of(release: &serde_json::Value) -> Vec<ReleaseAsset> {
        release["assets"]
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

    #[test]
    fn stable_tags_are_told_from_candidates() {
        assert!(is_stable_tag("v2026.41.1"));
        assert!(is_stable_tag("v9.4.0"));
        assert!(!is_stable_tag("candidate-v2026.42.0"));
        assert!(!is_stable_tag("2026.41.1"));
        assert!(!is_stable_tag("v2026.41"));
        assert!(!is_stable_tag("v2026.41.1-rc1"));
        let e = LemonadeEngine;
        let all = releases();
        let accepted: Vec<&str> = all
            .iter()
            .filter(|r| e.accepts_release(r))
            .map(|r| r["tag_name"].as_str().unwrap())
            .collect();
        assert_eq!(accepted, vec!["v2026.41.1"]);
    }

    #[test]
    fn picks_the_embeddable_archive_of_this_os() {
        let all = releases();
        let stable = all.iter().find(|r| r["tag_name"] == "v2026.41.1").unwrap();
        let picked = pick_assets(&assets_of(stable));
        if cfg!(all(windows, target_arch = "x86_64")) {
            let (a, backend) = picked.unwrap();
            assert_eq!(a.len(), 1);
            assert_eq!(a[0].name, "lemonade-embeddable-2026.41.1-windows-x64.zip");
            assert_eq!(a[0].size, 5_379_955);
            assert_eq!(
                a[0].digest.as_deref(),
                Some("sha256:055b582d6c64f3a7a60438edf371b1fc75bfdd481b4f104175ba521f57eadb08")
            );
            assert!(backend.is_none());
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(
                picked.unwrap().0[0].name,
                "lemonade-embeddable-2026.41.1-ubuntu-x64.tar.gz"
            );
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            assert_eq!(
                picked.unwrap().0[0].name,
                "lemonade-embeddable-2026.41.1-macos-arm64.tar.gz"
            );
        }
        // The installer packages are not the embeddable build.
        let msi = vec![ReleaseAsset {
            name: "lemonade.msi".into(),
            download_url: String::new(),
            size: 1,
            digest: None,
            integrity: AssetIntegrity::Missing,
        }];
        assert!(pick_assets(&msi).is_none());
    }

    #[test]
    fn the_family_is_a_server_without_model_formats() {
        let e = crate::engine::engine(LEMONADE_FAMILY).expect("registered");
        assert_eq!(e.kind(), RuntimeKind::Server);
        assert_eq!(e.release_source().repo, "lemonade-sdk/lemonade");
        assert_eq!(e.release_source().binary, "lemond");
        assert!(e.as_server().is_some() && e.as_job().is_none());
        for f in [
            ModelFormat::Gguf,
            ModelFormat::GgmlBin,
            ModelFormat::Onnx,
            ModelFormat::Safetensors,
        ] {
            assert!(!e.supports(f));
        }
        // No model file ever resolves to it.
        assert_ne!(
            crate::engine::server_for_model(Path::new("a.gguf")).map(|s| s.family()),
            Some(LEMONADE_FAMILY)
        );
        assert!(e.param_catalog().params.is_empty());
    }

    #[test]
    fn launch_arguments_bind_loopback_and_keep_the_dirs_positional() {
        let s = LemonadeEngine;
        let config = LaunchConfig {
            host: "127.0.0.1".into(),
            port: 11450,
            ..test_config()
        };
        let out = s
            .build_launch(&config, &ServerCaps::default(), None)
            .unwrap();
        let a = &out.args;
        let at = |flag: &str| a.iter().position(|x| x == flag).unwrap();
        assert_eq!(a[at("--port") + 1], "11450");
        assert_eq!(a[at("--host") + 1], "127.0.0.1");
        assert!(a.contains(&"--no-broadcast".to_string()));
        assert!(a[a.len() - 2].ends_with("cache") && a[a.len() - 1].ends_with("config"));
        let lan = LaunchConfig {
            host: "0.0.0.0".into(),
            ..test_config()
        };
        let e = s
            .build_launch(&lan, &ServerCaps::default(), None)
            .unwrap_err();
        assert!(e.contains("process.lemonade_loopback_only"), "{e}");
        assert_eq!(
            s.openai_base("http://127.0.0.1:11450"),
            "http://127.0.0.1:11450/v1"
        );
        assert!(s.health(200, r#"{"status":"ok"}"#));
        assert!(!s.health(503, r#"{"status":"ok"}"#));
        assert!(!s.health(200, "ok"));
    }

    fn test_config() -> LaunchConfig {
        serde_json::from_value(serde_json::json!({
            "model_path": "", "gpu_layers": 0, "context_size": 0,
            "host": "127.0.0.1", "port": 0, "threads": null
        }))
        .unwrap()
    }
}
