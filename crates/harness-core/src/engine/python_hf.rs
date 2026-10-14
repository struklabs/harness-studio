//! The Python engine (`python-hf`) as an [`Engine`]: a Hugging Face model folder served by
//! `python/hf_worker.py` (transformers pipelines, sentence-transformers, diffusers) from the
//! managed `hf-cpu` environment ([`crate::pyenv::envs::HF_CPU`]).
//!
//! A launch runs
//!
//! ```text
//! <env python> <env>/hf_worker.py --model <dir> --task <task> --host 127.0.0.1 --port <port>
//!     [--alias <alias>] [--threads <n>] <catalog>
//! ```
//!
//! On Linux the worker runs without network (`pyenv::isolation`): it listens on a Unix socket
//! (`--uds`, see [`listen_on_socket`]) and Harness relays the loopback port to it. Every run has
//! an API key (`HARNESS_WORKER_API_KEY`): the worker refuses requests without it, so only
//! Harness and the gateway can use the port.
//!
//! A model is a folder with `config.json` (or a diffusers `model_index.json`) and weights whose
//! task the worker serves ([`detect`]); remote code is never run, so a folder whose config
//! has an `auto_map` is not one. Text generators stay with mistral.rs.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use super::{Engine, MetricsSource, ModelFormat, ReleaseSource, RuntimeKind, ServerEngine};
use crate::capabilities::RuntimeCapabilities;
use crate::domain::ModelKind;
use crate::error_key::err;
use crate::gguf::GgufMetadata;
use crate::params::{ParamCatalog, RenderContext};
use crate::process::{BuildOutput, LaunchConfig, ServerCaps};
use crate::runtime::ReleaseAsset;
use crate::telemetry::Metrics;
use crate::vram::{
    EstimateSource, FitCategory, FitInput, FitResult, Measurement, MemoryBudget, MemoryEstimate,
};

/// Family id of the Python engine.
pub const PYTHON_HF: &str = "python-hf";

/// The managed environment the worker runs in.
pub const ENV: &str = crate::pyenv::envs::HF_CPU.name;

/// The worker script, written next to the environment ([`worker_script`]).
const WORKER: &str = include_str!("../../python/hf_worker.py");

/// Environment variable the worker reads its API key from.
pub const API_KEY_ENV: &str = "HARNESS_WORKER_API_KEY";

/// Flags a custom argument line may never set (Harness owns the model, task, bind and code
/// policy).
const PROTECTED_FLAGS: &[&str] = &[
    "--model",
    "--task",
    "--host",
    "--port",
    "--uds",
    "--alias",
    "--trust-remote-code",
];

/// Tasks the worker serves (`hf_worker.py` `TASKS`).
pub const TASKS: &[&str] = &[
    "text-classification",
    "token-classification",
    "zero-shot-classification",
    "fill-mask",
    "question-answering",
    "table-question-answering",
    "summarization",
    "translation",
    "text2text-generation",
    "feature-extraction",
    "sentence-similarity",
    "text-ranking",
    "image-classification",
    "zero-shot-image-classification",
    "object-detection",
    "zero-shot-object-detection",
    "image-segmentation",
    "depth-estimation",
    "image-to-text",
    "image-feature-extraction",
    "document-question-answering",
    "visual-question-answering",
    "mask-generation",
    "automatic-speech-recognition",
    "audio-classification",
    "zero-shot-audio-classification",
    "text-to-speech",
    "text-to-audio",
    "text-to-image",
];

/// The Python engine.
#[derive(Debug, Clone, Copy, Default)]
pub struct PythonHfEngine;

/// The parsed catalog (`params_pyhf.toml`).
pub fn catalog() -> &'static ParamCatalog {
    static CATALOG: OnceLock<ParamCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        toml::from_str(include_str!("../params_pyhf.toml"))
            .expect("params_pyhf.toml is valid (unit tested)")
    })
}

