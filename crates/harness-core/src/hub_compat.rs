//! What Harness can do with a Hugging Face repo, from the listing's own fields (no extra
//! request): the model file format and the runtime family that runs it.
//!
//! The Hub is no longer limited to `filter=gguf` (`HfSearchOptions::include_non_gguf`), so a
//! listing mixes repos Harness runs (GGUF on llama.cpp, whisper ggml `.bin` on whisper.cpp) with
//! repos no Harness runtime runs (safetensors, diffusers...). [`detect_format`] reads the
//! format from `tags`, `library_name` and the `siblings` file names; [`Compatibility`] says what
//! follows from it.

use serde::{Deserialize, Serialize};

/// Repo metadata can identify audio.cpp before any GGUF extension fallback.
pub fn is_audio_cpp_repo(item: &serde_json::Value) -> bool {
    let marker = |s: &str| matches!(s.to_ascii_lowercase().as_str(), "audio.cpp" | "audiocpp");
    matches!(
        item["id"].as_str(),
        Some("audio-cpp/audio.cpp-gguf" | "audio-cpp/SAM-Audio-GGUF")
    ) || item["library_name"].as_str().is_some_and(marker)
        || item["cardData"]["library_name"]
            .as_str()
            .is_some_and(marker)
        || item["gguf"]["architecture"] == "audiocpp"
        || item["gguf"]["general.architecture"] == "audiocpp"
        || item["tags"]
            .as_array()
            .is_some_and(|tags| tags.iter().any(|v| v.as_str().is_some_and(marker)))
}

pub fn audio_cpp_repo_tts(item: &serde_json::Value) -> bool {
    if audio_cpp_custom_code(item) {
        return false;
    }
    if let Some(family) = item["gguf"]["audiocpp.model_spec.family"]
        .as_str()
        .or_else(|| item["gguf"]["family"].as_str())
    {
        return crate::engine::audio_cpp::task(family) == Some("tts");
    }
    item["id"] == "audio-cpp/audio.cpp-gguf"
        || item["pipeline_tag"] == "text-to-speech"
        || item["cardData"]["pipeline_tag"] == "text-to-speech"
        || item["tags"]
            .as_array()
            .is_some_and(|tags| tags.iter().any(|v| v == "text-to-speech"))
}

/// Requires audio.cpp provenance; a generic audio-to-audio tag never captures other engines.
pub fn audio_cpp_repo_processing(item: &serde_json::Value) -> bool {
    if !is_audio_cpp_repo(item) || audio_cpp_custom_code(item) {
        return false;
    }
    if item["id"] == "audio-cpp/SAM-Audio-GGUF" {
        return true;
    }
    item["gguf"]["audiocpp.model_spec.family"]
        .as_str()
        .or_else(|| item["gguf"]["family"].as_str())
        .is_some_and(|f| crate::engine::audio_cpp::processing_task(f).is_some())
}

fn audio_cpp_custom_code(item: &serde_json::Value) -> bool {
    [&item["tags"], &item["cardData"]["tags"]]
        .into_iter()
        .any(|tags| {
            tags.as_array()
                .is_some_and(|t| t.iter().any(|v| v == "custom_code"))
        })
}

pub fn audio_cpp_compatibility(item: &serde_json::Value) -> Compatibility {
    if audio_cpp_repo_tts(item) || audio_cpp_repo_processing(item) {
        Compatibility::Runnable {
            runtime: crate::engine::AUDIO_CPP.into(),
        }
    } else {
        Compatibility::Unsupported {
            format: ModelFormat::Gguf,
        }
    }
}

/// Runtime family that runs GGUF models.
pub const LLAMA_FAMILY: &str = "llama.cpp";
/// Runtime family that runs whisper ggml models.
pub const WHISPER_FAMILY: &str = "whisper.cpp";
/// Runtime family that runs image / video generation models (safetensors, `.ckpt` and GGUF
/// diffusion models).
pub const SDCPP_FAMILY: &str = "stable-diffusion.cpp";
/// Runtime family that runs text-to-speech models (Kokoro, Piper) through sherpa-onnx.
pub const SHERPA_FAMILY: &str = "sherpa-onnx";
/// Runtime family that runs ONNX encoders (`harness-onnx`: sentence embedders and
/// cross-encoder rerankers on ONNX Runtime).
pub const ONNX_FAMILY: &str = crate::onnx_runtime::ONNX_FAMILY;
/// Runtime family that runs safetensors checkpoints of chat models (a repo folder with
/// `config.json`), optional: the user builds it and registers its folder.
pub const MISTRALRS_FAMILY: &str = crate::engine::MISTRALRS;
/// Hub tasks whose safetensors repos mistral.rs serves: chat models, with or without vision.
pub const MISTRALRS_TASKS: [&str; 2] = ["text-generation", "image-text-to-text"];
/// Hub tasks (`pipeline_tag`) whose ONNX repos the ONNX family runs: an ONNX file of any other
/// task (vision, speech, generation, classification) is a graph Harness has no server for.
pub const ONNX_TASKS: [&str; 20] = [
    "feature-extraction",
    "sentence-similarity",
    "text-ranking",
    // Encoder task heads served on `/hf` (harness-onnx `Task::Classify`...).
    "text-classification",
    "zero-shot-classification",
    "token-classification",
    "fill-mask",
    "question-answering",
    // Vision heads (harness-onnx `vision_pkg`): `pixel_values` in, the pipeline's JSON out.
    "image-classification",
    "object-detection",
    "image-segmentation",
    "depth-estimation",
    // Dual encoders (CLIP, SigLIP): the image scored against the labels of the request.
    "zero-shot-image-classification",
    // Audio heads (harness-onnx `audio_pkg`): a WAV in, ranked labels out.
    "audio-classification",
    // Bare image backbones (DINOv2, ViT...): the hidden states as features.
    "image-feature-extraction",
    // Image captioners (`VisionEncoderDecoderModel`): an encoder and a text decoder.
    "image-to-text",
    // Open-vocabulary detectors (OWL-ViT, OWLv2): boxes for the labels of the request.
    "zero-shot-object-detection",
    // Text encoder-decoders (T5, BART, Marian): an encoder and a text decoder.
    "summarization",
    "translation",
    "text2text-generation",
];

/// Hub tasks of the onnxruntime-genai chat models the ONNX family runs (a repo with
/// `genai_config.json`); an untagged repo (`None`) counts, the file is the evidence.
pub const GENAI_TASKS: [&str; 3] = ["text-generation", "conversational", "text2text-generation"];

/// The format a repo's model files are in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ModelFormat {
    Gguf,
    /// whisper.cpp's `ggml-*.bin`.
    WhisperGgml,
    Safetensors,
    Onnx,
    /// Diffusers pipeline (a folder of components, `model_index.json`).
    Diffusers,
    /// `pytorch_model.bin`, `.pt`, `.pth`, `.ckpt`.
    Pytorch,
    Mlx,
    /// CTranslate2 (faster-whisper and similar; its weights are a `model.bin`).
    Ctranslate2,
    /// Core ML packages (`.mlmodelc`, `.mlpackage`).
    CoreMl,
    /// Nothing the listing shows says what the files are.
    Unknown,
}

/// Whether Harness can run a repo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum Compatibility {
    /// An installed (or always available) runtime family runs it.
    Runnable { runtime: String },
    /// The family that runs it is not installed yet.
    NeedsRuntime { runtime: String },
    /// The family that runs it has no build for this OS / architecture (ONNX Runtime on an
    /// Intel Mac): nothing to install, the model cannot run here.
    UnsupportedPlatform {
        runtime: String,
        os: String,
        arch: String,
    },
    /// No Harness runtime runs this format.
    Unsupported { format: ModelFormat },
}

