//! `harness-onnx` as an [`Engine`]: Hugging Face ONNX encoder models (sentence embedders,
//! cross-encoder rerankers) on ONNX Runtime, behind OpenAI-compatible `/v1/embeddings` and
//! `/v1/rerank`.
//!
//! Unlike llama.cpp and whisper.cpp this family has no GitHub release to pick from: the sidecar
//! binary is built with the app (`crates/harness-onnx`, staged next to `harness` by
//! `src-tauri/build.rs`) and the ONNX Runtime library it loads is a managed download
//! ([`crate::onnx_runtime`]). A launch runs `harness-onnx --model <file.onnx> --ort-lib <lib>`;
//! the sidecar reads the rest (tokenizer, pooling, normalization, maximum length, embedder or
//! reranker) from the repo files around the model.
//!
//! The server has no authentication (loopback only, like whisper-server): the gateway is the
//! front door and forwards `/v1/embeddings` and `/v1/rerank` to it unchanged.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use super::{Engine, MetricsSource, ModelFormat, ReleaseSource, RuntimeKind, ServerEngine};
use crate::capabilities::RuntimeCapabilities;
use crate::error_key::err;
use crate::gguf::GgufMetadata;
use crate::params::{ParamCatalog, RenderContext};
use crate::process::{BuildOutput, LaunchConfig, ServerCaps};
use crate::runtime::ReleaseAsset;
use crate::telemetry::Metrics;
use crate::vram::{
    EstimateSource, FitCategory, FitInput, FitResult, Measurement, MemoryBudget, MemoryEstimate,
};

pub use crate::onnx_runtime::ONNX_FAMILY;

/// Harness-level setting that picks the execution provider (`auto`, `cpu`, `directml`,
/// `cuda`); it is rendered as `--provider` and decides which ONNX Runtime build is loaded.
pub const PROVIDER_PARAM: &str = "provider";

/// Flags a custom argument line (or a catalog value) may never set.
const PROTECTED_FLAGS: &[&str] = &[
    "-m",
    "--model",
    "--host",
    "--port",
    "--ort-lib",
    "--genai-lib",
    "--alias",
    "-a",
];

/// The ONNX engine.
#[derive(Debug, Clone, Copy, Default)]
pub struct OnnxEngine;

/// The parsed catalog (`params_onnx.toml`).
pub fn catalog() -> &'static ParamCatalog {
    static CATALOG: OnceLock<ParamCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        toml::from_str(include_str!("../params_onnx.toml"))
            .expect("params_onnx.toml is valid (unit tested)")
    })
}

fn is_protected(arg: &str) -> bool {
    let flag = arg.split('=').next().unwrap_or(arg);
    PROTECTED_FLAGS.contains(&flag)
}

/// The provider a launch asks for: the `provider` setting, else `cpu` for a definition that
/// keeps the model off the GPU (`gpu_layers` 0), else `auto`.
pub fn provider_of(config: &LaunchConfig) -> String {
    let chosen = match config.params.get(PROVIDER_PARAM) {
        Some(crate::params::ParamValue::Text(s)) if !s.trim().is_empty() => {
            s.trim().to_ascii_lowercase()
        }
        _ if config.gpu_layers == 0 => "cpu".to_string(),
        _ => "auto".to_string(),
    };
    // A chat model made for onnxruntime-genai says where it runs (`directml-int4-...` folders
    // are DirectML, `cpu-int4-...` the CPU, `cuda-...` CUDA): "auto" follows its config.
    if chosen == "auto" {
        if let Some(genai) = harness_onnx::genai_pkg::GenaiPackage::discover(&config.model_path) {
            return genai.config_provider().to_string();
        }
    }
    chosen
}

/// Tokens of context a chat model is served with when the launch does not say: what the sidecar
/// defaults to (`harness_onnx::chat_engine::DEFAULT_CONTEXT`).
const DEFAULT_CHAT_CONTEXT: u64 = 4096;

