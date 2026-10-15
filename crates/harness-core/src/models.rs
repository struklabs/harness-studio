use crate::config::AppSettings;
use crate::error_key::err;
use crate::gguf::{read_gguf_metadata, GgufMetadata};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelPreset {
    pub gpu_layers: u32,
    pub context_size: u32,
    pub threads: Option<u32>,
    #[serde(default)]
    pub flash_attn: bool,
    #[serde(default)]
    pub cache_type_k: Option<String>,
    #[serde(default)]
    pub cache_type_v: Option<String>,
    #[serde(default)]
    pub parallel_slots: Option<u32>,
    #[serde(default)]
    pub logical_batch: Option<u32>,
    #[serde(default)]
    pub physical_batch: Option<u32>,
    #[serde(default)]
    pub context_shift: Option<bool>,
    #[serde(default)]
    pub split_mode: Option<String>,
    #[serde(default)]
    pub main_gpu: Option<u32>,
    #[serde(default)]
    pub tensor_split: Option<String>,
    #[serde(default)]
    pub rope_scaling: Option<String>,
    #[serde(default)]
    pub rope_freq_base: Option<f32>,
    #[serde(default)]
    pub rope_freq_scale: Option<f32>,
    #[serde(default)]
    pub chat_template: Option<String>,
    /// Input only (a newly typed key, or a legacy plaintext value); never serialized.
    /// `Some("")` clears the saved key. The key itself lives in the OS keychain.
    #[serde(default, skip_serializing)]
    pub api_key: Option<String>,
    /// Keychain reference of the saved key (safe to store and to send to the UI).
    #[serde(default)]
    pub api_key_ref: Option<String>,
    #[serde(default)]
    pub custom_args: Option<String>,
    #[serde(default)]
    pub draft_model_path: Option<String>,
    #[serde(default)]
    pub draft_gpu_layers: Option<u32>,
    #[serde(default)]
    pub lora_path: Option<String>,
    #[serde(default)]
    pub lora_scale: Option<f32>,
    #[serde(default)]
    pub mmproj_path: Option<String>,
    // llama-server's own MCP option. (The old `server_tools` / `--tools` field is gone: files
    // saved with it still load, the value is ignored.)
    #[serde(default)]
    pub mcp_enabled: bool,
    /// Catalog parameters (`params.toml`) without a typed field above.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub params: crate::params::ParamValues,
    /// Runtime id (`runtimes` registry; `None` = the official runtime).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
}

impl ModelPreset {
    /// The launch config this preset describes for `model_path` (no host/port).
    pub fn to_launch_config(&self, model_path: &Path) -> crate::process::LaunchConfig {
        crate::process::LaunchConfig {
            model_path: model_path.to_path_buf(),
            gpu_layers: self.gpu_layers,
            context_size: self.context_size,
            threads: self.threads,
            flash_attn: self.flash_attn,
            cache_type_k: self.cache_type_k.clone(),
            cache_type_v: self.cache_type_v.clone(),
            parallel_slots: self.parallel_slots,
            logical_batch: self.logical_batch,
            physical_batch: self.physical_batch,
            context_shift: self.context_shift,
            split_mode: self.split_mode.clone(),
            main_gpu: self.main_gpu,
            tensor_split: self.tensor_split.clone(),
            rope_scaling: self.rope_scaling.clone(),
            rope_freq_base: self.rope_freq_base,
            rope_freq_scale: self.rope_freq_scale,
            chat_template: self.chat_template.clone(),
            api_key: self.api_key.clone(),
            api_key_ref: self.api_key_ref.clone(),
            custom_args: self.custom_args.clone(),
            draft_model_path: self.draft_model_path.as_ref().map(PathBuf::from),
            draft_gpu_layers: self.draft_gpu_layers,
            lora_path: self.lora_path.as_ref().map(PathBuf::from),
            lora_scale: self.lora_scale,
            mmproj_path: self.mmproj_path.as_ref().map(PathBuf::from),
            mcp_enabled: self.mcp_enabled,
            params: self.params.clone(),
            runtime: self.runtime.clone(),
            ..Default::default()
        }
    }