/// A protected flag, or an abbreviation of one (`--trust`, `--ud`): the worker's parser takes
/// no abbreviations, and this refuses them before it is asked.
/// The worker's flags as capabilities, from the catalog (its flags are the worker's own; there is
/// no binary to probe): without them every catalog setting would be skipped as unknown.
fn documented_caps() -> &'static RuntimeCapabilities {
    static CAPS: OnceLock<RuntimeCapabilities> = OnceLock::new();
    CAPS.get_or_init(|| {
        let help: String = catalog()
            .params
            .iter()
            .map(|p| format!("{} N    {}\n", p.flags.join(", "), p.id))
            .collect();
        RuntimeCapabilities::from_outputs(Default::default(), "", &help, "")
    })
}

fn is_protected(arg: &str) -> bool {
    let flag = arg.split('=').next().unwrap_or(arg);
    flag.starts_with("--") && flag.len() > 2 && PROTECTED_FLAGS.iter().any(|p| p.starts_with(flag))
}

// ---- model folders -------------------------------------------------------------------------

fn read_json(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Weight files the worker loads: safetensors, or PyTorch `.bin` files (read with
/// `torch.load(weights_only=True)`), at the top of the folder and one level down (diffusers
/// components, sentence-transformers modules).
pub fn weight_files(dir: &Path) -> Vec<PathBuf> {
    fn is_weight(p: &Path) -> bool {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        name.ends_with(".safetensors")
            || (name.ends_with(".bin")
                && (name.starts_with("pytorch_model")
                    || name.starts_with("diffusion_pytorch_model")
                    || name.starts_with("model")))
    }
    let mut out = Vec::new();
    let entries = |d: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(d)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect()
    };
    for path in entries(dir) {
        if path.is_file() && is_weight(&path) {
            out.push(path);
        } else if path.is_dir()
            && !path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        {
            out.extend(
                entries(&path)
                    .into_iter()
                    .filter(|p| p.is_file() && is_weight(p)),
            );
        }
    }
    out.sort();
    out
}

/// `pipeline_tag` of the model card's front matter (`README.md`).
fn card_pipeline_tag(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("README.md")).ok()?;
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines {
        let line = line.trim_end();
        if line.trim() == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix("pipeline_tag:") {
            let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
            return Some(value.to_string()).filter(|v| !v.is_empty());
        }
    }
    None
}