impl ModelFormat {
    /// The runtime family that runs this format, `None` when no Harness runtime does.
    pub fn runtime(self) -> Option<&'static str> {
        match self {
            ModelFormat::Gguf => Some(LLAMA_FAMILY),
            ModelFormat::WhisperGgml => Some(WHISPER_FAMILY),
            ModelFormat::Onnx => Some(ONNX_FAMILY),
            _ => None,
        }
    }

    /// Compatibility before looking at what is installed: the whisper.cpp runtime is an
    /// optional download, so [`resolve_installed`] decides between runnable and needs-runtime.
    /// An ONNX file is runnable here (a local file the library found next to its tokenizer);
    /// for a Hub repo use [`hub_compatibility`], which also looks at the task.
    pub fn compatibility(self) -> Compatibility {
        match self.runtime() {
            Some(runtime) => Compatibility::Runnable {
                runtime: runtime.to_string(),
            },
            None => Compatibility::Unsupported { format: self },
        }
    }
}

/// Turns a runnable verdict into [`Compatibility::NeedsRuntime`] when its family is not among
/// the `installed` runtime families. llama.cpp and the ONNX family are never reported missing:
/// the official runtime is managed and fetched on first use. An ONNX verdict on a platform
/// without any ONNX Runtime build becomes [`Compatibility::UnsupportedPlatform`].
pub fn resolve_installed(compat: &mut Compatibility, installed: &[String]) {
    resolve_installed_on(compat, installed, crate::onnx_runtime::supported());
}

/// [`resolve_installed`] with the platform fact given: `onnx_supported` is
/// [`crate::onnx_runtime::supported`].
pub fn resolve_installed_on(
    compat: &mut Compatibility,
    installed: &[String],
    onnx_supported: bool,
) {
    if let Compatibility::Runnable { runtime } = compat {
        if runtime == ONNX_FAMILY && !onnx_supported {
            *compat = Compatibility::UnsupportedPlatform {
                runtime: std::mem::take(runtime),
                os: std::env::consts::OS.to_string(),
                arch: std::env::consts::ARCH.to_string(),
            };
            return;
        }
        // The ONNX sidecar ships with the app and its ONNX Runtime library is fetched on first
        // use, like the official llama.cpp runtime.
        if runtime != LLAMA_FAMILY
            && runtime != ONNX_FAMILY
            && (!installed.iter().any(|f| f == runtime)
                || (runtime == crate::engine::AUDIO_CPP
                    && !crate::config::AppSettings::load().audio_cpp_enabled))
        {
            *compat = Compatibility::NeedsRuntime {
                runtime: std::mem::take(runtime),
            };
        }
    }
}

fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase()
}

/// Format of a repo from its tags, `library_name` and file names (`files` may be empty when the
/// response carried no `siblings`; the tags then decide). GGUF wins over everything else: a
/// repo that ships a `.gguf` next to its original weights is run as GGUF.
pub fn detect_format(tags: &[String], library: Option<&str>, files: &[String]) -> ModelFormat {
    let names: Vec<String> = files.iter().map(|f| file_name(f)).collect();
    let has_ext = |ext: &str| names.iter().any(|n| n.ends_with(ext));
    let tag = |wanted: &str| tags.iter().any(|t| t.eq_ignore_ascii_case(wanted));
    let library = library.map(str::to_ascii_lowercase);
    let library = library.as_deref();

    if has_ext(".gguf") || tag("gguf") {
        return ModelFormat::Gguf;
    }
    if names
        .iter()
        .any(|n| n.starts_with("ggml-") && n.ends_with(".bin"))
    {
        return ModelFormat::WhisperGgml;
    }
    if library == Some("diffusers")
        || tag("diffusers")
        || names.iter().any(|n| n == "model_index.json")
    {
        return ModelFormat::Diffusers;
    }
    if matches!(library, Some("mlx") | Some("mlx-audio")) || tag("mlx") || tag("mlx-audio") {
        return ModelFormat::Mlx;
    }
    if library == Some("ctranslate2") || tag("ctranslate2") {
        return ModelFormat::Ctranslate2;
    }
    if library == Some("coreml")
        || tag("coreml")
        || names
            .iter()
            .any(|n| n.ends_with(".mlmodelc") || n.ends_with(".mlpackage"))
    {
        return ModelFormat::CoreMl;
    }
    // Original weights win over exports: a transformers repo with an `onnx/` folder is safetensors.
    if library == Some("onnx") || library == Some("transformers.js") {
        return ModelFormat::Onnx;
    }
    if has_ext(".safetensors") || tag("safetensors") {
        return ModelFormat::Safetensors;
    }
    if has_ext(".onnx") || tag("onnx") {
        return ModelFormat::Onnx;
    }
    if has_ext(".bin") || has_ext(".pt") || has_ext(".pth") || has_ext(".ckpt") || tag("pytorch") {
        return ModelFormat::Pytorch;
    }
    ModelFormat::Unknown
}

/// Whether a Hub repo is made for onnxruntime-genai: a `genai_config.json` in its listing (one
/// per variant folder, or at the root). The ONNX family serves it as a chat model.
pub fn is_genai_repo(tags: &[String], files: &[String]) -> bool {
    files.iter().any(|f| file_name(f) == "genai_config.json")
        || tags.iter().any(|t| {
            t.eq_ignore_ascii_case("onnxruntime-genai")
                || t.eq_ignore_ascii_case("onnxruntime_genai")
        })
}

/// CTC evidence from config.json (not a repo-name guess).
pub fn is_ctc_config(config: &serde_json::Value) -> bool {
    harness_onnx::asr_ctc::is_ctc_config(config)
}

/// The files required by an exported HF CTC package.
pub fn has_ctc_files(files: &[String]) -> bool {
    ["config.json", "vocab.json", "preprocessor_config.json"]
        .iter()
        .all(|f| files.iter().any(|p| p == f))
        && files.iter().any(|f| {
            let graph = f.strip_prefix("onnx/").unwrap_or(f);
            graph.ends_with(".onnx") && !graph.contains(['/', '\\'])
        })
}

/// Whether the sidecar serves the ONNX repo of `task` whose `config` (`config.json` as the Hub
/// listing gives it) is this: the generative tasks are served for some architectures only (T5 /
/// BART / Marian, not mBART / M2M100 / NLLB; ViT + GPT-2 captioners, not BLIP / GIT). Without a
/// config, or for the other tasks, the task alone decides.
pub fn onnx_config_served(task: Option<&str>, config: Option<&serde_json::Value>) -> bool {
    let Some(config) = config else {
        return true;
    };
    let architectures = || -> Vec<&str> {
        config
            .get("architectures")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default()
    };
    match task {
        Some("summarization" | "translation" | "text2text-generation") => {
            harness_onnx::seq2seq_pkg::is_seq2seq_config(config)
        }
        Some("image-to-text") => architectures().contains(&"VisionEncoderDecoderModel"),
        _ => true,
    }
}

/// Whether `task` is one the ONNX family serves ([`ONNX_TASKS`]).
pub fn is_onnx_task(task: Option<&str>) -> bool {
    task.is_some_and(|t| ONNX_TASKS.contains(&t))
}

/// Format of a Hub repo, with its task: [`detect_format`], except that an embedding or
/// reranking repo that ships an ONNX export next to its original weights (every
/// sentence-transformers repo with an `onnx/` folder) is [`ModelFormat::Onnx`], because that is
/// the file Harness runs. The `onnx` tag (or an `.onnx` file in the listing) is the evidence.
pub fn detect_hub_format(
    tags: &[String],
    library: Option<&str>,
    files: &[String],
    task: Option<&str>,
) -> ModelFormat {
    let format = detect_format(tags, library, files);
    // A genai repo is ONNX whatever else the listing shows (a custom-code tag, no `onnx` tag).
    if is_genai_repo(tags, files)
        && matches!(
            format,
            ModelFormat::Safetensors | ModelFormat::Pytorch | ModelFormat::Unknown
        )
    {
        return ModelFormat::Onnx;
    }
    let has_export = tags.iter().any(|t| t.eq_ignore_ascii_case("onnx"))
        || files.iter().any(|f| file_name(f).ends_with(".onnx"));
    match format {
        ModelFormat::Safetensors | ModelFormat::Pytorch | ModelFormat::Unknown
            if has_export && is_onnx_task(task) =>
        {
            ModelFormat::Onnx
        }
        other => other,
    }
}