    pub fn from_launch_config(c: &crate::process::LaunchConfig) -> Self {
        let text = |p: &Option<PathBuf>| p.as_ref().map(|p| p.to_string_lossy().to_string());
        Self {
            gpu_layers: c.gpu_layers,
            context_size: c.context_size,
            threads: c.threads,
            flash_attn: c.flash_attn,
            cache_type_k: c.cache_type_k.clone(),
            cache_type_v: c.cache_type_v.clone(),
            parallel_slots: c.parallel_slots,
            logical_batch: c.logical_batch,
            physical_batch: c.physical_batch,
            context_shift: c.context_shift,
            split_mode: c.split_mode.clone(),
            main_gpu: c.main_gpu,
            tensor_split: c.tensor_split.clone(),
            rope_scaling: c.rope_scaling.clone(),
            rope_freq_base: c.rope_freq_base,
            rope_freq_scale: c.rope_freq_scale,
            chat_template: c.chat_template.clone(),
            api_key: None,
            api_key_ref: c.api_key_ref.clone(),
            custom_args: c.custom_args.clone(),
            draft_model_path: text(&c.draft_model_path),
            draft_gpu_layers: c.draft_gpu_layers,
            lora_path: text(&c.lora_path),
            lora_scale: c.lora_scale,
            mmproj_path: text(&c.mmproj_path),
            mcp_enabled: c.mcp_enabled,
            params: c.params.clone(),
            runtime: c.runtime.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LocalModel {
    pub id: String,
    pub name: String,
    pub filename: String,
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub path: PathBuf, // Launch path (for split models, path to shard 1)
    pub size_bytes: u64,
    pub architecture: Option<String>,
    pub context_length: Option<u64>,
    pub quantization: String,
    pub is_split: bool,
    pub total_shards: Option<u32>,
    pub present_shards: Option<u32>,
    pub is_ready: bool,
    #[cfg_attr(feature = "openapi", schema(value_type = Vec<String>))]
    pub shard_paths: Vec<PathBuf>,
    pub root_dir: String, // Directory root this model belongs to
    /// Parsed GGUF header (architecture hyper-parameters for memory estimates).
    #[serde(default)]
    pub gguf: Option<GgufMetadata>,
    /// "ollama" | "lmstudio" for models linked in place from another app
    /// (not inside a library directory; never moved or deleted by us).
    #[serde(default)]
    pub source: Option<String>,
    /// Vision projector that belongs to this model (known for imported models).
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub mmproj_path: Option<PathBuf>,
    /// What the file is (LLM, embedding, reranker, projector, adapter), from its GGUF header.
    #[serde(default)]
    pub kind: crate::domain::ModelKind,
    /// Library directory or linked from another app.
    #[serde(default)]
    pub origin: crate::domain::ModelOrigin,
    /// Hugging Face pipeline task of a [`crate::domain::ModelKind::Pipeline`] model
    /// (`text-classification`, `token-classification`...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
}

/// A native decision model (`<arch>.decision.type` in its GGUF header, see
/// [`GgufMetadata::decision_type`]).
pub fn is_decision_model(path: &Path) -> bool {
    cached_metadata(path).is_some_and(|m| m.decision_type.is_some())
}

/// Header parsing is cached by (path, size, mtime) so rescans are cheap.
pub(crate) fn cached_metadata(path: &Path) -> Option<GgufMetadata> {
    use std::sync::Mutex;
    type Key = (PathBuf, u64, Option<std::time::SystemTime>);
    static CACHE: Mutex<Option<HashMap<Key, GgufMetadata>>> = Mutex::new(None);

    let md = fs::metadata(path).ok()?;
    let key = (path.to_path_buf(), md.len(), md.modified().ok());
    if let Some(hit) = CACHE.lock().unwrap().as_ref().and_then(|c| c.get(&key)) {
        return Some(hit.clone());
    }
    let meta = read_gguf_metadata(path).ok()?;
    CACHE
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(key, meta.clone());
    Some(meta)
}

/// `.gguf`, `.bin` (a whisper ggml model; the magic is checked when the file is scanned),
/// `.safetensors` / `.ckpt` (image models; the tensor names are checked when the file is
/// scanned) or `.onnx` (an ONNX encoder; kept only when a tokenizer sits next to it).
fn is_model_extension(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["gguf", "bin", "safetensors", "ckpt", "onnx"]
            .iter()
            .any(|x| e.eq_ignore_ascii_case(x))
    })
}

fn onnx_task_kind(task: harness_onnx::package::Task) -> crate::domain::ModelKind {
    match task {
        harness_onnx::package::Task::Embedding => crate::domain::ModelKind::Embedding,
        harness_onnx::package::Task::Rerank => crate::domain::ModelKind::Reranker,
        harness_onnx::package::Task::AsrCtc => crate::domain::ModelKind::Asr,
        _ => crate::domain::ModelKind::Pipeline,
    }
}

/// What an encoder, vision, audio or CTC speech `.onnx` file serves (embedding, reranking, speech
/// recognition or a task pipeline), `None` for any other file.
pub fn onnx_kind(path: &Path) -> Option<crate::domain::ModelKind> {
    harness_onnx::package::library_task(path)
        .map(onnx_task_kind)
        .or_else(|| pipeline_package(path).map(|_| crate::domain::ModelKind::Pipeline))
}

/// The name, pipeline task and other graphs (a captioner's decoder) of a vision or audio
/// classification package (a processor config and a task head in `config.json`, no
/// tokenizer-driven encoder task).
fn pipeline_package(path: &Path) -> Option<(String, &'static str, Vec<PathBuf>)> {
    if let Some(p) = harness_onnx::vision_pkg::VisionPackage::discover(path) {
        let others = p.caption.iter().map(|c| c.decoder_path.clone()).collect();
        return Some((p.name.clone(), p.task.pipeline_task(), others));
    }
    if let Some(p) = harness_onnx::seq2seq_pkg::Seq2SeqPackage::discover(path) {
        return Some((p.name.clone(), p.task, vec![p.decoder_path.clone()]));
    }
    harness_onnx::audio_pkg::AudioPackage::discover(path)
        .map(|p| (p.name, "audio-classification", Vec::new()))
}

/// The library entry of a vision or audio model (`preprocessor_config.json` and a task head in
/// `config.json`): a task pipeline.
fn vision_entry(path: &Path, root_dir: &str) -> Option<LocalModel> {
    let (package_name, task, others) = pipeline_package(path)?;
    let stem = path.file_stem()?.to_string_lossy().to_string();
    // A captioner's size is its encoder and decoder graphs, with their external weights.
    let mut graphs = vec![path.to_path_buf()];
    graphs.extend(others);
    let mut size = 0;
    for graph in &graphs {
        size += fs::metadata(graph).map(|m| m.len()).unwrap_or(0);
        let graph_stem = graph.file_stem()?.to_string_lossy().to_string();
        for ext in ["onnx_data", "onnx.data"] {
            size += fs::metadata(graph.with_file_name(format!("{graph_stem}.{ext}")))
                .map(|m| m.len())
                .unwrap_or(0);
        }
    }
    Some(LocalModel {
        id: path.to_string_lossy().to_string(),
        name: harness_onnx::package::alias_of(path).unwrap_or(package_name),
        filename: path.file_name()?.to_string_lossy().to_string(),
        path: path.to_path_buf(),
        size_bytes: size,
        architecture: Some("onnx".to_string()),
        context_length: None,
        quantization: harness_onnx::package::variant_quantization(&stem).to_string(),
        is_split: false,
        total_shards: None,
        present_shards: None,
        is_ready: true,
        shard_paths: vec![path.to_path_buf()],
        root_dir: root_dir.to_string(),
        gguf: None,
        source: None,
        mmproj_path: None,
        kind: crate::domain::ModelKind::Pipeline,
        origin: crate::domain::ModelOrigin::Library,
        task: Some(task.to_string()),
    })
}

/// The library entry of an `.onnx` file: an embedding/reranking package with a
/// tokenizer, or a CTC speech package with audio configs and a vocabulary.
fn onnx_entry(path: &Path, root_dir: &str) -> Option<LocalModel> {
    if let Some(genai) = harness_onnx::genai_pkg::GenaiPackage::discover(path) {
        return genai_entry(path, &genai, root_dir);
    }
    // The other graphs of a genai folder (vision, embeddings) are parts of that model, not
    // encoders of their own.
    if path
        .parent()
        .is_some_and(|d| d.join(harness_onnx::genai_pkg::CONFIG_FILE).is_file())
    {
        return None;
    }
    let package = match harness_onnx::package::ModelPackage::discover(path) {
        Ok(p) => p,
        Err(_) => return vision_entry(path, root_dir),
    };
    let stem = path.file_stem()?.to_string_lossy().to_string();
    let filename = path.file_name()?.to_string_lossy().to_string();
    // `model.onnx` is the repo's own model; other files are named after their variant (the same
    // name the model's alias gets).
    let name = harness_onnx::package::alias_of(path).unwrap_or_else(|| package.name.clone());
    // External weights (`model.onnx_data`) belong to the model.
    let mut size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    for ext in ["onnx_data", "onnx.data"] {
        let data = path.with_file_name(format!("{}.{}", path.file_stem()?.to_string_lossy(), ext));
        size += fs::metadata(data).map(|m| m.len()).unwrap_or(0);
    }
    Some(LocalModel {
        id: path.to_string_lossy().to_string(),
        name,
        filename,
        path: path.to_path_buf(),
        size_bytes: size,
        architecture: Some("onnx".to_string()),
        context_length: (package.task != harness_onnx::package::Task::AsrCtc)
            .then_some(package.max_length as u64),
        quantization: harness_onnx::package::variant_quantization(&stem).to_string(),
        is_split: false,
        total_shards: None,
        present_shards: None,
        is_ready: true,
        shard_paths: vec![path.to_path_buf()],
        root_dir: root_dir.to_string(),
        gguf: None,
        source: None,
        mmproj_path: None,
        kind: onnx_task_kind(package.task),
        origin: crate::domain::ModelOrigin::Library,
        task: package.pipeline_task().map(String::from),
    })
}

/// The library entry of the decoder of an onnxruntime-genai chat model (a folder with
/// `genai_config.json`), named like its alias: the repo and the variant folder
/// (`Phi-3-mini-4k-instruct-onnx-directml-int4-awq-block-128`). Vision and speech models of the
/// same format are not chat models here.
fn genai_entry(
    path: &Path,
    genai: &harness_onnx::genai_pkg::GenaiPackage,
    root_dir: &str,
) -> Option<LocalModel> {
    if !genai.text_only {
        return None;
    }
    let folder = genai.dir.file_name()?.to_string_lossy().to_string();
    let name = harness_onnx::genai_pkg::display_name(&genai.dir)?;
    Some(LocalModel {
        id: path.to_string_lossy().to_string(),
        name,
        filename: path.file_name()?.to_string_lossy().to_string(),
        path: path.to_path_buf(),
        size_bytes: genai.weight_bytes(),
        architecture: Some("onnx".to_string()),
        context_length: (genai.context_length > 0).then_some(genai.context_length as u64),
        quantization: harness_onnx::genai_pkg::precision_of(&folder).to_string(),
        is_split: false,
        total_shards: None,
        present_shards: None,
        is_ready: true,
        shard_paths: vec![path.to_path_buf()],
        root_dir: root_dir.to_string(),
        gguf: None,
        source: None,
        mmproj_path: None,
        kind: crate::domain::ModelKind::Llm,
        origin: crate::domain::ModelOrigin::Library,
        task: None,
    })
}

/// Kind and quantization of a whisper ggml `.bin` from its file name: `ggml-base.bin` is an
/// f16 model, `ggml-large-v3-turbo-q5_0.bin` Q5_0. The silero VAD model (`ggml-silero-*.bin`)
/// has the same container but is not a speech recognizer.
fn ggml_bin_traits(filename: &str) -> (crate::domain::ModelKind, String) {
    let kind = if filename.to_ascii_lowercase().contains("silero") {
        crate::domain::ModelKind::Unknown
    } else {
        crate::domain::ModelKind::Asr
    };
    let quant = match crate::gguf::extract_quant_from_filename(filename).as_str() {
        "Unknown" => "F16".to_string(),
        q => q.to_string(),
    };
    (kind, quant)
}

/// Kind of a single GGUF file: [`crate::domain::detect_kind`], except that an image generator
/// (a diffusion architecture, or stable-diffusion.cpp's metadata-less conversion told by its
/// tensors) is [`crate::domain::ModelKind::Image`].
fn image_aware_kind(path: &Path, metadata: Option<&GgufMetadata>) -> crate::domain::ModelKind {
    use crate::domain::ModelKind;
    if crate::engine::crispasr::is_model(path) {
        return ModelKind::Asr;
    }
    if crate::engine::crispasr::is_asr_gguf(path) {
        return ModelKind::Unknown;
    }
    let kind = crate::domain::detect_kind(metadata);
    if matches!(kind, ModelKind::Unknown | ModelKind::Llm)
        && crate::engine::is_image_gguf(path, metadata)
    {
        ModelKind::Image
    } else {
        kind
    }
}

/// Name and "quantization" of an image checkpoint file: the stem, and a precision token of its
/// name (`fp16`, `fp8`, `bf16`) or the container (`SAFETENSORS`, `CKPT`).
fn image_file_traits(filename: &str) -> (String, String) {
    let path = Path::new(filename);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| filename.to_string());
    let lower = stem.to_ascii_lowercase();
    let precision = ["fp32", "fp16", "bf16", "fp8"]
        .into_iter()
        .find(|p| lower.contains(p))
        .map(str::to_ascii_uppercase);
    let container = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_uppercase())
        .unwrap_or_default();
    (stem, precision.unwrap_or(container))
}