/// The task of a transformers `config.json` from its model type and architecture.
fn config_task(cfg: &serde_json::Value, dir: &Path) -> Option<&'static str> {
    let model_type = cfg.get("model_type").and_then(|v| v.as_str()).unwrap_or("");
    match model_type {
        "vilt" => return Some("visual-question-answering"),
        "blip" | "vision-encoder-decoder" | "git" => return Some("image-to-text"),
        "whisper" | "wav2vec2" | "hubert" | "moonshine" => {
            let archs = archs(cfg);
            if archs
                .iter()
                .any(|a| a.ends_with("ForSequenceClassification"))
            {
                return Some("audio-classification");
            }
            return Some("automatic-speech-recognition");
        }
        "vits" | "musicgen" | "bark" => return Some("text-to-speech"),
        "marian" | "m2m_100" | "fsmt" | "nllb-moe" => return Some("translation"),
        // Encoder-decoders whose card says what they were tuned for (summarization...); the
        // general prompt-in, text-out task otherwise.
        "t5" | "mt5" | "umt5" | "longt5" | "bart" | "mbart" | "pegasus" | "pegasus_x" | "led"
        | "bigbird_pegasus" | "prophetnet" => return Some("text2text-generation"),
        "clip" | "siglip" => return Some("zero-shot-image-classification"),
        "clap" => return Some("zero-shot-audio-classification"),
        "owlvit" | "owlv2" | "grounding-dino" => return Some("zero-shot-object-detection"),
        "sam" => return Some("mask-generation"),
        _ => {}
    }
    let archs = archs(cfg);
    let has = |suffix: &str| archs.iter().any(|a| a.ends_with(suffix));
    if dir.join("modules.json").is_file() {
        return Some("sentence-similarity");
    }
    if has("ForSequenceClassification") {
        let labels: Vec<String> = cfg
            .get("id2label")
            .and_then(|v| v.as_object())
            .map(|m| {
                m.values()
                    .filter_map(|v| v.as_str())
                    .map(str::to_ascii_lowercase)
                    .collect()
            })
            .unwrap_or_default();
        // A one-label head scores (query, document) pairs: a cross-encoder. An NLI head
        // (entailment / contradiction) is what zero-shot classification runs on.
        return Some(if labels.len() == 1 {
            "text-ranking"
        } else if labels.iter().any(|l| l.starts_with("entail"))
            && labels.iter().any(|l| l.starts_with("contradict"))
        {
            "zero-shot-classification"
        } else {
            "text-classification"
        });
    }
    if has("ForTokenClassification") {
        return Some("token-classification");
    }
    if has("ForQuestionAnswering") {
        return Some("question-answering");
    }
    if has("ForMaskedLM") {
        return Some("fill-mask");
    }
    if has("ForImageClassification") {
        return Some("image-classification");
    }
    if has("ForObjectDetection") {
        return Some("object-detection");
    }
    if has("ForSemanticSegmentation")
        || has("ForUniversalSegmentation")
        || has("ForInstanceSegmentation")
    {
        return Some("image-segmentation");
    }
    if has("ForDepthEstimation") {
        return Some("depth-estimation");
    }
    if has("ForAudioClassification") {
        return Some("audio-classification");
    }
    if has("ForCTC") || has("ForSpeechSeq2Seq") {
        return Some("automatic-speech-recognition");
    }
    if has("ForTextToSpeech") || has("ForTextToWaveform") {
        return Some("text-to-speech");
    }
    if has("ForSeq2SeqLM") {
        return Some("text2text-generation");
    }
    None
}

fn archs(cfg: &serde_json::Value) -> Vec<&str> {
    cfg.get("architectures")
        .and_then(|a| a.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default()
}

/// Libraries a diffusers `model_index.json` component may come from; anything else is a module
/// of the repo itself.
const INDEX_LIBRARIES: [&str; 2] = ["diffusers", "transformers"];

/// Whether a folder asks for code of its own (the worker refuses it too): an `auto_map` in any
/// JSON file at the top or one level down (the model's, a component's, the tokenizer's config),
/// a diffusers component from another library, or a sentence-transformers module that is not
/// one of the library's own.
pub fn asks_for_code(dir: &Path) -> bool {
    let json_files = |d: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(d)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "json"))
            .collect()
    };
    let mut files = json_files(dir);
    for sub in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = sub.path();
        if p.is_dir() && !sub.file_name().to_string_lossy().starts_with('.') {
            files.extend(json_files(&p));
        }
    }
    if files
        .iter()
        .filter_map(|f| read_json(f))
        .any(|v| v.get("auto_map").is_some())
    {
        return true;
    }
    if let Some(index) = read_json(&dir.join("model_index.json")).and_then(|v| match v {
        serde_json::Value::Object(m) => Some(m),
        _ => None,
    }) {
        let foreign = index.values().any(|v| {
            v.as_array()
                .and_then(|a| a.first())
                .is_some_and(|lib| lib.as_str().is_some_and(|l| !INDEX_LIBRARIES.contains(&l)))
        });
        if foreign {
            return true;
        }
    }
    read_json(&dir.join("modules.json"))
        .and_then(|v| v.as_array().cloned())
        .is_some_and(|modules| {
            modules.iter().any(|m| {
                !m.get("type")
                    .and_then(|t| t.as_str())
                    .is_some_and(|t| t.starts_with("sentence_transformers."))
            })
        })
}

