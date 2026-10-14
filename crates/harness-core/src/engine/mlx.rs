//! The MLX engines as [`Engine`]s (Apple Silicon, macOS 14 or later), two flavours of one
//! [`MlxEngine`]:
//!
//! * `mlx`: a text-generation model folder in MLX's format (`mlx-community/...`), served by
//!   `python/mlx_worker.py` around mlx-lm's OpenAI-compatible server from the managed `mlx`
//!   environment;
//! * `mlx-vlm`: a vision-language folder (its config has a `vision_config`), served by
//!   `python/mlx_vlm_worker.py` around mlx-vlm's server from the managed `mlx-vlm` environment.
//!
//! A launch runs
//!
//! ```text
//! <env python> <env>/<worker>.py --model <dir> --port <port> [--alias <alias>] <catalog>
//! ```
//!
//! The workers listen on 127.0.0.1 only, ask every request but `/health` for the per-run API
//! key (`HARNESS_WORKER_API_KEY`, from the environment), serve only the folder they were started
//! with and never run remote code; the vision worker takes images only inline (`data:` URLs).
//! macOS has no network isolation for them (the Linux Python engine's `unshare` has no
//! counterpart there).
//!
//! An MLX folder is a transformers generator (`config.json` + safetensors) that MLX wrote: its
//! config has MLX's `quantization` (`bits`, `group_size`), or its card says
//! `library_name: mlx`. mistral.rs can't read those weights, so it leaves them to these engines.

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

/// Family id of the MLX engine.
pub const MLX: &str = "mlx";

/// Family id of the MLX vision engine (mlx-vlm).
pub const MLX_VLM: &str = "mlx-vlm";

/// The managed environment the text worker runs in (`crate::pyenv::envs::MLX` on Apple Silicon).
pub const ENV: &str = "mlx";

/// The managed environment the vision worker runs in (`crate::pyenv::envs::MLX_VLM`).
pub const VLM_ENV: &str = "mlx-vlm";

/// The worker scripts, written next to their environments ([`worker_script`]).
const WORKER: &str = include_str!("../../python/mlx_worker.py");
const VLM_WORKER: &str = include_str!("../../python/mlx_vlm_worker.py");

/// Flags a custom argument line may never set (Harness owns the model, bind and code policy).
const PROTECTED_FLAGS: &[&str] = &[
    "--model",
    "--host",
    "--port",
    "--alias",
    "--trust-remote-code",
    "--adapter-path",
    "--draft-model",
    "--allowed-origins",
];

/// What a launch without a `max_tokens` parameter gets: mlx-lm's own default (512) cuts
/// ordinary chat answers short.
const DEFAULT_MAX_TOKENS: u32 = 8192;

/// An MLX engine: the text one (`mlx`) or the vision one (`mlx-vlm`).
#[derive(Debug, Clone, Copy, Default)]
pub struct MlxEngine {
    vision: bool,
}

impl MlxEngine {
    /// `mlx` (mlx-lm).
    pub const TEXT: MlxEngine = MlxEngine { vision: false };
    /// `mlx-vlm`.
    pub const VISION: MlxEngine = MlxEngine { vision: true };

    fn env(&self) -> &'static str {
        if self.vision {
            VLM_ENV
        } else {
            ENV
        }
    }

    /// Whether `path` is a folder this flavour runs.
    pub fn runs(&self, path: &Path) -> bool {
        if self.vision {
            is_vlm_dir(path)
        } else {
            is_model_dir(path)
        }
    }
}

/// The parsed catalog (`params_mlx.toml`, or `params_mlx_vlm.toml` for the vision engine).
pub fn catalog(vision: bool) -> &'static ParamCatalog {
    static TEXT: OnceLock<ParamCatalog> = OnceLock::new();
    static VISION: OnceLock<ParamCatalog> = OnceLock::new();
    if vision {
        VISION.get_or_init(|| {
            toml::from_str(include_str!("../params_mlx_vlm.toml"))
                .expect("params_mlx_vlm.toml is valid (unit tested)")
        })
    } else {
        TEXT.get_or_init(|| {
            toml::from_str(include_str!("../params_mlx.toml"))
                .expect("params_mlx.toml is valid (unit tested)")
        })
    }
}