/// [`hub_compatibility`] for a repo that may be an onnxruntime-genai chat model (`genai`, see
/// [`is_genai_repo`]): a text-generation (or untagged) genai repo is runnable by the ONNX family.
pub fn hub_compatibility_for(
    format: ModelFormat,
    task: Option<&str>,
    genai: bool,
) -> Compatibility {
    if genai && format == ModelFormat::Onnx && task.is_none_or(|t| GENAI_TASKS.contains(&t)) {
        return format.compatibility();
    }
    hub_compatibility(format, task)
}

/// What Harness can do with a Hub repo of `format` and `task`. ONNX is runnable only for the
/// encoder tasks in [`ONNX_TASKS`]; a safetensors-only embedder stays unsupported (no ONNX
/// export to run).
pub fn hub_compatibility(format: ModelFormat, task: Option<&str>) -> Compatibility {
    if format == ModelFormat::Onnx && !is_onnx_task(task) {
        return Compatibility::Unsupported { format };
    }
    if format == ModelFormat::Gguf && !is_llama_task(task) {
        return Compatibility::Unsupported { format };
    }
    format.compatibility()
}

/// Whether llama-server serves a GGUF repo of `task` in Harness: chat (with or without vision),
/// embeddings, reranking and decision models, or an untagged repo (the file decides). GGUF is
/// also the container
/// of text-to-speech, speech-recognition and audio models for other engines (audio.cpp,
/// transcribe.cpp, CrispASR...) that llama-server cannot load; image GGUFs take the
/// stable-diffusion.cpp route before this.
pub fn is_llama_task(task: Option<&str>) -> bool {
    use crate::model_registry::Capability;
    task.is_none_or(|t| {
        t == crate::hub_tasks::DECISION_TASK
            || crate::model_registry::task_capabilities(t).iter().any(|c| {
                matches!(
                    c,
                    Capability::Chat | Capability::Embedding | Capability::Rerank
                )
            })
    })
}

/// Architectures (`config.json` `architectures`) the mistral.rs loaders serve, from its
/// "Supported models" reference (docs.mistralrs.dev/reference/supported-models, text and
/// multimodal chat tables). The runtime is user-registered, so no version is pinned; this is a
/// conservative list of documented loaders. A newer mistral.rs may serve more.
const MISTRALRS_ARCHITECTURES: &[&str] = &[
    "MistralForCausalLM",
    "GemmaForCausalLM",
    "MixtralForCausalLM",
    "LlamaForCausalLM",
    "PhiForCausalLM",
    "Phi3ForCausalLM",
    "Qwen2ForCausalLM",
    "Gemma2ForCausalLM",
    "Gemma3ForCausalLM",
    "Starcoder2ForCausalLM",
    "PhiMoEForCausalLM",
    "DeepseekV2ForCausalLM",
    "DeepseekV3ForCausalLM",
    "Qwen3ForCausalLM",
    "Glm4ForCausalLM",
    "Glm4MoeLiteForCausalLM",
    "Glm4MoeForCausalLM",
    "Qwen3MoeForCausalLM",
    "SmolLM3ForCausalLM",
    "GraniteMoeHybridForCausalLM",
    "GptOssForCausalLM",
    "HunYuanDenseV1ForCausalLM",
    "HunYuanMoEV1ForCausalLM",
    "Qwen3NextForCausalLM",
    "Qwen3_5ForCausalLM",
    "Lfm2ForCausalLM",
    "Lfm2MoeForCausalLM",
    "Phi3VForCausalLM",
    "Idefics2ForConditionalGeneration",
    "LlavaNextForConditionalGeneration",
    "LlavaForConditionalGeneration",
    "Lfm2VlForConditionalGeneration",
    "MllamaForConditionalGeneration",
    "Qwen2VLForConditionalGeneration",
    "Idefics3ForConditionalGeneration",
    "MiniCPMO",
    "Phi4MMForCausalLM",
    "Qwen2_5_VLForConditionalGeneration",
    "Gemma3ForConditionalGeneration",
    "Mistral3ForConditionalGeneration",
    "Llama4ForConditionalGeneration",
    "Gemma3nForConditionalGeneration",
    "Qwen3VLForConditionalGeneration",
    "Qwen3VLMoeForConditionalGeneration",
    "Qwen3_5ForConditionalGeneration",
    "Qwen3_5MoeForConditionalGeneration",
    "Qwen4ExpForConditionalGeneration",
    "Gemma4ForConditionalGeneration",
    "MuseGlimmerForConditionalGeneration",
];

/// Whether a Hub `config` names an architecture mistral.rs serves. A listing without a config,
/// or one with no `architectures`, is not judged (the file layout decides).
fn mistralrs_serves_config(config: Option<&serde_json::Value>) -> bool {
    let Some(archs) = config
        .and_then(|c| c.get("architectures"))
        .and_then(|a| a.as_array())
    else {
        return true;
    };
    let archs: Vec<&str> = archs.iter().filter_map(|a| a.as_str()).collect();
    archs.is_empty() || archs.iter().any(|a| MISTRALRS_ARCHITECTURES.contains(a))
}

/// mistral.rs's verdict on a Hub repo: a safetensors chat model ([`MISTRALRS_TASKS`]) whose
/// listing shows a `config.json` and `.safetensors` weights at the repo root (a listing without
/// file names is taken by its tags). `None` for every other repo. Whether the runtime is
/// installed is settled later ([`resolve_installed`]). A `config` whose `architectures` is not in
/// [`MISTRALRS_ARCHITECTURES`] (GPT-2, GPT-NeoX, draft models...) is not a mistral.rs repo.
pub fn mistralrs_compatibility(
    format: ModelFormat,
    task: Option<&str>,
    files: &[String],
    config: Option<&serde_json::Value>,
) -> Option<Compatibility> {
    if format != ModelFormat::Safetensors
        || !task.is_some_and(|t| MISTRALRS_TASKS.contains(&t))
        || !mistralrs_serves_config(config)
    {
        return None;
    }
    let root = |f: &&String| !f.contains('/') && !f.contains('\\');
    let has_config = files.iter().filter(root).any(|f| f == "config.json");
    let has_weights = files
        .iter()
        .filter(root)
        .any(|f| f.to_ascii_lowercase().ends_with(".safetensors"));
    (files.is_empty() || (has_config && has_weights)).then(|| Compatibility::Runnable {
        runtime: MISTRALRS_FAMILY.to_string(),
    })
}

/// Runtime family of the MLX engine (mlx-lm, Apple Silicon).
pub const MLX_FAMILY: &str = crate::engine::MLX;

/// Runtime family of the MLX vision engine (mlx-vlm, Apple Silicon).
pub const MLX_VLM_FAMILY: &str = crate::engine::MLX_VLM;

/// Runtime family of the MLX audio engine (mlx-audio speech synthesis and recognition, Apple
/// Silicon).
pub const MLX_AUDIO_FAMILY: &str = crate::engine::MLX_AUDIO;

