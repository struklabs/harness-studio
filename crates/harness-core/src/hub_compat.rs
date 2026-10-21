//! What Harness can do with a Hugging Face repo, from the listing's own fields (no extra
//! request): the model file format and the runtime family that runs it.
//!
//! The Hub is no longer limited to `filter=gguf` (`HfSearchOptions::include_non_gguf`), so a
//! listing mixes repos Harness runs (GGUF on llama.cpp, whisper ggml `.bin` on whisper.cpp) with
//! repos no Harness runtime runs (safetensors, diffusers...). [`detect_format`] reads the
//! format from `tags`, `library_name` and the `siblings` file names; [`Compatibility`] says what
//! follows from it.

use serde::{Deserialize, Serialize};

/// Missing frontend requirements read from model metadata, independent of repo layout.
pub fn audio_cpp_not_runnable(item: &serde_json::Value) -> Option<crate::hub_tasks::NotRunnable> {
    let family = item["gguf"]["audiocpp.model_spec.family"]
        .as_str()
        .or_else(|| item["gguf"]["family"].as_str())
        // Listings can omit the GGUF header block. The exported model's config
        // carries the same family; require a GGUF file before using that fallback.
        .or_else(|| {
            item["siblings"]
                .as_array()
                .filter(|files| {
                    files.iter().any(|file| {
                        file["rfilename"]
                            .as_str()
                            .is_some_and(|p| p.ends_with(".gguf"))
                    })
                })
                .and_then(|_| item["config"]["model_type"].as_str())
        });
    (family == Some("vieneu_v3_turbo")).then_some(crate::hub_tasks::NotRunnable::NeedsSeaG2p)
}

/// A proven native speech folder takes precedence over an unrelated conversion.
pub fn native_speech_format(item: &serde_json::Value, files: &[String]) -> Option<ModelFormat> {
    let task = item["pipeline_tag"].as_str()?;
    if is_audio_cpp_repo(item)
        || crate::engine::transcribe_cpp::is_hub_repo(item)
        || item["id"].as_str().is_some_and(|id| {
            crate::engine::crispasr::supports_repo(id, Some(task))
                || crate::engine::crispasr_tts::supports_repo(id, Some(task))
        })
    {
        return None;
    }
    if !matches!(
        task,
        "automatic-speech-recognition" | "text-to-speech" | "text-to-audio"
    ) || item["tags"]
        .as_array()
        .is_some_and(|tags| tags.iter().any(|t| t == "custom_code"))
        || item["config"].get("auto_map").is_some()
    {
        return None;
    }
    let model_type = item["config"]["model_type"].as_str()?;
    let has = |name: &str| files.iter().any(|f| f == name);
    (crate::engine::python_hf::serves_model_type(Some(task), Some(model_type))
        && has("config.json")
        && has("model.safetensors")
        && (has("processor_config.json") || has("preprocessor_config.json"))
        && has("tokenizer_config.json")
        && (has("tokenizer.json") || has("vocab.json")))
    .then_some(ModelFormat::Safetensors)
}

/// Repo metadata can identify audio.cpp before any GGUF extension fallback.
pub fn is_audio_cpp_repo(item: &serde_json::Value) -> bool {
    let marker = |s: &str| matches!(s.to_ascii_lowercase().as_str(), "audio.cpp" | "audiocpp");
    item["id"].as_str().is_some_and(|id| {
        id == "audio-cpp/audio.cpp-gguf" || crate::engine::audio_cpp::family_of_repo(id).is_some()
    }) || audio_cpp_not_runnable(item).is_some()
        || item["library_name"].as_str().is_some_and(marker)
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
    audio_cpp_repo_family(item)
        .is_some_and(|f| crate::engine::audio_cpp::processing_task(f).is_some())
}

/// The family a single-package repository holds, else the one its GGUF header names.
fn audio_cpp_repo_family(item: &serde_json::Value) -> Option<&str> {
    item["id"]
        .as_str()
        .and_then(crate::engine::audio_cpp::family_of_repo)
        .or_else(|| {
            item["gguf"]["audiocpp.model_spec.family"]
                .as_str()
                .or_else(|| item["gguf"]["family"].as_str())
        })
}

/// A speaker diarization package (it answers with speaker turns, not audio).
pub fn audio_cpp_repo_diarization(item: &serde_json::Value) -> bool {
    audio_cpp_repo_processing(item)
        && audio_cpp_repo_family(item).is_some_and(crate::engine::audio_cpp::is_diarization)
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
    if audio_cpp_not_runnable(item).is_none()
        && (audio_cpp_repo_tts(item) || audio_cpp_repo_processing(item))
    {
        Compatibility::Runnable {
            runtime: crate::engine::AUDIO_CPP.into(),
        }
    } else {
        Compatibility::Unsupported {
            format: ModelFormat::Gguf,
        }
    }
}

