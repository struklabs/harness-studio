//! mistral.rs as an [`Engine`]: Hugging Face safetensors checkpoints (LLMs and VLMs) served
//! directly from their repo folder behind an OpenAI-compatible chat server (ROADMAP D6).
//!
//! There is no managed download: the official Windows release is CPU-only, so the user builds
//! the CLI (`cargo build --release --features cuda -p mistralrs-cli`) and registers the folder
//! that holds `mistralrs(.exe)` as a local runtime (Settings > Registry, family
//! `mistral.rs`). A launch runs
//!
//! ```text
//! mistralrs serve -m <model dir> --host 127.0.0.1 -p <port> --no-ui [--cpu]
//!     [--max-seq-len <ctx>] [--max-seqs <n>] [--pa-context-len <ctx>] <catalog> <custom>
//! ```
//!
//! A model is a folder with `config.json` and `*.safetensors` (a transformers checkpoint, not a
//! diffusers component, see [`is_model_dir`]); the library lists such folders as LLMs and
//! [`crate::engine::server_for`] sends their launches here.
//!
//! The server has no authentication of its own: it is bound to loopback and the gateway is the
//! front door. It has no llama.cpp `/props`, `/slots` or `/tokenize`: telemetry, capability
//! probes and the gateway's token admission skip it (see `ActiveServerInfo::is_llama`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use super::{Engine, MetricsSource, ModelFormat, ReleaseSource, RuntimeKind, ServerEngine};
use crate::capabilities::RuntimeCapabilities;
use crate::error_key::err;
use crate::gguf::GgufMetadata;
use crate::params::{ParamCatalog, ParamKind, ParamValue, RenderContext};
use crate::process::{BuildOutput, LaunchConfig, ServerCaps};
use crate::runtime::ReleaseAsset;
use crate::telemetry::Metrics;
use crate::vram::{
    EstimateSource, FitCategory, FitInput, FitResult, Measurement, MemoryBudget, MemoryEstimate,
};

/// Family id of mistral.rs runtimes.
pub const MISTRALRS: &str = "mistral.rs";

/// Model id the gateway sends upstream: mistral.rs answers `default` with the model it was
/// started with (the client's alias is a Harness name it does not know).
pub const UPSTREAM_MODEL: &str = "default";

/// Concurrent requests the gateway lets through when the definition sets none (mistral.rs
/// batches them; its own limit, `--max-seqs`, defaults to 32).
pub const DEFAULT_CONCURRENCY: u32 = 8;

/// Flags a custom argument line may never set (Harness owns the model, bind and UI).
const PROTECTED_FLAGS: &[&str] = &["-m", "--model-id", "--host", "-p", "--port", "--no-ui"];

/// The mistral.rs engine.
#[derive(Debug, Clone, Copy, Default)]
pub struct MistralRsEngine;

/// The parsed catalog (`params_mistralrs.toml`).
pub fn catalog() -> &'static ParamCatalog {
    static CATALOG: OnceLock<ParamCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        toml::from_str(include_str!("../params_mistralrs.toml"))
            .expect("params_mistralrs.toml is valid (unit tested)")
    })
}

fn is_protected(arg: &str) -> bool {
    let flag = arg.split('=').next().unwrap_or(arg);
    PROTECTED_FLAGS.contains(&flag)
}

// ---- model folders -------------------------------------------------------------------------

/// Top-level `*.safetensors` of a folder, sorted. A `consolidated*.safetensors` (Mistral's
/// original-format copy next to the transformers shards) is left out when shards exist.
pub fn weight_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("safetensors"))
        })
        .collect();
    files.sort();
    let consolidated = |p: &PathBuf| {
        p.file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("consolidated"))
    };
    if files.iter().any(|p| !consolidated(p)) {
        files.retain(|p| !consolidated(p));
    }
    files
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Architectures (`config.json` `architectures`) that generate text.
fn is_generative_arch(arch: &str) -> bool {
    arch.ends_with("ForCausalLM")
        || arch.ends_with("ForConditionalGeneration")
        || arch.ends_with("LMHeadModel")
        || arch.ends_with("ForImageTextToText")
}

/// Whether a parsed `config.json` is a transformers text generator (not a diffusers component,
/// a text encoder or an embedding model).
fn is_generative_config(cfg: &serde_json::Value) -> bool {
    if cfg.get("_diffusers_version").is_some() || cfg.get("_class_name").is_some() {
        return false;
    }
    cfg.get("architectures")
        .and_then(|a| a.as_array())
        .is_some_and(|a| a.iter().filter_map(|v| v.as_str()).any(is_generative_arch))
}

/// Whether `path` is a folder mistral.rs runs: a transformers checkpoint (see
/// [`is_checkpoint_dir`]) whose weights are not MLX's (the MLX engine runs those).
pub fn is_model_dir(path: &Path) -> bool {
    is_checkpoint_dir(path) && !super::mlx::is_mlx_checkpoint(path)
}