/// The MLX engines' verdict on a Hub repo: an MLX repo (`library_name: mlx` or the `mlx` tag)
/// with a `config.json` and safetensors at the root (a listing without file names is taken by
/// its tags), `text-generation` or untagged for `mlx`, `image-text-to-text` for `mlx-vlm`,
/// `text-to-speech` of a tested architecture (by the repo name: Qwen3-TTS) and
/// `automatic-speech-recognition` of one (Whisper, Parakeet: [`crate::engine::mlx::is_stt_repo`])
/// for `mlx-audio`.
/// Runnable where MLX runs (Apple Silicon; installed or not is settled later by
/// [`resolve_installed`]); on a Mac whose macOS is too old for the MLX wheels,
/// [`Compatibility::UnsupportedPlatform`]; `None` elsewhere and for every other repo.
pub fn mlx_compatibility(
    repo: &str,
    format: ModelFormat,
    task: Option<&str>,
    files: &[String],
    tags: &[String],
) -> Option<Compatibility> {
    mlx_compatibility_on(
        repo,
        format,
        task,
        files,
        tags,
        cfg!(all(target_os = "macos", target_arch = "aarch64")),
        crate::pyenv::envs::spec(crate::engine::mlx::ENV)
            .and_then(crate::pyenv::envs::platform_refusal)
            .is_some(),
    )
}

/// The task of an MLX speech recognition repo that names none: mlx-audio's own conversions
/// (`whisper-large-v3-turbo-asr-fp16`) have no `pipeline_tag`, and an untagged MLX repo would
/// otherwise be taken for a chat model. Apple Silicon only, where mlx-audio runs; elsewhere the
/// repo keeps its (missing) task.
pub fn mlx_stt_task(
    repo: &str,
    format: ModelFormat,
    task: Option<&str>,
    tags: &[String],
) -> Option<&'static str> {
    mlx_stt_task_on(
        repo,
        format,
        task,
        tags,
        cfg!(all(target_os = "macos", target_arch = "aarch64")),
    )
}

/// [`mlx_stt_task`] with the platform given.
pub fn mlx_stt_task_on(
    repo: &str,
    format: ModelFormat,
    task: Option<&str>,
    tags: &[String],
    apple_silicon: bool,
) -> Option<&'static str> {
    (apple_silicon
        && task.is_none()
        && format == ModelFormat::Mlx
        && crate::engine::mlx::is_stt_repo(repo, tags))
    .then_some("automatic-speech-recognition")
}

/// [`mlx_compatibility`] with the platform facts given.
pub fn mlx_compatibility_on(
    repo: &str,
    format: ModelFormat,
    task: Option<&str>,
    files: &[String],
    tags: &[String],
    apple_silicon: bool,
    os_too_old: bool,
) -> Option<Compatibility> {
    let family = match task {
        // A speech model without a task (Qwen3-ASR) is no chat model.
        None | Some("text-generation") if crate::engine::mlx::is_audio_repo(tags) => return None,
        None | Some("text-generation") => MLX_FAMILY,
        Some("image-text-to-text") => MLX_VLM_FAMILY,
        Some("text-to-speech") if crate::engine::mlx::is_tts_repo(repo) => MLX_AUDIO_FAMILY,
        Some("automatic-speech-recognition")
            if crate::engine::mlx::is_stt_repo(repo, tags)
                && crate::engine::mlx::stt_repo_files_ok(repo, tags, files) =>
        {
            MLX_AUDIO_FAMILY
        }
        Some(_) => return None,
    };
    if !apple_silicon || format != ModelFormat::Mlx {
        return None;
    }
    let root = |f: &&String| !f.contains('/') && !f.contains('\\');
    let has_config = files.iter().filter(root).any(|f| f == "config.json");
    let has_weights = files
        .iter()
        .filter(root)
        .any(|f| f.to_ascii_lowercase().ends_with(".safetensors"));
    if !files.is_empty() && !(has_config && has_weights) {
        return None;
    }
    Some(if os_too_old {
        Compatibility::UnsupportedPlatform {
            runtime: family.to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        }
    } else {
        Compatibility::Runnable {
            runtime: family.to_string(),
        }
    })
}

/// Runtime family of the Python engine (transformers / sentence-transformers task models).
pub const PYTHON_HF_FAMILY: &str = crate::engine::PYTHON_HF;

/// The Python engine's verdict on a Hub repo: a transformers or sentence-transformers repo of a
/// task the worker serves ([`crate::engine::python_hf::TASKS`]), with safetensors or PyTorch
/// weights and a `config.json` at the root (a listing without file names is taken by its tags),
/// or a diffusers text-to-image or image-to-image pipeline ([`diffusers_pipeline`]). Remote code is never run, so
/// a `custom_code` repo is not one; a text-to-image repo with a single-file checkpoint stays with
/// stable-diffusion.cpp. `None` for every other repo. Whether the engine is installed is settled
/// later ([`resolve_installed`]).
pub fn python_hf_compatibility(
    format: ModelFormat,
    library: Option<&str>,
    task: Option<&str>,
    tags: &[String],
    files: &[String],
) -> Option<Compatibility> {
    if format == ModelFormat::Diffusers {
        return diffusers_pipeline(library, task, tags, files).then(|| Compatibility::Runnable {
            runtime: PYTHON_HF_FAMILY.to_string(),
        });
    }
    if !matches!(format, ModelFormat::Safetensors | ModelFormat::Pytorch) {
        return None;
    }
    let task = task.filter(|t| *t != "text-to-image")?;
    if !crate::engine::python_hf::TASKS.contains(&task) {
        return None;
    }
    if !matches!(
        library.map(str::to_ascii_lowercase).as_deref(),
        None | Some("transformers") | Some("sentence-transformers")
    ) {
        return None;
    }
    if tags.iter().any(|t| t.eq_ignore_ascii_case("custom_code")) {
        return None;
    }
    let root = |f: &&String| !f.contains('/') && !f.contains('\\');
    let has_config = files.iter().filter(root).any(|f| f == "config.json");
    let has_weights = files.iter().filter(root).any(|f| {
        let f = f.to_ascii_lowercase();
        f.ends_with(".safetensors") || (f.starts_with("pytorch_model") && f.ends_with(".bin"))
    });
    (files.is_empty() || (has_config && has_weights)).then(|| Compatibility::Runnable {
        runtime: PYTHON_HF_FAMILY.to_string(),
    })
}

/// Whether a diffusers pipeline's download would hold PyTorch `.bin` weights: a component
/// folder with a `.bin` and no safetensors (`segmind/tiny-sd`).
pub fn diffusers_pickle_weights(files: &[String]) -> bool {
    let mut bin = std::collections::BTreeSet::new();
    let mut safe = std::collections::BTreeSet::new();
    for f in files {
        let f = f.replace('\\', "/").to_ascii_lowercase();
        let Some((dir, name)) = f.split_once('/') else {
            continue;
        };
        if name.contains('/') {
            continue;
        }
        if name.ends_with(".safetensors") {
            safe.insert(dir.to_string());
        } else if name.ends_with(".bin") && !name.contains(".index") {
            bin.insert(dir.to_string());
        }
    }
    bin.difference(&safe).next().is_some()
}

