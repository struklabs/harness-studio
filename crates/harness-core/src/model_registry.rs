//! Model registry: one answer to "what is this model, which runtime runs it, does it fit here".
//!
//! The pieces already existed, each in its own place: what a Hub repo can do comes from its
//! task ([`crate::hub_tasks`]), what a local file is from its header ([`crate::domain::detect_kind`]
//! and the library's [`crate::library::ModelCaps`]), which runtime runs a format from
//! [`crate::hub_compat`], and how it fits from [`crate::vram`] and the whisper engine. This
//! module joins them without adding a guess of its own:
//!
//! capability ([`Capability`]) -> format ([`ModelFormat`]) -> runtime family ([`Compatibility`])
//! -> hardware fit ([`fit_on_machine`]).
//!
//! * [`local_record`]: a scanned file (Kütüphanem). Capabilities come from the library's caps.
//! * [`hub_capabilities`]: a Hub repo, from its task and format.
//! * [`fit_on_machine`]: the memory estimate of the engine that runs the file (llama.cpp for
//!   GGUF, whisper.cpp for ggml `.bin`).
//!
//! Formats without a runtime (safetensors, ONNX, diffusers...) and capabilities without one
//! (speech synthesis, image generation) are recorded as they are: the compatibility says
//! "unsupported", nothing is faked.

use crate::domain::ModelKind;
use crate::hardware::SystemHardwareInfo;
use crate::hub_compat::{self, Compatibility, ModelFormat};
use crate::library::ModelCaps;
use crate::models::LocalModel;
use crate::vram::{FitInput, FitResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// What a model can do. A model has several (a VLM chats and sees).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum Capability {
    /// Text generation.
    Chat,
    /// Understands images.
    Vision,
    /// Understands audio input (llama.cpp audio models; not speech recognition).
    Audio,
    Embedding,
    Rerank,
    /// Speech recognition (whisper.cpp, or sherpa-onnx for ONNX models).
    Asr,
    /// Speech synthesis (sherpa-onnx: Kokoro, Piper).
    Tts,
    /// Image / video generation. No Harness runtime yet.
    ImageGeneration,
}

/// Capability, format and runtime verdict of one model (a local file or a Hub repo).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelRecord {
    pub capabilities: Vec<Capability>,
    pub format: ModelFormat,
    /// Runtime family that runs the format (`None`: no Harness runtime does).
    pub runtime: Option<String>,
    /// `None` for a file that is not served on its own (projector, adapter, unreadable).
    pub compatibility: Option<Compatibility>,
}

impl ModelRecord {
    /// Settles whether the runtime family is installed ([`hub_compat::resolve_installed`]).
    pub fn resolve_installed(&mut self, installed: &[String]) {
        if let Some(c) = self.compatibility.as_mut() {
            hub_compat::resolve_installed(c, installed);
        }
    }
}

/// Capabilities of a Hugging Face task (`pipeline_tag`). Tasks Harness has no use for
/// (classification, detection...) give none. The NLP tasks run prompted through chat completion
/// ([`crate::hub_tasks::task_support`] marks those partial).
pub fn task_capabilities(task: &str) -> &'static [Capability] {
    use Capability::*;
    match task {
        "text-generation" | "translation" | "summarization" | "question-answering" => &[Chat],
        "image-text-to-text"
        | "visual-question-answering"
        | "any-to-any"
        | "image-to-text"
        | "document-question-answering"
        | "video-text-to-text" => &[Chat, Vision],
        "audio-text-to-text" => &[Chat, Audio],
        "feature-extraction" | "sentence-similarity" => &[Embedding],
        "text-ranking" => &[Rerank],
        "automatic-speech-recognition" => &[Asr],
        "text-to-speech" | "text-to-audio" => &[Tts],
        "text-to-image" | "image-to-image" | "text-to-video" | "image-to-video" => {
            &[ImageGeneration]
        }
        _ => &[],
    }
}