/// Weights and runtime memory of an onnxruntime-genai model (`None` for any other file): the
/// graph with its external data, plus the KV cache of the context and a share of the weights
/// for activations. Estimates, never exact.
fn genai_bytes(path: &Path, context_size: u32) -> Option<(u64, u64)> {
    let genai = harness_onnx::genai_pkg::GenaiPackage::discover(path)?;
    let weights = genai.weight_bytes();
    let mut ctx = if context_size > 0 {
        context_size as u64
    } else {
        DEFAULT_CHAT_CONTEXT
    };
    if genai.context_length > 0 {
        ctx = ctx.min(genai.context_length as u64);
    }
    let kv = genai.kv_bytes_per_token() * ctx;
    Some((weights, kv + (256 << 20).max(weights / 8)))
}

impl Engine for OnnxEngine {
    fn family(&self) -> &'static str {
        ONNX_FAMILY
    }

    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Server
    }

    fn release_source(&self) -> ReleaseSource {
        // Not a GitHub family: `repo` is empty, the binary comes with the app.
        ReleaseSource {
            repo: "",
            binary: "harness-onnx",
        }
    }

    fn release_asset(
        &self,
        _release: &serde_json::Value,
        _query: &crate::hardware::AssetQuery,
    ) -> Option<crate::runtime::ReleaseAssetInfo> {
        None
    }

    fn pick_release_assets(
        &self,
        _assets: &[ReleaseAsset],
        _backends: &[String],
    ) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
        None
    }

    fn supports(&self, format: ModelFormat) -> bool {
        format == ModelFormat::Onnx
    }

    fn detect_capabilities(&self, binary: &Path) -> Arc<RuntimeCapabilities> {
        crate::capabilities::registry()
            .get_or_probe_with(binary, crate::capabilities::whisper::probe_binary)
    }

    fn cached_capabilities(&self, binary: &Path) -> Option<Arc<RuntimeCapabilities>> {
        crate::capabilities::registry().cached(binary)
    }

    fn probe_capabilities(&self, binary: &Path) -> RuntimeCapabilities {
        crate::capabilities::whisper::probe_binary(binary)
    }

    fn param_catalog(&self) -> &'static ParamCatalog {
        catalog()
    }

    fn as_server(&self) -> Option<&dyn ServerEngine> {
        Some(self)
    }
}

impl ServerEngine for OnnxEngine {
    /// The ONNX sidecar reads local models only: no network on Linux.
    fn netns_bridge(&self) -> bool {
        true
    }
    fn build_launch(
        &self,
        config: &LaunchConfig,
        _caps: &ServerCaps,
        runtime: Option<&RuntimeCapabilities>,
    ) -> Result<BuildOutput, String> {
        if !crate::network::is_loopback_host(&config.host) {
            return Err(err("process.onnx_loopback_only")
                .with("host", &config.host)
                .into());
        }
        let mut provider = provider_of(config);
        // An onnxruntime-genai chat model needs the GenAI library next to the ONNX Runtime
        // (Windows builds carry it).
        if harness_onnx::genai_pkg::GenaiPackage::discover(&config.model_path).is_some()
            && !cfg!(windows)
        {
            return Err(err("onnx.genai_unsupported_platform").into());
        }
        let (lib, fallback) = lib_with_fallback(&provider)?;
        if let Some(to) = fallback {
            provider = to.to_string();
        }
        let mut out = build_args(config, &lib, runtime, &provider)?;
        if let Some(to) = fallback {
            out.skipped.push(crate::params::SkippedParam {
                id: PROVIDER_PARAM.into(),
                reason: format!("cuda_runtime_missing:{to}"),
            });
        }
        Ok(out)
    }