/// A diffusers text-to-image or image-to-image pipeline (InstructPix2Pix, the SDXL refiner,
/// Qwen-Image-Edit) the Python engine loads from its folders: `model_index.json`
/// at the root and component weights (safetensors or PyTorch `.bin`) one folder down, no
/// remote code. A listing without file names is taken by its library and tags.
fn diffusers_pipeline(
    library: Option<&str>,
    task: Option<&str>,
    tags: &[String],
    files: &[String],
) -> bool {
    if !matches!(task, Some("text-to-image" | "image-to-image"))
        || !matches!(
            library.map(str::to_ascii_lowercase).as_deref(),
            None | Some("diffusers")
        )
        || tags.iter().any(|t| t.eq_ignore_ascii_case("custom_code"))
    {
        return false;
    }
    if files.is_empty() {
        return true;
    }
    let component_weights = files.iter().any(|f| {
        let f = f.replace('\\', "/").to_ascii_lowercase();
        f.split('/').count() == 2 && (f.ends_with(".safetensors") || f.ends_with(".bin"))
    });
    files.iter().any(|f| f == "model_index.json") && component_weights
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn onnx_generators_are_served_for_known_architectures_only() {
        let served =
            |task, config: serde_json::Value| onnx_config_served(Some(task), Some(&config));
        let marian =
            serde_json::json!({"model_type": "marian", "architectures": ["MarianMTModel"]});
        let nllb = serde_json::json!({"model_type": "m2m_100", "architectures": ["M2M100ForConditionalGeneration"]});
        assert!(served("translation", marian));
        assert!(!served("translation", nllb));
        assert!(served(
            "image-to-text",
            serde_json::json!({"architectures": ["VisionEncoderDecoderModel"]})
        ));
        assert!(!served(
            "image-to-text",
            serde_json::json!({"architectures": ["BlipForConditionalGeneration"]})
        ));
        // Other tasks, and listings without a config, are left to the task.
        assert!(served(
            "image-classification",
            serde_json::json!({"model_type": "vit"})
        ));
        assert!(onnx_config_served(Some("translation"), None));
    }

    #[test]
    fn python_hf_takes_diffusers_text_to_image_pipelines() {
        let runnable = Some(Compatibility::Runnable {
            runtime: PYTHON_HF_FAMILY.into(),
        });
        let tiny = s(&[
            "model_index.json",
            "unet/config.json",
            "unet/diffusion_pytorch_model.bin",
            "vae/diffusion_pytorch_model.safetensors",
        ]);
        let verdict = |library, task, tags: &[&str], files: &[String]| {
            python_hf_compatibility(ModelFormat::Diffusers, library, task, &s(tags), files)
        };
        assert_eq!(
            verdict(Some("diffusers"), Some("text-to-image"), &[], &tiny),
            runnable
        );
        // tiny-sd's unet has only a .bin; a component with safetensors next to its .bin does not count.
        assert!(diffusers_pickle_weights(&tiny));
        assert!(!diffusers_pickle_weights(&s(&[
            "model_index.json",
            "unet/diffusion_pytorch_model.bin",
            "unet/diffusion_pytorch_model.safetensors",
            "unet/diffusion_pytorch_model.bin.index.json",
        ])));
        // A listing without file names is taken by its library and task.
        assert_eq!(
            verdict(Some("diffusers"), Some("text-to-image"), &[], &[]),
            runnable
        );
        // Image-to-image pipelines too (the worker edits with them).
        assert_eq!(
            verdict(Some("diffusers"), Some("image-to-image"), &[], &tiny),
            runnable
        );
        // Other tasks, remote code, no weights in component folders, no model_index.json.
        assert_eq!(
            verdict(Some("diffusers"), Some("text-to-video"), &[], &tiny),
            None
        );
        assert_eq!(
            verdict(
                Some("diffusers"),
                Some("text-to-image"),
                &["custom_code"],
                &tiny
            ),
            None
        );
        assert_eq!(
            verdict(
                Some("diffusers"),
                Some("text-to-image"),
                &[],
                &s(&["model_index.json", "v1-5-pruned.safetensors"])
            ),
            None
        );
        assert_eq!(
            verdict(Some("diffusers"), Some("text-to-image"), &[], &tiny[1..]),
            None
        );
    }

    #[test]
    fn python_hf_takes_task_repos_without_remote_code() {
        let files = s(&["config.json", "model.safetensors", "tokenizer.json"]);
        let runnable = Some(Compatibility::Runnable {
            runtime: PYTHON_HF_FAMILY.into(),
        });
        let verdict = |format, library, task, tags: &[&str], files: &[String]| {
            python_hf_compatibility(format, library, task, &s(tags), files)
        };
        assert_eq!(
            verdict(
                ModelFormat::Safetensors,
                Some("transformers"),
                Some("token-classification"),
                &[],
                &files
            ),
            runnable
        );
        assert_eq!(
            verdict(
                ModelFormat::Pytorch,
                Some("transformers"),
                Some("visual-question-answering"),
                &[],
                &s(&["config.json", "pytorch_model.bin"])
            ),
            runnable
        );
        // Remote code, chat models, other libraries and image generation are not its own.
        assert_eq!(
            verdict(
                ModelFormat::Safetensors,
                Some("transformers"),
                Some("text-classification"),
                &["custom_code"],
                &files
            ),
            None
        );
        assert_eq!(
            verdict(
                ModelFormat::Safetensors,
                Some("transformers"),
                Some("text-generation"),
                &[],
                &files
            ),
            None
        );
        assert_eq!(
            verdict(
                ModelFormat::Safetensors,
                Some("timm"),
                Some("image-classification"),
                &[],
                &files
            ),
            None
        );
        assert_eq!(
            verdict(
                ModelFormat::Diffusers,
                Some("diffusers"),
                Some("text-to-image"),
                &[],
                &files
            ),
            None
        );
        // No root config: not a folder the worker loads.
        assert_eq!(
            verdict(
                ModelFormat::Safetensors,
                None,
                Some("fill-mask"),
                &[],
                &s(&["model.safetensors"])
            ),
            None
        );
        // A missing engine is reported as such.
        let mut c = Compatibility::Runnable {
            runtime: PYTHON_HF_FAMILY.into(),
        };
        resolve_installed_on(&mut c, &[], true);
        assert_eq!(
            c,
            Compatibility::NeedsRuntime {
                runtime: PYTHON_HF_FAMILY.into()
            }
        );
    }

    #[test]
    fn hub_ctc_config_uses_sidecar_architectures() {
        for architecture in [
            "Wav2Vec2ForCTC",
            "HubertForCTC",
            "WavLMForCTC",
            "Data2VecAudioForCTC",
            "Wav2Vec2ConformerForCTC",
            "UniSpeechForCTC",
            "UniSpeechSatForCTC",
            "SEWForCTC",
        ] {
            assert!(is_ctc_config(
                &serde_json::json!({"architectures": [architecture]})
            ));
        }
        for architecture in [
            "UnsupportedForCTC",
            "Wav2Vec2Model",
            "WhisperForConditionalGeneration",
        ] {
            assert!(!is_ctc_config(
                &serde_json::json!({"architectures": [architecture]})
            ));
        }
        assert!(!is_ctc_config(
            &serde_json::json!({"architectures": [null, 42]})
        ));
    }

    #[test]
    fn hub_ctc_files_accept_root_or_onnx_graphs_only() {
        for graph in ["model.onnx", "model_int8.onnx", "onnx/model.onnx"] {
            assert!(has_ctc_files(&s(&[
                "config.json",
                "vocab.json",
                "preprocessor_config.json",
                graph
            ])));
        }
        for graph in [
            "nested/model.onnx",
            "onnx/nested/model.onnx",
            "model.safetensors",
        ] {
            assert!(!has_ctc_files(&s(&[
                "config.json",
                "vocab.json",
                "preprocessor_config.json",
                graph
            ])));
        }
        assert!(!has_ctc_files(&s(&[
            "config.json",
            "preprocessor_config.json",
            "model.onnx"
        ])));
    }

    #[test]
    fn gguf_by_file_or_tag_beats_original_weights() {
        let f = detect_format(&[], None, &s(&["m-Q4_K_M.gguf", "model.safetensors"]));
        assert_eq!(f, ModelFormat::Gguf);
        assert_eq!(
            detect_format(&s(&["safetensors", "gguf"]), None, &[]),
            ModelFormat::Gguf
        );
    }

    /// GGUF repos of the Hub's most downloaded speech and audio models (2026-10-13), with their
    /// `pipeline_tag`: containers for audio.cpp, transcribe.cpp, NeMo and other engines that
    /// llama-server does not load. They were listed as runnable on llama.cpp.
    const AUDIO_GGUF_REPOS: [(&str, &str); 19] = [
        ("audio-cpp/audio.cpp-gguf", "text-to-speech"),
        ("pnnbao-ump/VieNeu-TTS-v3-Turbo", "text-to-speech"),
        ("ampixa/sanoTTS", "text-to-speech"),
        ("Serveurperso/Qwen3-TTS-GGUF", "text-to-speech"),
        ("drbaph/Higgs-Audio-v3-Studio", "text-to-speech"),
        ("audio-cpp/AuK-Base-and-Flash-GGUF", "text-to-speech"),
        ("pnnbao-ump/VieNeu-TTS-0.3B-q4-gguf", "text-to-speech"),
        ("kenpath/svara-tts-v1", "text-to-speech"),
        (
            "handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf",
            "automatic-speech-recognition",
        ),
        (
            "handy-computer/parakeet-unified-en-0.6b-gguf",
            "automatic-speech-recognition",
        ),
        (
            "nvidia/nemotron-3.5-asr-streaming-0.6b",
            "automatic-speech-recognition",
        ),
        (
            "handy-computer/cohere-transcribe-03-2026-gguf",
            "automatic-speech-recognition",
        ),
        ("audio-cpp/SAM-Audio-GGUF", "audio-to-audio"),
        ("LiquidAI/LFM2.5-Audio-1.5B-JP-GGUF", "audio-to-audio"),
        ("chenmozhijin/BSRoformer-GGUF", "audio-to-audio"),
        ("cstr/qwen3-tts-tokenizer-12hz-GGUF", "audio-to-audio"),
        ("audio-cpp/RE-USE-GGUF", "audio-to-audio"),
        ("scragnog/Ace-Step-1.5-ScragVAE", "audio-to-audio"),
        ("LocalAI-io/LocalVQE", "audio-to-audio"),
    ];

    #[test]
    fn a_gguf_of_a_task_llama_server_does_not_serve_is_not_runnable() {
        for (repo, task) in AUDIO_GGUF_REPOS {
            assert_eq!(
                hub_compatibility(ModelFormat::Gguf, Some(task)),
                Compatibility::Unsupported {
                    format: ModelFormat::Gguf
                },
                "{repo}"
            );
        }
        // Chat, vision, embedding, reranking and untagged GGUF repos stay on llama.cpp.
        for task in [
            None,
            Some("text-generation"),
            Some("image-text-to-text"),
            Some("feature-extraction"),
            Some("sentence-similarity"),
            Some("text-ranking"),
            Some("audio-text-to-text"),
        ] {
            assert_eq!(
                hub_compatibility(ModelFormat::Gguf, task),
                Compatibility::Runnable {
                    runtime: LLAMA_FAMILY.to_string()
                },
                "{task:?}"
            );
        }
    }

    #[test]
    fn whisper_ggml_bin_is_not_pytorch() {
        let files = s(&["ggml-base.bin", "ggml-tiny.en-q5_1.bin", "README.md"]);
        assert_eq!(detect_format(&[], None, &files), ModelFormat::WhisperGgml);
        let torch = s(&["pytorch_model.bin", "config.json"]);
        assert_eq!(detect_format(&[], None, &torch), ModelFormat::Pytorch);
    }

    #[test]
    fn other_formats_are_read_from_tags_library_and_files() {
        assert_eq!(
            detect_format(&[], Some("diffusers"), &[]),
            ModelFormat::Diffusers
        );
        assert_eq!(
            detect_format(&[], None, &s(&["model_index.json", "unet/x.safetensors"])),
            ModelFormat::Diffusers
        );
        assert_eq!(
            detect_format(&s(&["onnx", "safetensors"]), None, &[]),
            ModelFormat::Safetensors
        );
        assert_eq!(detect_format(&s(&["onnx"]), None, &[]), ModelFormat::Onnx);
        assert_eq!(
            detect_format(&s(&["onnx", "safetensors"]), Some("transformers.js"), &[]),
            ModelFormat::Onnx
        );
        assert_eq!(
            detect_format(&s(&["transformers", "safetensors"]), None, &[]),
            ModelFormat::Safetensors
        );
        assert_eq!(detect_format(&s(&["mlx"]), None, &[]), ModelFormat::Mlx);
        // faster-whisper: a CTranslate2 `model.bin`, not PyTorch.
        assert_eq!(
            detect_format(&s(&["ctranslate2", "audio"]), None, &s(&["model.bin"])),
            ModelFormat::Ctranslate2
        );
        assert_eq!(
            detect_format(&s(&["coreml"]), None, &[]),
            ModelFormat::CoreMl
        );
        assert_eq!(detect_format(&[], None, &[]), ModelFormat::Unknown);
    }

    #[test]
    fn compatibility_follows_format_and_installed_runtimes() {
        let mut gguf = ModelFormat::Gguf.compatibility();
        resolve_installed(&mut gguf, &[]);
        assert_eq!(
            gguf,
            Compatibility::Runnable {
                runtime: "llama.cpp".into()
            }
        );

        let mut whisper = ModelFormat::WhisperGgml.compatibility();
        resolve_installed(&mut whisper, &s(&["llama.cpp"]));
        assert_eq!(
            whisper,
            Compatibility::NeedsRuntime {
                runtime: "whisper.cpp".into()
            }
        );
        let mut whisper = ModelFormat::WhisperGgml.compatibility();
        resolve_installed(&mut whisper, &s(&["whisper.cpp"]));
        assert!(matches!(whisper, Compatibility::Runnable { .. }));

        let mut st = ModelFormat::Safetensors.compatibility();
        resolve_installed(&mut st, &s(&["whisper.cpp"]));
        assert_eq!(
            st,
            Compatibility::Unsupported {
                format: ModelFormat::Safetensors
            }
        );
    }

    #[test]
    fn onnx_without_a_platform_build_is_unsupported_here() {
        let onnx = || hub_compatibility(ModelFormat::Onnx, Some("feature-extraction"));
        let mut c = onnx();
        resolve_installed_on(&mut c, &[], true);
        assert_eq!(
            c,
            Compatibility::Runnable {
                runtime: ONNX_FAMILY.into()
            }
        );
        let mut c = onnx();
        resolve_installed_on(&mut c, &[], false);
        assert_eq!(
            c,
            Compatibility::UnsupportedPlatform {
                runtime: ONNX_FAMILY.into(),
                os: std::env::consts::OS.into(),
                arch: std::env::consts::ARCH.into(),
            }
        );
        // Other families do not depend on it.
        let mut gguf = ModelFormat::Gguf.compatibility();
        resolve_installed_on(&mut gguf, &[], false);
        assert!(matches!(gguf, Compatibility::Runnable { .. }));
    }

    #[test]
    fn sentence_transformers_repo_with_an_onnx_export_is_onnx() {
        // sentence-transformers/all-MiniLM-L6-v2: safetensors + onnx/ + tokenizer.json.
        let tags = s(&[
            "sentence-transformers",
            "onnx",
            "safetensors",
            "bert",
            "feature-extraction",
        ]);
        let f = detect_hub_format(
            &tags,
            Some("sentence-transformers"),
            &[],
            Some("feature-extraction"),
        );
        assert_eq!(f, ModelFormat::Onnx);
        assert_eq!(
            hub_compatibility(f, Some("feature-extraction")),
            Compatibility::Runnable {
                runtime: ONNX_FAMILY.into()
            }
        );
        // Files in the listing count as evidence as well as the tag.
        let files = s(&["model.safetensors", "onnx/model.onnx", "tokenizer.json"]);
        assert_eq!(
            detect_hub_format(&[], None, &files, Some("text-ranking")),
            ModelFormat::Onnx
        );
        // No ONNX export: the original weights stay unsupported.
        let plain = s(&["sentence-transformers", "safetensors"]);
        let f = detect_hub_format(&plain, None, &[], Some("feature-extraction"));
        assert_eq!(f, ModelFormat::Safetensors);
        assert!(matches!(
            hub_compatibility(f, Some("feature-extraction")),
            Compatibility::Unsupported { .. }
        ));
    }

    #[test]
    fn genai_chat_repos_are_runnable_by_the_onnx_family() {
        // microsoft/Phi-3-mini-4k-instruct-onnx: one genai folder per target, no pipeline tag
        // worth trusting.
        let files = s(&[
            "directml/directml-int4-awq-block-128/genai_config.json",
            "directml/directml-int4-awq-block-128/model.onnx",
            "directml/directml-int4-awq-block-128/model.onnx.data",
            "README.md",
        ]);
        let tags = s(&["onnx", "custom_code"]);
        assert!(is_genai_repo(&tags, &files));
        let f = detect_hub_format(&tags, None, &files, Some("text-generation"));
        assert_eq!(f, ModelFormat::Onnx);
        for task in [Some("text-generation"), None] {
            assert_eq!(
                hub_compatibility_for(f, task, true),
                Compatibility::Runnable {
                    runtime: ONNX_FAMILY.into()
                }
            );
        }
        // A vision-language genai repo, or an ONNX text model without genai files, is not.
        assert!(matches!(
            hub_compatibility_for(f, Some("image-text-to-text"), true),
            Compatibility::Unsupported { .. }
        ));
        assert!(matches!(
            hub_compatibility_for(ModelFormat::Onnx, Some("text-generation"), false),
            Compatibility::Unsupported { .. }
        ));
        // The tag alone is evidence too, and a safetensors listing with genai files is ONNX.
        assert!(is_genai_repo(&s(&["onnxruntime-genai"]), &[]));
        let mixed = s(&["model.safetensors", "genai_config.json"]);
        assert_eq!(
            detect_hub_format(&[], None, &mixed, Some("text-generation")),
            ModelFormat::Onnx
        );
    }

    #[test]
    fn onnx_of_other_tasks_is_not_runnable() {
        let tags = s(&["onnx", "safetensors"]);
        // SAM with an ONNX export: not a task the ONNX engine serves, format unchanged.
        assert_eq!(
            detect_hub_format(&tags, None, &[], Some("mask-generation")),
            ModelFormat::Safetensors
        );
        // A repo that is ONNX only (transformers.js) for such a task is Onnx but unsupported.
        let f = detect_hub_format(&tags, Some("transformers.js"), &[], Some("mask-generation"));
        assert_eq!(f, ModelFormat::Onnx);
        let unsupported = Compatibility::Unsupported {
            format: ModelFormat::Onnx,
        };
        assert_eq!(hub_compatibility(f, Some("mask-generation")), unsupported);
        // A vision head with an ONNX export runs.
        assert_eq!(
            detect_hub_format(&tags, None, &[], Some("image-classification")),
            ModelFormat::Onnx
        );
        assert_eq!(hub_compatibility(f, None), unsupported);
        // GGUF always wins.
        let gguf = s(&["gguf", "onnx"]);
        assert_eq!(
            detect_hub_format(&gguf, None, &[], Some("feature-extraction")),
            ModelFormat::Gguf
        );
    }

    #[test]
    fn safetensors_chat_checkpoints_are_mistralrs_repos() {
        let runnable = Some(Compatibility::Runnable {
            runtime: MISTRALRS_FAMILY.into(),
        });
        let files = s(&["config.json", "model.safetensors", "tokenizer.json"]);
        for task in ["text-generation", "image-text-to-text"] {
            assert_eq!(
                mistralrs_compatibility(ModelFormat::Safetensors, Some(task), &files, None),
                runnable
            );
        }
        // Without file names the tags decide.
        assert_eq!(
            mistralrs_compatibility(ModelFormat::Safetensors, Some("text-generation"), &[], None),
            runnable
        );
        // An architecture outside the mistral.rs loaders (GPT-2, GPT-NeoX, draft models) is not.
        let cfg = |a: &str| serde_json::json!({"architectures": [a]});
        for (arch, served) in [
            ("Qwen3ForCausalLM", true),
            ("Gemma4ForConditionalGeneration", true),
            ("GPT2LMHeadModel", false),
            ("GPTNeoXForCausalLM", false),
            ("DSparkDraftModel", false),
        ] {
            let got = mistralrs_compatibility(
                ModelFormat::Safetensors,
                Some("text-generation"),
                &files,
                Some(&cfg(arch)),
            );
            assert_eq!(got.is_some(), served, "{arch}");
        }
        // A config without `architectures` is not judged.
        assert!(mistralrs_compatibility(
            ModelFormat::Safetensors,
            Some("text-generation"),
            &files,
            Some(&serde_json::json!({"model_type": "gpt2"})),
        )
        .is_some());
        // Other tasks, formats and layouts are not.
        assert_eq!(
            mistralrs_compatibility(
                ModelFormat::Safetensors,
                Some("feature-extraction"),
                &files,
                None
            ),
            None
        );
        assert_eq!(
            mistralrs_compatibility(ModelFormat::Safetensors, None, &files, None),
            None
        );
        assert_eq!(
            mistralrs_compatibility(ModelFormat::Gguf, Some("text-generation"), &files, None),
            None
        );
        let nested = s(&["ckpt/config.json", "ckpt/model.safetensors"]);
        assert_eq!(
            mistralrs_compatibility(
                ModelFormat::Safetensors,
                Some("text-generation"),
                &nested,
                None
            ),
            None
        );
        // mistral.rs is an optional runtime: missing until registered.
        let mut c = runnable.clone().unwrap();
        resolve_installed(&mut c, &s(&["llama.cpp"]));
        assert_eq!(
            c,
            Compatibility::NeedsRuntime {
                runtime: "mistral.rs".into()
            }
        );
    }

    #[test]
    fn mlx_speech_recognition_repos_are_mlx_audio_on_apple_silicon() {
        let files = s(&[
            "config.json",
            "model.safetensors",
            "preprocessor_config.json",
            "tokenizer.json",
        ]);
        let audio = Some(Compatibility::Runnable {
            runtime: MLX_AUDIO_FAMILY.into(),
        });
        let asr = Some("automatic-speech-recognition");
        // mlx-audio's conversions: library_name mlx-audio, or only the mlx tag.
        let small = s(&["mlx-audio", "safetensors", "whisper", "mlx"]);
        assert_eq!(
            detect_format(&s(&["mlx-audio"]), None, &[]),
            ModelFormat::Mlx
        );
        assert_eq!(detect_format(&[], Some("mlx-audio"), &[]), ModelFormat::Mlx);
        for (repo, tags) in [
            ("mlx-community/whisper-small-asr-fp16", &small),
            (
                "mlx-community/parakeet-tdt-0.6b-v3",
                &s(&["mlx", "safetensors"]),
            ),
            // A tag names the architecture when the name does not.
            ("someone/large-v3-turbo-mlx", &s(&["mlx", "whisper"])),
        ] {
            let format = detect_format(tags, None, &files);
            assert_eq!(
                mlx_compatibility_on(repo, format, asr, &files, tags, true, false),
                audio,
                "{repo}"
            );
            // Elsewhere: no MLX verdict, as before.
            assert_eq!(
                mlx_compatibility_on(repo, format, asr, &files, tags, false, false),
                None
            );
        }
        // An untested recognizer, or a repo of npz weights, is not offered.
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/moonshine-tiny",
                ModelFormat::Mlx,
                asr,
                &files,
                &s(&["mlx", "moonshine"]),
                true,
                false
            ),
            None
        );
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/whisper-tiny-mlx",
                ModelFormat::Mlx,
                asr,
                &s(&["config.json", "weights.npz"]),
                &s(&["mlx"]),
                true,
                false
            ),
            None
        );

        // mlx-examples' older layout has no processor: mlx-audio cannot transcribe with it.
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/whisper-large-v3-turbo",
                ModelFormat::Mlx,
                asr,
                &s(&["README.md", "config.json", "weights.safetensors"]),
                &s(&["mlx", "whisper"]),
                true,
                false
            ),
            None
        );
        // Parakeet has its own tokenizer (no Whisper processor).
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/parakeet-tdt-0.6b-v3",
                ModelFormat::Mlx,
                asr,
                &s(&["config.json", "model.safetensors", "tokenizer.model"]),
                &s(&["mlx"]),
                true,
                false
            ),
            audio
        );

        // An untested recognizer without a task is not taken for a chat model either.
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/Qwen3-ASR-0.6B-4bit",
                ModelFormat::Mlx,
                None,
                &files,
                &s(&["mlx-audio", "safetensors", "qwen3_asr", "mlx", "asr"]),
                true,
                false
            ),
            None
        );

        // whisper-large-v3-turbo-asr-fp16 names no task: it is a recognizer, not a chat model.
        let turbo = "mlx-community/whisper-large-v3-turbo-asr-fp16";
        let tags = s(&["mlx", "safetensors", "whisper", "speech-to-text", "stt"]);
        assert_eq!(
            mlx_stt_task_on(turbo, ModelFormat::Mlx, None, &tags, true),
            asr
        );
        assert_eq!(
            mlx_compatibility_on(turbo, ModelFormat::Mlx, asr, &files, &tags, true, false),
            audio
        );
        // A tagged task stays; other platforms and other formats keep no task.
        assert_eq!(
            mlx_stt_task_on(
                turbo,
                ModelFormat::Mlx,
                Some("text-generation"),
                &tags,
                true
            ),
            None
        );
        assert_eq!(
            mlx_stt_task_on(turbo, ModelFormat::Mlx, None, &tags, false),
            None
        );
        assert_eq!(
            mlx_stt_task_on(turbo, ModelFormat::Safetensors, None, &tags, true),
            None
        );
        assert_eq!(
            mlx_stt_task_on(
                "mlx-community/Qwen3-0.6B-4bit",
                ModelFormat::Mlx,
                None,
                &s(&["mlx"]),
                true
            ),
            None
        );
    }

    #[test]
    fn mlx_chat_repos_run_on_apple_silicon_only() {
        let files = s(&[
            "config.json",
            "model.safetensors",
            "tokenizer.json",
            "README.md",
        ]);
        let format = detect_format(&s(&["mlx", "safetensors"]), Some("mlx"), &files);
        assert_eq!(format, ModelFormat::Mlx);
        let runnable = Some(Compatibility::Runnable {
            runtime: MLX_FAMILY.into(),
        });
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/x",
                format,
                Some("text-generation"),
                &files,
                &[],
                true,
                false
            ),
            runnable
        );
        // Untagged, or a listing without file names.
        assert_eq!(
            mlx_compatibility_on("mlx-community/x", format, None, &[], &[], true, false),
            runnable
        );
        // A vision repo is mlx-vlm's.
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/x",
                format,
                Some("image-text-to-text"),
                &files,
                &[],
                true,
                false
            ),
            Some(Compatibility::Runnable {
                runtime: MLX_VLM_FAMILY.into()
            })
        );
        // A Qwen3-TTS repo is mlx-audio's; a TTS repo of an untested architecture is nobody's.
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/Qwen3-TTS-12Hz-0.6B-CustomVoice-8bit",
                format,
                Some("text-to-speech"),
                &files,
                &[],
                true,
                false
            ),
            Some(Compatibility::Runnable {
                runtime: MLX_AUDIO_FAMILY.into()
            })
        );
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/Kokoro-82M-bf16",
                format,
                Some("text-to-speech"),
                &files,
                &[],
                true,
                false
            ),
            None
        );
        // Not on an Intel Mac / Windows / Linux; other tasks; no weights at the root.
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/x",
                format,
                Some("text-generation"),
                &files,
                &[],
                false,
                false
            ),
            None
        );
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/x",
                format,
                Some("image-text-to-text"),
                &files,
                &[],
                false,
                false
            ),
            None
        );
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/x",
                format,
                Some("text-to-speech"),
                &files,
                &[],
                true,
                false
            ),
            None
        );
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/x",
                format,
                None,
                &s(&["config.json", "README.md"]),
                &[],
                true,
                false
            ),
            None
        );
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/x",
                ModelFormat::Safetensors,
                None,
                &files,
                &[],
                true,
                false
            ),
            None
        );
        // macOS 13: says so instead of offering an engine that can't install.
        assert!(matches!(
            mlx_compatibility_on("mlx-community/x", format, None, &files, &[], true, true),
            Some(Compatibility::UnsupportedPlatform { runtime, .. }) if runtime == MLX_FAMILY
        ));
        // Installed or not.
        let mut c = runnable.clone().unwrap();
        resolve_installed_on(&mut c, &[], true);
        assert_eq!(
            c,
            Compatibility::NeedsRuntime {
                runtime: MLX_FAMILY.into()
            }
        );
    }

    #[test]
    fn the_onnx_family_is_never_reported_missing() {
        let mut c = hub_compatibility(ModelFormat::Onnx, Some("text-ranking"));
        resolve_installed(&mut c, &[]);
        assert!(matches!(c, Compatibility::Runnable { .. }));
    }
}