/// (parent dir, base name, total shard count)
type SplitKey = (PathBuf, String, u32);
/// (shard index, path, size)
type ShardEntry = (u32, PathBuf, u64);

#[derive(Default, Clone)]
pub struct ModelManager;

impl ModelManager {
    /// Scans every configured model directory (primary + extra).
    pub fn scan_models(&self) -> Vec<LocalModel> {
        let settings = AppSettings::load();
        let mut models = self.scan_directories(&settings.all_models_dirs());
        models.extend(Self::external_models(&models));
        models.sort_by(|a, b| a.name.cmp(&b.name));
        models
    }

    /// Models linked from Ollama / LM Studio that aren't already in a library dir.
    fn external_models(known: &[LocalModel]) -> Vec<LocalModel> {
        crate::importer::load_external()
            .into_iter()
            .filter(|e| e.path.is_file() && !known.iter().any(|m| m.path == e.path))
            .map(|e| {
                let metadata = cached_metadata(&e.path);
                let kind = crate::domain::detect_kind(metadata.as_ref());
                let filename = e
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let source = match e.source {
                    crate::importer::ImportSource::Ollama => "ollama",
                    crate::importer::ImportSource::Lmstudio => "lmstudio",
                };
                LocalModel {
                    id: e.path.to_string_lossy().to_string(),
                    name: e.name.clone(),
                    filename,
                    size_bytes: fs::metadata(&e.path).map(|m| m.len()).unwrap_or(0),
                    architecture: metadata.as_ref().and_then(|m| m.architecture.clone()),
                    context_length: metadata.as_ref().and_then(|m| m.context_length),
                    quantization: metadata
                        .as_ref()
                        .map(|m| m.quantization.clone())
                        .unwrap_or_else(|| crate::gguf::extract_quant_from_filename(&e.name)),
                    is_split: false,
                    total_shards: None,
                    present_shards: None,
                    is_ready: true,
                    shard_paths: vec![e.path.clone()],
                    root_dir: e
                        .path
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    gguf: metadata,
                    source: Some(source.to_string()),
                    mmproj_path: e.mmproj_path.filter(|p| p.is_file()),
                    kind,
                    origin: crate::domain::ModelOrigin::Linked,
                    task: None,
                    path: e.path,
                }
            })
            .collect()
    }

    /// Looks up a model by id in a fresh scan, so callers never act on
    /// file paths supplied by the frontend.
    pub fn find_model(&self, model_id: &str) -> Option<LocalModel> {
        self.scan_models().into_iter().find(|m| m.id == model_id)
    }