/// Original VibeVoice checkpoints use incompatible config and weight layouts.
pub fn unsupported_vibevoice(config: Option<&serde_json::Value>, files: &[String]) -> bool {
    let Some(config) = config.filter(|c| c["model_type"] == "vibevoice") else {
        return false;
    };
    if config.get("decoder_config").is_some() || config.get("acoustic_tokenizer_config").is_some() {
        return true;
    }
    !(config.get("text_config").is_some() && config.get("audio_config").is_some()
        || files.iter().any(|f| {
            matches!(
                f.as_str(),
                "tokenizer.json" | "tokenizer.model" | "tokenizer_config.json"
            )
        }))
}

/// AuK needs several GGUF components; the current one-GGUF package builder cannot assemble it.
pub fn unsupported_auk_package(repo: &str) -> bool {
    repo.eq_ignore_ascii_case("audio-cpp/AuK-Base-and-Flash-GGUF")
}

/// SDXL refiner is a different architecture from the native SDXL base model.
pub fn is_sdxl_refiner(repo: &str, files: &[String]) -> bool {
    repo.to_ascii_lowercase()
        .contains("stable-diffusion-xl-refiner")
        || files.iter().any(|f| {
            let lower = f.to_ascii_lowercase();
            let name = lower.rsplit('/').next().unwrap_or(&lower);
            name.starts_with("sd_xl_refiner_") || name.starts_with("sdxl-refiner")
        })
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

/// Runtime family of the MLX retrieval engine (mlx-embeddings embedders, Qwen3-Reranker; Apple
/// Silicon).
pub const MLX_EMBED_FAMILY: &str = crate::engine::MLX_EMBED;

/// The MLX engines' verdict on a Hub repo: an MLX repo (`library_name: mlx` or the `mlx` tag)
/// with a `config.json` and safetensors at the root (a listing without file names is taken by
/// its tags), `text-generation` or untagged for `mlx`, `image-text-to-text` for `mlx-vlm`,
/// `text-to-speech` of a tested architecture (by the repo name: Qwen3-TTS, Chatterbox Turbo,
/// Soprano, VoxCPM2, Spark-TTS) and `automatic-speech-recognition` of one (Whisper, Parakeet:
/// [`crate::engine::mlx::is_stt_repo`]) for `mlx-audio`, `feature-extraction` /
/// `sentence-similarity` of a tested embedder architecture and `text-ranking` of a Qwen3
/// reranker ([`crate::engine::mlx::retrieval_repo_task`]) for `mlx-embed`.
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
        Some("text-to-speech") if crate::engine::mlx::is_tts_repo(repo, tags) => MLX_AUDIO_FAMILY,
        Some("automatic-speech-recognition")
            if crate::engine::mlx::is_stt_repo(repo, tags)
                && crate::engine::mlx::stt_repo_files_ok(repo, tags, files) =>
        {
            MLX_AUDIO_FAMILY
        }
        // Vocal separation (Mel-Band-RoFormer) behind `/v1/audio/process`.
        Some("audio-to-audio") if crate::engine::mlx::is_sts_repo(tags) => MLX_AUDIO_FAMILY,
        // Embedders and rerankers behind `/v1/embeddings` and `/v1/rerank`.
        Some(_) if crate::engine::mlx::retrieval_repo_task(task, tags).is_some() => {
            MLX_EMBED_FAMILY
        }
        Some(_) => return None,
    };
    if !apple_silicon || format != ModelFormat::Mlx {
        return None;
    }
    let root = |f: &&String| !f.contains('/') && !f.contains('\\');
    let has_config = files.iter().filter(root).any(|f| f == "config.json");
    // mlx-examples' older Whisper conversions have `weights.npz` (recognition only).
    let has_weights = files.iter().filter(root).any(|f| {
        let f = f.to_ascii_lowercase();
        f.ends_with(".safetensors")
            || (task == Some("automatic-speech-recognition") && f.ends_with(".npz"))
    });
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

/// Runtime family of the MLX image engine (mflux, Apple Silicon).
pub const MFLUX_FAMILY: &str = crate::engine::MFLUX;

/// The mflux engine's verdict on a Hub repo: a FLUX.2 [klein] conversion in mflux's saved format
/// (its tags name a base model of [`crate::engine::mlx::MFLUX_MODELS`] as `base_model:...`, and
/// its files are [`is_mflux_layout`]) of an image task. Runnable on Apple Silicon (installed or not is
/// settled later), [`Compatibility::UnsupportedPlatform`] on a macOS too old for the wheels,
/// `None` elsewhere and for every other repo.
pub fn mflux_compatibility(
    task: Option<&str>,
    files: &[String],
    tags: &[String],
) -> Option<Compatibility> {
    mflux_compatibility_on(
        task,
        files,
        tags,
        cfg!(all(target_os = "macos", target_arch = "aarch64")),
        crate::pyenv::envs::spec(crate::engine::mlx::MFLUX_ENV)
            .and_then(crate::pyenv::envs::platform_refusal)
            .is_some(),
    )
}