#[cfg(test)]
mod audio_cpp_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn processing_requires_provenance_and_never_captures_other_audio_engines() {
        for family in crate::engine::audio_cpp::PROCESS_FAMILIES {
            let item = json!({"id":"audited/model", "gguf":{"architecture":"audiocpp", "audiocpp.model_spec.family":family}});
            assert!(is_audio_cpp_repo(&item));
            assert!(audio_cpp_repo_processing(&item));
            assert!(!audio_cpp_repo_tts(&item));
            assert!(matches!(
                audio_cpp_compatibility(&item),
                Compatibility::Runnable { .. }
            ));
            assert_eq!(
                crate::hub_tasks::analyze_repo(&item)
                    .pipeline_tag
                    .as_deref(),
                Some("audio-to-audio")
            );
            let mut custom = item;
            custom["tags"] = json!(["custom_code"]);
            assert!(!audio_cpp_repo_processing(&custom));
            assert!(matches!(
                audio_cpp_compatibility(&custom),
                Compatibility::Unsupported { .. }
            ));
        }
        assert!(audio_cpp_repo_processing(
            &json!({"id":"audio-cpp/SAM-Audio-GGUF"})
        ));
        for repo in [
            "mlx-community/mel-roformer-mlx",
            "speechbrain/sepformer",
            "other/audiosr",
            "other/vocoder",
        ] {
            let item = json!({"id":repo,"pipeline_tag":"audio-to-audio","tags":["gguf"]});
            assert!(!is_audio_cpp_repo(&item));
            assert!(!audio_cpp_repo_processing(&item));
        }
    }
    #[test]
    fn audio_cpp_metadata_wins_over_the_gguf_extension_and_non_tts_is_unsupported() {
        for marker in [
            json!({"library_name":"audio.cpp"}),
            json!({"library_name":"audiocpp"}),
            json!({"gguf":{"architecture":"audiocpp","audiocpp.model_spec.family":"pocket_tts"}}),
        ] {
            let mut item = marker;
            item["id"] = json!("audio/example");
            item["pipeline_tag"] = json!("text-to-speech");
            assert!(is_audio_cpp_repo(&item));
            let mut compatibility = audio_cpp_compatibility(&item);
            assert_eq!(
                compatibility,
                Compatibility::Runnable {
                    runtime: crate::engine::AUDIO_CPP.into()
                }
            );
            resolve_installed(&mut compatibility, &[]);
            assert_eq!(
                compatibility,
                Compatibility::NeedsRuntime {
                    runtime: crate::engine::AUDIO_CPP.into()
                }
            );
            item["gguf"] =
                json!({"architecture":"audiocpp", "audiocpp.model_spec.family":"sensevoice"});
            assert!(matches!(
                audio_cpp_compatibility(&item),
                Compatibility::Unsupported { .. }
            ));
            assert_eq!(
                crate::hub_tasks::analyze_repo(&item).not_runnable,
                Some(crate::hub_tasks::NotRunnable::AudioCppUnsupported)
            );
        }
    }
}
