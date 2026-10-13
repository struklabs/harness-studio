//! What Harness can do with a Hugging Face repo, from the listing's own fields (no extra
//! request): the model file format and the runtime family that runs it.
//!
//! The Hub is no longer limited to `filter=gguf` (`HfSearchOptions::include_non_gguf`), so a
//! listing mixes repos Harness runs (GGUF on llama.cpp, whisper ggml `.bin` on whisper.cpp) with
//! repos no Harness runtime runs (safetensors, diffusers...). [`detect_format`] reads the
//! format from `tags`, `library_name` and the `siblings` file names; [`Compatibility`] says what
//! follows from it.

use serde::{Deserialize, Serialize};

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
pub const ONNX_TASKS: [&str; 3] = ["feature-extraction", "sentence-similarity", "text-ranking"];

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
            && !installed.iter().any(|f| f == runtime)
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
    if library == Some("mlx") || tag("mlx") {
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
/// embeddings and reranking, or an untagged repo (the file decides). GGUF is also the container
/// of text-to-speech, speech-recognition and audio models for other engines (audio.cpp,
/// transcribe.cpp, CrispASR...) that llama-server cannot load; image GGUFs take the
/// stable-diffusion.cpp route before this.
pub fn is_llama_task(task: Option<&str>) -> bool {
    use crate::model_registry::Capability;
    task.is_none_or(|t| {
        crate::model_registry::task_capabilities(t).iter().any(|c| {
            matches!(
                c,
                Capability::Chat | Capability::Embedding | Capability::Rerank
            )
        })
    })
}

/// mistral.rs's verdict on a Hub repo: a safetensors chat model ([`MISTRALRS_TASKS`]) whose
/// listing shows a `config.json` and `.safetensors` weights at the repo root (a listing without
/// file names is taken by its tags). `None` for every other repo. Whether the runtime is
/// installed is settled later ([`resolve_installed`]).
pub fn mistralrs_compatibility(
    format: ModelFormat,
    task: Option<&str>,
    files: &[String],
) -> Option<Compatibility> {
    if format != ModelFormat::Safetensors || !task.is_some_and(|t| MISTRALRS_TASKS.contains(&t)) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| x.to_string()).collect()
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
        // An image model with an ONNX export: not an encoder task, format unchanged.
        assert_eq!(
            detect_hub_format(&tags, None, &[], Some("image-classification")),
            ModelFormat::Safetensors
        );
        // A repo that is ONNX only (transformers.js) for a vision task is Onnx but unsupported.
        let f = detect_hub_format(
            &tags,
            Some("transformers.js"),
            &[],
            Some("image-classification"),
        );
        assert_eq!(f, ModelFormat::Onnx);
        let unsupported = Compatibility::Unsupported {
            format: ModelFormat::Onnx,
        };
        assert_eq!(
            hub_compatibility(f, Some("image-classification")),
            unsupported
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
                mistralrs_compatibility(ModelFormat::Safetensors, Some(task), &files),
                runnable
            );
        }
        // Without file names the tags decide.
        assert_eq!(
            mistralrs_compatibility(ModelFormat::Safetensors, Some("text-generation"), &[]),
            runnable
        );
        // Other tasks, formats and layouts are not.
        assert_eq!(
            mistralrs_compatibility(ModelFormat::Safetensors, Some("feature-extraction"), &files),
            None
        );
        assert_eq!(
            mistralrs_compatibility(ModelFormat::Safetensors, None, &files),
            None
        );
        assert_eq!(
            mistralrs_compatibility(ModelFormat::Gguf, Some("text-generation"), &files),
            None
        );
        let nested = s(&["ckpt/config.json", "ckpt/model.safetensors"]);
        assert_eq!(
            mistralrs_compatibility(ModelFormat::Safetensors, Some("text-generation"), &nested),
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
    fn the_onnx_family_is_never_reported_missing() {
        let mut c = hub_compatibility(ModelFormat::Onnx, Some("text-ranking"));
        resolve_installed(&mut c, &[]);
        assert!(matches!(c, Compatibility::Runnable { .. }));
    }
}