/// The base model of [`crate::engine::mlx::MFLUX_MODELS`] a repo's tags name
/// (`base_model:black-forest-labs/FLUX.2-klein-4B`, also `base_model:quantized:...` /
/// `base_model:finetune:...`), as mflux's model name.
pub fn mflux_base_of(tags: &[String]) -> Option<&'static str> {
    tags.iter().find_map(|t| {
        let rest = t.strip_prefix("base_model:")?;
        let repo = rest.rsplit_once(':').map_or(rest, |(_, repo)| repo);
        crate::engine::mlx::mflux_model_for_base(repo)
    })
}

/// Whether a repo's files are mflux's saved layout: `transformer/`, `text_encoder/` and `vae/`
/// (and FLUX.1's `text_encoder_2/` when there is one) each with numbered safetensors shards
/// (`0.safetensors`) and `model.safetensors.index.json`, `tokenizer/tokenizer.json` (and
/// `tokenizer_2/tokenizer.json` with a second encoder), and no `model_index.json` (a diffusers
/// pipeline's).
pub fn is_mflux_layout(files: &[String]) -> bool {
    let shard = |part: &str| {
        files.iter().any(|f| {
            f.strip_prefix(part)
                .and_then(|r| r.strip_prefix('/'))
                .and_then(|r| r.strip_suffix(".safetensors"))
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        })
    };
    !files.iter().any(|f| f == "model_index.json")
        && files.iter().any(|f| f == "tokenizer/tokenizer.json")
        && crate::engine::mlx::MFLUX_PARTS
            .iter()
            .chain(
                crate::engine::mlx::MFLUX_EXTRA_PARTS
                    .iter()
                    .filter(|part| files.iter().any(|f| f.starts_with(&format!("{part}/")))),
            )
            .all(|part| {
                shard(part)
                    && files
                        .iter()
                        .any(|f| *f == format!("{part}/model.safetensors.index.json"))
            })
        && (!files.iter().any(|f| f.starts_with("text_encoder_2/"))
            || files.iter().any(|f| f == "tokenizer_2/tokenizer.json"))
}

