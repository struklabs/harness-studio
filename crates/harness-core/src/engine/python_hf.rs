//! The Python engine (`python-hf`) as an [`Engine`]: a Hugging Face model folder served by
//! `python/hf_worker.py` (transformers pipelines, sentence-transformers, diffusers) from the
//! managed `hf-cuda` environment where it is installed, else `hf-cpu` ([`env`]).
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
//! has an `auto_map` is not one. Text generators stay with mistral.rs, except where mistral.rs
//! is not registered: a safetensors chat or vision-chat folder then runs here as the
//! `text-generation` / `image-text-to-text` tasks ([`chat_task`]), served on
//! `/v1/chat/completions` by transformers.

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

/// Runtime family of Liquid's LFM2.5-Audio folders: the same Python worker engine, run in its own
/// managed environment ([`crate::pyenv::envs::LIQUID_AUDIO`]) with Liquid's `liquid-audio` package
/// and its own worker ([`LIQUID_WORKER`]). The model library and the gateway see an ordinary
/// text-to-speech model of the Python engine; only the interpreter and the script differ.
pub const LIQUID_AUDIO: &str = "liquid-audio";

/// The architecture of an LFM2-Audio `config.json` (`Lfm2AudioForConditionalGeneration`).
const LIQUID_ARCHITECTURE: &str = "Lfm2AudioForConditionalGeneration";

/// Voices of the model (its four system prompts), the names the worker takes.
pub const LIQUID_VOICES: &[&str] = &["us_female", "us_male", "uk_female", "uk_male"];

/// The Liquid worker script, written next to its environment ([`worker_script_for`]).
const LIQUID_WORKER: &str = include_str!("../../python/liquid_audio_worker.py");

/// Whether `dir` is an LFM2-Audio model folder in Liquid's own format (`liquid-audio` loads it):
/// a `config.json` of the Lfm2Audio architecture, the audio detokenizer and safetensors weights.
pub fn is_liquid_audio_dir(dir: &Path) -> bool {
    dir.is_dir()
        && dir.join("model.safetensors").is_file()
        && dir
            .join("audio_detokenizer")
            .join("model.safetensors")
            .is_file()
        && read_json(&dir.join("config.json"))
            .is_some_and(|cfg| archs(&cfg).contains(&LIQUID_ARCHITECTURE))
}

/// Whether `dir` is a CTranslate2 Whisper export (faster-whisper: `Systran/faster-whisper-*` and
/// fine-tunes): `model.bin`, a tokenizer or vocabulary and a `config.json` that has no
/// transformers `model_type` / `architectures` but carries Whisper's `alignment_heads`,
/// `lang_ids` or `suppress_ids`. Transformers weights next to it win (the folder is then an
/// ordinary Whisper checkpoint). Other CTranslate2 models (translation, NLLB...) are not served.
pub fn is_ct2_whisper_dir(dir: &Path) -> bool {
    let has = |name: &str| dir.join(name).is_file();
    dir.is_dir()
        && has("model.bin")
        && (has("tokenizer.json") || has("vocabulary.json") || has("vocabulary.txt"))
        && !weight_files(dir)
            .iter()
            .any(|p| p.extension().is_some_and(|e| e == "safetensors"))
        && read_json(&dir.join("config.json")).is_some_and(|cfg| {
            cfg.is_object()
                && cfg.get("model_type").is_none()
                && cfg.get("architectures").is_none()
                && ["alignment_heads", "lang_ids", "suppress_ids"]
                    .iter()
                    .any(|k| cfg.get(*k).is_some())
        })
}

/// `model_type` of the Qwen2.5-Omni checkpoints the worker runs as `any-to-any`: audio, image and
/// text in; text and speech out (`Qwen2_5OmniForConditionalGeneration`).
const OMNI_MODEL_TYPE: &str = "qwen2_5_omni";

/// Whether `dir` is a Qwen2.5-Omni checkpoint folder (safetensors weights, no remote code).
pub fn is_omni_dir(dir: &Path) -> bool {
    dir.is_dir()
        && !weight_files(dir).is_empty()
        && read_json(&dir.join("config.json")).is_some_and(|cfg| {
            cfg.get("model_type").and_then(|v| v.as_str()) == Some(OMNI_MODEL_TYPE)
        })
        && !asks_for_code(dir)
}

/// Whether the model folder answers with speech of its own on `/v1/chat/completions` (OpenAI's
/// `modalities: ["text", "audio"]`): Qwen2.5-Omni on the Python engine, LFM2.5-Audio on Liquid's.
pub fn speaks_natively(dir: &Path) -> bool {
    is_liquid_audio_dir(dir) || is_omni_dir(dir)
}

/// Whether a Hub repo (a listing item or `/api/models/<id>`) is an LFM2-Audio repo in Liquid's
/// own format: library `liquid-audio` with the audio detokenizer next to the weights. Its GGUF
/// sibling repos are llama.cpp's (speech recognition only) and are not this.
pub fn is_liquid_audio_repo(item: &serde_json::Value) -> bool {
    let library = item
        .get("library_name")
        .and_then(|l| l.as_str())
        .is_some_and(|l| l.eq_ignore_ascii_case(LIQUID_AUDIO));
    let files: Vec<&str> = item
        .get("siblings")
        .and_then(|s| s.as_array())
        .map(|s| {
            s.iter()
                .filter_map(|f| f.get("rfilename")?.as_str())
                .collect()
        })
        .unwrap_or_default();
    library
        && files.contains(&"model.safetensors")
        && files.contains(&"audio_detokenizer/model.safetensors")
}

/// The environment a model folder's worker runs in: Liquid's for an LFM2-Audio folder, else the
/// Python engine's ([`env`]).
pub fn env_for(model: &Path) -> &'static crate::pyenv::envs::EnvSpec {
    if is_liquid_audio_dir(model) {
        &crate::pyenv::envs::LIQUID_AUDIO
    } else {
        env()
    }
}

/// The managed environment the worker runs in: the CUDA build when it is ready for this
/// version, else the CPU build (the worker picks `cuda` itself when torch has it).
pub fn env() -> &'static crate::pyenv::envs::EnvSpec {
    let root = crate::pyenv::envs::root();
    env_in(&root)
}

pub(crate) fn env_in(root: &Path) -> &'static crate::pyenv::envs::EnvSpec {
    use crate::pyenv::envs::{status_in, ENVS, HF_CPU};
    ENVS.iter()
        .find(|s| s.gpu_build_of == Some(HF_CPU.name) && status_in(root, s).ready)
        .unwrap_or(&HF_CPU)
}

/// The worker script, written next to the environment ([`worker_script`]).
const WORKER: &str = include_str!("../../python/hf_worker.py");

/// Environment variable the worker reads its API key from.
pub const API_KEY_ENV: &str = "HARNESS_WORKER_API_KEY";

/// Environment variable with the browser origins a worker answers (comma-joined, see
/// [`crate::network::worker_origins`]); a request from any other `Origin` is refused.
pub const ORIGINS_ENV: &str = "HARNESS_WORKER_ORIGINS";

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