/// A transformers text-generator checkpoint (`config.json` of a text generator) with
/// safetensors weights, not a component of a diffusers pipeline nor a Python-engine task model.
pub(crate) fn is_checkpoint_dir(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    let config = path.join("config.json");
    if !config.is_file() || path.join("model_index.json").exists() {
        return false;
    }
    if path
        .parent()
        .is_some_and(|p| p.join("model_index.json").is_file())
    {
        return false;
    }
    if weight_files(path).is_empty() {
        return false;
    }
    // Encoder-decoders the Python engine serves (Whisper, BLIP, BART summarizers...) are
    // `ForConditionalGeneration` too, but not chat models.
    read_json(&config).is_some_and(|c| is_generative_config(&c))
        && !super::python_hf::is_model_dir(path)
}

/// What the files of a model folder say about it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelDirInfo {
    pub weights: Vec<PathBuf>,
    pub weights_bytes: u64,
    /// `model_type` (`qwen2`, `llama`, `gemma3`...).
    pub architecture: Option<String>,
    /// `max_position_embeddings`.
    pub context_length: Option<u64>,
    pub layers: Option<u64>,
    pub kv_heads: Option<u64>,
    pub head_dim: Option<u64>,
    /// `torch_dtype` / `dtype` (`bfloat16`).
    pub dtype: Option<String>,
    /// `quantization_config.quant_method` (`awq`, `gptq`, `fp8`...).
    pub quant_method: Option<String>,
    /// A vision tower (`vision_config`) or an image processor.
    pub vision: bool,
    /// A chat template (in `tokenizer_config.json` or its own file).
    pub chat_template: bool,
    /// Every shard `model.safetensors.index.json` names is on disk (true without an index).
    pub complete: bool,
}

/// [`ModelDirInfo`] of a folder that [`is_model_dir`] accepts; `None` otherwise.
pub fn inspect(dir: &Path) -> Option<ModelDirInfo> {
    if !is_model_dir(dir) {
        return None;
    }
    inspect_checkpoint(dir)
}

/// [`inspect`] of any text-generator checkpoint (an MLX folder too).
pub(crate) fn inspect_checkpoint(dir: &Path) -> Option<ModelDirInfo> {
    if !is_checkpoint_dir(dir) {
        return None;
    }
    let cfg = read_json(&dir.join("config.json"))?;
    let text = cfg.get("text_config").cloned().unwrap_or_default();
    let num = |key: &str| -> Option<u64> {
        cfg.get(key)
            .and_then(|v| v.as_u64())
            .or_else(|| text.get(key).and_then(|v| v.as_u64()))
    };
    let string = |key: &str| -> Option<String> {
        cfg.get(key)
            .and_then(|v| v.as_str())
            .or_else(|| text.get(key).and_then(|v| v.as_str()))
            .map(String::from)
    };
    let heads = num("num_attention_heads");
    let head_dim = num("head_dim").or_else(|| {
        let hidden = num("hidden_size")?;
        heads.filter(|&h| h > 0).map(|h| hidden / h)
    });
    let weights = weight_files(dir);
    let weights_bytes = weights
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    let tokenizer_template = read_json(&dir.join("tokenizer_config.json"))
        .is_some_and(|t| t.get("chat_template").is_some_and(|c| !c.is_null()));
    let complete = match read_json(&dir.join("model.safetensors.index.json")) {
        Some(index) => index
            .get("weight_map")
            .and_then(|m| m.as_object())
            .is_some_and(|m| {
                m.values()
                    .filter_map(|v| v.as_str())
                    .all(|f| dir.join(f).is_file())
            }),
        None => true,
    };
    Some(ModelDirInfo {
        weights,
        weights_bytes,
        architecture: string("model_type"),
        context_length: num("max_position_embeddings"),
        layers: num("num_hidden_layers"),
        kv_heads: num("num_key_value_heads").or(heads),
        head_dim,
        dtype: string("torch_dtype").or_else(|| string("dtype")),
        quant_method: cfg
            .get("quantization_config")
            .and_then(|q| q.get("quant_method"))
            .and_then(|m| m.as_str())
            .map(String::from),
        vision: cfg.get("vision_config").is_some()
            || dir.join("preprocessor_config.json").is_file(),
        chat_template: tokenizer_template
            || dir.join("chat_template.jinja").is_file()
            || dir.join("chat_template.json").is_file(),
        complete,
    })
}

/// The trained context of a model folder (`max_position_embeddings`).
pub fn trained_context(dir: &Path) -> Option<u64> {
    inspect(dir).and_then(|i| i.context_length)
}

impl ModelDirInfo {
    /// What the library shows as the quantization: the pre-quantization method (`AWQ`), else
    /// the weights' type (`BF16`), else the container.
    pub fn quantization_label(&self) -> String {
        if let Some(q) = &self.quant_method {
            return q.to_ascii_uppercase();
        }
        match self.dtype.as_deref() {
            Some("bfloat16") => "BF16".into(),
            Some("float16") => "F16".into(),
            Some("float32") => "F32".into(),
            Some(other) => other.to_ascii_uppercase(),
            None => "SAFETENSORS".into(),
        }
    }