/// [`mflux_compatibility`] with the platform facts given.
pub fn mflux_compatibility_on(
    task: Option<&str>,
    files: &[String],
    tags: &[String],
    apple_silicon: bool,
    os_too_old: bool,
) -> Option<Compatibility> {
    let name = mflux_base_of(tags)?;
    if !apple_silicon || !matches!(task, Some("text-to-image" | "image-to-image") | None) {
        return None;
    }
    // FLUX.1 has its second text encoder; the others have none.
    let second = files.iter().any(|f| f.starts_with("text_encoder_2/"));
    if !is_mflux_layout(files) || crate::engine::mlx::mflux_has_second_encoder(name) != second {
        return None;
    }
    Some(if os_too_old {
        Compatibility::UnsupportedPlatform {
            runtime: MFLUX_FAMILY.to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        }
    } else {
        Compatibility::Runnable {
            runtime: MFLUX_FAMILY.to_string(),
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
///
/// A safetensors chat repo (`text-generation`, `image-text-to-text`) is the Python engine's last
/// resort: callers ask [`mistralrs_compatibility`] first (and a GGUF is llama.cpp's before
/// either). Its `config` must not carry a `quantization_config` (bitsandbytes, GPTQ, AWQ... need
/// packages the environment does not ship).
pub fn python_hf_compatibility(
    format: ModelFormat,
    library: Option<&str>,
    task: Option<&str>,
    tags: &[String],
    files: &[String],
    config: Option<&serde_json::Value>,
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
    if crate::engine::python_hf::CHAT_TASKS.contains(&task)
        && (format != ModelFormat::Safetensors
            || config.is_some_and(|c| c.get("quantization_config").is_some()))
    {
        return None;
    }
    // Any-to-any is one architecture here: Qwen2.5-Omni (audio, pictures and text in; text and
    // speech out). Other "any-to-any" repos need classes the worker does not carry.
    if task == "any-to-any"
        && config
            .and_then(|c| c.get("model_type"))
            .and_then(|v| v.as_str())
            != Some("qwen2_5_omni")
    {
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

/// The Python engine's verdict on a CTranslate2 Whisper export (faster-whisper): a repo of the
/// `ctranslate2` library for speech recognition whose name says Whisper, with `model.bin`, a
/// tokenizer or vocabulary and a `config.json` at the root. Every other CTranslate2 repo
/// (translation models, NLLB...) is `None` and stays refused. A listing without file names is
/// taken by its name and tags.
pub fn ct2_whisper_compatibility(
    repo_id: &str,
    format: ModelFormat,
    task: Option<&str>,
    tags: &[String],
    files: &[String],
) -> Option<Compatibility> {
    if format != ModelFormat::Ctranslate2
        || task != Some("automatic-speech-recognition")
        || !repo_id.to_ascii_lowercase().contains("whisper")
        || tags.iter().any(|t| t.eq_ignore_ascii_case("custom_code"))
    {
        return None;
    }
    let root = |f: &&String| !f.contains('/') && !f.contains('\\');
    let has = |name: &str| files.iter().filter(root).any(|f| f == name);
    (files.is_empty()
        || (has("model.bin")
            && has("config.json")
            && (has("tokenizer.json") || has("vocabulary.json") || has("vocabulary.txt"))
            && !files
                .iter()
                .any(|f| f.to_ascii_lowercase().ends_with(".safetensors"))))
    .then(|| Compatibility::Runnable {
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
    if !matches!(
        task,
        Some("text-to-image" | "image-to-image" | "text-to-video" | "image-to-video")
    ) || !matches!(
        library.map(str::to_ascii_lowercase).as_deref(),
        None | Some("diffusers")
    ) || tags.iter().any(|t| t.eq_ignore_ascii_case("custom_code"))
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
    fn ocr_models_keep_their_format_specific_runtime() {
        for task in ["image-to-text", "document-question-answering"] {
            assert_eq!(
                hub_compatibility(ModelFormat::Gguf, Some(task)),
                Compatibility::Runnable {
                    runtime: LLAMA_FAMILY.into()
                }
            );
        }
        assert_eq!(
            python_hf_compatibility(
                ModelFormat::Safetensors,
                Some("transformers"),
                Some("document-question-answering"),
                &[],
                &s(&["config.json", "model.safetensors"]),
                None
            ),
            Some(Compatibility::Runnable {
                runtime: PYTHON_HF_FAMILY.into()
            })
        );
        assert_eq!(
            hub_compatibility(ModelFormat::Gguf, Some("object-detection")),
            Compatibility::Unsupported {
                format: ModelFormat::Gguf
            }
        );
    }

    #[test]
    fn ct2_whisper_goes_to_the_python_engine_and_other_ct2_repos_stay_refused() {
        let asr = Some("automatic-speech-recognition");
        let files = s(&[
            "config.json",
            "model.bin",
            "tokenizer.json",
            "vocabulary.txt",
        ]);
        let verdict = |id: &str, format, task, files: &[String]| {
            ct2_whisper_compatibility(id, format, task, &s(&["ctranslate2"]), files)
        };
        let python = Some(Compatibility::Runnable {
            runtime: PYTHON_HF_FAMILY.to_string(),
        });
        let ct2 = ModelFormat::Ctranslate2;
        assert_eq!(
            verdict("Systran/faster-whisper-tiny", ct2, asr, &files),
            python
        );
        assert_eq!(
            verdict("deepdml/faster-whisper-large-v3-turbo-ct2", ct2, asr, &[]),
            python
        );
        // Another CTranslate2 model, another task, another format, an incomplete repo.
        assert_eq!(verdict("Systran/nllb-200-ct2", ct2, asr, &files), None);
        assert_eq!(
            verdict(
                "Systran/faster-whisper-tiny",
                ct2,
                Some("translation"),
                &files
            ),
            None
        );
        assert_eq!(
            verdict("openai/whisper-tiny", ModelFormat::Safetensors, asr, &files),
            None
        );
        assert_eq!(
            verdict(
                "Systran/faster-whisper-tiny",
                ct2,
                asr,
                &s(&["config.json", "model.bin"])
            ),
            None
        );
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
            python_hf_compatibility(ModelFormat::Diffusers, library, task, &s(tags), files, None)
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
            runnable
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
    fn only_qwen2_5_omni_is_an_any_to_any_repo_of_the_python_engine() {
        let files = s(&["config.json", "model-00001-of-00003.safetensors"]);
        let tf = Some("transformers");
        let omni = serde_json::json!({"model_type": "qwen2_5_omni"});
        let other = serde_json::json!({"model_type": "gemma3n"});
        let verdict = |config: Option<&serde_json::Value>| {
            python_hf_compatibility(
                ModelFormat::Safetensors,
                tf,
                Some("any-to-any"),
                &[],
                &files,
                config,
            )
        };
        assert_eq!(
            verdict(Some(&omni)),
            Some(Compatibility::Runnable {
                runtime: PYTHON_HF_FAMILY.into()
            })
        );
        assert_eq!(verdict(Some(&other)), None);
        assert_eq!(verdict(None), None);
        let quantized =
            serde_json::json!({"model_type": "qwen2_5_omni", "quantization_config": {}});
        assert_eq!(verdict(Some(&quantized)), None);
    }

    /// Safetensors chat repos (text and vision) are the Python engine's last resort: GGUF is
    /// llama.cpp's, mistral.rs comes before it, and a repo transformers cannot load as it is
    /// (remote code, quantized weights, pickles, another library) is not offered.
    #[test]
    fn python_hf_is_the_last_resort_for_safetensors_chat_repos() {
        let files = s(&["config.json", "model.safetensors", "tokenizer.json"]);
        let python = Some(Compatibility::Runnable {
            runtime: PYTHON_HF_FAMILY.into(),
        });
        let verdict = |format,
                       library,
                       task,
                       tags: &[&str],
                       files: &[String],
                       config: Option<&serde_json::Value>| {
            python_hf_compatibility(format, library, task, &s(tags), files, config)
        };
        let st = ModelFormat::Safetensors;
        for task in ["text-generation", "image-text-to-text"] {
            let tf = Some("transformers");
            assert_eq!(
                verdict(st, tf, Some(task), &[], &files, None),
                python,
                "{task}"
            );
            // Weights in several shards at the root, a listing without file names.
            let shards = s(&[
                "config.json",
                "model-00001-of-00002.safetensors",
                "model-00002-of-00002.safetensors",
            ]);
            assert_eq!(verdict(st, None, Some(task), &[], &shards, None), python);
            assert_eq!(verdict(st, tf, Some(task), &[], &[], None), python);
            // Pickles, remote code, other libraries and GGUF are not offered.
            let bin = s(&["config.json", "pytorch_model.bin"]);
            assert_eq!(
                verdict(ModelFormat::Pytorch, tf, Some(task), &[], &bin, None),
                None
            );
            assert_eq!(
                verdict(st, tf, Some(task), &["custom_code"], &files, None),
                None
            );
            assert_eq!(
                verdict(st, Some("mlx"), Some(task), &[], &files, None),
                None
            );
            assert_eq!(
                verdict(ModelFormat::Gguf, tf, Some(task), &[], &files, None),
                None
            );
        }
        let tf = Some("transformers");
        let awq = serde_json::json!({"quantization_config": {"quant_method": "awq"}});
        assert_eq!(
            verdict(st, tf, Some("text-generation"), &[], &files, Some(&awq)),
            None
        );
        let gpt2 = serde_json::json!({"architectures": ["GPT2LMHeadModel"]});
        assert_eq!(
            verdict(st, tf, Some("text-generation"), &[], &files, Some(&gpt2)),
            python
        );
        // Order: mistral.rs answers first for the architectures it loads...
        let qwen = serde_json::json!({"architectures": ["Qwen2ForCausalLM"]});
        assert_eq!(
            mistralrs_compatibility(st, Some("text-generation"), &files, Some(&qwen)),
            Some(Compatibility::Runnable {
                runtime: MISTRALRS_FAMILY.into()
            })
        );
        // ...and not for GPT-2, which is left to the Python engine.
        assert_eq!(
            mistralrs_compatibility(st, Some("text-generation"), &files, Some(&gpt2)),
            None
        );
    }

    #[test]
    fn python_video_verdicts_require_native_weights_and_no_custom_code() {
        let runnable = Some(Compatibility::Runnable {
            runtime: PYTHON_HF_FAMILY.into(),
        });
        for task in ["text-to-video", "image-to-video"] {
            let files = s(&[
                "model_index.json",
                "transformer/diffusion_pytorch_model.safetensors",
            ]);
            assert_eq!(
                python_hf_compatibility(
                    ModelFormat::Diffusers,
                    Some("diffusers"),
                    Some(task),
                    &[],
                    &files,
                    None
                ),
                runnable
            );
            assert_eq!(
                python_hf_compatibility(
                    ModelFormat::Diffusers,
                    Some("diffusers"),
                    Some(task),
                    &s(&["custom_code"]),
                    &files,
                    None
                ),
                None
            );
        }
        let files = s(&["config.json", "model.safetensors"]);
        assert_eq!(
            python_hf_compatibility(
                ModelFormat::Safetensors,
                Some("transformers"),
                Some("video-text-to-text"),
                &[],
                &files,
                None
            ),
            runnable
        );
        assert_eq!(
            python_hf_compatibility(
                ModelFormat::Onnx,
                Some("transformers"),
                Some("video-text-to-text"),
                &[],
                &files,
                None
            ),
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
            python_hf_compatibility(format, library, task, &s(tags), files, None)
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
        // Remote code, other libraries and image generation are not its own.
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
                &["custom_code"],
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
    /// llama-server does not load. They were listed as runnable on llama.cpp. (LFM2.5-Audio's
    /// `audio-to-audio` GGUF has a projector: `hub_tasks::analyze_repo` reads it as audio chat.)
    const AUDIO_GGUF_REPOS: [(&str, &str); 18] = [
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
        // An untested recognizer is not offered.
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
        // mlx-examples' layouts have no processor: it is downloaded with them (npz weights,
        // `weights.safetensors`, quantized weights with a tiktoken vocabulary).
        for (repo, files) in [
            (
                "mlx-community/whisper-tiny-mlx",
                vec!["config.json", "weights.npz"],
            ),
            (
                "mlx-community/whisper-large-v3-turbo",
                vec!["README.md", "config.json", "weights.safetensors"],
            ),
            (
                "mlx-community/whisper-large-v3-turbo-4bit",
                vec!["config.json", "model.safetensors", "multilingual.tiktoken"],
            ),
        ] {
            assert_eq!(
                mlx_compatibility_on(
                    repo,
                    ModelFormat::Mlx,
                    asr,
                    &s(&files),
                    &s(&["mlx"]),
                    true,
                    false
                ),
                audio,
                "{repo}"
            );
        }
        // Vocal separation: an MLX audio-to-audio repo of a tested architecture (by its
        // `model_type` tag) runs on mlx-audio; another audio-to-audio repo does not.
        let sep = &s(&["README.md", "config.json", "model.safetensors"]);
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/mel-roformer-kim-vocal-2-mlx",
                ModelFormat::Mlx,
                Some("audio-to-audio"),
                sep,
                &s(&["mlx", "safetensors", "mel_band_roformer", "audio-to-audio"]),
                true,
                false
            ),
            audio
        );
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/sam-audio-small",
                ModelFormat::Mlx,
                Some("audio-to-audio"),
                sep,
                &s(&["mlx-audio", "sam_audio", "audio-to-audio"]),
                true,
                false
            ),
            None
        );
        // npz weights count for recognition only: not for a text-to-speech repo.
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/Qwen3-TTS-12Hz-0.6B-Base-8bit",
                ModelFormat::Mlx,
                Some("text-to-speech"),
                &s(&["config.json", "weights.npz"]),
                &s(&["mlx"]),
                true,
                false
            ),
            None
        );
        // Weights without a processor or a known layout are not offered; npz is no chat model.
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/whisper-odd",
                ModelFormat::Mlx,
                asr,
                &s(&["config.json", "model.safetensors"]),
                &s(&["mlx"]),
                true,
                false
            ),
            None
        );
        assert_eq!(
            mlx_compatibility_on(
                "mlx-community/some-chat",
                ModelFormat::Mlx,
                None,
                &s(&["config.json", "weights.npz"]),
                &s(&["mlx"]),
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
    fn mlx_retrieval_repos_are_mlx_embed_on_apple_silicon() {
        let files = s(&["config.json", "model.safetensors", "tokenizer.json"]);
        let embed_tags = s(&["sentence-transformers", "bert", "feature-extraction", "mlx"]);
        let format = detect_format(&embed_tags, Some("sentence-transformers"), &files);
        assert_eq!(format, ModelFormat::Mlx);
        let runnable = Some(Compatibility::Runnable {
            runtime: MLX_EMBED_FAMILY.into(),
        });
        let on = |task, tags: &[String], apple| {
            mlx_compatibility_on(
                "mlx-community/x",
                ModelFormat::Mlx,
                Some(task),
                &files,
                tags,
                apple,
                false,
            )
        };
        assert_eq!(on("feature-extraction", &embed_tags, true), runnable);
        let rerank_tags = s(&["mlx", "qwen3", "text-ranking", "reranker"]);
        assert_eq!(on("text-ranking", &rerank_tags, true), runnable);
        assert_eq!(on("text-ranking", &rerank_tags, false), None);
        // An architecture the worker does not run.
        assert_eq!(on("feature-extraction", &s(&["mlx", "t5"]), true), None);
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
        for repo in [
            "mlx-community/chatterbox-turbo-4bit",
            "mlx-community/Soprano-80M-bf16",
            "mlx-community/VoxCPM2-4bit",
            "mlx-community/Spark-TTS-0.5B-bf16",
        ] {
            assert_eq!(
                mlx_compatibility_on(
                    repo,
                    format,
                    Some("text-to-speech"),
                    &files,
                    &[],
                    true,
                    false
                ),
                Some(Compatibility::Runnable {
                    runtime: MLX_AUDIO_FAMILY.into()
                }),
                "{repo}"
            );
            assert_eq!(
                mlx_compatibility_on(
                    repo,
                    format,
                    Some("text-to-speech"),
                    &files,
                    &[],
                    false,
                    false
                ),
                None,
                "{repo}"
            );
        }
        // The Hub tags a CSM repo with its config's `model_type`: csm-1b-8bit says `sam`, which
        // mlx-audio does not load as CSM, so only the `sesame` builds are offered.
        let tagged = |t: &str| {
            vec![
                "mlx".to_string(),
                t.to_string(),
                "text-to-speech".to_string(),
            ]
        };
        // The Hub's own tags of the three mlx-community builds (2026-10-16).
        let real = |t: &[&str]| t.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let csm_tags = real(&[
            "mlx",
            "safetensors",
            "sesame",
            "text-to-speech",
            "en",
            "base_model:sesame/csm-1b",
            "base_model:finetune:sesame/csm-1b",
            "license:apache-2.0",
            "region:us",
        ]);
        let csm_8bit_tags = real(&[
            "mlx",
            "safetensors",
            "sam",
            "model_hub_mixin",
            "pytorch_model_hub_mixin",
            "text-to-speech",
            "en",
            "license:apache-2.0",
            "8-bit",
            "region:us",
        ]);
        for (repo, tags, offered) in [
            ("mlx-community/csm-1b-fp16", tagged("sesame"), true),
            ("mlx-community/csm-1b-8bit", tagged("sam"), false),
            ("mlx-community/csm-1b", csm_tags.clone(), true),
            ("mlx-community/csm-1b-fp16", csm_tags, true),
            ("mlx-community/csm-1b-8bit", csm_8bit_tags, false),
        ] {
            let compat = mlx_compatibility_on(
                repo,
                format,
                Some("text-to-speech"),
                &files,
                &tags,
                true,
                false,
            );
            assert_eq!(compat.is_some(), offered, "{repo}: {compat:?}");
        }
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
    /// Repositories that hold one audio.cpp package: the listing shows no model family, the
    /// repository id says which.
    #[test]
    fn single_package_repos_are_runnable_on_audio_cpp() {
        for (repo, tag, task) in [
            ("audio-cpp/RE-USE-GGUF", "audio-to-audio", false),
            ("audio-cpp/SAM-Audio-GGUF", "audio-to-audio", false),
            (
                "audio-cpp/Nemotron-3-Diarization-GGUF",
                "voice-activity-detection",
                true,
            ),
        ] {
            // What the Hub lists for them: no family in the GGUF header block.
            let item = json!({
                "id": repo,
                "pipeline_tag": task_tag(repo),
                "tags": ["gguf", "audio.cpp"],
                "gguf": {"total": 9607747, "architecture": "audiocpp", "totalFileSize": 39642144},
                "siblings": [{"rfilename": "LICENSE"}, {"rfilename": "model.gguf"}],
            });
            assert!(is_audio_cpp_repo(&item), "{repo}");
            assert!(audio_cpp_repo_processing(&item), "{repo}");
            assert_eq!(audio_cpp_repo_diarization(&item), task, "{repo}");
            assert!(!audio_cpp_repo_tts(&item), "{repo}");
            assert_eq!(
                audio_cpp_compatibility(&item),
                Compatibility::Runnable {
                    runtime: crate::engine::AUDIO_CPP.into()
                }
            );
            let analysis = crate::hub_tasks::analyze_repo(&item);
            assert_eq!(analysis.pipeline_tag.as_deref(), Some(tag));
            assert_eq!(analysis.not_runnable, None);
        }
        // A lookalike from someone else is not captured.
        let item = json!({"id": "someone/RE-USE-GGUF", "pipeline_tag": "audio-to-audio",
            "gguf": {"architecture": "gtcrn"}});
        assert!(!is_audio_cpp_repo(&item));
    }

    fn task_tag(repo: &str) -> &'static str {
        if repo.contains("Diarization") {
            "voice-activity-detection"
        } else {
            "audio-to-audio"
        }
    }

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
            // Diarization answers with speaker turns: the task its own repositories declare.
            assert_eq!(
                crate::hub_tasks::analyze_repo(&item)
                    .pipeline_tag
                    .as_deref(),
                Some(if family.ends_with("_diar") {
                    "voice-activity-detection"
                } else {
                    "audio-to-audio"
                })
            );
            assert_eq!(audio_cpp_repo_diarization(&item), family.ends_with("_diar"));
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

#[cfg(test)]
mod mflux_tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| x.to_string()).collect()
    }
    /// `mflux-community/flux2-klein-4b-mflux-q4`'s files and tags (also the layout of
    /// `mlx-community/FLUX.2-Klein-4B-4bit`).
    fn q4_files() -> Vec<String> {
        s(&[
            ".gitattributes",
            "README.md",
            "text_encoder/0.safetensors",
            "text_encoder/1.safetensors",
            "text_encoder/model.safetensors.index.json",
            "tokenizer/chat_template.jinja",
            "tokenizer/tokenizer.json",
            "tokenizer/tokenizer_config.json",
            "transformer/0.safetensors",
            "transformer/1.safetensors",
            "transformer/model.safetensors.index.json",
            "vae/0.safetensors",
            "vae/model.safetensors.index.json",
        ])
    }

    #[test]
    fn mflux_klein_repos_run_on_apple_silicon_only() {
        let tags = s(&[
            "mflux",
            "safetensors",
            "mlx",
            "image-to-image",
            "flux2",
            "base_model:black-forest-labs/FLUX.2-klein-4B",
            "base_model:quantized:black-forest-labs/FLUX.2-klein-4B",
            "license:apache-2.0",
        ]);
        let runnable = Some(Compatibility::Runnable {
            runtime: MFLUX_FAMILY.into(),
        });
        let on = |task, files: &[String], tags: &[String]| {
            mflux_compatibility_on(task, files, tags, true, false)
        };
        assert_eq!(on(Some("image-to-image"), &q4_files(), &tags), runnable);
        assert_eq!(on(Some("text-to-image"), &q4_files(), &tags), runnable);
        assert_eq!(mflux_base_of(&tags), Some("flux2-klein-4b"));
        // mlx-community's: `diffusion-single-file`, `base_model:finetune:...`.
        let community = s(&[
            "diffusion-single-file",
            "safetensors",
            "base_model:black-forest-labs/FLUX.2-klein-4B",
            "base_model:finetune:black-forest-labs/FLUX.2-klein-4B",
        ]);
        assert_eq!(
            on(Some("image-to-image"), &q4_files(), &community),
            runnable
        );
        // The 9B (BFL's non-commercial license) runs too; the card shows its license.
        let nine = s(&[
            "base_model:quantized:black-forest-labs/FLUX.2-klein-9B",
            "license:other",
        ]);
        assert_eq!(on(Some("image-to-image"), &q4_files(), &nine), runnable);
        // Elsewhere, on an old macOS, for another base model, task or layout: not mflux's.
        assert_eq!(
            mflux_compatibility_on(Some("image-to-image"), &q4_files(), &tags, false, false),
            None
        );
        assert_eq!(
            mflux_compatibility_on(Some("image-to-image"), &q4_files(), &tags, true, true),
            Some(Compatibility::UnsupportedPlatform {
                runtime: MFLUX_FAMILY.into(),
                os: std::env::consts::OS.into(),
                arch: std::env::consts::ARCH.into(),
            })
        );
        let qwen = s(&["base_model:quantized:Qwen/Qwen-Image-Edit-2511"]);
        assert_eq!(on(Some("image-to-image"), &q4_files(), &qwen), None);
        let kontext = s(&["base_model:quantized:black-forest-labs/FLUX.1-Kontext-dev"]);
        assert_eq!(on(Some("image-to-image"), &q4_files(), &kontext), None);
        assert_eq!(on(Some("text-generation"), &q4_files(), &tags), None);
        // Z-Image and Qwen-Image have klein's layout; FLUX.1 adds its T5 encoder and tokenizer.
        let zimage = s(&["base_model:Tongyi-MAI/Z-Image-Turbo"]);
        assert_eq!(mflux_base_of(&zimage), Some("z-image-turbo"));
        assert_eq!(on(Some("text-to-image"), &q4_files(), &zimage), runnable);
        let qwen_image = s(&["base_model:Qwen/Qwen-Image"]);
        assert_eq!(
            on(Some("text-to-image"), &q4_files(), &qwen_image),
            runnable
        );
        let mut flux1 = q4_files();
        flux1.extend(s(&[
            "text_encoder_2/0.safetensors",
            "text_encoder_2/model.safetensors.index.json",
            "tokenizer_2/tokenizer.json",
        ]));
        for base in [
            "FLUX.1-schnell",
            "FLUX.1-dev",
            "FLUX.1-Krea-dev",
            "FLUX.1-Fill-dev",
        ] {
            let tags = s(&[&format!("base_model:black-forest-labs/{base}")]);
            assert_eq!(on(Some("text-to-image"), &flux1, &tags), runnable, "{base}");
            // Without the second encoder (or klein with one) the folder would not load.
            assert_eq!(
                on(Some("text-to-image"), &q4_files(), &tags),
                None,
                "{base}"
            );
        }
        assert_eq!(on(Some("text-to-image"), &flux1, &tags), None);
        let mut no_t5_tokenizer = flux1.clone();
        no_t5_tokenizer.retain(|f| !f.starts_with("tokenizer_2/"));
        assert!(!is_mflux_layout(&no_t5_tokenizer));
        // BFL's own diffusers repo: `model_index.json` and diffusers' weight names.
        let diffusers = s(&[
            "model_index.json",
            "transformer/diffusion_pytorch_model.safetensors",
            "transformer/config.json",
            "vae/diffusion_pytorch_model.safetensors",
            "text_encoder/model-00001-of-00002.safetensors",
            "tokenizer/tokenizer.json",
        ]);
        assert!(!is_mflux_layout(&diffusers));
        assert_eq!(on(Some("image-to-image"), &diffusers, &tags), None);
        let mut no_vae = q4_files();
        no_vae.retain(|f| !f.starts_with("vae/"));
        assert!(!is_mflux_layout(&no_vae));
        assert!(is_mflux_layout(&q4_files()));
        // A listing without files is not taken on its tags alone.
        assert_eq!(on(Some("image-to-image"), &[], &tags), None);
    }
}