/// The chat tasks of the worker (transformers `AutoModelForCausalLM` /
/// `AutoModelForImageTextToText`, served on `/v1/chat/completions`). They are the fallback of a
/// safetensors chat repo when neither a GGUF nor mistral.rs runs it, so a model card that says
/// one of them is not a task folder of [`detect`]: [`chat_task`] decides.
pub const CHAT_TASKS: [&str; 3] = ["text-generation", "image-text-to-text", "any-to-any"];

/// Tasks the worker serves (`hf_worker.py` `TASKS`).
pub const TASKS: &[&str] = &[
    "text-generation",
    "image-text-to-text",
    "any-to-any",
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
    "image-to-image",
    "text-to-video",
    "image-to-video",
    "video-text-to-text",
];

/// The transformers version of the environment locks (`python/hf-*.lock`); the model type lists
/// below are read out of it.
pub const TRANSFORMERS_VERSION: &str = "5.19.0";

/// `model_type`s transformers' text-to-speech pipeline loads (its text-to-spectrogram and
/// text-to-waveform auto mappings in [`TRANSFORMERS_VERSION`]), without `speecht5` and
/// `fastspeech2_conformer`: their pipeline downloads a separate vocoder repo at load, which the
/// offline worker cannot. Most TTS repos on the Hub need their own package (`qwen3_tts`,
/// Kokoro, Chatterbox...): the worker cannot load those either.
pub const TTS_MODEL_TYPES: &[&str] = &[
    "bark",
    "csm",
    "fastspeech2_conformer_with_hifigan",
    "higgs_audio_v2",
    "musicgen",
    "musicgen_melody",
    "qwen2_5_omni",
    "qwen3_omni_moe",
    "seamless_m4t",
    "seamless_m4t_v2",
    "vibevoice",
    "vits",
];

/// `model_type`s transformers' speech recognition pipeline loads (CTC, speech seq2seq and transducer auto
/// mappings in [`TRANSFORMERS_VERSION`]).
pub const ASR_MODEL_TYPES: &[&str] = &[
    "canary",
    "cohere_asr",
    "data2vec-audio",
    "dia",
    "fun_asr_nano",
    "granite_speech",
    "granite_speech5_ctc",
    "granite_speech_plus",
    "hubert",
    "kyutai_speech_to_text",
    "lasr_ctc",
    "moonshine",
    "moonshine_streaming",
    "nemotron3_5_asr",
    "nemotron_asr_streaming",
    "parakeet_ctc",
    "parakeet_rnnt",
    "parakeet_tdt",
    "pop2piano",
    "qwen3_asr",
    "seamless_m4t",
    "seamless_m4t_v2",
    "sew",
    "sew-d",
    "speech-encoder-decoder",
    "speech_to_text",
    "speecht5",
    "unispeech",
    "unispeech-sat",
    "vibevoice",
    "vibevoice_asr",
    "voxtral",
    "voxtral_realtime",
    "wav2vec2",
    "wav2vec2-bert",
    "wav2vec2-conformer",
    "wavlm",
    "whisper",
];

/// Required processor packages absent from all three hash-pinned HF environments.
/// In transformers 5.19.0, NemotronAsrStreamingFeatureExtractor and
/// ParakeetFeatureExtractor require librosa to construct their mel filters.
const EXTRA_DEPENDENCIES: &[(&str, &str)] = &[
    ("nemotron3_5_asr", "librosa"),
    ("nemotron_asr_streaming", "librosa"),
    ("parakeet_ctc", "librosa"),
    ("parakeet_rnnt", "librosa"),
    ("parakeet_tdt", "librosa"),
];

pub fn missing_dependency(model_type: Option<&str>) -> Option<&'static str> {
    let model_type = model_type?;
    EXTRA_DEPENDENCIES
        .iter()
        .find_map(|(model, package)| (*model == model_type).then_some(*package))
}

/// Whether the worker's transformers loads a model of `task` with this `config.json`
/// `model_type`: checked for speech (text to speech, recognition), where many Hub repos are
/// made for another package; every other task, or a repo without a model type, is left to the
/// task.
pub fn serves_model_type(task: Option<&str>, model_type: Option<&str>) -> bool {
    let Some(model_type) = model_type.filter(|m| !m.is_empty()) else {
        return true;
    };
    match task {
        Some("text-to-speech" | "text-to-audio") => TTS_MODEL_TYPES.contains(&model_type),
        Some("automatic-speech-recognition") => ASR_MODEL_TYPES.contains(&model_type),
        _ => true,
    }
}

/// Whether the worker loads a model of `task` with these `config.json` `architectures`: a
/// reranker (`text-ranking`) must be a sequence-classification model, the head a cross-encoder
/// scores a (query, document) pair with. A decoder-only reranker (Qwen3-Reranker reads
/// "yes" / "no" off its language-model head) would load with a random head and score noise. No
/// architectures, or any other task, is left to the task.
pub fn serves_architectures(task: Option<&str>, architectures: &[&str]) -> bool {
    match task {
        Some("text-ranking") if !architectures.is_empty() => architectures
            .iter()
            .any(|a| a.ends_with("ForSequenceClassification")),
        _ => true,
    }
}

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

/// A protected flag, or an abbreviation of one (`--trust`, `--ud`): the worker's parser takes
/// no abbreviations, and this refuses them before it is asked.
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
    /// The answers of folders whose JSON files did not change (size and time of each): every
    /// engine lookup for a model asks again, and a tokenizer is megabytes of JSON.
    static ANSWERS: OnceLock<std::sync::Mutex<Answers>> = OnceLock::new();
    let files = json_files_of(dir);
    let stamp: Vec<FileStamp> = files
        .iter()
        .map(|f| {
            let meta = std::fs::metadata(f).ok();
            (
                f.clone(),
                meta.as_ref().map_or(0, |m| m.len()),
                meta.and_then(|m| m.modified().ok()),
            )
        })
        .collect();
    let answers = ANSWERS.get_or_init(Default::default);
    if let Some((seen, answer)) = answers.lock().unwrap().get(dir) {
        if *seen == stamp {
            return *answer;
        }
    }
    let answer = asks_for_code_in(dir, &files);
    let mut answers = answers.lock().unwrap();
    if answers.len() >= 256 {
        answers.clear();
    }
    answers.insert(dir.to_path_buf(), (stamp, answer));
    answer
}

/// A JSON file's path, size and modification time.
type FileStamp = (PathBuf, u64, Option<std::time::SystemTime>);

/// [`asks_for_code`] answers by folder, with the files stamps they were made from.
type Answers = std::collections::HashMap<PathBuf, (Vec<FileStamp>, bool)>;

/// JSON files that are plain data (a tokenizer's vocabulary, a weight index): megabytes long and
/// never the place of an `auto_map`. The worker reads every JSON file of the folder anyway.
fn is_data_json(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    matches!(name.as_str(), "tokenizer.json" | "vocab.json") || name.ends_with(".index.json")
}

/// The JSON files [`asks_for_code`] reads: the ones at the top of the folder and one level down.
fn json_files_of(dir: &Path) -> Vec<PathBuf> {
    let json_files = |d: &Path| -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(d)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "json"))
            .collect();
        files.sort();
        files
    };
    let mut files = json_files(dir);
    let mut subs: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    subs.sort();
    for sub in subs {
        files.extend(json_files(&sub));
    }
    files
}