    /// KV cache of `ctx` tokens in 16-bit (`0` when the shape is unknown).
    pub fn kv_bytes(&self, ctx: u64) -> u64 {
        match (self.layers, self.kv_heads, self.head_dim) {
            (Some(l), Some(h), Some(d)) => 2 * l * h * d * ctx * 2,
            _ => 0,
        }
    }
}

/// Bits per weight an in-situ quantization (`--isq`) leaves; `None` for an unknown value.
fn isq_bits(isq: &str) -> Option<f64> {
    let v = isq.trim().to_ascii_lowercase();
    if let Ok(n) = v.parse::<f64>() {
        return Some(n);
    }
    Some(match v.as_str() {
        "q2k" => 2.6,
        "q3k" => 3.4,
        "q4_0" | "q4_1" => 4.5,
        "q4k" => 4.5,
        "q5_0" | "q5_1" | "q5k" => 5.5,
        "q6k" => 6.6,
        "q8_0" | "q8_1" | "q8k" => 8.5,
        "hqq4" | "afq4" => 4.5,
        "hqq8" | "afq8" | "fp8" | "f8e4m3" => 8.0,
        "afq2" => 2.5,
        "afq3" => 3.5,
        "afq6" => 6.5,
        _ => return None,
    })
}

fn text_param(config: &LaunchConfig, id: &str) -> Option<String> {
    match config.params.get(id) {
        Some(ParamValue::Text(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    }
}

/// A rough memory need: the weights (scaled down by an in-situ quantization of unquantized
/// weights), the KV cache of `ctx` tokens and a fixed overhead. Never claimed exact.
pub fn need_bytes(info: &ModelDirInfo, ctx: u64, isq: Option<&str>) -> u64 {
    let mut weights = info.weights_bytes;
    if let Some(bits) = isq.and_then(isq_bits) {
        if info.quant_method.is_none() {
            let source = match info.dtype.as_deref() {
                Some("float32") => 32.0,
                _ => 16.0,
            };
            if bits < source {
                weights = (weights as f64 * bits / source) as u64;
            }
        }
    }
    const MIB: u64 = 1024 * 1024;
    let overhead = (512 * MIB).max(weights / 20);
    weights + info.kv_bytes(ctx) + overhead
}

/// The context a launch asks for: its own, else the trained one capped at Harness's default
/// chat context.
fn launch_context(config: &LaunchConfig, info: &ModelDirInfo) -> u64 {
    if config.context_size > 0 {
        return u64::from(config.context_size);
    }
    u64::from(crate::prepare::default_chat_context(info.context_length))
}

/// Device memory a launch takes (zero for a CPU launch, `gpu_layers` 0).
pub fn vram_need(config: &LaunchConfig) -> u64 {
    if config.gpu_layers == 0 {
        return 0;
    }
    let Some(info) = inspect(&config.model_path) else {
        return 0;
    };
    need_bytes(
        &info,
        launch_context(config, &info),
        text_param(config, "isq").as_deref(),
    )
}

/// Concurrent requests the gateway sends to a server of this launch.
pub fn concurrency(config: &LaunchConfig) -> u32 {
    config
        .parallel_slots
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_CONCURRENCY)
}

/// A JSON request body with its `model` set to [`UPSTREAM_MODEL`]; `None` when the body is not
/// a JSON object (it is then sent unchanged).
pub fn upstream_body(body: &[u8]) -> Option<Vec<u8>> {
    let mut v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let obj = v.as_object_mut()?;
    obj.insert("model".into(), UPSTREAM_MODEL.into());
    serde_json::to_vec(&v).ok()
}

// ---- the library ---------------------------------------------------------------------------

/// The library's view of a model folder.
pub fn to_local_model(dir: &Path, info: &ModelDirInfo, root: &Path) -> crate::models::LocalModel {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    crate::models::LocalModel {
        id: dir.to_string_lossy().to_string(),
        filename: name.clone(),
        name,
        path: dir.to_path_buf(),
        size_bytes: info.weights_bytes,
        architecture: info.architecture.clone(),
        context_length: info.context_length,
        quantization: info.quantization_label(),
        is_split: false,
        total_shards: None,
        present_shards: None,
        is_ready: info.complete,
        shard_paths: info.weights.clone(),
        root_dir: root.to_string_lossy().to_string(),
        gguf: None,
        source: None,
        mmproj_path: None,
        kind: crate::domain::ModelKind::Llm,
        origin: crate::domain::ModelOrigin::Library,
        task: None,
    }
}

/// Safetensors model folders under `roots` (a few levels deep: `<root>/<owner>/<repo>`).
pub fn library_models(roots: &[PathBuf]) -> Vec<crate::models::LocalModel> {
    roots
        .iter()
        .flat_map(|root| {
            crate::tts::scan_folders(
                std::slice::from_ref(root),
                |dir| inspect(dir).map(|info| (dir.to_path_buf(), info)),
                |a, b| a.0 == b.0,
            )
            .into_iter()
            .map(|(dir, info)| to_local_model(&dir, &info, root))
            .collect::<Vec<_>>()
        })
        .collect()
}

/// Whether a library model is a mistral.rs model folder.
pub fn is_library_model(model: &crate::models::LocalModel) -> bool {
    model.kind == crate::domain::ModelKind::Llm && model.gguf.is_none() && is_model_dir(&model.path)
}

/// Removes a model folder (everything in it); refuses anything that is not a model folder
/// strictly inside one of `roots`.
pub fn delete_model_dir(dir: &Path, roots: &[PathBuf]) -> Result<(), String> {
    if !is_model_dir(dir) {
        return Err(err("mistralrs.model_not_found")
            .with("model", dir.display())
            .into());
    }
    remove_folder_within(dir, roots)
}

/// Removes `dir` (everything in it) when it is strictly inside one of `roots`.
pub(crate) fn remove_folder_within(dir: &Path, roots: &[PathBuf]) -> Result<(), String> {
    let inside = roots.iter().any(|r| {
        crate::paths::is_within(dir, r)
            && crate::paths::normalize(dir) != crate::paths::normalize(r)
    });
    if !inside {
        return Err(err("library.outside_library")
            .with("path", dir.display())
            .into());
    }
    std::fs::remove_dir_all(dir).map_err(|e| {
        String::from(
            err("fs.remove_failed")
                .with("path", dir.display())
                .with("error", e),
        )
    })
}

// ---- the engine ----------------------------------------------------------------------------

/// Capabilities as documented: every catalog flag, for a build whose `serve --help` could not
/// be read (the catalog is then rendered as documented instead of being dropped).
fn documented_caps() -> &'static RuntimeCapabilities {
    static CAPS: OnceLock<RuntimeCapabilities> = OnceLock::new();
    CAPS.get_or_init(|| {
        let mut help = String::from("Options:\n");
        for p in &catalog().params {
            let hint = if p.kind == ParamKind::Bool {
                ""
            } else {
                " <VALUE>"
            };
            let values = if p.values.is_empty() {
                String::new()
            } else {
                format!(" [possible values: {}]", p.values.join(", "))
            };
            help.push_str(&format!(
                "      {}{hint}  {}{values}\n",
                p.flags.join(", "),
                p.id
            ));
        }
        help.push_str("  -m, --model-id <MODEL_ID>  model\n");
        crate::capabilities::mistralrs::from_outputs(Default::default(), "", &help)
    })
}