/// Capabilities of a Hub repo: its declared (or inferred) task, and a whisper ggml repo is
/// speech recognition whatever its tag says.
pub fn hub_capabilities(task: Option<&str>, format: ModelFormat) -> Vec<Capability> {
    let mut caps: Vec<Capability> = task.map(task_capabilities).unwrap_or(&[]).to_vec();
    if format == ModelFormat::WhisperGgml && !caps.contains(&Capability::Asr) {
        caps.push(Capability::Asr);
    }
    caps
}

/// What Harness can do with a Hub repo: the format's runtime, except that a text-to-image repo
/// whose weights are a single file (GGUF, safetensors, a diffusers repo with a full checkpoint
/// or a `.ckpt`) runs on stable-diffusion.cpp. The Tauri command settles whether that runtime
/// is installed ([`hub_compat::resolve_installed`]).
pub fn hub_compatibility(format: ModelFormat, capabilities: &[Capability]) -> Compatibility {
    let image = capabilities.contains(&Capability::ImageGeneration)
        && matches!(
            format,
            ModelFormat::Gguf
                | ModelFormat::Safetensors
                | ModelFormat::Diffusers
                | ModelFormat::Pytorch
        );
    if image {
        Compatibility::Runnable {
            runtime: hub_compat::SDCPP_FAMILY.to_string(),
        }
    } else {
        format.compatibility()
    }
}

/// Capabilities of a local file from the library's caps (header, template, projector).
pub fn local_capabilities(caps: &ModelCaps) -> Vec<Capability> {
    [
        (caps.chat, Capability::Chat),
        (caps.vision, Capability::Vision),
        (caps.audio, Capability::Audio),
        (caps.embedding, Capability::Embedding),
        (caps.rerank, Capability::Rerank),
        (caps.asr, Capability::Asr),
        (caps.image, Capability::ImageGeneration),
        (caps.tts, Capability::Tts),
    ]
    .into_iter()
    .filter_map(|(on, c)| on.then_some(c))
    .collect()
}

// GGUF headers are already attached to scanned files. Negative speech checks need no
// filesystem lookup (the positive cases still use the engine's exact acceptance rules).
fn could_be_crisp_asr(model: &LocalModel) -> bool {
    model
        .gguf
        .as_ref()
        .is_none_or(crate::engine::crispasr::supports_metadata)
}

fn could_be_crisp_tts(model: &LocalModel) -> bool {
    model.gguf.as_ref().is_none_or(|g| {
        g.architecture
            .as_deref()
            .is_some_and(crate::engine::crispasr_tts::is_tts_arch)
    })
}

/// Format of a scanned file: the library lists GGUF files, whisper ggml `.bin` files (the
/// scan told them apart by the ggml magic: [`ModelKind::Asr`]), text-to-speech folders
/// (ONNX, [`ModelKind::Tts`]), speech-recognition folders of sherpa-onnx (ONNX,
/// [`ModelKind::Asr`] with a `.onnx` file, see [`is_sherpa_asr`]) and ONNX encoders.
pub fn local_format(model: &LocalModel) -> ModelFormat {
    if model.architecture.as_deref() == Some("audiocpp")
        || (could_be_crisp_tts(model) && crate::engine::crispasr_tts::is_model(&model.path))
    {
        return ModelFormat::Gguf;
    }
    match model.kind {
        ModelKind::Image => match crate::engine::ModelFormat::of(&model.path) {
            crate::engine::ModelFormat::Safetensors => ModelFormat::Safetensors,
            crate::engine::ModelFormat::Ckpt => ModelFormat::Pytorch,
            _ => ModelFormat::Gguf,
        },
        ModelKind::Asr
            if crate::engine::ModelFormat::of(&model.path) == crate::engine::ModelFormat::Onnx =>
        {
            ModelFormat::Onnx
        }
        ModelKind::Asr if crate::engine::crispasr::is_asr_gguf(&model.path) => ModelFormat::Gguf,
        ModelKind::Asr => ModelFormat::WhisperGgml,
        ModelKind::Tts => ModelFormat::Onnx,
        _ if model.gguf.is_none() && crate::engine::mlx::is_any_model_dir(&model.path) => {
            ModelFormat::Mlx
        }
        _ if crate::engine::ModelFormat::of(&model.path) == crate::engine::ModelFormat::Onnx => {
            ModelFormat::Onnx
        }
        _ if crate::engine::is_mistralrs_library_model(model) => ModelFormat::Safetensors,
        _ => ModelFormat::Gguf,
    }
}