/// Capabilities as documented: every flag of the catalog. The worker has no `--help` to probe
/// (its flags are the catalog's own), and without capabilities nothing would be rendered.
fn documented_caps(vision: bool) -> &'static RuntimeCapabilities {
    static TEXT: OnceLock<RuntimeCapabilities> = OnceLock::new();
    static VISION: OnceLock<RuntimeCapabilities> = OnceLock::new();
    let cell = if vision { &VISION } else { &TEXT };
    cell.get_or_init(|| {
        let help: String = catalog(vision)
            .params
            .iter()
            .map(|p| format!("{} N    {}\n", p.flags.join(", "), p.id))
            .collect();
        RuntimeCapabilities::from_outputs(Default::default(), "", &help, "")
    })
}

/// A flag Harness owns, or an abbreviation of one (Python's argparse expands `--trust` to
/// `--trust-remote-code`; the worker refuses those too).
fn is_protected(arg: &str) -> bool {
    let flag = arg.split('=').next().unwrap_or(arg);
    flag.len() > 2 && flag.starts_with("--") && PROTECTED_FLAGS.iter().any(|p| p.starts_with(flag))
}

// ---- model folders -------------------------------------------------------------------------

fn read_json(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// A field of the model card's front matter (`README.md`).
fn card_field(dir: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("README.md")).ok()?;
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let prefix = format!("{key}:");
    for line in lines {
        let line = line.trim_end();
        if line.trim() == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix(&prefix) {
            let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
            return Some(value.to_string()).filter(|v| !v.is_empty());
        }
    }
    None
}

/// Bits per weight of MLX's quantization (`"quantization": {"group_size": 64, "bits": 4}`).
pub fn quant_bits(cfg: &serde_json::Value) -> Option<u64> {
    let q = cfg.get("quantization")?;
    q.get("group_size")?;
    q.get("bits")?.as_u64()
}

/// Whether a folder's weights are MLX's: its config has MLX's quantization or its card says
/// `library_name: mlx`. Platform independent: elsewhere such a folder is simply not run.
pub fn is_mlx_checkpoint(dir: &Path) -> bool {
    read_json(&dir.join("config.json")).is_some_and(|c| quant_bits(&c).is_some())
        || card_field(dir, "library_name").is_some_and(|l| l.eq_ignore_ascii_case("mlx"))
}

/// Whether an MLX folder has a vision tower, `None` when `path` is not an MLX folder.
fn mlx_vision(path: &Path) -> Option<bool> {
    (super::mistralrs::is_checkpoint_dir(path) && is_mlx_checkpoint(path)).then(|| {
        read_json(&path.join("config.json")).is_some_and(|c| c.get("vision_config").is_some())
    })
}

/// Whether `path` is a folder the `mlx` engine runs: an MLX text generator (not a vision model).
pub fn is_model_dir(path: &Path) -> bool {
    mlx_vision(path) == Some(false)
}

/// Whether `path` is a folder the `mlx-vlm` engine runs: an MLX vision-language model.
pub fn is_vlm_dir(path: &Path) -> bool {
    mlx_vision(path) == Some(true)
}

/// Whether `path` is an MLX folder of either engine.
pub fn is_any_model_dir(path: &Path) -> bool {
    mlx_vision(path).is_some()
}

/// What the files of an MLX folder say (chat template, vision tower...); `None` for other paths.
pub fn inspect(dir: &Path) -> Option<super::mistralrs::ModelDirInfo> {
    if !is_any_model_dir(dir) {
        return None;
    }
    super::mistralrs::inspect_checkpoint(dir)
}

