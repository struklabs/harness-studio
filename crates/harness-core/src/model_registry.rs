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

/// Format of a scanned file: the library lists GGUF files, whisper ggml `.bin` files (the
/// scan told them apart by the ggml magic: [`ModelKind::Asr`]), text-to-speech folders
/// (ONNX, [`ModelKind::Tts`]), speech-recognition folders of sherpa-onnx (ONNX,
/// [`ModelKind::Asr`] with a `.onnx` file, see [`is_sherpa_asr`]) and ONNX encoders.
pub fn local_format(model: &LocalModel) -> ModelFormat {
    match model.kind {
        ModelKind::Image => match crate::engine::ModelFormat::of(&model.path) {
            crate::engine::ModelFormat::Safetensors => ModelFormat::Safetensors,
            crate::engine::ModelFormat::Ckpt => ModelFormat::Pytorch,
            _ => ModelFormat::Gguf,
        },
        ModelKind::Asr if is_sherpa_asr(model) => ModelFormat::Onnx,
        ModelKind::Asr => ModelFormat::WhisperGgml,
        ModelKind::Tts => ModelFormat::Onnx,
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

/// Families of the installed runtimes (`runtimes.json`), and the Python engine when its
/// environment is ready.
pub fn installed_families() -> Vec<String> {
    let mut families: Vec<String> = crate::runtimes::RuntimeRegistry::global()
        .entries()
        .unwrap_or_default()
        .into_iter()
        .map(|e| e.family)
        .collect();
    if crate::engine::python_hf::binary().is_ok() {
        families.push(crate::engine::PYTHON_HF.to_string());
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
    let engine = if is_whisper_file(&path) {
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
}