    pub fn scan_directories(&self, dirs: &[PathBuf]) -> Vec<LocalModel> {
        let mut models = Vec::new();
        for dir in dirs {
            if !dir.exists() {
                let _ = fs::create_dir_all(dir);
                continue;
            }
            let mut paths = Vec::new();
            self.collect_gguf_files(dir, &mut paths);
            models.extend(self.scan_files(dir, paths));
            // Text-to-speech folders (Kokoro, Piper) are found by their files, not an extension.
            models.extend(crate::tts::library_models(std::slice::from_ref(dir)));
            // So are the speech-recognition ones (Parakeet, SenseVoice...) of sherpa-onnx.
            models.extend(crate::asr::library_models(std::slice::from_ref(dir)));
            // Safetensors checkpoints (a repo folder with `config.json`) run on mistral.rs.
            models.extend(crate::engine::mistralrs_library_models(
                std::slice::from_ref(dir),
            ));
            // Task models (classifiers, embedders, ASR, diffusers...) run on the Python engine.
            models.extend(crate::engine::python_hf::library_models(
                std::slice::from_ref(dir),
            ));
            // MLX checkpoints (mlx-community...) run on the MLX engine, on Apple Silicon.
            models.extend(crate::engine::mlx::library_models(std::slice::from_ref(
                dir,
            )));
        }
        models.sort_by(|a, b| a.name.cmp(&b.name));
        models
    }