    fn health_path(&self) -> &'static str {
        "/health"
    }

    fn health(&self, status: u16, body: &str) -> bool {
        status == 200
            && serde_json::from_str::<serde_json::Value>(body)
                .ok()
                .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(|s| s == "ok"))
                .unwrap_or(false)
    }

    /// The sidecar serves `/v1/embeddings` and `/v1/rerank` itself.
    fn openai_base(&self, host_url: &str) -> String {
        format!("{}/v1", host_url.trim_end_matches('/'))
    }

    /// No `/metrics`, no slots (liveness comes from `/health`).
    fn metrics_source(&self) -> MetricsSource {
        MetricsSource {
            metrics_path: "",
            slots_path: None,
        }
    }

    fn parse_metrics(&self, _text: &str) -> Metrics {
        Metrics::default()
    }

    fn estimate_memory(
        &self,
        input: &FitInput,
        _meta: Option<&GgufMetadata>,
        budget: &MemoryBudget,
        _measured: &[Measurement],
    ) -> FitResult {
        let genai = input
            .model_path
            .as_deref()
            .and_then(|p| genai_bytes(Path::new(p), input.context_size));
        let model = if let Some((weights, _)) = genai {
            weights
        } else if input.model_size_bytes > 0 {
            input.model_size_bytes
        } else {
            input
                .model_path
                .as_deref()
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|m| m.len())
                .unwrap_or(0)
        };
        let gpu = budget.vram_bytes > 0 && input.gpu_layers > 0;
        let compute = genai.map_or_else(|| compute_bytes(model), |(_, c)| c);
        let total = model + compute;
        let estimate = MemoryEstimate {
            vram_bytes: if gpu { total } else { 0 },
            ram_bytes: if gpu { 0 } else { total },
            model_gpu_bytes: if gpu { model } else { 0 },
            model_host_bytes: if gpu { 0 } else { model },
            compute_gpu_bytes: if gpu { compute } else { 0 },
            compute_host_bytes: if gpu { 0 } else { compute },
            slots: 1,
            exact: false,
            ..Default::default()
        };
        let category = match (gpu, total) {
            (true, t) if t * 10 <= budget.vram_bytes * 9 => FitCategory::FullGpu,
            (true, t) if t <= budget.vram_bytes => FitCategory::Tight,
            (false, t) if t * 10 <= budget.ram_bytes * 9 => FitCategory::CpuHeavy,
            _ => FitCategory::OomRisk,
        };
        let pct = |n: u64, d: u64| (n * 100).checked_div(d).unwrap_or(0) as u32;
        FitResult {
            category,
            vram_percentage: pct(estimate.vram_bytes, budget.vram_bytes),
            ram_percentage: pct(estimate.ram_bytes, budget.ram_bytes),
            estimate,
            budget: *budget,
            source: EstimateSource::Estimate,
            measured_at: None,
            recommended_gpu_layers: if gpu { 999 } else { 0 },
            slots: None,
            kv: None,
        }
    }
}

/// The command line of a launch with the ONNX Runtime library at `lib`.
/// The ONNX Runtime library for `provider`. A CUDA launch whose CUDA build is not installed
/// (never installed, or a download that failed) runs on DirectML when that build is there, and
/// says so (the second value); otherwise the error asks for the install.
fn lib_with_fallback(provider: &str) -> Result<(PathBuf, Option<&'static str>), String> {
    match crate::onnx_runtime::lib_for_provider(provider) {
        Ok(lib) => Ok((lib, None)),
        Err(e) if provider == "cuda" && cfg!(windows) => {
            crate::onnx_runtime::lib_for_provider("directml")
                .map(|lib| (lib, Some("directml")))
                .map_err(|_| e)
        }
        Err(e) => Err(e),
    }
}

fn build_args(
    config: &LaunchConfig,
    lib: &Path,
    runtime: Option<&RuntimeCapabilities>,
    provider: &str,
) -> Result<BuildOutput, String> {
    let mut args: Vec<String> = vec![
        "--model".into(),
        config.model_path.to_string_lossy().to_string(),
        "--host".into(),
        config.host.clone(),
        "--port".into(),
        config.port.to_string(),
        "--ort-lib".into(),
        lib.to_string_lossy().to_string(),
        "--provider".into(),
        provider.to_string(),
    ];
    if let Some(alias) = crate::process::served_alias(config) {
        args.push("--alias".into());
        args.push(alias);
    }
    if config.context_size > 0
        && harness_onnx::genai_pkg::GenaiPackage::discover(&config.model_path).is_some()
    {
        args.push("--ctx-size".into());
        args.push(config.context_size.to_string());
    }
    if let Some(threads) = config.threads.filter(|&n| n > 0) {
        args.push("--threads".into());
        args.push(threads.to_string());
    }
    let ctx = RenderContext {
        host: config.host.clone(),
        ..Default::default()
    };
    // CTC packages can be launched without tokenizer.json; explicit task wins.
    if !config.params.contains_key("task")
        && harness_onnx::package::library_task(&config.model_path)
            == Some(harness_onnx::package::Task::AsrCtc)
    {
        args.extend(["--task".into(), "asr_ctc".into()]);
    }
    // `provider` is sent above (it also picks the library).
    let mut values = config.params.clone();
    values.remove(PROVIDER_PARAM);
    let rendered = crate::params::render_params(catalog(), runtime, &values, &ctx)?;
    args.extend(rendered.args);
    if let Some(custom) = config
        .custom_args
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        let custom = crate::process::split_args(custom)?;
        if let Some(bad) = custom.iter().find(|a| is_protected(a)) {
            return Err(err("params.protected_flag").with("flag", bad).into());
        }
        args.extend(custom);
    }
    Ok(BuildOutput {
        args,
        skipped: rendered.skipped,
    })
}