fn asks_for_code_in(dir: &Path, files: &[PathBuf]) -> bool {
    if files
        .iter()
        .filter(|f| !is_data_json(f))
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
/// of a diffusers pipeline, or a text generator (mistral.rs runs those, [`chat_task`] the rest).
pub fn detect(dir: &Path) -> Option<String> {
    // An MLX speech folder (Whisper's MLX port has a transformers config) is mlx-audio's; a
    // Spark-TTS folder speaks too, though its config is a Qwen2 one.
    // An MLX embedder or reranker (a transformers config, MLX's weights) is mlx-embed's.
    if !dir.is_dir()
        || super::mlx::is_audio_dir(dir)
        || super::mlx::is_spark_dir(dir)
        || super::mlx::is_retrieval_dir(dir)
    {
        return None;
    }
    if dir
        .parent()
        .is_some_and(|p| p.join("model_index.json").is_file())
    {
        return None;
    }
    // Liquid's LFM2.5-Audio: speech output through `liquid-audio` (its card says audio-to-audio,
    // a task the transformers worker does not know).
    // Both it and Qwen2.5-Omni hear, think and speak: `any-to-any`, chatting on /v1/chat/completions
    // (Liquid's keeps /v1/audio/speech too).
    if is_liquid_audio_dir(dir) || is_omni_dir(dir) {
        return Some("any-to-any".into());
    }
    // A CTranslate2 Whisper export (faster-whisper) is served by the worker's faster-whisper.
    if is_ct2_whisper_dir(dir) {
        return Some("automatic-speech-recognition".into());
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
        if !class.ends_with("Pipeline")
            || ["Inpaint", "Audio", "Upscale", "Depth2Img", "ControlNet"]
                .iter()
                .any(|w| class.contains(w))
            || weight_files(dir).is_empty()
        {
            return None;
        }
        if class.contains("Video") || class.starts_with("CogVideoX") || class.starts_with("LTX") {
            return Some(
                if class.contains("ImageToVideo") {
                    "image-to-video"
                } else {
                    "text-to-video"
                }
                .into(),
            );
        }
        // A pipeline that needs a start image (InstructPix2Pix, Qwen-Image-Edit, SDXL refiner).
        // FLUX.2 klein and FLUX.1 Kontext also draw from text alone: text-to-image, and the
        // worker edits with the same pipeline.
        let edits = ["Img2Img", "InstructPix2Pix", "Edit"]
            .iter()
            .any(|w| class.contains(w));
        return Some(
            if edits {
                "image-to-image"
            } else {
                "text-to-image"
            }
            .into(),
        );
    }
    if asks_for_code(dir) {
        return None;
    }
    let cfg = read_json(&dir.join("config.json"))?;
    if cfg.get("_diffusers_version").is_some() || weight_files(dir).is_empty() {
        return None;
    }
    let from_config = config_task(&cfg, dir);
    if cfg.get("model_type").and_then(|v| v.as_str()) == Some("smolvlm") {
        return Some("video-text-to-text".into());
    }
    if let Some(tag) = card_pipeline_tag(dir) {
        // Most cross-encoder cards (BAAI/bge-reranker-v2-m3) say `text-classification`: a head
        // with one label scores (query, document) pairs, and `/v1/rerank` needs that task.
        if tag == "text-classification" && from_config == Some("text-ranking") {
            return Some("text-ranking".into());
        }
        // A chat or generation card (text-generation, image-text-to-text...) is not a task
        // folder: it is a chat checkpoint ([`chat_task`]).
        return (TASKS.contains(&tag.as_str()) && !CHAT_TASKS.contains(&tag.as_str()))
            .then_some(tag);
    }
    from_config.map(String::from)
}

/// Whether `path` is a folder this engine runs as a task model ([`detect`]). A safetensors chat
/// checkpoint is a mistral.rs folder first ([`crate::engine::is_mistralrs_dir`]); it runs here
/// only through [`chat_task`].
pub fn is_model_dir(path: &Path) -> bool {
    detect(path).is_some()
}

/// The chat task the worker would run a transformers text-generator checkpoint with:
/// `image-text-to-text` for a model with a vision tower, `text-generation` otherwise. `None`
/// for a folder that is not a mistral.rs-style chat checkpoint (no weights, an encoder, a task
/// model, a diffusers component) or that asks for remote code (`auto_map`), which the worker
/// never runs.
pub fn chat_task(dir: &Path) -> Option<&'static str> {
    if !super::mistralrs::is_model_dir(dir) || asks_for_code(dir) {
        return None;
    }
    // `is_model_dir` has just vouched for the folder.
    let vision = super::mistralrs::inspect_checkpoint(dir).is_some_and(|i| i.vision);
    Some(if vision {
        "image-text-to-text"
    } else {
        "text-generation"
    })
}

/// The task a launch of `dir` runs: a task folder's ([`detect`]), else a chat checkpoint's
/// ([`chat_task`]).
pub fn launch_task(dir: &Path) -> Option<String> {
    detect(dir).or_else(|| chat_task(dir).map(String::from))
}

/// The library kind of a task: embedding and ranking models keep their kinds (the gateway
/// sends `/v1/embeddings` and `/v1/rerank` to them), chat tasks are language models (they chat
/// on `/v1/chat/completions`), everything else is a pipeline model.
pub fn kind_of(task: &str) -> ModelKind {
    match task {
        "feature-extraction" | "sentence-similarity" => ModelKind::Embedding,
        "text-ranking" => ModelKind::Reranker,
        t if CHAT_TASKS.contains(&t) => ModelKind::Llm,
        _ => ModelKind::Pipeline,
    }
}

/// The serving mode of a model folder.
pub fn serve_mode(dir: &Path) -> crate::serve_mode::ServeMode {
    crate::serve_mode::ServeMode::for_kind(
        launch_task(dir).map_or(ModelKind::Pipeline, |t| kind_of(&t)),
    )
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

pub(crate) fn library_models_in(
    root: &Path,
    tree: &crate::models::discovery::DirectoryTree,
) -> Vec<crate::models::LocalModel> {
    tree.scan(
        root,
        &["config.json", "model_index.json"],
        |dir| {
            detect(dir)
                .filter(|task| !has_onnx(dir) || task == "video-text-to-text")
                .map(|task| (dir.to_path_buf(), task))
        },
        |m| m.0.clone(),
    )
    .into_iter()
    .map(|(dir, task)| to_local_model(&dir, task, root))
    .collect()
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
                        .filter(|task| !has_onnx(dir) || task == "video-text-to-text")
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
    let dir = crate::pyenv::envs::env_dir_in(&crate::pyenv::envs::root(), env().name);
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
    binary_in(&crate::pyenv::envs::root())
}

/// [`binary`] for the environments under `root`.
pub fn binary_in(root: &Path) -> Result<PathBuf, String> {
    let spec = env_in(root);
    let status = crate::pyenv::envs::status_in(root, spec);
    if !status.ready {
        return Err(format!(
            "{}: {}",
            crate::runtimes::ERR_FAMILY_NOT_INSTALLED,
            PYTHON_HF
        ));
    }
    Ok(crate::pyenv::envs::python_in(root, spec.name))
}

/// The worker script of the model at `model`, written next to its environment: the Liquid worker
/// for an LFM2-Audio folder, else [`worker_script`].
pub fn worker_script_for(model: &Path) -> Result<PathBuf, String> {
    if !is_liquid_audio_dir(model) {
        return worker_script();
    }
    let dir = crate::pyenv::envs::env_dir_in(
        &crate::pyenv::envs::root(),
        crate::pyenv::envs::LIQUID_AUDIO.name,
    );
    let path = dir.join("liquid_audio_worker.py");
    if std::fs::read_to_string(&path).ok().as_deref() == Some(LIQUID_WORKER) {
        return Ok(path);
    }
    std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(&path, LIQUID_WORKER))
        .map_err(|e| {
            String::from(
                err("fs.write_failed")
                    .with("path", path.display())
                    .with("error", e),
            )
        })?;
    Ok(path)
}