/// Whether a speech-recognition file is one of sherpa-onnx's ONNX models (whisper.cpp's are
/// ggml `.bin` files).
pub fn is_sherpa_asr(model: &LocalModel) -> bool {
    model.kind == ModelKind::Asr && crate::asr::is_sherpa_file(&model.path)
}

/// Whether a runtime serves the file by itself (a projector or adapter rides on an LLM).
fn standalone(kind: ModelKind) -> bool {
    matches!(
        kind,
        ModelKind::Llm
            | ModelKind::Embedding
            | ModelKind::Reranker
            | ModelKind::Asr
            | ModelKind::Image
            | ModelKind::Tts
    )
}

/// The record of a scanned file. Whether the runtime is installed is settled by
/// [`ModelRecord::resolve_installed`] (a local fact, not part of the scan).
pub fn local_record(model: &LocalModel, caps: &ModelCaps) -> ModelRecord {
    let format = local_format(model);
    if model.architecture.as_deref() == Some("audiocpp") {
        let runtime = crate::engine::AUDIO_CPP.to_string();
        return ModelRecord {
            capabilities: local_capabilities(caps),
            format,
            runtime: Some(runtime.clone()),
            compatibility: Some(if model.kind == ModelKind::Tts {
                Compatibility::Runnable { runtime }
            } else {
                Compatibility::Unsupported { format }
            }),
        };
    }
    if (could_be_crisp_asr(model) && crate::engine::crispasr::is_model(&model.path))
        || (could_be_crisp_tts(model) && crate::engine::crispasr_tts::is_model(&model.path))
    {
        return ModelRecord {
            capabilities: if crate::engine::crispasr_tts::is_model(&model.path) {
                vec![Capability::Tts]
            } else {
                vec![Capability::Asr]
            },
            format,
            runtime: Some(crate::engine::CRISPASR.into()),
            compatibility: Some(Compatibility::Runnable {
                runtime: crate::engine::CRISPASR.into(),
            }),
        };
    }
    if model.gguf.as_ref().map_or_else(
        || crate::engine::crispasr_tts::is_tts_gguf(&model.path),
        |g| {
            g.architecture
                .as_deref()
                .is_some_and(crate::engine::crispasr_tts::is_tts_arch)
        },
    ) {
        return ModelRecord {
            capabilities: vec![Capability::Tts],
            format: ModelFormat::Gguf,
            runtime: None,
            compatibility: Some(Compatibility::Unsupported {
                format: ModelFormat::Gguf,
            }),
        };
    }
    if model.kind == ModelKind::Image {
        // The format alone says llama.cpp (a GGUF) or nothing (safetensors): an image model is
        // stable-diffusion.cpp's whatever its container.
        return ModelRecord {
            capabilities: local_capabilities(caps),
            format,
            runtime: Some(hub_compat::SDCPP_FAMILY.to_string()),
            compatibility: Some(Compatibility::Runnable {
                runtime: hub_compat::SDCPP_FAMILY.to_string(),
            }),
        };
    }
    if let Some(family) = (format == ModelFormat::Mlx)
        .then(|| crate::engine::mlx::family_of(&model.path))
        .flatten()
    {
        // An MLX folder: its MLX engine (chat, vision or speech) runs it.
        return ModelRecord {
            capabilities: local_capabilities(caps),
            format,
            runtime: Some(family.to_string()),
            compatibility: Some(Compatibility::Runnable {
                runtime: family.to_string(),
            }),
        };
    }
    if model.gguf.is_none() && crate::engine::python_hf::is_model_dir(Path::new(&model.path)) {
        // A Hugging Face task or diffusers folder: the Python engine runs it (the format alone
        // would say GGUF, so llama.cpp).
        let dir = Path::new(&model.path);
        let format = if dir.join("model_index.json").is_file() {
            ModelFormat::Diffusers
        } else if crate::engine::python_hf::weight_files(dir)
            .iter()
            .any(|w| w.extension().is_some_and(|e| e == "safetensors"))
        {
            ModelFormat::Safetensors
        } else {
            ModelFormat::Pytorch
        };
        let runtime = crate::engine::PYTHON_HF.to_string();
        return ModelRecord {
            capabilities: local_capabilities(caps),
            format,
            runtime: Some(runtime.clone()),
            compatibility: Some(Compatibility::Runnable { runtime }),
        };
    }
    if format == ModelFormat::Safetensors && model.kind == ModelKind::Llm {
        // A transformers checkpoint folder: mistral.rs runs it as it is.
        return ModelRecord {
            capabilities: local_capabilities(caps),
            format,
            runtime: Some(hub_compat::MISTRALRS_FAMILY.to_string()),
            compatibility: Some(Compatibility::Runnable {
                runtime: hub_compat::MISTRALRS_FAMILY.to_string(),
            }),
        };
    }
    // Speech folders are ONNX too but run on sherpa-onnx, not on the ONNX encoder family that
    // `ModelFormat::runtime` names.
    let tts = model.kind == ModelKind::Tts || is_sherpa_asr(model);
    let runtime = if tts {
        Some(hub_compat::SHERPA_FAMILY)
    } else {
        format.runtime()
    };
    ModelRecord {
        capabilities: local_capabilities(caps),
        format,
        runtime: runtime.map(String::from),
        compatibility: standalone(model.kind).then(|| match (tts, runtime) {
            (true, Some(runtime)) => Compatibility::Runnable {
                runtime: runtime.to_string(),
            },
            _ => format.compatibility(),
        }),
    }
}