/// Activations and the runtime's arena on top of the weights: a floor for the small encoders
/// (MiniLM, 90 MB, takes about 250 MB in a DirectML session) and a share of the weights for
/// the large ones. An estimate, never claimed exact.
fn compute_bytes(model: u64) -> u64 {
    const MIB: u64 = 1024 * 1024;
    (160 * MIB).max(model / 3)
}

/// Device memory a loaded ONNX model takes when the launch uses a GPU provider (zero for the
/// CPU provider, which keeps everything in system memory).
pub fn vram_need(config: &LaunchConfig) -> u64 {
    if provider_of(config) == "cpu" {
        return 0;
    }
    if let Some((weights, compute)) = genai_bytes(&config.model_path, config.context_size) {
        return weights + compute;
    }
    let size = std::fs::metadata(&config.model_path)
        .map(|m| m.len())
        .unwrap_or(0);
    size + compute_bytes(size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::ParamValue;

    fn config() -> LaunchConfig {
        LaunchConfig {
            model_path: "C:/m/onnx/model.onnx".into(),
            host: "127.0.0.1".into(),
            port: 11450,
            gpu_layers: 999,
            ..Default::default()
        }
    }

    #[test]
    fn catalog_parses_and_flags_are_not_protected() {
        let c = catalog();
        assert!(c.get("provider").is_some() && c.get("pooling").is_some());
        for p in &c.params {
            for f in &p.flags {
                assert!(!is_protected(f), "{f} is set by Harness itself");
            }
        }
    }

    #[test]
    fn onnx_ctc_launch_detects_audio_without_a_text_tokenizer() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::create_dir(root.join("onnx")).unwrap();
        let model = root.join("onnx/model.onnx");
        std::fs::write(&model, b"fixture").unwrap();
        for (name, value) in [
            (
                "config.json",
                r#"{"architectures":["Wav2Vec2ForCTC"],"vocab_size":2,"pad_token_id":1}"#,
            ),
            (
                "preprocessor_config.json",
                r#"{"feature_size":1,"sampling_rate":16000}"#,
            ),
            ("vocab.json", r#"{"a":0,"<pad>":1}"#),
        ] {
            std::fs::write(root.join(name), value).unwrap();
        }
        let mut c = config();
        c.model_path = model;
        let caps = crate::capabilities::whisper::from_help(
            crate::capabilities::BinaryKey::default(),
            harness_onnx::cli::HELP,
        );
        let launch = build_args(
            &c,
            Path::new("C:/rt/onnxruntime.dll"),
            Some(&caps),
            &provider_of(&c),
        )
        .unwrap();
        assert!(launch.args.join(" ").contains("--task asr_ctc"));
        assert_eq!(launch.args.iter().filter(|a| *a == "--task").count(), 1);
        c.params
            .insert("task".into(), ParamValue::Text("asr_ctc".into()));
        let launch = build_args(
            &c,
            Path::new("C:/rt/onnxruntime.dll"),
            Some(&caps),
            &provider_of(&c),
        )
        .unwrap();
        assert!(launch.args.join(" ").contains("--task asr_ctc"));
        assert_eq!(launch.args.iter().filter(|a| *a == "--task").count(), 1);
    }

    /// The sidecar's own `--help` is what Harness parses: every catalog flag is in it, the
    /// flags Harness sets itself too, and rendering goes through the parsed table.
    #[test]
    fn catalog_flags_exist_in_the_sidecar_help() {
        let caps = crate::capabilities::whisper::from_help(
            crate::capabilities::BinaryKey::default(),
            harness_onnx::cli::HELP,
        );
        assert!(caps.help_parsed);
        for p in &catalog().params {
            for f in &p.flags {
                assert!(
                    caps.has_flag(f),
                    "{f} ({}) is not in harness-onnx --help",
                    p.id
                );
            }
        }
        for f in [
            "--model",
            "--host",
            "--port",
            "--alias",
            "--ort-lib",
            "--provider",
            "--ctx-size",
            "--genai-lib",
        ] {
            assert!(caps.has_flag(f), "{f}");
        }
    }

    #[test]
    fn launch_arguments() {
        let caps = crate::capabilities::whisper::from_help(
            crate::capabilities::BinaryKey::default(),
            harness_onnx::cli::HELP,
        );
        let mut c = config();
        c.threads = Some(6);
        c.alias = Some("minilm".into());
        c.params
            .insert(PROVIDER_PARAM.into(), ParamValue::Text("cpu".into()));
        c.params
            .insert("pooling".into(), ParamValue::Text("cls".into()));
        c.params.insert("batch_size".into(), ParamValue::Int(8));
        let lib = Path::new("C:/rt/onnxruntime.dll");
        let out = build_args(&c, lib, Some(&caps), &provider_of(&c)).unwrap();
        let line = out.args.join(" ");
        assert!(
            line.starts_with("--model C:/m/onnx/model.onnx --host 127.0.0.1 --port 11450"),
            "{line}"
        );
        assert!(
            line.contains("--ort-lib C:/rt/onnxruntime.dll --provider cpu"),
            "{line}"
        );
        assert!(
            line.contains("--alias minilm") && line.contains("--threads 6"),
            "{line}"
        );
        assert!(
            line.contains("--pooling cls") && line.contains("--batch-size 8"),
            "{line}"
        );
        // The provider is sent once.
        assert_eq!(out.args.iter().filter(|a| *a == "--provider").count(), 1);
        assert!(out.skipped.is_empty(), "{:?}", out.skipped);

        // A flag Harness owns cannot be smuggled in through the custom arguments.
        c.custom_args = Some("--ort-lib /tmp/evil.dll".into());
        assert!(build_args(&c, lib, Some(&caps), &provider_of(&c))
            .unwrap_err()
            .starts_with("params.protected_flag"));
        c.custom_args = Some("--max-length 128".into());
        let ok = build_args(&c, lib, Some(&caps), &provider_of(&c)).unwrap();
        assert!(ok.args.join(" ").ends_with("--max-length 128"));
    }

    #[test]
    fn provider_follows_the_setting_then_gpu_layers() {
        let mut c = config();
        assert_eq!(provider_of(&c), "auto");
        c.gpu_layers = 0;
        assert_eq!(provider_of(&c), "cpu");
        c.params
            .insert(PROVIDER_PARAM.into(), ParamValue::Text("DirectML".into()));
        assert_eq!(provider_of(&c), "directml");
    }

    /// A genai folder on disk: `<root>/<group>/<variant>/model.onnx` and its config.
    fn genai_model(root: &Path, group: &str, variant: &str, provider: &str) -> std::path::PathBuf {
        let dir = root
            .join("Phi-3-mini-4k-instruct-onnx")
            .join(group)
            .join(variant);
        std::fs::create_dir_all(&dir).unwrap();
        let options = if provider.is_empty() {
            "[]".to_string()
        } else {
            format!("[{{\"{provider}\": {{}}}}]")
        };
        std::fs::write(
            dir.join("genai_config.json"),
            format!(
                r#"{{"model": {{"type": "phi3", "context_length": 4096, "decoder": {{
                "filename": "model.onnx", "num_hidden_layers": 32, "num_key_value_heads": 32,
                "head_size": 96, "session_options": {{"provider_options": {options}}}}}}}}}"#
            ),
        )
        .unwrap();
        std::fs::write(dir.join("model.onnx"), vec![0u8; 1000]).unwrap();
        std::fs::write(dir.join("model.onnx.data"), vec![0u8; 3_000_000]).unwrap();
        dir.join("model.onnx")
    }

    #[test]
    fn a_genai_model_follows_its_own_target_and_counts_its_kv_cache() {
        let d = tempfile::tempdir().unwrap();
        let dml = genai_model(d.path(), "directml", "directml-int4", "dml");
        let cpu = genai_model(d.path(), "cpu_and_mobile", "cpu-int4", "");
        let mut c = config();
        c.model_path = dml.clone();
        assert_eq!(provider_of(&c), "directml");
        c.model_path = cpu.clone();
        assert_eq!(provider_of(&c), "cpu");
        // The user's choice wins; no GPU layers means the CPU.
        c.model_path = dml.clone();
        c.params
            .insert(PROVIDER_PARAM.into(), ParamValue::Text("cpu".into()));
        assert_eq!(provider_of(&c), "cpu");
        c.params.clear();
        c.gpu_layers = 0;
        assert_eq!(provider_of(&c), "cpu");

        // Weights (graph + external data) and the KV cache of the context on the GPU.
        let mut c = config();
        c.model_path = dml;
        c.context_size = 2048;
        let weights = 1000 + 3_000_000u64;
        let kv = 2 * 2 * 32 * 32 * 96 * 2048u64;
        let need = vram_need(&c);
        assert!(need >= weights + kv, "{need}");
        assert!(need < weights + kv + (512 << 20), "{need}");
        // The context cannot exceed the model's own.
        c.context_size = 1 << 20;
        assert!(vram_need(&c) < weights + 2 * 2 * 32 * 32 * 96 * 4096 + (512 << 20));
        // On the CPU provider nothing is on the GPU.
        c.params
            .insert(PROVIDER_PARAM.into(), ParamValue::Text("cpu".into()));
        assert_eq!(vram_need(&c), 0);
    }

    #[test]
    fn a_genai_launch_passes_the_context_size() {
        let d = tempfile::tempdir().unwrap();
        let mut c = config();
        c.model_path = genai_model(d.path(), "directml", "directml-int4", "dml");
        let lib = Path::new("C:/rt/onnxruntime.dll");
        let line = |c: &LaunchConfig| {
            build_args(c, lib, None, &provider_of(c))
                .unwrap()
                .args
                .join(" ")
        };
        assert!(!line(&c).contains("--ctx-size"));
        c.context_size = 8192;
        assert!(line(&c).contains("--ctx-size 8192"), "{}", line(&c));
        assert!(line(&c).contains("--provider directml"), "{}", line(&c));
        // An encoder never gets it.
        c.model_path = "C:/m/onnx/model.onnx".into();
        assert!(!line(&c).contains("--ctx-size"));
    }

    #[test]
    fn protected_flags_are_caught_in_both_spellings() {
        assert!(is_protected("--ort-lib"));
        assert!(is_protected("--port=1234"));
        assert!(is_protected("-m"));
        assert!(!is_protected("--pooling"));
    }

    #[test]
    fn health_needs_status_ok() {
        let e = OnnxEngine;
        assert!(e.health(200, r#"{"status":"ok"}"#));
        assert!(!e.health(503, r#"{"status":"loading"}"#));
        assert!(!e.health(200, r#"{"status":"loading"}"#));
        assert_eq!(
            e.openai_base("http://127.0.0.1:1/"),
            "http://127.0.0.1:1/v1"
        );
    }

    #[test]
    fn a_remote_bind_is_refused() {
        let mut c = config();
        c.host = "0.0.0.0".into();
        let e = OnnxEngine.build_launch(&c, &ServerCaps::default(), None);
        assert!(e.unwrap_err().starts_with("process.onnx_loopback_only"));
    }

    #[test]
    fn memory_estimate_has_a_floor_and_cpu_costs_no_vram() {
        let budget = MemoryBudget {
            vram_bytes: 8 << 30,
            ram_bytes: 16 << 30,
            ..Default::default()
        };
        let input = FitInput {
            model_size_bytes: 90 << 20,
            gpu_layers: 999,
            ..Default::default()
        };
        let gpu = OnnxEngine.estimate_memory(&input, None, &budget, &[]);
        assert_eq!(gpu.category, FitCategory::FullGpu);
        assert!(gpu.estimate.vram_bytes >= (90 + 160) << 20);
        let cpu = OnnxEngine.estimate_memory(
            &FitInput {
                gpu_layers: 0,
                ..input
            },
            None,
            &budget,
            &[],
        );
        assert_eq!(cpu.estimate.vram_bytes, 0);
        assert_eq!(cpu.category, FitCategory::CpuHeavy);
    }
}