/// The interpreter a launch of the model at `model` runs: [`binary`], or Liquid's environment's
/// for an LFM2-Audio folder (`runtime.family_not_installed: liquid-audio` while it is missing).
pub fn binary_for(model: &Path) -> Result<PathBuf, String> {
    if !is_liquid_audio_dir(model) {
        return binary();
    }
    liquid_binary()
}

/// The interpreter of Liquid's environment, when it is ready.
pub fn liquid_binary() -> Result<PathBuf, String> {
    let root = crate::pyenv::envs::root();
    let spec = &crate::pyenv::envs::LIQUID_AUDIO;
    if !crate::pyenv::envs::status_in(&root, spec).ready {
        return Err(format!(
            "{}: {}",
            crate::runtimes::ERR_FAMILY_NOT_INSTALLED,
            LIQUID_AUDIO
        ));
    }
    Ok(crate::pyenv::envs::python_in(&root, spec.name))
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
pub fn vram_need(config: &LaunchConfig) -> u64 {
    let Some(device) = gpu_device(config) else {
        return 0;
    };
    let dir = config.model_path.as_path();
    // Without a safetensors header to read: `config.json`'s dtype, or a diffusers pipeline's
    // `*.fp16.*` variant files.
    let config_16 = stored_16_bit(&dir.to_string_lossy());
    let weights: u64 = weight_files(dir)
        .iter()
        .filter_map(|f| {
            let size = std::fs::metadata(f).ok()?.len();
            let bits = safetensors_float_bits(f).unwrap_or(
                if config_16 || f.to_string_lossy().to_ascii_lowercase().contains(".fp16.") {
                    16
                } else {
                    32
                },
            );
            Some(loaded_weight_bytes(config, device, size, bits))
        })
        .sum();
    // The CUDA / MPS context and the interpreter's tensors take ~300 MB; the KV cache and the
    // activations a quarter of the weights; a diffusers pipeline's activations at 512² and up
    // ~1.5 GB (the worker slices attention and tiles the VAE on MPS).
    let overhead = if dir.join("model_index.json").is_file() {
        1536u64 << 20
    } else {
        weights / 4
    };
    weights + overhead + (300u64 << 20)
}

/// Bits per value of a safetensors file's floating-point tensors (the most bytes), from its
/// header; `None` for another format or a header that cannot be read.
fn safetensors_float_bits(file: &Path) -> Option<u64> {
    use std::io::Read;
    if !file.to_string_lossy().ends_with(".safetensors") {
        return None;
    }
    let mut f = std::fs::File::open(file).ok()?;
    let mut len = [0u8; 8];
    f.read_exact(&mut len).ok()?;
    let len = u64::from_le_bytes(len);
    if len > 100 << 20 {
        return None;
    }
    let mut header = vec![0u8; len as usize];
    f.read_exact(&mut header).ok()?;
    let header: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&header).ok()?;
    // Bytes of 16-bit and of 32-bit floats.
    let mut bytes = [0u64; 2];
    for tensor in header.values() {
        let span = tensor
            .get("data_offsets")
            .and_then(|o| o.as_array())
            .and_then(|o| o.get(1)?.as_u64()?.checked_sub(o.first()?.as_u64()?))
            .unwrap_or(0);
        match tensor.get("dtype").and_then(|d| d.as_str()) {
            Some("BF16" | "F16") => bytes[0] += span,
            Some("F32") => bytes[1] += span,
            _ => {}
        }
    }
    match bytes {
        [0, 0] => None,
        [b16, b32] if b16 >= b32 => Some(16),
        _ => Some(32),
    }
}

/// The worker's `auto` device is MPS here (`hf_worker.py`, `liquid_audio_worker.py`).
const APPLE_SILICON: bool = cfg!(all(target_os = "macos", target_arch = "aarch64"));

/// The GPU a Python worker puts its weights on: CUDA with the GPU build of its environment,
/// MPS (unified memory) on Apple Silicon, none on a CPU build or when the launch asks for the
/// CPU (`device` param).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuDevice {
    Cuda,
    Mps,
}

pub fn gpu_device(config: &LaunchConfig) -> Option<GpuDevice> {
    let asked = match config.params.get("device") {
        Some(crate::params::ParamValue::Text(d)) => d.trim().to_ascii_lowercase(),
        _ => String::new(),
    };
    match asked.as_str() {
        "cpu" => None,
        "cuda" => Some(GpuDevice::Cuda),
        "mps" => Some(GpuDevice::Mps),
        _ if APPLE_SILICON => Some(GpuDevice::Mps),
        _ => env_for(&config.model_path)
            .gpu_build_of
            .is_some()
            .then_some(GpuDevice::Cuda),
    }
}