/// Families of the installed runtimes (`runtimes.json`), ONNX Runtime when one of its builds is
/// installed (it keeps its own folder, not `runtimes.json`), and the Python engine when its
/// environment is ready.
pub fn installed_families() -> Vec<String> {
    let mut families: Vec<String> = crate::runtimes::RuntimeRegistry::global()
        .entries()
        .unwrap_or_default()
        .into_iter()
        .map(|e| e.family)
        .collect();
    if crate::onnx_runtime::any_installed_in(&crate::onnx_runtime::root()) {
        families.push(crate::onnx_runtime::ONNX_FAMILY.to_string());
    }
    if crate::engine::python_hf::binary().is_ok() {
        families.push(crate::engine::PYTHON_HF.to_string());
    }
    if crate::engine::mlx::binary().is_ok() {
        families.push(crate::engine::MLX.to_string());
    }
    if crate::engine::mlx::vlm_binary().is_ok() {
        families.push(crate::engine::MLX_VLM.to_string());
        // The audio engine runs in the same environment.
        families.push(crate::engine::MLX_AUDIO.to_string());
    }
    families
}

fn is_whisper_file(path: &str) -> bool {
    crate::engine::server_for_model(Path::new(path))
        .is_some_and(|e| e.family() == crate::engine::WHISPER_CPP)
}

fn is_image_file(path: &str) -> bool {
    crate::engine::is_image_path(Path::new(path))
}

fn is_onnx_file(path: &str) -> bool {
    crate::engine::server_for_model(Path::new(path))
        .is_some_and(|e| e.family() == crate::engine::ONNX_FAMILY)
}