    /// Inspects only existing GGUF paths and their split siblings, without changing storage settings.
    pub fn inspect_gguf_paths(&self, paths: &[PathBuf]) -> Vec<LocalModel> {
        let split_regex = Regex::new(r"(?i)^(.*?)-(\d{5})-of-(\d{5})\.gguf$").unwrap();
        let mut groups: HashMap<PathBuf, std::collections::HashSet<PathBuf>> = HashMap::new();
        for path in paths {
            let Ok(md) = path.symlink_metadata() else {
                continue;
            };
            if !md.is_file() || md.file_type().is_symlink() {
                continue;
            }
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
            {
                continue;
            }
            let Some(parent) = path.parent() else {
                continue;
            };
            let files = groups.entry(parent.to_path_buf()).or_default();
            files.insert(path.clone());
            if let Some(caps) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| split_regex.captures(n))
            {
                let base = &caps[1];
                let total: u32 = caps[3].parse().unwrap_or(0);
                if total > 0 && total <= 100 {
                    for i in 1..=total {
                        let sibling =
                            parent.join(format!("{}-{:05}-of-{:05}.gguf", base, i, total));
                        if let Ok(md) = sibling.symlink_metadata() {
                            if md.is_file() && !md.file_type().is_symlink() {
                                files.insert(sibling);
                            }
                        }
                    }
                }
            }
        }
        let mut models: Vec<_> = groups
            .into_iter()
            .flat_map(|(dir, files)| self.scan_files(&dir, files.into_iter().collect()))
            .collect();
        models.sort_by(|a, b| a.name.cmp(&b.name));
        models
    }

    fn scan_files(&self, dir: &Path, gguf_paths: Vec<PathBuf>) -> Vec<LocalModel> {
        let mut all_models = Vec::new();
        let split_regex = Regex::new(r"(?i)^(.*?)-(\d{5})-of-(\d{5})\.gguf$").unwrap();
        let root_dir_str = dir.to_string_lossy().to_string();
        let mut split_groups: HashMap<SplitKey, Vec<ShardEntry>> = HashMap::new();
        let mut single_files: Vec<PathBuf> = Vec::new();
        let mut ggml_files: Vec<PathBuf> = Vec::new();
        let mut image_files: Vec<PathBuf> = Vec::new();
        let mut onnx_files: Vec<PathBuf> = Vec::new();

        for path in gguf_paths {
            let filename = match path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };
            if filename.to_ascii_lowercase().ends_with(".onnx") {
                onnx_files.push(path);
                continue;
            }
            // A `.bin` is a model only when it carries the ggml magic (whisper.cpp); a
            // `.safetensors` only when its tensors are an image generator's (VAEs, text
            // encoders and language models are not listed); a `.ckpt` is an image checkpoint.
            if !filename.to_ascii_lowercase().ends_with(".gguf") {
                match crate::engine::ModelFormat::of(&path) {
                    crate::engine::ModelFormat::Safetensors => {
                        if crate::engine::is_image_safetensors(&path) {
                            image_files.push(path);
                        }
                    }
                    crate::engine::ModelFormat::Ckpt => image_files.push(path),
                    _ => {
                        if crate::engine::is_ggml_file(&path) {
                            ggml_files.push(path);
                        }
                    }
                }
                continue;
            }

            if let Some(caps) = split_regex.captures(&filename) {
                let base_name = caps.get(1).unwrap().as_str().to_string();
                let shard_idx: u32 = caps.get(2).unwrap().as_str().parse().unwrap_or(1);
                let total_shards: u32 = caps.get(3).unwrap().as_str().parse().unwrap_or(1);

                let parent_dir = path.parent().unwrap_or(Path::new("")).to_path_buf();
                let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

                split_groups
                    .entry((parent_dir, base_name, total_shards))
                    .or_default()
                    .push((shard_idx, path, size));
            } else {
                single_files.push(path);
            }
        }

        // 1. Process regular single models
        for path in single_files {
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let size_bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

            let metadata = cached_metadata(&path);
            let architecture = metadata.as_ref().and_then(|m| m.architecture.clone());
            let context_length = metadata.as_ref().and_then(|m| m.context_length);
            let quantization = metadata
                .as_ref()
                .map(|m| m.quantization.clone())
                .unwrap_or_else(|| crate::gguf::extract_quant_from_filename(&filename));

            let id = path.to_string_lossy().to_string();
            let name = filename.trim_end_matches(".gguf").to_string();
            let kind = image_aware_kind(&path, metadata.as_ref());
            // stable-diffusion.cpp's own conversions name no architecture.
            let architecture = architecture
                .or_else(|| (kind == crate::domain::ModelKind::Image).then(|| "diffusion".into()));

            all_models.push(LocalModel {
                id,
                name,
                filename,
                path: path.clone(),
                size_bytes,
                architecture,
                context_length,
                quantization,
                is_split: false,
                total_shards: None,
                present_shards: None,
                is_ready: true,
                shard_paths: vec![path],
                root_dir: root_dir_str.clone(),
                kind,
                origin: crate::domain::ModelOrigin::Library,
                task: (kind == crate::domain::ModelKind::Tts).then(|| "text-to-speech".into()),
                gguf: metadata,
                source: None,
                mmproj_path: None,
            });
        }

        // Whisper ggml models (speech recognition).
        for path in ggml_files {
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let (kind, quantization) = ggml_bin_traits(&filename);
            all_models.push(LocalModel {
                id: path.to_string_lossy().to_string(),
                name: filename.trim_end_matches(".bin").to_string(),
                size_bytes: fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
                filename,
                architecture: Some("whisper".to_string()),
                context_length: None,
                quantization,
                is_split: false,
                total_shards: None,
                present_shards: None,
                is_ready: true,
                shard_paths: vec![path.clone()],
                root_dir: root_dir_str.clone(),
                kind,
                origin: crate::domain::ModelOrigin::Library,
                task: None,
                gguf: None,
                source: None,
                mmproj_path: None,
                path,
            });
        }

        // Image models: a checkpoint or a standalone diffusion model (stable-diffusion.cpp).
        for path in image_files {
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let (name, quantization) = image_file_traits(&filename);
            all_models.push(LocalModel {
                id: path.to_string_lossy().to_string(),
                name,
                size_bytes: fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
                filename,
                architecture: Some("diffusion".to_string()),
                context_length: None,
                quantization,
                is_split: false,
                total_shards: None,
                present_shards: None,
                is_ready: true,
                shard_paths: vec![path.clone()],
                root_dir: root_dir_str.clone(),
                kind: crate::domain::ModelKind::Image,
                origin: crate::domain::ModelOrigin::Library,
                task: None,
                gguf: None,
                source: None,
                mmproj_path: None,
                path,
            });
        }
        // ONNX encoders (embedding / reranking), with their tokenizer next to them.
        all_models.extend(
            onnx_files
                .iter()
                .filter_map(|path| onnx_entry(path, &root_dir_str)),
        );

        // 2. Process split model groups
        for ((_p_dir, base_name, total_shards), mut shards) in split_groups {
            shards.sort_by_key(|s| s.0);

            let present_shards = shards.len() as u32;
            let size_bytes: u64 = shards.iter().map(|s| s.2).sum();
            let shard_paths: Vec<PathBuf> = shards.iter().map(|s| s.1.clone()).collect();

            let is_complete = present_shards == total_shards
                && shards
                    .iter()
                    .enumerate()
                    .all(|(i, s)| s.0 == (i as u32 + 1));

            let first_shard_opt = shards.iter().find(|s| s.0 == 1).map(|s| s.1.clone());
            let launch_path = first_shard_opt.unwrap_or_else(|| shard_paths[0].clone());

            let metadata = cached_metadata(&launch_path);
            let architecture = metadata.as_ref().and_then(|m| m.architecture.clone());
            let context_length = metadata.as_ref().and_then(|m| m.context_length);
            let quantization = metadata
                .as_ref()
                .map(|m| m.quantization.clone())
                .unwrap_or_else(|| crate::gguf::extract_quant_from_filename(&base_name));

            let id = launch_path.to_string_lossy().to_string();
            let filename = format!("{}-00001-of-{:05}.gguf", base_name, total_shards);

            all_models.push(LocalModel {
                id,
                name: base_name,
                filename,
                path: launch_path,
                size_bytes,
                architecture,
                context_length,
                quantization,
                is_split: true,
                total_shards: Some(total_shards),
                present_shards: Some(present_shards),
                is_ready: is_complete,
                shard_paths,
                root_dir: root_dir_str.clone(),
                kind: crate::domain::detect_kind(metadata.as_ref()),
                origin: crate::domain::ModelOrigin::Library,
                task: (crate::domain::detect_kind(metadata.as_ref())
                    == crate::domain::ModelKind::Tts)
                    .then(|| "text-to-speech".into()),
                gguf: metadata,
                source: None,
                mmproj_path: None,
            });
        }
        all_models.sort_by(|a, b| a.name.cmp(&b.name));
        all_models
    }

    fn collect_gguf_files(&self, dir: &Path, result: &mut Vec<PathBuf>) {
        const MAX_SCAN_DEPTH: usize = 8;
        let mut visited = std::collections::HashSet::new();
        if let Ok(c) = fs::canonicalize(dir) {
            visited.insert(c);
        }
        let mut stack = vec![(dir.to_path_buf(), 0usize)];

        while let Some((current_dir, current_depth)) = stack.pop() {
            let Ok(entries) = fs::read_dir(&current_dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(meta) = fs::symlink_metadata(&path) else {
                    continue;
                };
                if meta.file_type().is_symlink() {
                    // Do not follow directory symlinks/junctions.
                    // If it is a file symlink pointing to a .gguf, allow collecting it.
                    if fs::metadata(&path).map(|m| m.is_file()).unwrap_or(false)
                        && is_model_extension(&path)
                    {
                        result.push(path);
                    }
                } else if meta.is_dir() {
                    // Skip .cache directory
                    if path.file_name().and_then(|n| n.to_str()) == Some(".cache") {
                        continue;
                    }
                    let canon = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                    if !visited.insert(canon) {
                        continue;
                    }
                    if current_depth < MAX_SCAN_DEPTH {
                        stack.push((path, current_depth + 1));
                    }
                } else if is_model_extension(&path) {
                    result.push(path);
                }
            }
        }
    }

    pub fn delete_model(&self, model_id: &str) -> Result<(), String> {
        let model = self
            .find_model(model_id)
            .ok_or_else(|| String::from(err("models.not_found").with("id", model_id)))?;
        let roots = AppSettings::load().all_models_dirs();
        crate::library::ops::delete_files(&model, &roots).map(|_| ())
    }

    /// Startup hook kept for the shell: imports the pre-v2 files (presets, profiles, last
    /// session) into the domain files, moving plaintext API keys into the keychain.
    pub fn migrate_secrets() {
        match crate::domain::store::Domain::global().migrate_legacy() {
            Ok(r) => r
                .errors
                .iter()
                .for_each(|e| eprintln!("[Harness Studio] v2 taşıma: {}", e)),
            Err(e) => eprintln!("[Harness Studio] v2 taşıma: {}", e),
        }
    }

    /// Only what is really saved for the model (`None` for a model never configured): the
    /// Dashboard then recommends a GPU layer count for the hardware instead of a fixed default.
    pub fn get_saved_preset(model_id: &str) -> Option<ModelPreset> {
        match crate::domain::store::Domain::global()
            .model_preset(&crate::domain::launch::LaunchDefaults::load(), model_id)
        {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[Harness Studio] Model ayarları okunamadı: {}", e);
                None
            }
        }
    }

    /// The Dashboard's saved settings for a model (its default ModelDefinition + serve
    /// settings), or the configured defaults when there are none.
    pub fn get_preset(model_id: &str) -> Option<ModelPreset> {
        if let Some(p) = Self::get_saved_preset(model_id) {
            return Some(p);
        }
        let settings = AppSettings::load();
        Some(ModelPreset {
            gpu_layers: settings.server.default_ngl,
            context_size: settings.server.default_ctx,
            ..Default::default()
        })
    }

    /// Moves a model's saved settings to a new model id (after the file was moved).
    pub fn rename_preset(old_id: &str, new_id: &str) {
        let result =
            crate::domain::store::Domain::global().rename_model(old_id, new_id, |old_ref| {
                let old_ref = old_ref?;
                let new_ref = crate::secrets::preset_ref(new_id);
                let moved =
                    crate::secrets::adopt_api_key(&new_ref, None, Some(old_ref)).unwrap_or(None);
                if old_ref != new_ref {
                    crate::secrets::api_key_delete(old_ref);
                }
                moved
            });
        if let Err(e) = result {
            eprintln!("[Harness Studio] Model ayarları taşınamadı: {}", e);
        }
    }

    /// "Save as default" / "Save as new definition" from the Dashboard form (see
    /// `Domain::save_form`); a typed API key moves to the keychain like `save_preset`.
    pub fn save_definition_form(
        target: Option<&str>,
        new_alias: Option<&str>,
        mut config: crate::process::LaunchConfig,
    ) -> Result<crate::domain::ModelDefinition, String> {
        let model_id = config.model_path.to_string_lossy().to_string();
        config.api_key_ref = crate::secrets::adopt_api_key(
            &crate::secrets::preset_ref(&model_id),
            config.api_key.as_deref(),
            config.api_key_ref.as_deref(),
        )?;
        config.api_key = None;
        crate::domain::store::Domain::global().save_form(target, new_alias, &config)
    }

    pub fn save_preset(model_id: &str, mut preset: ModelPreset) -> Result<(), String> {
        preset.api_key_ref = crate::secrets::adopt_api_key(
            &crate::secrets::preset_ref(model_id),
            preset.api_key.as_deref(),
            preset.api_key_ref.as_deref(),
        )?;
        preset.api_key = None;
        crate::domain::store::Domain::global().save_model_preset(model_id, &preset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_split_gguf_grouping() {
        let temp_dir = std::env::temp_dir().join("harness_split_test");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // 1. Create a complete split set of 3 shards
        for i in 1..=3 {
            let shard_name = format!("Llama-70B-Q4_K_M-0000{}-of-00003.gguf", i);
            let mut f = fs::File::create(temp_dir.join(&shard_name)).unwrap();
            f.write_all(b"GGUF").unwrap();
            f.write_all(&3u32.to_le_bytes()).unwrap();
            f.write_all(&0u64.to_le_bytes()).unwrap();
            f.write_all(&0u64.to_le_bytes()).unwrap();
        }

        // 2. Create an incomplete split set (only shard 1 of 2)
        {
            let mut f =
                fs::File::create(temp_dir.join("Incomplete-Model-00001-of-00002.gguf")).unwrap();
            f.write_all(b"GGUF").unwrap();
            f.write_all(&3u32.to_le_bytes()).unwrap();
            f.write_all(&0u64.to_le_bytes()).unwrap();
            f.write_all(&0u64.to_le_bytes()).unwrap();
        }

        // 3. Create a single non-split model
        {
            let mut f = fs::File::create(temp_dir.join("Mistral-7B-Q4_K_M.gguf")).unwrap();
            f.write_all(b"GGUF").unwrap();
            f.write_all(&3u32.to_le_bytes()).unwrap();
            f.write_all(&0u64.to_le_bytes()).unwrap();
            f.write_all(&0u64.to_le_bytes()).unwrap();
        }

        let models = ModelManager.scan_directories(std::slice::from_ref(&temp_dir));

        assert_eq!(models.len(), 3);

        // Verify complete split model
        let split_complete = models
            .iter()
            .find(|m| m.name == "Llama-70B-Q4_K_M")
            .unwrap();
        assert!(split_complete.is_split);
        assert_eq!(split_complete.total_shards, Some(3));
        assert_eq!(split_complete.present_shards, Some(3));
        assert!(split_complete.is_ready);
        assert_eq!(split_complete.shard_paths.len(), 3);
        assert!(split_complete
            .path
            .to_string_lossy()
            .contains("-00001-of-00003"));

        // Verify incomplete split model
        let split_incomplete = models
            .iter()
            .find(|m| m.name == "Incomplete-Model")
            .unwrap();
        assert!(split_incomplete.is_split);
        assert_eq!(split_incomplete.total_shards, Some(2));
        assert_eq!(split_incomplete.present_shards, Some(1));
        assert!(!split_incomplete.is_ready); // NOT READY!

        // Verify single model
        let single = models
            .iter()
            .find(|m| m.name == "Mistral-7B-Q4_K_M")
            .unwrap();
        assert!(!single.is_split);
        assert!(single.is_ready);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    /// Removes the legacy backups this test left in the shared test data root.
    fn remove_backups(prefix: &str) {
        for e in fs::read_dir(crate::paths::config_dir()).unwrap().flatten() {
            if e.file_name().to_string_lossy().starts_with(prefix) {
                let _ = fs::remove_file(e.path());
            }
        }
    }

    /// True if any file in the (test) config dir contains `needle`.
    fn config_files_contain(needle: &str) -> bool {
        fs::read_dir(crate::paths::config_dir())
            .unwrap()
            .flatten()
            .filter(|e| e.path().is_file())
            .any(|e| {
                fs::read_to_string(e.path())
                    .map(|t| t.contains(needle))
                    .unwrap_or(false)
            })
    }

    /// A never-configured model has no saved preset (the Dashboard then recommends -ngl), while
    /// `get_preset` still falls back to the configured defaults for other callers.
    #[test]
    fn test_saved_preset_is_none_without_settings() {
        let model_id = "never_configured_model_xyz.gguf";
        assert!(ModelManager::get_saved_preset(model_id).is_none());
        let fallback = ModelManager::get_preset(model_id).unwrap();
        assert_eq!(
            fallback.gpu_layers,
            crate::config::AppSettings::load().server.default_ngl
        );
    }

    #[test]
    fn test_model_preset_save_load() {
        let model_id = "test_model_123";
        let preset = ModelPreset {
            gpu_layers: 45,
            context_size: 16384,
            threads: Some(12),
            ..Default::default()
        };

        ModelManager::save_preset(model_id, preset.clone()).unwrap();
        let loaded = ModelManager::get_preset(model_id).unwrap();

        assert_eq!(loaded.gpu_layers, 45);
        assert_eq!(loaded.context_size, 16384);
        assert_eq!(loaded.threads, Some(12));

        // API key: typed once -> keychain + reference; never in the file or in serialized JSON.
        let key_model = "test_model_with_key";
        ModelManager::save_preset(
            key_model,
            ModelPreset {
                api_key: Some("preset-secret-1".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        let loaded = ModelManager::get_preset(key_model).unwrap();
        let owner = crate::secrets::preset_ref(key_model);
        assert_eq!(loaded.api_key, None);
        assert_eq!(loaded.api_key_ref.as_deref(), Some(owner.as_str()));
        assert_eq!(
            crate::secrets::api_key_get(&owner).as_deref(),
            Some("preset-secret-1")
        );
        assert!(!serde_json::to_string(&loaded)
            .unwrap()
            .contains("preset-secret-1"));
        assert!(!config_files_contain("preset-secret-1"));
        // Saving again without a new key (UI sends the reference back) keeps the key.
        ModelManager::save_preset(key_model, loaded.clone()).unwrap();
        assert!(crate::secrets::api_key_get(&owner).is_some());

        // Legacy plaintext preset file is migrated on load.
        let legacy_model = "test_model_legacy_key";
        fs::write(
            crate::paths::presets_file(),
            format!(
                r#"{{"{legacy_model}":{{"gpu_layers":1,"context_size":512,"api_key":"legacy-secret-2"}}}}"#
            ),
        )
        .unwrap();
        let migrated = ModelManager::get_preset(legacy_model).unwrap();
        assert_eq!(migrated.api_key, None);
        let legacy_ref = crate::secrets::preset_ref(legacy_model);
        assert_eq!(migrated.api_key_ref.as_deref(), Some(legacy_ref.as_str()));
        assert_eq!(
            crate::secrets::api_key_get(&legacy_ref).as_deref(),
            Some("legacy-secret-2")
        );
        // The legacy file was moved aside (`presets.json.pre-v2*`) without the plaintext key.
        assert!(!crate::paths::presets_file().exists());
        assert!(!config_files_contain("legacy-secret-2"));

        // Rename moves the key with the preset.
        ModelManager::rename_preset(legacy_model, "test_model_renamed");
        let new_ref = crate::secrets::preset_ref("test_model_renamed");
        assert_eq!(
            crate::secrets::api_key_get(&new_ref).as_deref(),
            Some("legacy-secret-2")
        );
        assert!(crate::secrets::api_key_get(&legacy_ref).is_none());
        crate::secrets::api_key_delete(&new_ref);
        crate::secrets::api_key_delete(&owner);

        // Cleanup (presets live in the isolated test data root)
        let _ = fs::remove_file(crate::paths::presets_file());
        remove_backups("presets.json.pre-v2");
    }

    #[test]
    fn test_collect_gguf_files_depth_and_cache() {
        let tmp = tempfile::tempdir().unwrap();
        // Create 10 nested levels with a .gguf file at each level
        let mut curr = tmp.path().to_path_buf();
        for i in 1..=10 {
            curr = curr.join(format!("depth_{}", i));
            fs::create_dir_all(&curr).unwrap();
            fs::write(curr.join("model.gguf"), b"GGUF").unwrap();
        }

        // Add a .cache directory with a .gguf file (must be ignored)
        let cache_dir = tmp.path().join(".cache");
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(cache_dir.join("cached.gguf"), b"GGUF").unwrap();

        let mut collected = Vec::new();
        ModelManager.collect_gguf_files(tmp.path(), &mut collected);

        // Max depth is 8, so depths 1..=8 are collected = 8 files.
        // Depths 9 and 10 must not be collected, and .cache must be skipped.
        assert_eq!(collected.len(), 8);
        assert!(!collected
            .iter()
            .any(|p| p.to_string_lossy().contains(".cache")));
    }

    /// Whisper ggml models are `.bin` files: only those with the ggml magic are models.
    #[test]
    fn scans_whisper_ggml_models() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("ggerganov").join("whisper.cpp");
        fs::create_dir_all(&dir).unwrap();
        let ggml = |name: &str, extra: usize| {
            let mut b = vec![0x6c, 0x6d, 0x67, 0x67];
            b.extend(vec![0u8; extra]);
            fs::write(dir.join(name), b).unwrap();
        };
        ggml("ggml-base.bin", 100);
        ggml("ggml-large-v3-turbo-q5_0.bin", 200);
        ggml("ggml-silero-v5.1.2.bin", 10);
        fs::write(dir.join("weights.bin"), b"not a ggml file").unwrap();
        fs::write(dir.join("model.gguf"), b"GGUF").unwrap();

        let models = ModelManager.scan_directories(&[tmp.path().to_path_buf()]);
        let by = |name: &str| models.iter().find(|m| m.name == name);
        assert!(
            by("weights").is_none(),
            "a .bin without the magic is no model"
        );
        let base = by("ggml-base").unwrap();
        assert_eq!(base.kind, crate::domain::ModelKind::Asr);
        assert_eq!(base.quantization, "F16");
        assert_eq!(base.architecture.as_deref(), Some("whisper"));
        assert_eq!(base.size_bytes, 104);
        assert!(base.is_ready && !base.is_split && base.gguf.is_none());
        let turbo = by("ggml-large-v3-turbo-q5_0").unwrap();
        assert_eq!(turbo.kind, crate::domain::ModelKind::Asr);
        assert_eq!(turbo.quantization, "Q5_0");
        // The VAD model shares the container but is not a speech recognizer.
        assert_eq!(
            by("ggml-silero-v5.1.2").unwrap().kind,
            crate::domain::ModelKind::Unknown
        );
        assert!(by("model").is_some(), "GGUF files are still found");
    }

    /// A folder with `genai_config.json` is a chat model (the decoder graph is listed, named
    /// after repo and variant folder, sized with its external data); its tokenizer does not make
    /// it an embedder, and the other graphs of the folder are not models.
    #[test]
    fn scans_onnxruntime_genai_chat_models() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp
            .path()
            .join("microsoft")
            .join("Phi-3-mini-4k-instruct-onnx");
        let dml = repo.join("directml").join("directml-int4-awq-block-128");
        fs::create_dir_all(&dml).unwrap();
        fs::write(
            dml.join("genai_config.json"),
            r#"{"model":{"type":"phi3","context_length":4096,"decoder":{"filename":"model.onnx",
               "session_options":{"provider_options":[{"dml":{}}]}}}}"#,
        )
        .unwrap();
        fs::write(dml.join("tokenizer.json"), "{}").unwrap();
        fs::write(dml.join("model.onnx"), vec![0u8; 1000]).unwrap();
        fs::write(dml.join("model.onnx.data"), vec![0u8; 5000]).unwrap();
        fs::write(dml.join("vision.onnx"), vec![0u8; 77]).unwrap();
        // A vision-language genai model is not a chat model here.
        let vlm = tmp.path().join("microsoft").join("Phi-3-vision");
        fs::create_dir_all(&vlm).unwrap();
        fs::write(
            vlm.join("genai_config.json"),
            r#"{"model":{"type":"phi3v","vision":{"filename":"v.onnx"},"decoder":{"filename":"model.onnx"}}}"#,
        )
        .unwrap();
        fs::write(vlm.join("tokenizer.json"), "{}").unwrap();
        fs::write(vlm.join("model.onnx"), vec![0u8; 10]).unwrap();

        let models = ModelManager.scan_directories(&[tmp.path().to_path_buf()]);
        assert_eq!(
            models.len(),
            1,
            "{:?}",
            models.iter().map(|m| &m.name).collect::<Vec<_>>()
        );
        let m = &models[0];
        assert_eq!(
            m.name,
            "Phi-3-mini-4k-instruct-onnx-directml-int4-awq-block-128"
        );
        assert_eq!(m.kind, crate::domain::ModelKind::Llm);
        assert_eq!(m.size_bytes, 6000);
        assert_eq!(m.quantization, "INT4");
        assert_eq!(m.context_length, Some(4096));
        assert!(m.path.ends_with("model.onnx") && m.gguf.is_none());
        assert_eq!(
            crate::hub_compat::ModelFormat::Onnx.runtime(),
            Some(crate::onnx_runtime::ONNX_FAMILY)
        );
        assert_eq!(
            crate::engine::ModelFormat::of(&m.path),
            crate::engine::ModelFormat::Onnx
        );
    }

    /// ONNX files are models only next to a tokenizer: an encoder repo shows its variants as
    /// embedding / reranking models, any other graph stays out of the library.
    #[test]
    fn scans_onnx_encoders() {
        let tmp = tempfile::tempdir().unwrap();
        let st = tmp
            .path()
            .join("sentence-transformers")
            .join("all-MiniLM-L6-v2");
        fs::create_dir_all(st.join("onnx")).unwrap();
        fs::write(st.join("tokenizer.json"), "{}").unwrap();
        fs::write(
            st.join("modules.json"),
            r#"[{"idx":0,"path":"","type":"sentence_transformers.models.Transformer"},
               {"idx":1,"path":"1_Pooling","type":"sentence_transformers.models.Pooling"}]"#,
        )
        .unwrap();
        fs::write(
            st.join("sentence_bert_config.json"),
            r#"{"max_seq_length":256}"#,
        )
        .unwrap();
        fs::write(st.join("onnx").join("model.onnx"), vec![0u8; 1000]).unwrap();
        fs::write(st.join("onnx").join("model_quantized.onnx"), vec![0u8; 300]).unwrap();

        let ce = tmp
            .path()
            .join("cross-encoder")
            .join("ms-marco-MiniLM-L6-v2");
        fs::create_dir_all(ce.join("onnx")).unwrap();
        fs::write(ce.join("tokenizer.json"), "{}").unwrap();
        fs::write(
            ce.join("config.json"),
            r#"{"architectures":["BertForSequenceClassification"]}"#,
        )
        .unwrap();
        fs::write(ce.join("onnx").join("model.onnx"), vec![0u8; 500]).unwrap();

        // A vision graph without tokenizer.
        let vision = tmp.path().join("acme").join("resnet");
        fs::create_dir_all(&vision).unwrap();
        fs::write(vision.join("model.onnx"), vec![0u8; 50]).unwrap();

        let models = ModelManager.scan_directories(&[tmp.path().to_path_buf()]);
        let by = |name: &str| models.iter().find(|m| m.name == name);
        let base = by("all-MiniLM-L6-v2").unwrap();
        assert_eq!(base.kind, crate::domain::ModelKind::Embedding);
        assert_eq!(base.quantization, "FP32");
        assert_eq!(base.context_length, Some(256));
        assert_eq!(base.size_bytes, 1000);
        assert_eq!(base.architecture.as_deref(), Some("onnx"));
        assert!(base.path.ends_with("model.onnx") && base.gguf.is_none());
        let quant = by("all-MiniLM-L6-v2-quantized").unwrap();
        assert_eq!(quant.quantization, "INT8");
        assert_eq!(quant.kind, crate::domain::ModelKind::Embedding);
        let rerank = by("ms-marco-MiniLM-L6-v2").unwrap();
        assert_eq!(rerank.kind, crate::domain::ModelKind::Reranker);
        assert_eq!(models.len(), 3, "the tokenizer-less graph is no model");
        assert_eq!(
            crate::model_registry::local_format(rerank),
            crate::hub_compat::ModelFormat::Onnx
        );
    }

    #[test]
    fn scans_onnx_vision_models() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("Xenova").join("detr-resnet-50");
        fs::create_dir_all(repo.join("onnx")).unwrap();
        fs::write(
            repo.join("config.json"),
            r#"{"architectures":["DetrForObjectDetection"],"id2label":{"0":"N/A","1":"person"}}"#,
        )
        .unwrap();
        fs::write(
            repo.join("preprocessor_config.json"),
            r#"{"size":{"shortest_edge":800,"longest_edge":1333}}"#,
        )
        .unwrap();
        let graph = repo.join("onnx").join("model_quantized.onnx");
        fs::write(&graph, vec![0u8; 400]).unwrap();
        let models = ModelManager.scan_directories(&[tmp.path().to_path_buf()]);
        assert_eq!(models.len(), 1);
        let m = &models[0];
        assert_eq!(m.kind, crate::domain::ModelKind::Pipeline);
        assert_eq!(m.task.as_deref(), Some("object-detection"));
        assert_eq!(m.name, "detr-resnet-50-quantized");
        assert_eq!(m.quantization, "INT8");
        assert_eq!(onnx_kind(&graph), Some(crate::domain::ModelKind::Pipeline));
    }

    #[test]
    fn scans_an_onnx_captioner_as_one_model() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("Xenova").join("vit-gpt2-image-captioning");
        let onnx = repo.join("onnx");
        fs::create_dir_all(&onnx).unwrap();
        fs::write(
            repo.join("config.json"),
            r#"{"architectures":["VisionEncoderDecoderModel"],"decoder_start_token_id":50256}"#,
        )
        .unwrap();
        fs::write(repo.join("preprocessor_config.json"), r#"{"size":224}"#).unwrap();
        fs::write(repo.join("tokenizer.json"), "{}").unwrap();
        fs::write(onnx.join("encoder_model.onnx"), vec![0u8; 300]).unwrap();
        fs::write(onnx.join("decoder_model_merged.onnx"), vec![0u8; 500]).unwrap();
        let models = ModelManager.scan_directories(&[tmp.path().to_path_buf()]);
        assert_eq!(models.len(), 1, "the decoder belongs to the encoder");
        let m = &models[0];
        assert_eq!(m.task.as_deref(), Some("image-to-text"));
        assert_eq!(m.size_bytes, 800);
    }

    #[test]
    fn scans_an_onnx_translator_as_one_model() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("Xenova").join("opus-mt-en-de");
        let onnx = repo.join("onnx");
        fs::create_dir_all(&onnx).unwrap();
        fs::write(
            repo.join("config.json"),
            r#"{"architectures":["MarianMTModel"],"model_type":"marian","decoder_start_token_id":58100}"#,
        )
        .unwrap();
        fs::write(repo.join("tokenizer.json"), "{}").unwrap();
        fs::write(onnx.join("encoder_model_quantized.onnx"), vec![0u8; 300]).unwrap();
        fs::write(
            onnx.join("decoder_model_merged_quantized.onnx"),
            vec![0u8; 500],
        )
        .unwrap();
        let models = ModelManager.scan_directories(&[tmp.path().to_path_buf()]);
        assert_eq!(models.len(), 1, "the decoder belongs to the encoder");
        let m = &models[0];
        assert_eq!(m.kind, crate::domain::ModelKind::Pipeline);
        assert_eq!(m.task.as_deref(), Some("translation"));
        assert_eq!(m.size_bytes, 800);
    }

    #[test]
    fn scans_onnx_audio_classifiers() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("Xenova").join("ast-finetuned-audioset");
        fs::create_dir_all(repo.join("onnx")).unwrap();
        fs::write(
            repo.join("config.json"),
            r#"{"architectures":["ASTForAudioClassification"],"id2label":{"0":"Speech"}}"#,
        )
        .unwrap();
        fs::write(
            repo.join("preprocessor_config.json"),
            r#"{"feature_extractor_type":"ASTFeatureExtractor","sampling_rate":16000}"#,
        )
        .unwrap();
        let graph = repo.join("onnx").join("model.onnx");
        fs::write(&graph, vec![0u8; 400]).unwrap();
        let models = ModelManager.scan_directories(&[tmp.path().to_path_buf()]);
        assert_eq!(models.len(), 1);
        let m = &models[0];
        assert_eq!(m.kind, crate::domain::ModelKind::Pipeline);
        assert_eq!(m.task.as_deref(), Some("audio-classification"));
        assert_eq!(m.name, "ast-finetuned-audioset");
        assert_eq!(onnx_kind(&graph), Some(crate::domain::ModelKind::Pipeline));
    }
}