/// The library's view of an MLX folder: a chat model like mistral.rs's, with MLX's bits as its
/// quantization (`4bit`).
fn to_local_model(dir: &Path, root: &Path) -> Option<crate::models::LocalModel> {
    let info = super::mistralrs::inspect_checkpoint(dir)?;
    let mut model = super::mistralrs::to_local_model(dir, &info, root);
    if let Some(bits) = read_json(&dir.join("config.json")).and_then(|c| quant_bits(&c)) {
        model.quantization = format!("{bits}bit");
    }
    Some(model)
}

/// MLX model folders under `roots` (a few levels deep: `<root>/<owner>/<repo>`). Only where
/// MLX runs: elsewhere they are not listed (mistral.rs can't run them either).
pub fn library_models(roots: &[PathBuf]) -> Vec<crate::models::LocalModel> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Vec::new();
    }
    roots
        .iter()
        .flat_map(|root| {
            crate::tts::scan_folders(
                std::slice::from_ref(root),
                |dir| is_any_model_dir(dir).then(|| dir.to_path_buf()),
                |a, b| a == b,
            )
            .into_iter()
            .filter_map(|dir| to_local_model(&dir, root))
            .collect::<Vec<_>>()
        })
        .collect()
}

// ---- the environment -----------------------------------------------------------------------

/// The worker script of this version, written next to its environment (rewritten when it
/// differs, so an app update brings its worker along).
pub fn worker_script(vision: bool) -> Result<PathBuf, String> {
    let (env, name, body) = if vision {
        (VLM_ENV, "mlx_vlm_worker.py", VLM_WORKER)
    } else {
        (ENV, "mlx_worker.py", WORKER)
    };
    let dir = crate::pyenv::envs::env_dir_in(&crate::pyenv::envs::root(), env);
    let path = dir.join(name);
    if std::fs::read_to_string(&path).ok().as_deref() == Some(body) {
        return Ok(path);
    }
    std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(&path, body))
        .map_err(|e| {
            String::from(
                err("fs.write_failed")
                    .with("path", path.display())
                    .with("error", e),
            )
        })?;
    Ok(path)
}

/// The interpreter a text launch runs: the `mlx` environment's, when it is installed for this
/// version; else `runtime.family_not_installed: mlx` (the UI offers the install), or why this
/// Mac can't have it.
pub fn binary() -> Result<PathBuf, String> {
    MlxEngine::TEXT.binary()
}

/// [`binary`] of the vision engine (the `mlx-vlm` environment).
pub fn vlm_binary() -> Result<PathBuf, String> {
    MlxEngine::VISION.binary()
}

impl MlxEngine {
    /// The interpreter of this flavour's environment (see [`binary`]).
    pub fn binary(&self) -> Result<PathBuf, String> {
        let root = crate::pyenv::envs::root();
        let Some(spec) = crate::pyenv::envs::spec(self.env()) else {
            return Err(err("mlx.unsupported_platform").into());
        };
        if let Some(refusal) = crate::pyenv::envs::platform_refusal(spec) {
            return Err(refusal);
        }
        if !crate::pyenv::envs::status_in(&root, spec).ready {
            return Err(format!(
                "{}: {}",
                crate::runtimes::ERR_FAMILY_NOT_INSTALLED,
                self.family()
            ));
        }
        Ok(crate::pyenv::envs::python_in(&root, self.env()))
    }
}

/// Unified memory a launch takes: the weights and a margin for the KV cache and MLX itself.
pub fn vram_need(config: &LaunchConfig) -> u64 {
    let weights: u64 = super::mistralrs::weight_files(&config.model_path)
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    weights + weights / 5 + (300 << 20)
}