/// The task the worker would run a folder with: `text-to-image` for a diffusers text-to-image
/// pipeline, else the model card's `pipeline_tag` when the worker serves it, else a task from
/// `config.json`. `None` for anything else: no weights, remote code (`auto_map`), a component
/// of a diffusers pipeline, or a text generator (mistral.rs runs those).
pub fn detect(dir: &Path) -> Option<String> {
    if !dir.is_dir() {
        return None;
    }
    if dir
        .parent()
        .is_some_and(|p| p.join("model_index.json").is_file())
    {
        return None;
    }
    let index = dir.join("model_index.json");
    if index.is_file() && asks_for_code(dir) {
        return None;
    }
    if index.is_file() {
        let class = read_json(&index)?
            .get("_class_name")
            .and_then(|v| v.as_str())
            .map(str::to_string)?;
        let text_to_image = class.ends_with("Pipeline")
            && ![
                "Img2Img",
                "Inpaint",
                "Video",
                "Audio",
                "Upscale",
                "Depth2Img",
                "ControlNet",
            ]
            .iter()
            .any(|w| class.contains(w));
        return (text_to_image && !weight_files(dir).is_empty()).then(|| "text-to-image".into());
    }
    if asks_for_code(dir) {
        return None;
    }
    let cfg = read_json(&dir.join("config.json"))?;
    if cfg.get("_diffusers_version").is_some() || weight_files(dir).is_empty() {
        return None;
    }
    if let Some(tag) = card_pipeline_tag(dir) {
        // A chat or generation card (text-generation, image-text-to-text...) is not ours.
        return TASKS.contains(&tag.as_str()).then_some(tag);
    }
    config_task(&cfg, dir).map(String::from)
}

/// Whether `path` is a folder this engine runs.
pub fn is_model_dir(path: &Path) -> bool {
    detect(path).is_some()
}

/// The library kind of a task: embedding and ranking models keep their kinds (the gateway
/// sends `/v1/embeddings` and `/v1/rerank` to them), everything else is a pipeline model.
pub fn kind_of(task: &str) -> ModelKind {
    match task {
        "feature-extraction" | "sentence-similarity" => ModelKind::Embedding,
        "text-ranking" => ModelKind::Reranker,
        _ => ModelKind::Pipeline,
    }
}

/// The serving mode of a model folder.
pub fn serve_mode(dir: &Path) -> crate::serve_mode::ServeMode {
    crate::serve_mode::ServeMode::for_kind(detect(dir).map_or(ModelKind::Pipeline, |t| kind_of(&t)))
}