impl Engine for MistralRsEngine {
    fn family(&self) -> &'static str {
        MISTRALRS
    }

    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Server
    }

    fn release_source(&self) -> ReleaseSource {
        // No managed download: the user builds the CLI and registers its folder.
        ReleaseSource {
            repo: "",
            binary: "mistralrs",
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
        format == ModelFormat::Safetensors
    }

    fn version_text(&self, output: &str) -> Option<String> {
        let v = crate::capabilities::mistralrs::parse_version(output);
        v.version.map(|_| v.raw)
    }

    fn detect_capabilities(&self, binary: &Path) -> Arc<RuntimeCapabilities> {
        crate::capabilities::registry()
            .get_or_probe_with(binary, crate::capabilities::mistralrs::probe_binary)
    }

    fn cached_capabilities(&self, binary: &Path) -> Option<Arc<RuntimeCapabilities>> {
        crate::capabilities::registry().cached(binary)
    }

    fn probe_capabilities(&self, binary: &Path) -> RuntimeCapabilities {
        crate::capabilities::mistralrs::probe_binary(binary)
    }

    fn param_catalog(&self) -> &'static ParamCatalog {
        catalog()
    }

    fn as_server(&self) -> Option<&dyn ServerEngine> {
        Some(self)
    }
}

impl ServerEngine for MistralRsEngine {
    fn build_launch(
        &self,
        config: &LaunchConfig,
        _caps: &ServerCaps,
        runtime: Option<&RuntimeCapabilities>,
    ) -> Result<BuildOutput, String> {
        if !crate::network::is_loopback_host(&config.host) {
            return Err(err("process.mistralrs_loopback_only")
                .with("host", &config.host)
                .into());
        }
        build_args(config, runtime)
    }