/// Memory fit of `input` on `hw`, by the engine that runs the file: whisper.cpp for a ggml
/// `.bin`, harness-onnx for an `.onnx` file, stable-diffusion.cpp for an image model, llama.cpp
/// ([`crate::vram::fit`]) for the rest. `gpu_build`: whether the installed whisper.cpp /
/// stable-diffusion.cpp build has a GPU backend (`None` = unknown, `Some(false)` keeps the
/// model on the CPU).
pub fn fit_for(input: FitInput, hw: &SystemHardwareInfo, gpu_build: Option<bool>) -> FitResult {
    let Some(path) = input.model_path.clone() else {
        return crate::vram::fit(input, hw);
    };
    if crate::engine::is_mistralrs_dir(Path::new(&path)) {
        // A safetensors folder: a rough estimate of its own (no GGUF header to read).
        let budget = crate::vram::budget_for_model(hw, Some(&path));
        return crate::engine::mistralrs().estimate_memory(&input, None, &budget, &[]);
    }
    if is_onnx_file(&path) {
        let input = crate::vram::complete_input(input);
        let budget = crate::vram::budget_for_model(hw, input.model_path.as_deref());
        return crate::engine::onnx().estimate_memory(&input, None, &budget, &[]);
    }
    let engine = if crate::engine::audio_cpp::is_model(Path::new(&path)) {
        crate::engine::audio_cpp()
    } else if is_whisper_file(&path) {
        crate::engine::whisper_cpp()
    } else if is_image_file(&path) {
        crate::engine::sd_cpp()
    } else {
        return crate::vram::fit(input, hw);
    };
    let mut input = crate::vram::complete_input(input);
    if gpu_build == Some(false) {
        input.gpu_layers = 0;
    }
    let budget = crate::vram::budget_for_model(hw, Some(&path));
    engine.estimate_memory(&input, None, &budget, &[])
}

/// [`fit_for`] with the installed runtime's build looked up.
pub fn fit_on_machine(input: FitInput, hw: &SystemHardwareInfo) -> FitResult {
    let gpu = input.model_path.as_deref().and_then(|p| {
        if is_whisper_file(p) {
            crate::engine::whisper_installed_build_has_gpu()
        } else if is_image_file(p) {
            crate::engine::sd_installed_build_has_gpu()
        } else {
            None
        }
    });
    fit_for(input, hw, gpu)
}

/// Memory fit of a stable-diffusion.cpp image model of `bytes` on `hw`: the main file plus the
/// companions it loads with (VAE, text encoders). For the Hub, whose files are not on disk to be
/// told apart from an LLM GGUF: the llama.cpp estimate read them as 32k-context chat models
/// (FLUX.2 klein 4B on a 4 GB card: Q4_K_M 2.43 GB "fits", Q4_0 2.29 GB "tight", Q2_K 1.7 GB
/// "fits", its 2.6 GB of companions not counted).
pub fn image_fit_on_machine(bytes: u64, hw: &SystemHardwareInfo) -> FitResult {
    let gpu = crate::engine::sd_installed_build_has_gpu() != Some(false);
    image_fit_for(bytes, hw, gpu)
}