/// The library's view of a model folder.
fn to_local_model(dir: &Path, task: String, root: &Path) -> crate::models::LocalModel {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let weights = weight_files(dir);
    let size = weights
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    let architecture = read_json(&dir.join("config.json"))
        .and_then(|c| {
            c.get("model_type")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .or_else(|| {
            read_json(&dir.join("model_index.json")).and_then(|c| {
                c.get("_class_name")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
        });
    crate::models::LocalModel {
        id: dir.to_string_lossy().to_string(),
        filename: name.clone(),
        name,
        path: dir.to_path_buf(),
        size_bytes: size,
        architecture,
        context_length: None,
        quantization: String::new(),
        is_split: false,
        total_shards: None,
        present_shards: None,
        is_ready: true,
        shard_paths: weights,
        root_dir: root.to_string_lossy().to_string(),
        gguf: None,
        source: None,
        mmproj_path: None,
        kind: kind_of(&task),
        origin: crate::domain::ModelOrigin::Library,
        task: Some(task),
    }
}

/// Whether a repo folder carries an ONNX export (`model.onnx`, `onnx/*.onnx`).
fn has_onnx(dir: &Path) -> bool {
    let any_onnx = |d: &Path| {
        std::fs::read_dir(d)
            .into_iter()
            .flatten()
            .flatten()
            .any(|e| {
                e.path()
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("onnx"))
            })
    };
    any_onnx(dir) || any_onnx(&dir.join("onnx"))
}

/// Model folders under `roots` this engine runs (a few levels deep: `<root>/<owner>/<repo>`).
pub fn library_models(roots: &[PathBuf]) -> Vec<crate::models::LocalModel> {
    roots
        .iter()
        .flat_map(|root| {
            crate::tts::scan_folders(
                std::slice::from_ref(root),
                // A repo with an ONNX export runs on harness-onnx (its `.onnx` is listed).
                |dir| {
                    detect(dir)
                        .filter(|_| !has_onnx(dir))
                        .map(|task| (dir.to_path_buf(), task))
                },
                |a, b| a.0 == b.0,
            )
            .into_iter()
            .map(|(dir, task)| to_local_model(&dir, task, root))
            .collect::<Vec<_>>()
        })
        .collect()
}

// ---- the environment -----------------------------------------------------------------------

/// The worker script of this version, written next to the environment (rewritten when it
/// differs, so an app update brings its worker along).
pub fn worker_script() -> Result<PathBuf, String> {
    let dir = crate::pyenv::envs::env_dir_in(&crate::pyenv::envs::root(), ENV);
    let path = dir.join("hf_worker.py");
    if std::fs::read_to_string(&path).ok().as_deref() == Some(WORKER) {
        return Ok(path);
    }
    std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(&path, WORKER))
        .map_err(|e| {
            String::from(
                err("fs.write_failed")
                    .with("path", path.display())
                    .with("error", e),
            )
        })?;
    Ok(path)
}

/// The interpreter a launch runs: the environment's, when it is installed for this version;
/// else `runtime.family_not_installed: python-hf` (the UI offers the install).
pub fn binary() -> Result<PathBuf, String> {
    let root = crate::pyenv::envs::root();
    let status = crate::pyenv::envs::status_in(&root, &crate::pyenv::envs::HF_CPU);
    if !status.ready {
        return Err(format!(
            "{}: {}",
            crate::runtimes::ERR_FAMILY_NOT_INSTALLED,
            PYTHON_HF
        ));
    }
    Ok(crate::pyenv::envs::python_in(&root, ENV))
}

/// Environment variables of a worker run: [`crate::pyenv::python_env`], the API key, and
/// `torch.load` limited to tensors (`weights_only`) for every caller, so no pickle in a model
/// folder runs code.
pub fn worker_env(api_key: &str) -> Vec<(&'static str, std::ffi::OsString)> {
    let mut env = crate::pyenv::python_env(&crate::pyenv::envs::root());
    env.push((API_KEY_ENV, api_key.into()));
    env.push(("TORCH_FORCE_WEIGHTS_ONLY_LOAD", "1".into()));
    env
}

/// Replaces `--host <h> --port <p>` of a launch with `--uds <socket>` (an isolated run listens
/// on a Unix socket; Harness relays the port to it).
pub fn listen_on_socket(args: &mut Vec<String>, socket: &Path) {
    for flag in ["--host", "--port"] {
        if let Some(i) = args.iter().position(|a| a == flag) {
            args.drain(i..(i + 2).min(args.len()));
        }
    }
    // After the script and the model/task pair, before the optional flags.
    let at = args
        .iter()
        .position(|a| a == "--task")
        .map_or(args.len(), |i| i + 2);
    let at = at.min(args.len());
    args.splice(
        at..at,
        ["--uds".to_string(), socket.to_string_lossy().to_string()],
    );
}

/// Device memory of a launch: none, the environment is the CPU build.
pub fn vram_need(_config: &LaunchConfig) -> u64 {
    0
}

impl Engine for PythonHfEngine {
    fn family(&self) -> &'static str {
        PYTHON_HF
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

    /// Folders are told apart by [`is_model_dir`], not by a file format.
    fn supports(&self, _format: ModelFormat) -> bool {
        false
    }

    fn detect_capabilities(&self, binary: &Path) -> Arc<RuntimeCapabilities> {
        Arc::new(self.probe_capabilities(binary))
    }

    fn cached_capabilities(&self, binary: &Path) -> Option<Arc<RuntimeCapabilities>> {
        Some(self.detect_capabilities(binary))
    }

    /// Nothing to probe (`python --help` says nothing of the worker): the catalog is the
    /// worker's own.
    fn probe_capabilities(&self, _binary: &Path) -> RuntimeCapabilities {
        RuntimeCapabilities::default()
    }

    fn param_catalog(&self) -> &'static ParamCatalog {
        catalog()
    }

    fn as_server(&self) -> Option<&dyn ServerEngine> {
        Some(self)
    }
}

impl ServerEngine for PythonHfEngine {
    fn build_launch(
        &self,
        config: &LaunchConfig,
        _caps: &ServerCaps,
        _runtime: Option<&RuntimeCapabilities>,
    ) -> Result<BuildOutput, String> {
        if !crate::network::is_loopback_host(&config.host) {
            return Err(err("process.pyhf_loopback_only")
                .with("host", &config.host)
                .into());
        }
        let task = detect(&config.model_path).ok_or_else(|| {
            String::from(err("pyhf.model_unsupported").with("path", config.model_path.display()))
        })?;
        build_args(config, &worker_script()?, &task)
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

    /// The weights plus PyTorch's own share (the interpreter, the libraries and activations),
    /// all in RAM.
    fn worker_env(&self, api_key: &str) -> Option<Vec<(&'static str, std::ffi::OsString)>> {
        Some(worker_env(api_key))
    }

    fn uses_socket(&self) -> bool {
        true
    }

    fn listen_on_socket(&self, args: &mut Vec<String>, socket: &Path) {
        listen_on_socket(args, socket);
    }

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
                    weight_files(Path::new(p))
                        .iter()
                        .filter_map(|f| std::fs::metadata(f).ok())
                        .map(|m| m.len())
                        .sum()
                })
                .unwrap_or(0)
        };
        // torch + transformers take ~600 MB before the first tensor. A diffusers pipeline on the
        // CPU loads in float32 (a float16 checkpoint doubles) and its activations at 512² and
        // up are another ~1.5 GB.
        let diffusers = input
            .model_path
            .as_deref()
            .is_some_and(|p| Path::new(p).join("model_index.json").is_file());
        let compute = if diffusers {
            (600u64 << 20) + model + (1536u64 << 20)
        } else {
            (600u64 << 20) + model / 4
        };
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
fn build_args(config: &LaunchConfig, script: &Path, task: &str) -> Result<BuildOutput, String> {
    let mut args: Vec<String> = vec![
        script.to_string_lossy().to_string(),
        "--model".into(),
        config.model_path.to_string_lossy().to_string(),
        "--task".into(),
        task.to_string(),
        "--host".into(),
        config.host.clone(),
        "--port".into(),
        config.port.to_string(),
    ];
    if let Some(alias) = crate::process::served_alias(config) {
        args.push("--alias".into());
        args.push(alias);
    }
    if let Some(threads) = config.threads.filter(|&n| n > 0) {
        args.push("--threads".into());
        args.push(threads.to_string());
    }
    let ctx = RenderContext {
        host: config.host.clone(),
        ..Default::default()
    };
    let rendered =
        crate::params::render_params(catalog(), Some(documented_caps()), &config.params, &ctx)?;
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

    #[test]
    fn catalog_parses() {
        assert!(!catalog().params.is_empty());
    }

    #[test]
    fn tasks_match_the_worker() {
        for task in TASKS {
            assert!(WORKER.contains(&format!("\"{task}\"")), "{task}");
        }
    }

    #[test]
    fn detects_tasks_from_config_and_card() {
        let cls = folder(&[
            (
                "config.json",
                r#"{"architectures":["BertForSequenceClassification"],"id2label":{"0":"NEG","1":"POS"}}"#,
            ),
            ("model.safetensors", "x"),
        ]);
        assert_eq!(detect(cls.path()).as_deref(), Some("text-classification"));
        let ranker = folder(&[
            (
                "config.json",
                r#"{"architectures":["XLMRobertaForSequenceClassification"],"id2label":{"0":"LABEL_0"}}"#,
            ),
            ("model.safetensors", "x"),
        ]);
        assert_eq!(detect(ranker.path()).as_deref(), Some("text-ranking"));
        let nli = folder(&[
            (
                "config.json",
                r#"{"architectures":["DistilBertForSequenceClassification"],"id2label":{"0":"ENTAILMENT","1":"NEUTRAL","2":"CONTRADICTION"}}"#,
            ),
            ("model.safetensors", "x"),
        ]);
        assert_eq!(
            detect(nli.path()).as_deref(),
            Some("zero-shot-classification")
        );
        let st = folder(&[
            ("config.json", r#"{"architectures":["BertModel"]}"#),
            ("modules.json", "[]"),
            ("model.safetensors", "x"),
        ]);
        assert_eq!(detect(st.path()).as_deref(), Some("sentence-similarity"));
        // The card wins over the architecture.
        let summ = folder(&[
            (
                "config.json",
                r#"{"architectures":["BartForConditionalGeneration"],"model_type":"bart"}"#,
            ),
            (
                "README.md",
                "---\nlicense: mit\npipeline_tag: summarization\n---\n# x",
            ),
            ("pytorch_model.bin", "x"),
        ]);
        assert_eq!(detect(summ.path()).as_deref(), Some("summarization"));
        let t5 = folder(&[
            (
                "config.json",
                r#"{"architectures":["T5ForConditionalGeneration"],"model_type":"t5"}"#,
            ),
            ("model.safetensors", "x"),
        ]);
        assert_eq!(detect(t5.path()).as_deref(), Some("text2text-generation"));
        let asr = folder(&[
            (
                "config.json",
                r#"{"architectures":["WhisperForConditionalGeneration"],"model_type":"whisper"}"#,
            ),
            ("model.safetensors", "x"),
        ]);
        assert_eq!(
            detect(asr.path()).as_deref(),
            Some("automatic-speech-recognition")
        );
    }

    #[test]
    fn refuses_generators_remote_code_and_bare_configs() {
        let llm = folder(&[
            ("config.json", r#"{"architectures":["Qwen3ForCausalLM"]}"#),
            ("README.md", "---\npipeline_tag: text-generation\n---\n"),
            ("model.safetensors", "x"),
        ]);
        assert_eq!(detect(llm.path()), None);
        let remote = folder(&[
            (
                "config.json",
                r#"{"architectures":["XForSequenceClassification"],"auto_map":{"AutoModel":"m.X"}}"#,
            ),
            ("model.safetensors", "x"),
        ]);
        assert_eq!(detect(remote.path()), None);
        // Remote code anywhere: a tokenizer's auto_map, a custom sentence-transformers module, a
        // diffusers component of the repo's own.
        let tok = folder(&[
            ("config.json", r#"{"architectures":["BertForMaskedLM"]}"#),
            (
                "tokenizer_config.json",
                r#"{"auto_map":{"AutoTokenizer":["t.T",null]}}"#,
            ),
            ("model.safetensors", "x"),
        ]);
        assert_eq!(detect(tok.path()), None);
        let st = folder(&[
            ("config.json", r#"{"architectures":["BertModel"]}"#),
            (
                "modules.json",
                r#"[{"idx":0,"path":"","type":"my_module.Custom"}]"#,
            ),
            ("model.safetensors", "x"),
        ]);
        assert_eq!(detect(st.path()), None);
        let sd = folder(&[
            (
                "model_index.json",
                r#"{"_class_name":"StableDiffusionPipeline","unet":["my_unet","UNet"]}"#,
            ),
            ("unet/diffusion_pytorch_model.safetensors", "x"),
        ]);
        assert_eq!(detect(sd.path()), None);
        let empty = folder(&[("config.json", r#"{"architectures":["BertForMaskedLM"]}"#)]);
        assert_eq!(detect(empty.path()), None);
    }

    #[test]
    fn detects_diffusers_text_to_image_but_not_its_components() {
        let sd = folder(&[
            (
                "model_index.json",
                r#"{"_class_name":"StableDiffusionPipeline"}"#,
            ),
            (
                "unet/config.json",
                r#"{"_class_name":"UNet2DConditionModel","_diffusers_version":"0.30"}"#,
            ),
            ("unet/diffusion_pytorch_model.safetensors", "x"),
            (
                "text_encoder/config.json",
                r#"{"architectures":["CLIPTextModel"],"model_type":"clip_text_model"}"#,
            ),
            ("text_encoder/model.safetensors", "x"),
        ]);
        assert_eq!(detect(sd.path()).as_deref(), Some("text-to-image"));
        assert_eq!(detect(&sd.path().join("text_encoder")), None);
        let inpaint = folder(&[
            (
                "model_index.json",
                r#"{"_class_name":"StableDiffusionInpaintPipeline"}"#,
            ),
            ("unet/diffusion_pytorch_model.safetensors", "x"),
        ]);
        assert_eq!(detect(inpaint.path()), None);
    }

    #[test]
    fn catalog_settings_reach_the_worker() {
        let dir = folder(&[
            (
                "config.json",
                r#"{"architectures":["BertForTokenClassification"]}"#,
            ),
            ("model.safetensors", "x"),
        ]);
        let mut config = LaunchConfig {
            model_path: dir.path().to_path_buf(),
            host: "127.0.0.1".into(),
            port: 8123,
            ..Default::default()
        };
        config.params.insert(
            "device".into(),
            crate::params::ParamValue::Text("cpu".into()),
        );
        config.params.insert(
            "dtype".into(),
            crate::params::ParamValue::Text("float32".into()),
        );
        let out = build_args(&config, Path::new("/w.py"), "token-classification").unwrap();
        assert!(out.skipped.is_empty(), "{:?}", out.skipped);
        assert!(
            out.args.windows(2).any(|w| w == ["--device", "cpu"]),
            "{:?}",
            out.args
        );
        assert!(
            out.args.windows(2).any(|w| w == ["--dtype", "float32"]),
            "{:?}",
            out.args
        );
    }

    #[test]
    fn launch_args_and_socket_rewrite() {
        let dir = folder(&[
            (
                "config.json",
                r#"{"architectures":["BertForTokenClassification"]}"#,
            ),
            ("model.safetensors", "x"),
        ]);
        let config = LaunchConfig {
            model_path: dir.path().to_path_buf(),
            host: "127.0.0.1".into(),
            port: 8123,
            threads: Some(4),
            ..Default::default()
        };
        let out = build_args(
            &config,
            Path::new("/w/hf_worker.py"),
            "token-classification",
        )
        .unwrap();
        assert_eq!(out.args[0], "/w/hf_worker.py");
        assert!(out
            .args
            .windows(2)
            .any(|w| w == ["--task", "token-classification"]));
        assert!(out.args.windows(2).any(|w| w == ["--port", "8123"]));
        let mut args = out.args.clone();
        listen_on_socket(&mut args, Path::new("/run/w.sock"));
        assert!(!args
            .iter()
            .any(|a| a == "--host" || a == "--port" || a == "8123"));
        assert!(args.windows(2).any(|w| w == ["--uds", "/run/w.sock"]));
        assert!(args.windows(2).any(|w| w == ["--threads", "4"]));

        let lan = LaunchConfig {
            host: "0.0.0.0".into(),
            ..config.clone()
        };
        assert!(PythonHfEngine
            .build_launch(&lan, &ServerCaps::default(), None)
            .is_err());
        let custom = LaunchConfig {
            custom_args: Some("--trust-remote-code".into()),
            ..config
        };
        assert!(build_args(&custom, Path::new("/w.py"), "fill-mask").is_err());
        // Abbreviations of protected flags are refused as well.
        for bad in ["--trust", "--trust-remote", "--ud=/tmp/x", "--mod", "--ta"] {
            let c = LaunchConfig {
                custom_args: Some(bad.into()),
                ..custom.clone()
            };
            assert!(
                build_args(&c, Path::new("/w.py"), "fill-mask").is_err(),
                "{bad}"
            );
        }
        let ok = LaunchConfig {
            custom_args: Some("--device cpu --dtype float32".into()),
            ..custom
        };
        assert!(build_args(&ok, Path::new("/w.py"), "fill-mask").is_ok());
    }
}