    fn health_path(&self) -> &'static str {
        "/health"
    }

    /// The server binds after the model is loaded: any success answer means ready.
    fn health(&self, status: u16, _body: &str) -> bool {
        (200..300).contains(&status)
    }

    fn openai_base(&self, host_url: &str) -> String {
        format!("{}/v1", host_url.trim_end_matches('/'))
    }

    /// No llama.cpp `/metrics` or `/slots` (liveness comes from `/health`).
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
        let info = input
            .model_path
            .as_deref()
            .and_then(|p| inspect(Path::new(p)))
            .unwrap_or_else(|| ModelDirInfo {
                weights_bytes: input.model_size_bytes,
                ..Default::default()
            });
        let ctx = if input.context_size > 0 {
            u64::from(input.context_size)
        } else {
            u64::from(crate::prepare::default_chat_context(info.context_length))
        };
        let total = need_bytes(&info, ctx, None);
        let kv = info.kv_bytes(ctx);
        let model = info.weights_bytes;
        let rest = total.saturating_sub(model + kv);
        let gpu = budget.vram_bytes > 0 && input.gpu_layers > 0;
        let estimate = MemoryEstimate {
            vram_bytes: if gpu { total } else { 0 },
            ram_bytes: if gpu { 0 } else { total },
            model_gpu_bytes: if gpu { model } else { 0 },
            model_host_bytes: if gpu { 0 } else { model },
            kv_bytes: kv,
            kv_gpu_bytes: if gpu { kv } else { 0 },
            compute_gpu_bytes: if gpu { rest } else { 0 },
            compute_host_bytes: if gpu { 0 } else { rest },
            total_layers: info.layers.unwrap_or(0),
            offloaded_layers: if gpu { info.layers.unwrap_or(0) } else { 0 },
            kv_cells: ctx,
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

/// The `mistralrs serve` command line of a launch.
pub fn build_args(
    config: &LaunchConfig,
    runtime: Option<&RuntimeCapabilities>,
) -> Result<BuildOutput, String> {
    let cpu = config.gpu_layers == 0;
    let mut args: Vec<String> = vec![
        "serve".into(),
        "-m".into(),
        config.model_path.to_string_lossy().to_string(),
        "--host".into(),
        config.host.clone(),
        "-p".into(),
        config.port.to_string(),
        "--no-ui".into(),
    ];
    let custom = match config
        .custom_args
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        Some(raw) => {
            let custom = crate::process::split_args(raw)?;
            if let Some(bad) = custom.iter().find(|a| is_protected(a)) {
                return Err(err("params.protected_flag").with("flag", bad).into());
            }
            custom
        }
        None => Vec::new(),
    };
    // clap refuses a flag given twice: the user's own argument wins over the typed field.
    let custom_has = |flag: &str| {
        custom
            .iter()
            .any(|a| a.split('=').next().unwrap_or(a) == flag)
    };
    if cpu && !custom_has("--cpu") {
        args.push("--cpu".into());
    }
    if config.context_size > 0 && !custom_has("--max-seq-len") {
        args.push("--max-seq-len".into());
        args.push(config.context_size.to_string());
    }
    if let Some(n) = config
        .parallel_slots
        .filter(|&n| n > 0 && !custom_has("--max-seqs"))
    {
        args.push("--max-seqs".into());
        args.push(n.to_string());
    }
    // The PagedAttention cache is sized to the context instead of a share of the free memory,
    // so a model leaves room for the others the gateway keeps loaded. Not when the user sizes
    // it (or turns it off) themselves.
    let pa_set = ["pa_context_len", "pa_memory_mb", "pa_memory_fraction"]
        .iter()
        .any(|id| config.params.contains_key(*id))
        || custom.iter().any(|a| a.starts_with("--pa-"))
        || text_param(config, "paged_attn").as_deref() == Some("off");
    if !cpu && config.context_size > 0 && !pa_set {
        args.push("--pa-context-len".into());
        args.push(config.context_size.to_string());
    }
    let caps = runtime
        .filter(|r| r.help_parsed)
        .unwrap_or_else(|| documented_caps());
    let ctx = RenderContext {
        host: config.host.clone(),
        ..Default::default()
    };
    let rendered = crate::params::render_params(catalog(), Some(caps), &config.params, &ctx)?;
    args.extend(rendered.args);
    args.extend(custom);
    Ok(BuildOutput {
        args,
        skipped: rendered.skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&crate::paths::test_dir_prefix(tag))
            .tempdir()
            .unwrap()
    }

    const QWEN_CONFIG: &str = r#"{
        "architectures": ["Qwen2ForCausalLM"],
        "model_type": "qwen2",
        "hidden_size": 896,
        "num_attention_heads": 14,
        "num_key_value_heads": 2,
        "num_hidden_layers": 24,
        "max_position_embeddings": 32768,
        "torch_dtype": "bfloat16"
    }"#;

    /// A Qwen2.5-0.5B-Instruct-like folder: config, tokenizer with a chat template, one shard.
    fn model_dir(root: &Path, name: &str) -> PathBuf {
        let dir = root.join("Qwen").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.json"), QWEN_CONFIG).unwrap();
        std::fs::write(
            dir.join("tokenizer_config.json"),
            r#"{"chat_template": "{% for m in messages %}{{ m.content }}{% endfor %}"}"#,
        )
        .unwrap();
        std::fs::write(dir.join("tokenizer.json"), "{}").unwrap();
        std::fs::write(dir.join("model.safetensors"), vec![0u8; 4096]).unwrap();
        dir
    }

    fn config(dir: &Path) -> LaunchConfig {
        LaunchConfig {
            model_path: dir.to_path_buf(),
            host: "127.0.0.1".into(),
            port: 11460,
            gpu_layers: 999,
            context_size: 16384,
            ..Default::default()
        }
    }

    #[test]
    fn catalog_parses_and_flags_are_documented() {
        let c = catalog();
        assert!(c.get("isq").is_some() && c.get("paged_attn").is_some());
        let help = crate::capabilities::mistralrs::from_outputs(
            Default::default(),
            "",
            &crate::capabilities::mistralrs::tests_help(),
        );
        for p in &c.params {
            assert!(p.label_key.contains('.') && p.help_key.contains('.'));
            for f in &p.flags {
                assert!(!is_protected(f), "{f} is set by Harness itself");
                assert!(
                    help.has_flag(f),
                    "{f} ({}) is not in mistralrs serve --help",
                    p.id
                );
            }
        }
        for f in PROTECTED_FLAGS {
            assert!(help.has_flag(f), "{f}");
        }
    }

    #[test]
    fn exact_argv_of_a_definition() {
        let root = temp("mrs-argv");
        let dir = model_dir(root.path(), "Qwen2.5-0.5B-Instruct");
        let mut c = config(&dir);
        c.parallel_slots = Some(4);
        c.params
            .insert("isq".into(), ParamValue::Text("q4k".into()));
        let out = MistralRsEngine
            .build_launch(&c, &ServerCaps::default(), None)
            .unwrap();
        let d = dir.to_string_lossy().to_string();
        assert_eq!(
            out.args,
            [
                "serve",
                "-m",
                d.as_str(),
                "--host",
                "127.0.0.1",
                "-p",
                "11460",
                "--no-ui",
                "--max-seq-len",
                "16384",
                "--max-seqs",
                "4",
                "--pa-context-len",
                "16384",
                "--isq",
                "q4k",
            ]
        );
        assert!(out.skipped.is_empty(), "{:?}", out.skipped);

        // The parsed help of the build gates the catalog the same way.
        let caps = crate::capabilities::mistralrs::from_outputs(
            Default::default(),
            "mistralrs 0.7.0",
            &crate::capabilities::mistralrs::tests_help(),
        );
        let gated = MistralRsEngine
            .build_launch(&c, &ServerCaps::default(), Some(&caps))
            .unwrap();
        assert_eq!(gated.args, out.args);

        // CPU: `--cpu`, no PagedAttention sizing; the user's own size wins on GPU.
        c.gpu_layers = 0;
        let cpu = build_args(&c, None).unwrap().args.join(" ");
        assert!(cpu.contains("--no-ui --cpu --max-seq-len 16384"), "{cpu}");
        assert!(!cpu.contains("--pa-context-len"), "{cpu}");
        c.gpu_layers = 999;
        c.params
            .insert("pa_memory_mb".into(), ParamValue::Int(4096));
        let own = build_args(&c, None).unwrap().args.join(" ");
        assert!(own.ends_with("--isq q4k --pa-memory-mb 4096"), "{own}");
        assert!(!own.contains("--pa-context-len"), "{own}");

        // A llama.cpp parameter of a shared definition is skipped, not sent.
        c.params.insert("cache_ram".into(), ParamValue::Int(0));
        let skipped = build_args(&c, None).unwrap();
        assert!(skipped.skipped.iter().any(|s| s.id == "cache_ram"));
        assert!(!skipped.args.iter().any(|a| a.contains("cache")));
    }

    #[test]
    fn custom_arguments_cannot_take_the_bind_or_model() {
        let root = temp("mrs-custom");
        let dir = model_dir(root.path(), "m");
        let mut c = config(&dir);
        c.custom_args = Some("--port 9999".into());
        assert!(build_args(&c, None)
            .unwrap_err()
            .starts_with("params.protected_flag"));
        c.custom_args = Some("--model-id=other".into());
        assert!(build_args(&c, None).is_err());
        c.custom_args = Some("--dtype bf16 --pa-cache-type f8e4m3".into());
        let ok = build_args(&c, None).unwrap().args.join(" ");
        assert!(ok.ends_with("--dtype bf16 --pa-cache-type f8e4m3"), "{ok}");
        // The user's PagedAttention flag replaces Harness's sizing.
        assert!(!ok.contains("--pa-context-len"), "{ok}");
        // A typed value is not sent a second time next to the user's own flag (clap refuses).
        c.custom_args = Some("--max-seq-len=2048 --max-seqs 2".into());
        c.parallel_slots = Some(4);
        let args = build_args(&c, None).unwrap().args;
        assert_eq!(
            args.iter()
                .filter(|a| a.starts_with("--max-seq-len"))
                .count(),
            1,
            "{args:?}"
        );
        assert_eq!(args.iter().filter(|a| *a == "--max-seqs").count(), 1);
    }

    #[test]
    fn the_library_records_a_folder_as_a_mistralrs_chat_model() {
        use crate::hub_compat::{Compatibility, ModelFormat};
        let root = temp("mrs-lib");
        let dir = model_dir(root.path(), "Qwen2.5-0.5B-Instruct");
        let m = &library_models(&[root.path().to_path_buf()])[0];
        // The folder is the repo folder the downloader made (`<library>/<owner>/<repo>`).
        assert_eq!(
            crate::library::ops::repo_of(m, &[root.path().to_path_buf()]).as_deref(),
            Some("Qwen/Qwen2.5-0.5B-Instruct")
        );
        assert_eq!(
            crate::model_registry::local_format(m),
            ModelFormat::Safetensors
        );
        let caps = crate::library::ModelCaps {
            chat: true,
            ..Default::default()
        };
        let mut rec = crate::model_registry::local_record(m, &caps);
        assert_eq!(rec.runtime.as_deref(), Some(MISTRALRS));
        assert_eq!(
            rec.compatibility,
            Some(Compatibility::Runnable {
                runtime: MISTRALRS.into()
            })
        );
        rec.resolve_installed(&["llama.cpp".to_string()]);
        assert_eq!(
            rec.compatibility,
            Some(Compatibility::NeedsRuntime {
                runtime: MISTRALRS.into()
            })
        );
        // Its fit is the folder's own estimate.
        let fit = crate::model_registry::fit_for(
            FitInput {
                model_path: Some(dir.to_string_lossy().to_string()),
                context_size: 4096,
                gpu_layers: 0,
                ..Default::default()
            },
            &crate::hardware::detect_hardware(),
            None,
        );
        assert_eq!(fit.estimate.vram_bytes, 0);
        assert!(fit.estimate.ram_bytes >= 4096 + (512 << 20));
    }

    #[test]
    fn a_remote_bind_is_refused() {
        let root = temp("mrs-bind");
        let dir = model_dir(root.path(), "m");
        let mut c = config(&dir);
        c.host = "0.0.0.0".into();
        let e = MistralRsEngine.build_launch(&c, &ServerCaps::default(), None);
        assert!(e
            .unwrap_err()
            .starts_with("process.mistralrs_loopback_only"));
    }

    #[test]
    fn model_folders_are_told_apart() {
        let root = temp("mrs-dirs");
        let dir = model_dir(root.path(), "Qwen2.5-0.5B-Instruct");
        assert!(is_model_dir(&dir));
        assert!(!is_model_dir(&dir.join("model.safetensors")));
        let info = inspect(&dir).unwrap();
        assert_eq!(info.architecture.as_deref(), Some("qwen2"));
        assert_eq!(info.context_length, Some(32768));
        assert_eq!(info.head_dim, Some(64));
        assert_eq!(info.kv_heads, Some(2));
        assert_eq!(info.quantization_label(), "BF16");
        assert!(info.chat_template && !info.vision && info.complete);
        assert_eq!(info.weights_bytes, 4096);
        // 2 (K, V) x 24 layers x 2 heads x 64 x 2 bytes per token.
        assert_eq!(info.kv_bytes(1), 2 * 24 * 2 * 64 * 2);

        // A diffusers component and a text encoder are not chat models.
        let flux = root.path().join("flux");
        let transformer = flux.join("transformer");
        std::fs::create_dir_all(&transformer).unwrap();
        std::fs::write(flux.join("model_index.json"), "{}").unwrap();
        std::fs::write(transformer.join("config.json"), QWEN_CONFIG).unwrap();
        std::fs::write(transformer.join("x.safetensors"), [0u8; 8]).unwrap();
        assert!(!is_model_dir(&transformer));
        let t5 = root.path().join("t5");
        std::fs::create_dir_all(&t5).unwrap();
        std::fs::write(
            t5.join("config.json"),
            r#"{"architectures":["T5EncoderModel"]}"#,
        )
        .unwrap();
        std::fs::write(t5.join("model.safetensors"), [0u8; 8]).unwrap();
        assert!(!is_model_dir(&t5));
        // No weights yet (only the config arrived).
        let empty = root.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::write(empty.join("config.json"), QWEN_CONFIG).unwrap();
        assert!(!is_model_dir(&empty));

        // Shards named by the index must all be there.
        std::fs::write(
            dir.join("model.safetensors.index.json"),
            r#"{"weight_map":{"a":"model.safetensors","b":"model-00002-of-00002.safetensors"}}"#,
        )
        .unwrap();
        assert!(!inspect(&dir).unwrap().complete);

        // The library lists the folder (once) as an LLM.
        let models = library_models(&[root.path().to_path_buf()]);
        assert_eq!(models.len(), 1, "{models:?}");
        let m = &models[0];
        assert_eq!(m.path, dir);
        assert_eq!(m.name, "Qwen2.5-0.5B-Instruct");
        assert_eq!(m.kind, crate::domain::ModelKind::Llm);
        assert!(!m.is_ready);
        assert!(is_library_model(m));
    }

    #[test]
    fn vision_and_mistral_layouts() {
        let root = temp("mrs-vlm");
        let dir = root.path().join("vlm");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.json"),
            r#"{"architectures":["Gemma3ForConditionalGeneration"],"model_type":"gemma3",
                "text_config":{"num_hidden_layers":34,"num_attention_heads":8,"num_key_value_heads":4,"head_dim":256,"max_position_embeddings":131072},
                "vision_config":{},"quantization_config":{"quant_method":"fp8"}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("chat_template.json"), "{}").unwrap();
        std::fs::write(dir.join("consolidated.safetensors"), [0u8; 100]).unwrap();
        std::fs::write(dir.join("model-00001-of-00001.safetensors"), [0u8; 10]).unwrap();
        let info = inspect(&dir).unwrap();
        assert!(info.vision && info.chat_template);
        assert_eq!(info.layers, Some(34));
        assert_eq!(info.context_length, Some(131072));
        assert_eq!(info.quantization_label(), "FP8");
        // The consolidated copy is not counted next to the shards.
        assert_eq!(info.weights_bytes, 10);
    }

    #[test]
    fn memory_need_scales_with_isq_and_context() {
        let info = ModelDirInfo {
            weights_bytes: 16 << 30,
            layers: Some(32),
            kv_heads: Some(8),
            head_dim: Some(128),
            dtype: Some("bfloat16".into()),
            ..Default::default()
        };
        let kv_8k = info.kv_bytes(8192);
        assert_eq!(kv_8k, 1 << 30, "Llama-3-8B: 1 GiB of KV per 8k tokens");
        let full = need_bytes(&info, 8192, None);
        assert_eq!(full, (16 << 30) + kv_8k + (16u64 << 30) / 20);
        let q4 = need_bytes(&info, 8192, Some("q4k"));
        assert!(q4 < full / 2, "{q4} vs {full}");
        assert_eq!(need_bytes(&info, 8192, Some("nonsense")), full);
        // An already quantized checkpoint is not scaled again.
        let awq = ModelDirInfo {
            quant_method: Some("awq".into()),
            ..info.clone()
        };
        assert_eq!(need_bytes(&awq, 8192, Some("4")), full);
    }

    #[test]
    fn estimate_and_vram_need() {
        let root = temp("mrs-fit");
        let dir = model_dir(root.path(), "m");
        let budget = MemoryBudget {
            vram_bytes: 8 << 30,
            ram_bytes: 16 << 30,
            ..Default::default()
        };
        let input = FitInput {
            model_path: Some(dir.to_string_lossy().to_string()),
            context_size: 4096,
            gpu_layers: 999,
            ..Default::default()
        };
        let fit = MistralRsEngine.estimate_memory(&input, None, &budget, &[]);
        assert_eq!(fit.category, FitCategory::FullGpu);
        assert!(!fit.estimate.exact);
        assert_eq!(fit.estimate.kv_bytes, 2 * 24 * 2 * 64 * 2 * 4096);
        let mut c = config(&dir);
        assert!(vram_need(&c) >= 512 << 20);
        c.gpu_layers = 0;
        assert_eq!(vram_need(&c), 0);
        assert_eq!(concurrency(&c), DEFAULT_CONCURRENCY);
        c.parallel_slots = Some(2);
        assert_eq!(concurrency(&c), 2);
    }

    #[test]
    fn health_and_upstream_body() {
        let e = MistralRsEngine;
        assert!(e.health(200, "OK"));
        assert!(!e.health(503, ""));
        assert_eq!(
            e.openai_base("http://127.0.0.1:1/"),
            "http://127.0.0.1:1/v1"
        );
        let body = br#"{"model":"qwen-chat","messages":[],"stream":true}"#;
        let out: serde_json::Value = serde_json::from_slice(&upstream_body(body).unwrap()).unwrap();
        assert_eq!(out["model"], UPSTREAM_MODEL);
        assert_eq!(out["stream"], true);
        assert!(upstream_body(b"not json").is_none());
        assert!(upstream_body(b"[1]").is_none());
    }

    #[test]
    fn deleting_a_folder_stays_inside_the_library() {
        let root = temp("mrs-del");
        let dir = model_dir(root.path(), "m");
        let other = temp("mrs-del-other");
        assert!(delete_model_dir(&dir, &[other.path().to_path_buf()]).is_err());
        assert!(delete_model_dir(root.path(), &[root.path().to_path_buf()]).is_err());
        delete_model_dir(&dir, &[root.path().to_path_buf()]).unwrap();
        assert!(!dir.exists());
    }
}