/// Bytes the weights take once loaded in the dtype the worker picks: a launch's own `dtype`,
/// else float16 for a chat model on MPS, float32 for a diffusers pipeline or LFM2-Audio on MPS,
/// float16 / the stored dtype on CUDA.
fn loaded_weight_bytes(
    config: &LaunchConfig,
    device: GpuDevice,
    stored: u64,
    stored_bits: u64,
) -> u64 {
    let dir = config.model_path.as_path();
    let asked = match config.params.get("dtype") {
        Some(crate::params::ParamValue::Text(d)) => d.trim().to_ascii_lowercase(),
        _ => String::new(),
    };
    let bits = match asked.as_str() {
        "float32" => 32,
        "float16" | "bfloat16" => 16,
        _ => match device {
            GpuDevice::Mps if chat_task(dir).is_some() => 16,
            GpuDevice::Mps
                if dir.join("model_index.json").is_file() || is_liquid_audio_dir(dir) =>
            {
                32
            }
            GpuDevice::Cuda
                if dir.join("model_index.json").is_file() || is_liquid_audio_dir(dir) =>
            {
                16
            }
            _ => stored_bits,
        },
    };
    stored / stored_bits * bits
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
        let task = launch_task(&config.model_path).ok_or_else(|| {
            String::from(err("pyhf.model_unsupported").with("path", config.model_path.display()))
        })?;
        if is_liquid_audio_dir(&config.model_path) {
            return build_liquid_args(config, &worker_script_for(&config.model_path)?);
        }
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
        // A chat checkpoint stored in 16 bits runs in float32 on a CPU without native bfloat16
        // (the worker's choice): twice the weights in RAM. MPS loads it in float16.
        let model = if input.model_path.as_deref().is_some_and(|p| {
            chat_task(Path::new(p)).is_some()
                && env().gpu_build_of.is_none()
                && !APPLE_SILICON
                && stored_16_bit(p)
        }) {
            model * 2
        } else {
            model
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

/// Whether a checkpoint's `config.json` says its weights are bfloat16 / float16.
fn stored_16_bit(dir: &str) -> bool {
    read_json(&Path::new(dir).join("config.json"))
        .and_then(|c| {
            ["dtype", "torch_dtype"]
                .iter()
                .find_map(|k| c.get(*k).and_then(|v| v.as_str()).map(String::from))
        })
        .is_some_and(|d| d.contains("16"))
}

/// The command line of an LFM2-Audio launch: the Liquid worker takes the model folder, the
/// address and an alias only (its device is chosen by the worker; custom arguments are not taken,
/// the worker has no flags to tune).
fn build_liquid_args(config: &LaunchConfig, script: &Path) -> Result<BuildOutput, String> {
    if config
        .custom_args
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
    {
        return Err(err("liquid_audio.custom_args_unsupported").into());
    }
    let mut args: Vec<String> = vec![
        script.to_string_lossy().to_string(),
        "--model".into(),
        config.model_path.to_string_lossy().to_string(),
        "--host".into(),
        config.host.clone(),
        "--port".into(),
        config.port.to_string(),
    ];
    if let Some(alias) = crate::process::served_alias(config) {
        args.push("--alias".into());
        args.push(alias);
    }
    Ok(BuildOutput {
        args,
        skipped: Vec::new(),
    })
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
    /// The model type lists were read out of the transformers the locks pin: a lock update
    /// must refresh them (`MODEL_FOR_TEXT_TO_SPECTROGRAM/WAVEFORM_MAPPING_NAMES`,
    /// `MODEL_FOR_CTC/SPEECH_SEQ_2_SEQ_MAPPING_NAMES` of transformers.models.auto).
    #[test]
    fn model_type_lists_follow_the_locked_transformers() {
        let pin = format!("transformers=={} ", super::TRANSFORMERS_VERSION);
        for lock in [
            include_str!("../../python/hf-cpu.lock"),
            include_str!("../../python/hf-cuda-windows.lock"),
            include_str!("../../python/hf-cuda-linux.lock"),
        ] {
            assert!(
                lock.contains(&pin),
                "a lock pins another transformers than {pin}"
            );
        }
        assert!(super::serves_model_type(
            Some("text-to-speech"),
            Some("vits")
        ));
        assert!(!super::serves_model_type(
            Some("text-to-speech"),
            Some("qwen3_tts")
        ));
        assert!(super::serves_model_type(
            Some("text-classification"),
            Some("anything")
        ));
        assert!(super::serves_model_type(Some("text-to-speech"), None));
        // SpeechT5 fetches microsoft/speecht5_hifigan at load: not offline-servable as TTS.
        assert!(!super::serves_model_type(
            Some("text-to-speech"),
            Some("speecht5")
        ));
        assert!(super::serves_model_type(
            Some("automatic-speech-recognition"),
            Some("speecht5")
        ));
    }

    #[test]
    fn rerankers_must_be_sequence_classification_models() {
        let ok = |task, archs: &[&str]| super::serves_architectures(Some(task), archs);
        assert!(ok("text-ranking", &["XLMRobertaForSequenceClassification"]));
        assert!(ok("text-ranking", &["Qwen3ForSequenceClassification"]));
        // Qwen3-Reranker reads yes / no off its language-model head: no cross-encoder.
        assert!(!ok("text-ranking", &["Qwen3ForCausalLM"]));
        // No architectures in the listing: the files decide later.
        assert!(ok("text-ranking", &[]));
        assert!(ok("feature-extraction", &["Qwen3Model"]));
        assert!(ok(
            "text-classification",
            &["BertForSequenceClassification"]
        ));
        assert!(super::serves_architectures(None, &["Qwen3ForCausalLM"]));
    }

    use super::*;

    #[test]
    fn the_worker_runs_in_the_ready_cuda_build_else_the_cpu_one() {
        use crate::pyenv::envs::HF_CPU;
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            env_in(dir.path()).name,
            HF_CPU.name,
            "nothing installed: the CPU build is asked for"
        );
        #[cfg(all(any(windows, target_os = "linux"), target_arch = "x86_64"))]
        {
            crate::pyenv::envs::tests::fake_ready(dir.path(), &HF_CPU);
            assert_eq!(env_in(dir.path()).name, HF_CPU.name);
            crate::pyenv::envs::tests::fake_ready(dir.path(), &crate::pyenv::envs::HF_CUDA);
            assert_eq!(env_in(dir.path()).name, "hf-cuda");
        }
    }

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
    fn detects_a_ctranslate2_whisper_export_as_speech_recognition() {
        let ct2 = r#"{"alignment_heads":[[1,2]],"lang_ids":[50259],"suppress_ids":[1]}"#;
        let export = folder(&[
            ("config.json", ct2),
            ("model.bin", "x"),
            ("tokenizer.json", "{}"),
            ("vocabulary.txt", "a"),
        ]);
        assert!(is_ct2_whisper_dir(export.path()));
        assert_eq!(
            detect(export.path()).as_deref(),
            Some("automatic-speech-recognition")
        );
        // A vocabulary alone is enough; the older exports have no tokenizer.json.
        let old = folder(&[
            ("config.json", ct2),
            ("model.bin", "x"),
            ("vocabulary.json", "[]"),
        ]);
        assert!(is_ct2_whisper_dir(old.path()));
        // Other CTranslate2 models (no Whisper keys), transformers configs, a missing
        // vocabulary and transformers weights next to the export are not served this way.
        let nllb = folder(&[
            (
                "config.json",
                r#"{"add_source_bos":false,"unk_token":"<unk>"}"#,
            ),
            ("model.bin", "x"),
            ("shared_vocabulary.json", "[]"),
            ("tokenizer.json", "{}"),
        ]);
        assert!(!is_ct2_whisper_dir(nllb.path()));
        assert_eq!(detect(nllb.path()), None);
        let transformers = folder(&[
            (
                "config.json",
                r#"{"model_type":"whisper","alignment_heads":[[1,2]]}"#,
            ),
            ("model.bin", "x"),
            ("tokenizer.json", "{}"),
        ]);
        assert!(!is_ct2_whisper_dir(transformers.path()));
        let bare = folder(&[("config.json", ct2), ("model.bin", "x")]);
        assert!(!is_ct2_whisper_dir(bare.path()));
        let both = folder(&[
            ("config.json", ct2),
            ("model.bin", "x"),
            ("tokenizer.json", "{}"),
            ("model.safetensors", "x"),
        ]);
        assert!(!is_ct2_whisper_dir(both.path()));
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
        // A cross-encoder whose card says text-classification is still a reranker, and a real
        // classifier with that card stays one.
        let card = "---
library_name: sentence-transformers
pipeline_tag: text-classification
---
# x";
        let bge = folder(&[
            (
                "config.json",
                r#"{"architectures":["XLMRobertaForSequenceClassification"],"id2label":{"0":"LABEL_0"}}"#,
            ),
            ("README.md", card),
            ("model.safetensors", "x"),
        ]);
        assert_eq!(detect(bge.path()).as_deref(), Some("text-ranking"));
        let sst = folder(&[
            (
                "config.json",
                r#"{"architectures":["BertForSequenceClassification"],"id2label":{"0":"NEG","1":"POS"}}"#,
            ),
            ("README.md", card),
            ("model.safetensors", "x"),
        ]);
        assert_eq!(detect(sst.path()).as_deref(), Some("text-classification"));
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
        for (class, task) in [
            ("StableDiffusionInstructPix2PixPipeline", "image-to-image"),
            ("StableDiffusionXLImg2ImgPipeline", "image-to-image"),
            ("QwenImageEditPlusPipeline", "image-to-image"),
            ("Flux2KleinPipeline", "text-to-image"),
            ("FluxKontextPipeline", "text-to-image"),
            ("LTXPipeline", "text-to-video"),
            ("LTXImageToVideoPipeline", "image-to-video"),
            ("CogVideoXPipeline", "text-to-video"),
        ] {
            let dir = folder(&[
                (
                    "model_index.json",
                    &format!(r#"{{"_class_name":"{class}"}}"#),
                ),
                ("transformer/diffusion_pytorch_model.safetensors", "x"),
            ]);
            assert_eq!(detect(dir.path()).as_deref(), Some(task), "{class}");
        }
    }

    #[test]
    fn video_understanding_detects_smolvlm_despite_its_image_card() {
        let dir = folder(&[
            ("config.json", r#"{"model_type":"smolvlm"}"#),
            ("model.safetensors", "x"),
            ("README.md", "---\npipeline_tag: image-text-to-text\n---\n"),
        ]);
        assert_eq!(detect(dir.path()).as_deref(), Some("video-text-to-text"));
        assert_eq!(kind_of("video-text-to-text"), ModelKind::Pipeline);
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

    fn liquid_folder() -> tempfile::TempDir {
        folder(&[
            (
                "config.json",
                r#"{"architectures":["Lfm2AudioForConditionalGeneration"],"codebooks":8}"#,
            ),
            ("model.safetensors", "x"),
            ("audio_detokenizer/model.safetensors", "x"),
            (
                "README.md",
                "---
pipeline_tag: audio-to-audio
library_name: liquid-audio
---
",
            ),
        ])
    }

    #[test]
    fn lfm2_audio_folders_chat_and_speak_in_their_own_environment() {
        let dir = liquid_folder();
        assert!(is_liquid_audio_dir(dir.path()));
        // The card says audio-to-audio, a task the transformers worker does not know: the folder
        // is still detected, as an any-to-any chat model that speaks (and keeps /v1/audio/speech).
        assert_eq!(detect(dir.path()).as_deref(), Some("any-to-any"));
        assert_eq!(launch_task(dir.path()).as_deref(), Some("any-to-any"));
        assert_eq!(kind_of("any-to-any"), ModelKind::Llm);
        assert!(speaks_natively(dir.path()));
        assert_eq!(env_for(dir.path()).name, "liquid-audio");
        // Without the audio detokenizer (a llama.cpp conversion, a partial download) it is not.
        std::fs::remove_file(dir.path().join("audio_detokenizer/model.safetensors")).unwrap();
        assert!(!is_liquid_audio_dir(dir.path()));
        assert_eq!(detect(dir.path()), None);
        assert_ne!(env_for(dir.path()).name, "liquid-audio");
    }

    #[test]
    fn qwen2_5_omni_folders_run_as_any_to_any_in_the_transformers_environment() {
        let config = r#"{"architectures":["Qwen2_5OmniModel"],"model_type":"qwen2_5_omni"}"#;
        let dir = folder(&[
            ("config.json", config),
            ("model-00001-of-00003.safetensors", "x"),
            (
                "README.md",
                "---
pipeline_tag: any-to-any
library_name: transformers
---
",
            ),
        ]);
        assert!(is_omni_dir(dir.path()));
        assert!(speaks_natively(dir.path()));
        assert!(!is_liquid_audio_dir(dir.path()));
        assert_eq!(detect(dir.path()).as_deref(), Some("any-to-any"));
        assert_eq!(launch_task(dir.path()).as_deref(), Some("any-to-any"));
        assert_ne!(env_for(dir.path()).name, "liquid-audio");
        // Remote code is never run, and a plain Qwen2.5 chat model does not speak.
        let remote = folder(&[
            (
                "config.json",
                r#"{"model_type":"qwen2_5_omni","auto_map":{"AutoModel":"x.Y"}}"#,
            ),
            ("model.safetensors", "x"),
        ]);
        assert!(!is_omni_dir(remote.path()));
        let qwen = folder(&[
            ("config.json", r#"{"model_type":"qwen2"}"#),
            ("model.safetensors", "x"),
        ]);
        assert!(!speaks_natively(qwen.path()));
    }

    #[test]
    fn lfm2_audio_launches_the_liquid_worker_without_task_or_custom_args() {
        let dir = liquid_folder();
        let config = LaunchConfig {
            model_path: dir.path().to_path_buf(),
            host: "127.0.0.1".into(),
            port: 8124,
            ..Default::default()
        };
        let out = build_liquid_args(&config, Path::new("/w/liquid_audio_worker.py")).unwrap();
        assert_eq!(out.args[0], "/w/liquid_audio_worker.py");
        assert!(out.args.windows(2).any(|w| w == ["--port", "8124"]));
        assert!(out.args.windows(2).any(|w| w == ["--host", "127.0.0.1"]));
        assert!(!out.args.iter().any(|a| a == "--task" || a == "--threads"));
        let mut args = out.args.clone();
        listen_on_socket(&mut args, Path::new("/run/l.sock"));
        assert!(args.windows(2).any(|w| w == ["--uds", "/run/l.sock"]));
        assert!(!args.iter().any(|a| a == "--port"));
        let custom = LaunchConfig {
            custom_args: Some("--device cpu".into()),
            ..config
        };
        let e = build_liquid_args(&custom, Path::new("/w/x.py")).unwrap_err();
        assert!(e.starts_with("liquid_audio.custom_args_unsupported"));
    }

    #[test]
    fn lfm2_audio_hub_repos_are_told_from_their_gguf_siblings() {
        let repo = serde_json::json!({
            "id": "LiquidAI/LFM2.5-Audio-1.5B",
            "library_name": "liquid-audio",
            "siblings": [
                {"rfilename": "model.safetensors"},
                {"rfilename": "audio_detokenizer/model.safetensors"},
                {"rfilename": "config.json"}
            ]
        });
        assert!(is_liquid_audio_repo(&repo));
        let gguf = serde_json::json!({
            "id": "LiquidAI/LFM2.5-Audio-1.5B-GGUF",
            "library_name": "liquid-audio",
            "siblings": [{"rfilename": "LFM2.5-Audio-1.5B-Q4_0.gguf"}]
        });
        assert!(!is_liquid_audio_repo(&gguf));
    }

    #[test]
    fn the_liquid_lock_pins_the_package_by_hash() {
        let lock = crate::pyenv::envs::LIQUID_AUDIO.lock;
        assert!(lock.contains("liquid-audio==1.3.0"));
        assert!(lock.contains("--index-url https://download.pytorch.org/whl/cpu"));
        assert!(!lock.contains("+cu"));
        let pins = lock
            .lines()
            .filter(|l| l.contains("==") && !l.trim_start().starts_with('#'))
            .count();
        assert!(pins > 40 && lock.matches("--hash=sha256:").count() >= pins);
        // The worker script ships and answers the voices the gateway maps.
        for voice in LIQUID_VOICES {
            assert!(LIQUID_WORKER.contains(&format!("\"{voice}\"")), "{voice}");
        }
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

    /// Browsers: the worker answers the Harness window's preflight (no key needed), refuses
    /// every other Origin with 403 before the key, and keeps the key for real requests. Run
    /// over a stand-in Starlette (the checks are the worker's own ASGI code).
    /// A diffusers pipeline whose PyTorch `.bin` is a malicious pickle does not load and its code
    /// does not run (`torch.load(weights_only=True)`): `python/test_hf_worker_pickle.py` with the
    /// hf-cpu environment's Python in `HS_PYHF_PYTHON` (torch, diffusers, transformers). Skipped
    /// without it.
    #[test]
    fn pickle_weights_do_not_run_code() {
        run_worker_script(
            "test_hf_worker_pickle.py",
            include_str!("../../python/test_hf_worker_pickle.py"),
        );
    }

    /// An image-to-image pipeline (InstructPix2Pix, tiny and random) loads as its own class and
    /// edits through itself; a `_class_name` that is not a diffusers pipeline does not load:
    /// `python/test_hf_worker_img2img.py`, with `HS_PYHF_PYTHON` as above.
    #[test]
    fn image_to_image_pipelines_edit() {
        run_worker_script(
            "test_hf_worker_img2img.py",
            include_str!("../../python/test_hf_worker_img2img.py"),
        );
    }

    /// The worker's chat helpers (request checking, message conversion, tool-call parsing, stop
    /// strings, SSE framing) need no torch: `python/test_hf_worker_chat.py` runs with any Python 3.
    #[test]
    fn chat_helpers_parse_and_frame() {
        let Some(python) = crate::engine::mlx::tests::system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let worker = dir.path().join("hf_worker.py");
        std::fs::write(&worker, WORKER).unwrap();
        let script = dir.path().join("test_hf_worker_chat.py");
        std::fs::write(&script, include_str!("../../python/test_hf_worker_chat.py")).unwrap();
        let out = std::process::Command::new(python)
            .arg("-I")
            .arg(&script)
            .arg(&worker)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("PASS"),
            "{stdout}
{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// The Liquid worker's chat helpers (request checking, OpenAI answer and SSE shaping) need no
    /// torch: `python/test_liquid_audio_worker.py` runs with any Python 3.
    #[test]
    fn liquid_chat_helpers_shape_requests_and_answers() {
        let Some(python) = crate::engine::mlx::tests::system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let worker = dir.path().join("liquid_audio_worker.py");
        std::fs::write(&worker, LIQUID_WORKER).unwrap();
        let script = dir.path().join("test_liquid_audio_worker.py");
        std::fs::write(
            &script,
            include_str!("../../python/test_liquid_audio_worker.py"),
        )
        .unwrap();
        let out = std::process::Command::new(python)
            .arg("-I")
            .arg(&script)
            .arg(&worker)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("PASS"),
            "{stdout}
{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn chat_checkpoints_are_chat_tasks_not_task_folders() {
        let text = folder(&[
            (
                "config.json",
                r#"{"architectures":["Qwen2ForCausalLM"],"model_type":"qwen2"}"#,
            ),
            (
                "README.md",
                "---
library_name: transformers
pipeline_tag: text-generation
---
# x",
            ),
            ("model.safetensors", "x"),
        ]);
        // The card's chat tag does not make it a task folder (mistral.rs keeps those)...
        assert_eq!(detect(text.path()), None);
        assert!(!is_model_dir(text.path()));
        // ...but the Python engine can run it as a chat model.
        assert_eq!(chat_task(text.path()), Some("text-generation"));
        assert_eq!(launch_task(text.path()).as_deref(), Some("text-generation"));
        let vlm = folder(&[
            (
                "config.json",
                r#"{"architectures":["Idefics3ForConditionalGeneration"],"model_type":"idefics3","vision_config":{}}"#,
            ),
            ("preprocessor_config.json", "{}"),
            ("model.safetensors", "x"),
        ]);
        assert_eq!(chat_task(vlm.path()), Some("image-text-to-text"));
        // An encoder, a task model, an empty folder and remote code are not chat checkpoints.
        let encoder = folder(&[
            ("config.json", r#"{"architectures":["BertModel"]}"#),
            ("model.safetensors", "x"),
        ]);
        let task_model = folder(&[
            (
                "config.json",
                r#"{"architectures":["BlipForConditionalGeneration"],"model_type":"blip"}"#,
            ),
            ("model.safetensors", "x"),
        ]);
        let remote = folder(&[
            (
                "config.json",
                r#"{"architectures":["XForCausalLM"],"auto_map":{"AutoModel":"m.M"}}"#,
            ),
            ("model.safetensors", "x"),
        ]);
        let empty = folder(&[("config.json", r#"{"architectures":["Qwen2ForCausalLM"]}"#)]);
        for dir in [&encoder, &task_model, &remote, &empty] {
            assert_eq!(chat_task(dir.path()), None, "{:?}", dir.path());
        }
        assert_eq!(
            launch_task(task_model.path()).as_deref(),
            Some("image-to-text")
        );
        // Library kinds: chat tasks are language models, the rest as before.
        for task in CHAT_TASKS {
            assert_eq!(kind_of(task), ModelKind::Llm);
            assert!(TASKS.contains(&task));
        }
        assert_eq!(kind_of("feature-extraction"), ModelKind::Embedding);
        assert_eq!(kind_of("fill-mask"), ModelKind::Pipeline);
    }

    /// A chat checkpoint stored in 16 bits is weighed as float32 on the CPU build.
    #[test]
    fn chat_memory_counts_float32_weights_on_the_cpu() {
        let dir = folder(&[
            (
                "config.json",
                r#"{"architectures":["Qwen2ForCausalLM"],"dtype":"bfloat16"}"#,
            ),
            ("model.safetensors", "x"),
        ]);
        let budget = MemoryBudget {
            ram_bytes: 64 << 30,
            ..Default::default()
        };
        let input = FitInput {
            model_path: Some(dir.path().to_string_lossy().to_string()),
            model_size_bytes: 1 << 30,
            ..Default::default()
        };
        let fit = PythonHfEngine.estimate_memory(&input, None, &budget, &[]);
        if env().gpu_build_of.is_none() && !APPLE_SILICON {
            assert_eq!(fit.estimate.model_host_bytes, 2 << 30);
        }
        // A task model is never doubled.
        let task = folder(&[
            (
                "config.json",
                r#"{"architectures":["BertForSequenceClassification"],"dtype":"bfloat16"}"#,
            ),
            ("model.safetensors", "x"),
        ]);
        let input = FitInput {
            model_path: Some(task.path().to_string_lossy().to_string()),
            ..input
        };
        let fit = PythonHfEngine.estimate_memory(&input, None, &budget, &[]);
        assert_eq!(fit.estimate.model_host_bytes, 1 << 30);
    }

    /// The GPU memory a worker reserves with the job scheduler: the weights in the dtype the
    /// worker loads them in on its device plus the KV cache / activations and the context; none
    /// on the CPU.
    #[test]
    fn gpu_need_follows_the_device_and_the_loaded_dtype() {
        const MIB: u64 = 1 << 20;
        let weights = vec![0u8; 8 * MIB as usize];
        let chat = folder(&[(
            "config.json",
            r#"{"architectures":["Qwen2ForCausalLM"],"dtype":"bfloat16"}"#,
        )]);
        std::fs::write(chat.path().join("model.safetensors"), &weights).unwrap();
        let launch = |dir: &Path, device: &str, dtype: Option<&str>| {
            let mut config = LaunchConfig {
                model_path: dir.to_path_buf(),
                ..Default::default()
            };
            config.params.insert(
                "device".into(),
                crate::params::ParamValue::Text(device.into()),
            );
            if let Some(dtype) = dtype {
                config.params.insert(
                    "dtype".into(),
                    crate::params::ParamValue::Text(dtype.into()),
                );
            }
            config
        };
        assert_eq!(vram_need(&launch(chat.path(), "cpu", None)), 0);
        // float16 on MPS: the stored size, a quarter more, 300 MB.
        assert_eq!(
            vram_need(&launch(chat.path(), "mps", None)),
            8 * MIB + 2 * MIB + 300 * MIB
        );
        // Asked for float32: twice the weights.
        assert_eq!(
            vram_need(&launch(chat.path(), "mps", Some("float32"))),
            16 * MIB + 4 * MIB + 300 * MIB
        );
        // A float16 diffusers pipeline runs in float32 on MPS, with ~1.5 GB of activations.
        let pipe = folder(&[(
            "model_index.json",
            r#"{"_class_name":"StableDiffusionPipeline"}"#,
        )]);
        std::fs::create_dir(pipe.path().join("unet")).unwrap();
        std::fs::write(
            pipe.path()
                .join("unet/diffusion_pytorch_model.fp16.safetensors"),
            &weights,
        )
        .unwrap();
        assert_eq!(
            vram_need(&launch(pipe.path(), "mps", None)),
            16 * MIB + 1536 * MIB + 300 * MIB
        );
        assert_eq!(
            vram_need(&launch(pipe.path(), "cuda", None)),
            8 * MIB + 1536 * MIB + 300 * MIB
        );
        // A bfloat16 safetensors file whose `config.json` names no dtype (LFM2.5-Audio): its
        // header says 16 bits, so float32 on MPS doubles it.
        let bf16 = folder(&[(
            "model_index.json",
            r#"{"_class_name":"StableDiffusionPipeline"}"#,
        )]);
        let header = format!(
            r#"{{"w":{{"dtype":"BF16","shape":[{n}],"data_offsets":[0,{b}]}}}}"#,
            n = 4 * MIB,
            b = 8 * MIB
        );
        let mut file = (header.len() as u64).to_le_bytes().to_vec();
        file.extend_from_slice(header.as_bytes());
        file.resize(file.len() + 8 * MIB as usize, 0);
        std::fs::create_dir(bf16.path().join("unet")).unwrap();
        std::fs::write(
            bf16.path().join("unet/diffusion_pytorch_model.safetensors"),
            &file,
        )
        .unwrap();
        let size = file.len() as u64;
        assert_eq!(
            vram_need(&launch(bf16.path(), "mps", None)),
            size / 16 * 32 + 1536 * MIB + 300 * MIB
        );
        // With no device asked: MPS on Apple Silicon, else CUDA only with the GPU build.
        let auto = LaunchConfig {
            model_path: chat.path().to_path_buf(),
            ..Default::default()
        };
        assert_eq!(
            gpu_device(&auto).is_some(),
            APPLE_SILICON || env().gpu_build_of.is_some()
        );
    }

    /// Runs a worker test script with the hf-cpu environment's Python (`HS_PYHF_PYTHON`, else
    /// skipped): `<script> <hf_worker.py> <empty dir>`, which prints PASS.
    fn run_worker_script(name: &str, body: &str) {
        let Some(python) = std::env::var_os("HS_PYHF_PYTHON") else {
            eprintln!("HS_PYHF_PYTHON not set, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let worker = dir.path().join("hf_worker.py");
        std::fs::write(&worker, WORKER).unwrap();
        let script = dir.path().join(name);
        std::fs::write(&script, body).unwrap();
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        // The worker drops torch's escape hatch from its environment.
        let out = std::process::Command::new(python)
            .env("TORCH_FORCE_NO_WEIGHTS_ONLY_LOAD", "1")
            .arg("-I")
            .arg(&script)
            .arg(&worker)
            .arg(&work)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("PASS"),
            "{stdout}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn the_worker_answers_only_harness_origins() {
        use crate::engine::mlx::tests::{cors_expected, system_python, CORS_DRIVER};
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hf_worker.py"), WORKER).unwrap();
        let stub = dir.path().join("starlette");
        std::fs::create_dir_all(&stub).unwrap();
        for (name, body) in [
            ("__init__.py", ""),
            (
                "applications.py",
                "class Starlette:\n    def __init__(self, routes):\n        pass\n    async def __call__(self, scope, receive, send):\n        await send({'type': 'http.response.start', 'status': 200, 'headers': []})\n        await send({'type': 'http.response.body', 'body': b'ok'})\n",
            ),
            (
                "concurrency.py",
                "async def run_in_threadpool(f, *a, **k):\n    return f(*a, **k)\n",
            ),
            ("requests.py", "class Request:\n    pass\n"),
            (
                "responses.py",
                "import json\nclass Response:\n    def __init__(self, content=b'', status_code=200, **_):\n        self.body, self.status_code = content, status_code\n    async def __call__(self, scope, receive, send):\n        await send({'type': 'http.response.start', 'status': self.status_code, 'headers': []})\n        await send({'type': 'http.response.body', 'body': self.body})\nclass JSONResponse(Response):\n    def __init__(self, content, status_code=200, **_):\n        super().__init__(json.dumps(content).encode(), status_code)\nclass StreamingResponse(Response):\n    pass\n",
            ),
            (
                "routing.py",
                "class Route:\n    def __init__(self, *a, **k):\n        pass\n",
            ),
        ] {
            std::fs::write(stub.join(name), body).unwrap();
        }
        let script = format!(
            "import sys, types; sys.path.insert(0, {:?})\nimport hf_worker as w\n{CORS_DRIVER}\nreport('hf', w.build_app(types.SimpleNamespace(state='ready', error=None)), 'GET', '/v1/models')\n",
            dir.path().to_string_lossy()
        );
        let out = std::process::Command::new(python)
            .args(["-I", "-c", &script])
            .env("HARNESS_WORKER_API_KEY", "k")
            .env("HARNESS_WORKER_ORIGINS", "tauri://localhost")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{text}");
        for line in cors_expected("hf", "-", 401) {
            assert!(text.contains(&line), "{line} missing:\n{text}");
        }
    }
}