fn image_fit_for(bytes: u64, hw: &SystemHardwareInfo, gpu: bool) -> FitResult {
    let input = FitInput {
        model_size_bytes: bytes,
        gpu_layers: if gpu { 999 } else { 0 },
        ..Default::default()
    };
    let budget = crate::vram::budget_for_model(hw, None);
    crate::engine::sd_cpp().estimate_memory(&input, None, &budget, &[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{detect_hardware, GpuInfo, GpuVendor, RecommendedBackend};
    use crate::vram::FitCategory;

    fn cpu_machine() -> SystemHardwareInfo {
        let mut hw = detect_hardware();
        hw.recommended_backend = RecommendedBackend::Cpu;
        hw.total_ram_bytes = 16 << 30;
        hw
    }

    fn gpu_machine(vram: u64) -> SystemHardwareInfo {
        let mut hw = detect_hardware();
        hw.recommended_backend = RecommendedBackend::Vulkan;
        hw.unified_memory_budget_bytes = None;
        hw.total_ram_bytes = 32 << 30;
        hw.gpus = vec![GpuInfo {
            index: 0,
            name: "test".into(),
            vendor: GpuVendor::Amd,
            vram_bytes: Some(vram),
            driver_version: None,
            is_discrete: true,
            is_default: true,
        }];
        hw
    }

    /// A whisper ggml file: the ggml magic and `size` bytes.
    fn whisper_file(dir: &Path, size: u64) -> String {
        let path = dir.join("ggml-base.bin");
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(size).unwrap();
        std::fs::write(&path, [0x6c, 0x6d, 0x67, 0x67]).unwrap();
        let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        f.set_len(size).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    fn tasks_map_to_capabilities() {
        use Capability::*;
        assert_eq!(task_capabilities("text-generation"), &[Chat]);
        assert_eq!(task_capabilities("image-text-to-text"), &[Chat, Vision]);
        assert_eq!(task_capabilities("feature-extraction"), &[Embedding]);
        assert_eq!(task_capabilities("text-ranking"), &[Rerank]);
        assert_eq!(task_capabilities("automatic-speech-recognition"), &[Asr]);
        assert_eq!(task_capabilities("text-to-speech"), &[Tts]);
        assert_eq!(task_capabilities("text-to-image"), &[ImageGeneration]);
        assert!(task_capabilities("image-classification").is_empty());
        assert!(task_capabilities("not-a-task").is_empty());
    }

    #[test]
    fn hub_repo_capability_follows_task_and_format() {
        // GGUF repo with no tag: the Hub's inferred task decides.
        assert_eq!(
            hub_capabilities(Some("feature-extraction"), ModelFormat::Gguf),
            vec![Capability::Embedding]
        );
        // whisper ggml is speech recognition even when HF carries no (or another) tag.
        assert_eq!(
            hub_capabilities(None, ModelFormat::WhisperGgml),
            vec![Capability::Asr]
        );
        assert_eq!(
            hub_capabilities(
                Some("automatic-speech-recognition"),
                ModelFormat::WhisperGgml
            ),
            vec![Capability::Asr]
        );
        assert!(hub_capabilities(None, ModelFormat::Safetensors).is_empty());
    }

    /// Formats no Harness runtime runs stay recorded as "no runtime", with their capability.
    #[test]
    fn capability_without_a_runtime_is_still_recorded() {
        let caps = hub_capabilities(Some("text-to-image"), ModelFormat::Diffusers);
        assert_eq!(caps, vec![Capability::ImageGeneration]);
        assert_eq!(ModelFormat::Diffusers.runtime(), None);
        // An ONNX file is runnable only for the tasks the ONNX engine serves (SAM is not one).
        assert_eq!(
            hub_compat::hub_compatibility(ModelFormat::Onnx, Some("mask-generation")),
            Compatibility::Unsupported {
                format: ModelFormat::Onnx
            }
        );
        assert_eq!(ModelFormat::Safetensors.runtime(), None);
    }

    /// A sherpa-onnx speech-recognition folder is a speech-to-text model of the ONNX format
    /// that the sherpa-onnx runtime runs; a whisper.cpp ggml file stays what it was.
    #[test]
    fn onnx_speech_recognition_runs_on_sherpa_onnx() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir
            .path()
            .join("sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8");
        std::fs::create_dir_all(&folder).unwrap();
        for f in [
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        ] {
            std::fs::write(folder.join(f), b"x").unwrap();
        }
        let asr = crate::asr::detect(&folder).unwrap();
        let mut model = crate::asr::to_local_model(&asr, dir.path());
        let caps = ModelCaps {
            asr: true,
            ..Default::default()
        };
        assert!(is_sherpa_asr(&model));
        let rec = local_record(&model, &caps);
        assert_eq!(rec.capabilities, vec![Capability::Asr]);
        assert_eq!(rec.format, ModelFormat::Onnx);
        assert_eq!(rec.runtime.as_deref(), Some("sherpa-onnx"));
        assert_eq!(
            rec.compatibility,
            Some(Compatibility::Runnable {
                runtime: "sherpa-onnx".into()
            })
        );
        model.path = dir.path().join("ggml-base.bin");
        assert!(!is_sherpa_asr(&model));
        let rec = local_record(&model, &caps);
        assert_eq!(rec.format, ModelFormat::WhisperGgml);
        assert_eq!(rec.runtime.as_deref(), Some("whisper.cpp"));
    }

    /// Folders of the Python engine (a diffusers pipeline, a transformers task model) name it,
    /// not the llama.cpp that the GGUF fallback format would.
    #[test]
    fn python_engine_folders_are_recorded_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let write = |rel: &str, body: &str| {
            let p = dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        write(
            "tiny-sd/model_index.json",
            r#"{"_class_name":"StableDiffusionPipeline"}"#,
        );
        write("tiny-sd/unet/diffusion_pytorch_model.safetensors", "x");
        write(
            "ner/config.json",
            r#"{"architectures":["BertForTokenClassification"],"id2label":{"0":"O"}}"#,
        );
        write("ner/model.safetensors", "x");
        let models = crate::engine::python_hf::library_models(&[dir.path().to_path_buf()]);
        let record = |name: &str| {
            let m = models.iter().find(|m| m.name.contains(name)).unwrap();
            local_record(m, &ModelCaps::default())
        };
        let runnable = Some(Compatibility::Runnable {
            runtime: "python-hf".into(),
        });
        let sd = record("tiny-sd");
        assert_eq!(sd.format, ModelFormat::Diffusers);
        assert_eq!(sd.runtime.as_deref(), Some("python-hf"));
        assert_eq!(sd.compatibility, runnable);
        let ner = record("ner");
        assert_eq!(ner.format, ModelFormat::Safetensors);
        assert_eq!(ner.runtime.as_deref(), Some("python-hf"));
        assert_eq!(ner.compatibility, runnable);
        // `list_library` settles the install: without the Python engine the folder needs it.
        let mut missing = record("tiny-sd");
        missing.resolve_installed(&["llama.cpp".to_string()]);
        assert_eq!(
            missing.compatibility,
            Some(Compatibility::NeedsRuntime {
                runtime: "python-hf".into()
            })
        );
        let mut installed = record("tiny-sd");
        installed.resolve_installed(&["python-hf".to_string()]);
        assert_eq!(installed.compatibility, runnable);
    }

    #[test]
    fn mlx_folders_are_recorded_as_mlx_with_their_engine() {
        let dir = tempfile::tempdir().unwrap();
        let folder = |name: &str, config: &str, card: &str| {
            let d = dir.path().join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("config.json"), config).unwrap();
            std::fs::write(d.join("model.safetensors"), b"x").unwrap();
            std::fs::write(d.join("README.md"), card).unwrap();
            d
        };
        let q = r#""quantization":{"group_size":64,"bits":4}"#;
        for (name, config, card, family) in [
            (
                "chat",
                format!(r#"{{"architectures":["Qwen3ForCausalLM"],"model_type":"qwen3",{q}}}"#),
                "",
                "mlx",
            ),
            (
                "vlm",
                format!(
                    r#"{{"architectures":["Qwen3VLForConditionalGeneration"],"model_type":"qwen3_vl","vision_config":{{}},{q}}}"#
                ),
                "",
                "mlx-vlm",
            ),
            (
                "tts",
                format!(
                    r#"{{"architectures":["Qwen3TTSForConditionalGeneration"],"model_type":"qwen3_tts",{q}}}"#
                ),
                "---\nlibrary_name: mlx-audio\n---\n",
                "mlx-audio",
            ),
        ] {
            let path = folder(name, &config, card);
            let info = crate::engine::mistralrs::inspect_checkpoint(&path).unwrap();
            let mut model = crate::engine::mistralrs::to_local_model(&path, &info, dir.path());
            if family == "mlx-audio" {
                model.kind = ModelKind::Pipeline;
            }
            assert_eq!(local_format(&model), ModelFormat::Mlx, "{name}");
            let rec = local_record(&model, &ModelCaps::default());
            assert_eq!(rec.runtime.as_deref(), Some(family), "{name}");
            assert_eq!(
                rec.compatibility,
                Some(Compatibility::Runnable {
                    runtime: family.into()
                }),
                "{name}"
            );
            // Deleting it removes the whole folder, only inside the library.
            assert!(
                crate::engine::mlx::delete_model_dir(&path, std::slice::from_ref(&path)).is_err()
            );
            crate::engine::mlx::delete_model_dir(&path, &[dir.path().to_path_buf()]).unwrap();
            assert!(!path.exists(), "{name}");
        }
    }

    #[test]
    fn whisper_fits_on_the_cpu_or_in_vram_by_the_installed_build() {
        let dir = tempfile::tempdir().unwrap();
        let path = whisper_file(dir.path(), 574 << 20);
        let input = FitInput {
            model_path: Some(path),
            gpu_layers: 999,
            ..Default::default()
        };

        let gpu = fit_for(input.clone(), &gpu_machine(8 << 30), Some(true));
        assert_eq!(gpu.category, FitCategory::FullGpu);
        assert!(gpu.estimate.vram_bytes > 574 << 20);

        // A CPU build of whisper.cpp keeps the model off the card even on a GPU machine.
        let cpu_build = fit_for(input.clone(), &gpu_machine(8 << 30), Some(false));
        assert_eq!(cpu_build.estimate.vram_bytes, 0);
        assert_eq!(cpu_build.category, FitCategory::CpuHeavy);

        let cpu = fit_for(input, &cpu_machine(), None);
        assert_eq!(cpu.estimate.vram_bytes, 0);
        assert_eq!(cpu.category, FitCategory::CpuHeavy);
    }

    #[test]
    fn gguf_goes_through_the_llama_estimate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.gguf");
        std::fs::write(&path, b"not a real header").unwrap();
        let input = FitInput {
            model_path: Some(path.to_string_lossy().to_string()),
            model_size_bytes: 2 << 30,
            context_size: 4096,
            gpu_layers: 999,
            ..Default::default()
        };
        let hw = gpu_machine(8 << 30);
        assert_eq!(
            fit_for(input.clone(), &hw, None),
            crate::vram::fit(input, &hw)
        );
    }

    /// The Hub's image files on a 4 GB card, with the companions they load with: the label only
    /// gets worse as the files grow, and a bundle larger than the card loads partly.
    #[test]
    fn an_image_model_fit_grows_with_its_files() {
        use crate::vram::FitCategory::*;
        let hw = gpu_machine(4 << 30);
        let rank = |c: FitCategory| {
            [FullGpu, Tight, Hybrid, CpuHeavy, OomRisk]
                .iter()
                .position(|x| *x == c)
        };
        let mb = |n: u64| n * 1_000_000;
        // FLUX.2 klein 4B Q2_K .. Q8_0 + Qwen3-4B Q4_K_M (2.33 GB) + VAE (321 MB).
        let companions = mb(2330) + mb(321);
        let mut last = 0;
        for main in [1700, 1960, 1980, 2290, 2410, 2430, 2510, 4010] {
            let r = image_fit_for(mb(main) + companions, &hw, true);
            let now = rank(r.category).unwrap();
            assert!(now >= last, "{main} MB: {:?}", r.category);
            assert_eq!(r.category, Hybrid, "{main} MB");
            last = now;
        }
        // SD-Turbo q8 alone fits the card.
        assert_eq!(image_fit_for(mb(1880), &hw, true).category, FullGpu);
        // A CPU build keeps everything in system memory.
        let cpu = image_fit_for(mb(1880), &hw, false);
        assert_eq!((cpu.category, cpu.estimate.vram_bytes), (CpuHeavy, 0));
    }
}
