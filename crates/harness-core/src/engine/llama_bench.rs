//! `llama-bench` as a [`JobEngine`]: the benchmark of a llama.cpp build. It ships in the same
//! release archives as `llama-server` and prints `llama-bench: benchmark 3/8: prompt run 1/5`
//! progress lines on stderr and result rows (`-o jsonl`) on stdout.
//!
//! It is a tool of the llama.cpp family rather than a runtime family of its own, so it is
//! not in [`super::engines`]; [`super::llama_bench`] is how callers get it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{
    Engine, JobEngine, JobProgress, LlamaCppEngine, ModelFormat, ReleaseSource, RuntimeKind,
};
use crate::capabilities::RuntimeCapabilities;
use crate::params::ParamCatalog;
use crate::runtime::ReleaseAsset;

/// Family id of the benchmark tool (events and logs; not a registrable runtime family).
pub const LLAMA_BENCH: &str = "llama-bench";

#[derive(Debug, Clone, Copy, Default)]
pub struct LlamaBenchEngine;

impl Engine for LlamaBenchEngine {
    fn family(&self) -> &'static str {
        LLAMA_BENCH
    }

    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Job
    }

    fn release_source(&self) -> ReleaseSource {
        ReleaseSource {
            repo: "ggml-org/llama.cpp",
            binary: "llama-bench",
        }
    }

    fn release_asset(
        &self,
        release: &serde_json::Value,
        query: &crate::hardware::AssetQuery,
    ) -> Option<crate::runtime::ReleaseAssetInfo> {
        LlamaCppEngine.release_asset(release, query)
    }

    fn pick_release_assets(
        &self,
        assets: &[ReleaseAsset],
        backends: &[String],
    ) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
        LlamaCppEngine.pick_release_assets(assets, backends)
    }

    fn supports(&self, format: ModelFormat) -> bool {
        LlamaCppEngine.supports(format)
    }

    fn detect_capabilities(&self, binary: &Path) -> Arc<RuntimeCapabilities> {
        LlamaCppEngine.detect_capabilities(binary)
    }

    fn cached_capabilities(&self, binary: &Path) -> Option<Arc<RuntimeCapabilities>> {
        LlamaCppEngine.cached_capabilities(binary)
    }

    fn probe_capabilities(&self, binary: &Path) -> RuntimeCapabilities {
        LlamaCppEngine.probe_capabilities(binary)
    }

    fn param_catalog(&self) -> &'static ParamCatalog {
        LlamaCppEngine.param_catalog()
    }

    fn as_job(&self) -> Option<&dyn JobEngine> {
        Some(self)
    }
}

impl JobEngine for LlamaBenchEngine {
    fn job_binary(&self, server_binary: &Path) -> Result<PathBuf, String> {
        let file = self.release_source().binary_file();
        let path = server_binary
            .parent()
            .map(|d| d.join(&file))
            .ok_or_else(|| {
                String::from(
                    crate::error_key::err("benchmark.no_location")
                        .with("path", server_binary.display()),
                )
            })?;
        if path.is_file() {
            Ok(path)
        } else {
            Err(crate::error_key::err("benchmark.binary_not_found")
                .with("path", path.display())
                .into())
        }
    }

    fn parse_progress(&self, line: &str) -> Option<JobProgress> {
        let (done, total, phase) = crate::benchmark::parse_progress(line)?;
        Some(JobProgress {
            done: done as u64,
            total: Some(total as u64),
            phase: Some(phase),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_a_job_engine_outside_the_runtime_families() {
        let e = crate::engine::llama_bench();
        assert_eq!(e.family(), LLAMA_BENCH);
        assert_eq!(e.kind(), RuntimeKind::Job);
        assert!(e.as_job().is_some() && e.as_server().is_none());
        // A tool of llama.cpp, not a registrable runtime family.
        assert!(crate::engine::engine(LLAMA_BENCH).is_none());
        assert!(crate::engine::engines()
            .iter()
            .all(|f| f.family() != LLAMA_BENCH));
        assert_eq!(e.release_source().repo, "ggml-org/llama.cpp");
        assert!(e.release_source().binary_file().starts_with("llama-bench"));
        assert!(e.supports(ModelFormat::Gguf) && !e.supports(ModelFormat::GgmlBin));
    }

    #[test]
    fn progress_lines_of_llama_bench() {
        let e = LlamaBenchEngine;
        assert_eq!(
            e.parse_progress("llama-bench: benchmark 3/8: prompt run 1/5"),
            Some(JobProgress {
                done: 3,
                total: Some(8),
                phase: Some("prompt run 1/5".into())
            })
        );
        assert_eq!(e.parse_progress("load_backend: loaded CPU backend"), None);
        assert_eq!(e.parse_progress(r#"{"n_prompt":64,"avg_ts":1.0}"#), None);
    }

    #[test]
    fn job_binary_is_found_next_to_the_server() {
        let dir = tempfile::tempdir().unwrap();
        let server = dir
            .path()
            .join(LlamaCppEngine.release_source().binary_file());
        std::fs::write(&server, b"x").unwrap();
        let err = LlamaBenchEngine.job_binary(&server).unwrap_err();
        assert!(err.contains("benchmark.binary_not_found"), "{err}");
        let bench = dir
            .path()
            .join(LlamaBenchEngine.release_source().binary_file());
        std::fs::write(&bench, b"x").unwrap();
        assert_eq!(LlamaBenchEngine.job_binary(&server).unwrap(), bench);
    }
}