impl Engine for MlxEngine {
    fn family(&self) -> &'static str {
        if self.vision {
            MLX_VLM
        } else {
            MLX
        }
    }

    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Server
    }

    fn release_source(&self) -> ReleaseSource {
        // Not a GitHub family: the interpreter comes from a managed environment.
        ReleaseSource {
            repo: "",
            binary: "python",
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

    /// Folders are told apart by [`is_model_dir`] / [`is_vlm_dir`], not by a file format.
    fn supports(&self, _format: ModelFormat) -> bool {
        false
    }

    fn detect_capabilities(&self, binary: &Path) -> Arc<RuntimeCapabilities> {
        Arc::new(self.probe_capabilities(binary))
    }

    fn cached_capabilities(&self, binary: &Path) -> Option<Arc<RuntimeCapabilities>> {
        Some(self.detect_capabilities(binary))
    }

    /// Nothing to probe: the catalog is the worker's own.
    fn probe_capabilities(&self, _binary: &Path) -> RuntimeCapabilities {
        RuntimeCapabilities::default()
    }

    fn param_catalog(&self) -> &'static ParamCatalog {
        catalog(self.vision)
    }

    fn as_server(&self) -> Option<&dyn ServerEngine> {
        Some(self)
    }
}

impl ServerEngine for MlxEngine {
    fn build_launch(
        &self,
        config: &LaunchConfig,
        _caps: &ServerCaps,
        _runtime: Option<&RuntimeCapabilities>,
    ) -> Result<BuildOutput, String> {
        if !crate::network::is_loopback_host(&config.host) {
            return Err(err("process.mlx_loopback_only")
                .with("host", &config.host)
                .into());
        }
        if !self.runs(&config.model_path) {
            return Err(err("mlx.model_unsupported")
                .with("path", config.model_path.display())
                .into());
        }
        build_args(config, &worker_script(self.vision)?, self.vision)
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

    /// A Python worker: [`crate::pyenv::python_env`] and the per-run key. It listens on its
    /// loopback port itself (no socket: macOS has no `unshare`).
    fn worker_env(&self, api_key: &str) -> Option<Vec<(&'static str, std::ffi::OsString)>> {
        Some(super::python_hf::worker_env(api_key))
    }

    /// The weights plus MLX's share (KV cache, activations, the interpreter), all in unified
    /// memory: measured on an M1 with Qwen3-4B 4-bit (2.1 GB of weights, 2.7 GB at peak).
    fn estimate_memory(
        &self,
        input: &FitInput,
        _meta: Option<&GgufMetadata>,
        budget: &MemoryBudget,
        _measured: &[Measurement],
    ) -> FitResult {
        let model = if input.model_size_bytes > 0 {
            input.model_size_bytes
        } else {
            input
                .model_path
                .as_deref()
                .map(|p| {
                    super::mistralrs::weight_files(Path::new(p))
                        .iter()
                        .filter_map(|f| std::fs::metadata(f).ok())
                        .map(|m| m.len())
                        .sum()
                })
                .unwrap_or(0)
        };
        let compute = (300u64 << 20) + model / 5;
        let total = model + compute;
        let estimate = MemoryEstimate {
            ram_bytes: total,
            model_host_bytes: model,
            compute_host_bytes: compute,
            slots: 1,
            exact: false,
            ..Default::default()
        };
        let category = if total * 10 <= budget.ram_bytes * 9 {
            FitCategory::CpuHeavy
        } else {
            FitCategory::OomRisk
        };
        FitResult {
            category,
            vram_percentage: 0,
            ram_percentage: (total * 100).checked_div(budget.ram_bytes).unwrap_or(0) as u32,
            estimate,
            budget: *budget,
            source: EstimateSource::Estimate,
            measured_at: None,
            recommended_gpu_layers: 0,
            slots: None,
            kv: None,
        }
    }
}

/// The command line of a launch: the worker script first (the program is the interpreter).
fn build_args(config: &LaunchConfig, script: &Path, vision: bool) -> Result<BuildOutput, String> {
    let mut args: Vec<String> = vec![
        script.to_string_lossy().to_string(),
        "--model".into(),
        config.model_path.to_string_lossy().to_string(),
        "--port".into(),
        config.port.to_string(),
    ];
    if let Some(alias) = crate::process::served_alias(config) {
        args.push("--alias".into());
        args.push(alias);
    }
    let ctx = RenderContext {
        host: config.host.clone(),
        ..Default::default()
    };
    let rendered = crate::params::render_params(
        catalog(vision),
        Some(documented_caps(vision)),
        &config.params,
        &ctx,
    )?;
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
    if !args
        .iter()
        .any(|a| a.split('=').next() == Some("--max-tokens"))
    {
        args.push("--max-tokens".into());
        args.push(DEFAULT_MAX_TOKENS.to_string());
    }
    Ok(BuildOutput {
        args,
        skipped: rendered.skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, body) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        dir
    }

    /// The files of `mlx-community/Qwen3-4B-Instruct-2507-4bit` (config trimmed).
    const MLX_CONFIG: &str = r#"{"architectures":["Qwen3ForCausalLM"],"model_type":"qwen3","max_position_embeddings":262144,"quantization":{"group_size":64,"bits":4,"mode":"affine"},"quantization_config":{"group_size":64,"bits":4,"mode":"affine"}}"#;

    /// `mlx-community/Qwen3-VL-2B-Instruct-4bit` (config trimmed).
    const VLM_CONFIG: &str = r#"{"architectures":["Qwen3VLForConditionalGeneration"],"model_type":"qwen3_vl","vision_config":{"depth":24},"text_config":{"num_hidden_layers":28},"quantization":{"group_size":64,"bits":4,"mode":"affine"}}"#;

    #[test]
    fn catalog_parses() {
        assert!(!catalog(false).params.is_empty());
        assert!(!catalog(true).params.is_empty());
    }

    #[test]
    fn mlx_folders_are_ours_and_not_mistral_rs() {
        let quantized = folder(&[
            ("config.json", MLX_CONFIG),
            ("model.safetensors", "x"),
            ("tokenizer.json", "{}"),
        ]);
        assert!(is_model_dir(quantized.path()));
        assert!(!super::super::mistralrs::is_model_dir(quantized.path()));
        let m = to_local_model(quantized.path(), quantized.path().parent().unwrap()).unwrap();
        assert_eq!(m.quantization, "4bit");
        assert_eq!(m.kind, crate::domain::ModelKind::Llm);
        assert_eq!(m.architecture.as_deref(), Some("qwen3"));

        // A bf16 conversion has no quantization; its card says MLX.
        let card = folder(&[
            (
                "config.json",
                r#"{"architectures":["LlamaForCausalLM"],"model_type":"llama"}"#,
            ),
            ("model.safetensors", "x"),
            (
                "README.md",
                "---\nlibrary_name: mlx\nlicense: apache-2.0\ntags:\n- mlx\n---\n# x\n",
            ),
        ]);
        assert!(is_model_dir(card.path()));
        assert!(!super::super::mistralrs::is_model_dir(card.path()));

        // A plain transformers checkpoint stays with mistral.rs.
        let plain = folder(&[
            (
                "config.json",
                r#"{"architectures":["LlamaForCausalLM"],"model_type":"llama","quantization_config":{"quant_method":"awq","bits":4}}"#,
            ),
            ("model.safetensors", "x"),
        ]);
        assert!(!is_model_dir(plain.path()));
        assert!(super::super::mistralrs::is_model_dir(plain.path()));

        // An MLX vision model is mlx-vlm's: not the text engine's, not mistral.rs's.
        let vlm = folder(&[
            ("config.json", VLM_CONFIG),
            ("model.safetensors", "x"),
            (
                "README.md",
                "---\nlibrary_name: transformers\npipeline_tag: image-text-to-text\ntags:\n- mlx\n---\n",
            ),
        ]);
        assert!(!is_model_dir(vlm.path()));
        assert!(is_vlm_dir(vlm.path()));
        assert!(MlxEngine::VISION.runs(vlm.path()));
        assert!(!MlxEngine::TEXT.runs(vlm.path()));
        assert!(!super::super::mistralrs::is_model_dir(vlm.path()));
        assert!(!super::super::python_hf::is_model_dir(vlm.path()));
        assert!(inspect(vlm.path()).unwrap().vision);
        let m = to_local_model(vlm.path(), vlm.path().parent().unwrap()).unwrap();
        assert_eq!(m.kind, crate::domain::ModelKind::Llm);
        assert_eq!(m.quantization, "4bit");
        // ...and the text folder is not a vision one.
        assert!(!is_vlm_dir(quantized.path()));
        assert!(!inspect(quantized.path()).unwrap().vision);
        assert!(inspect(plain.path()).is_none());
    }

    #[test]
    fn launch_args_keep_harness_flags_and_a_long_answer_limit() {
        let dir = folder(&[("config.json", MLX_CONFIG), ("model.safetensors", "x")]);
        let config = LaunchConfig {
            model_path: dir.path().to_path_buf(),
            host: "127.0.0.1".into(),
            port: 11450,
            ..Default::default()
        };
        let out = build_args(&config, Path::new("/env/mlx_worker.py"), false).unwrap();
        assert_eq!(out.args[0], "/env/mlx_worker.py");
        let at = |flag: &str| {
            out.args
                .iter()
                .position(|a| a == flag)
                .map(|i| out.args[i + 1].as_str())
        };
        assert_eq!(at("--port"), Some("11450"));
        assert_eq!(at("--max-tokens"), Some("8192"));
        // The worker binds 127.0.0.1 itself; no host on the command line.
        assert!(at("--host").is_none());

        // The catalog is rendered (the worker has no --help to probe).
        let mut set = config.clone();
        set.params
            .insert("max_tokens".into(), crate::params::ParamValue::Int(512));
        set.params.insert(
            "prompt_cache_size".into(),
            crate::params::ParamValue::Int(4),
        );
        let out = build_args(&set, Path::new("w.py"), false).unwrap();
        assert!(out.skipped.is_empty(), "{:?}", out.skipped);
        let args = out.args.join(" ");
        assert!(args.contains("--max-tokens 512"), "{args}");
        assert!(args.contains("--prompt-cache-size 4"), "{args}");
        assert_eq!(args.matches("--max-tokens").count(), 1, "{args}");

        let mut custom = config.clone();
        custom.custom_args = Some("--max-tokens 100".into());
        let out = build_args(&custom, Path::new("w.py"), false).unwrap();
        assert_eq!(
            out.args.iter().filter(|a| *a == "--max-tokens").count(),
            1,
            "{:?}",
            out.args
        );
        for bad in [
            "--trust-remote-code",
            "--trust",
            "--trust-remote",
            "--host 0.0.0.0",
            "--ho 0.0.0.0",
            "--adapter-path /x",
            "--adapter /x",
            "--model=/etc",
            "--mod /etc",
            "--allowed *",
        ] {
            let mut c = config.clone();
            c.custom_args = Some(bad.into());
            let e = build_args(&c, Path::new("w.py"), false).unwrap_err();
            assert!(e.contains("params.protected_flag"), "{bad}: {e}");
            let e = build_args(&c, Path::new("w.py"), true).unwrap_err();
            assert!(e.contains("params.protected_flag"), "{bad}: {e}");
        }
    }

    #[test]
    fn vision_launch_args() {
        let dir = folder(&[("config.json", VLM_CONFIG), ("model.safetensors", "x")]);
        let mut config = LaunchConfig {
            model_path: dir.path().to_path_buf(),
            host: "127.0.0.1".into(),
            port: 11451,
            ..Default::default()
        };
        config.params.insert(
            "kv_bits".into(),
            crate::params::ParamValue::Text("8".into()),
        );
        let out = build_args(&config, Path::new("/env/mlx_vlm_worker.py"), true).unwrap();
        assert_eq!(out.args[0], "/env/mlx_vlm_worker.py");
        let at = |flag: &str| {
            out.args
                .iter()
                .position(|a| a == flag)
                .map(|i| out.args[i + 1].as_str())
        };
        assert_eq!(at("--max-tokens"), Some("8192"));
        assert_eq!(at("--kv-bits"), Some("8"));
        assert_eq!(MlxEngine::VISION.family(), MLX_VLM);
        assert_eq!(MlxEngine::TEXT.family(), MLX);
        // A text folder is not the vision engine's to launch, nor the reverse.
        let e = MlxEngine::TEXT
            .build_launch(&config, &ServerCaps::default(), None)
            .unwrap_err();
        assert!(e.contains("mlx.model_unsupported"), "{e}");
    }

    #[test]
    fn launches_are_loopback_only() {
        let dir = folder(&[("config.json", MLX_CONFIG), ("model.safetensors", "x")]);
        let config = LaunchConfig {
            model_path: dir.path().to_path_buf(),
            host: "0.0.0.0".into(),
            port: 11450,
            ..Default::default()
        };
        for engine in [MlxEngine::TEXT, MlxEngine::VISION] {
            let e = engine
                .build_launch(&config, &ServerCaps::default(), None)
                .unwrap_err();
            assert!(e.contains("process.mlx_loopback_only"), "{e}");
        }
    }

    /// The worker's own argument filter, run with a system Python when there is one (it needs
    /// neither MLX nor the environment): abbreviations of Harness's flags and repeats exit.
    #[test]
    fn the_worker_refuses_abbreviations_and_repeats() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("mlx_worker.py");
        std::fs::write(&script, WORKER).unwrap();
        let python = if cfg!(windows) { "python" } else { "python3" };
        let check = |args: &[&str]| -> Option<(bool, String)> {
            let code = format!(
                "import sys; sys.path.insert(0, {:?}); import mlx_worker as w; print(w._split_args({:?}))",
                dir.path().to_string_lossy(),
                args
            );
            let out = std::process::Command::new(python)
                .args(["-I", "-c", &code])
                .env("HARNESS_WORKER_API_KEY", "k")
                .output()
                .ok()?;
            let text = String::from_utf8_lossy(&out.stdout).to_string()
                + &String::from_utf8_lossy(&out.stderr);
            Some((out.status.success(), text))
        };
        let Some((ok, text)) = check(&["--model", "/m", "--port", "1", "--max-tokens", "9"]) else {
            eprintln!("no {python}, skipping");
            return;
        };
        assert!(ok, "{text}");
        assert!(text.contains("'--host', '127.0.0.1'"), "{text}");
        for bad in [
            &["--trust"][..],
            &["--trust-remote"],
            &["--trust-remote-code"],
            &["--ho", "0.0.0.0"],
            &["--allowed-origins", "*"],
            &["--adapter", "/x"],
            &["--draft", "/x"],
            &["--model", "/m", "--mod", "/etc"],
            &["--model", "/m", "--model", "/etc"],
            &["--port", "1", "--port=2"],
        ] {
            let (ok, text) = check(bad).unwrap();
            assert!(!ok, "{bad:?} was accepted: {text}");
        }
    }

    #[test]
    fn the_worker_guards_what_harness_relies_on() {
        // The key comes from the environment and is compared in constant time; the bind,
        // remote code and the served folder are fixed.
        assert!(WORKER.contains("os.environ.pop(\"HARNESS_WORKER_API_KEY\""));
        assert!(WORKER.contains("hmac.compare_digest"));
        assert!(WORKER.contains("HOST = \"127.0.0.1\""));
        assert!(WORKER.contains("\"--trust-remote-code\", \"--host\""));
        assert!(WORKER.contains("pinned_load"));
        assert!(WORKER.contains("HF_HUB_OFFLINE"));
        assert!(WORKER.contains("HARNESS_WORKER_API_KEY is required"));
        assert!(WORKER.contains("allow_abbrev"));
        assert!(WORKER.contains("HTTPS_PROXY"));
    }
}
