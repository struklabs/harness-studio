//! The MLX engines as [`Engine`]s (Apple Silicon, macOS 14 or later), two flavours of one
//! [`MlxEngine`]:
//!
//! * `mlx`: a text-generation model folder in MLX's format (`mlx-community/...`), served by
//!   `python/mlx_worker.py` around mlx-lm's OpenAI-compatible server from the managed `mlx`
//!   environment;
//! * `mlx-vlm`: a vision-language folder (its config has a `vision_config`), served by
//!   `python/mlx_vlm_worker.py` around mlx-vlm's server from the managed `mlx-vlm` environment;
//! * `mlx-audio`: a text-to-speech folder of mlx-audio (Qwen3-TTS) or a speech recognition one
//!   (Whisper, Parakeet), served by `python/mlx_audio_worker.py` (Harness's own small server
//!   around mlx-audio's model, which the `mlx-vlm` environment carries; `--task transcription`
//!   for recognition).
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

/// Family id of the MLX audio engine (mlx-audio: text to speech).
pub const MLX_AUDIO: &str = "mlx-audio";

/// The managed environment the text worker runs in (`crate::pyenv::envs::MLX` on Apple Silicon).
pub const ENV: &str = "mlx";

/// The managed environment the vision and audio workers run in (`crate::pyenv::envs::MLX_VLM`:
/// mlx-vlm brings mlx-audio along).
pub const VLM_ENV: &str = "mlx-vlm";

/// The worker scripts, written next to their environments ([`worker_script`]).
const WORKER: &str = include_str!("../../python/mlx_worker.py");
const VLM_WORKER: &str = include_str!("../../python/mlx_vlm_worker.py");
const AUDIO_WORKER: &str = include_str!("../../python/mlx_audio_worker.py");

/// mlx-audio text-to-speech architectures (`model_type`) Harness runs: the ones checked end to
/// end. Others (Kokoro needs misaki and spaCy data fetched at run time) are not offered.
const TTS_TYPES: &[&str] = &["qwen3_tts"];

/// mlx-audio speech recognition architectures (`model_type`) Harness runs. A NeMo Parakeet
/// config has no `model_type` but its `target` class (see [`is_parakeet_config`]).
const STT_TYPES: &[&str] = &["whisper", "parakeet", "parakeet_tdt"];

/// What the name of a Hub repo of a [`TTS_TYPES`] model contains (the listing has no config).
const TTS_REPO_NAMES: &[&str] = &["qwen3-tts"];

/// Whether a Hub repo id names a text-to-speech model of a tested architecture.
pub fn is_tts_repo(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    TTS_REPO_NAMES.iter().any(|n| id.contains(n))
}

/// `model_type`s of mlx-audio's speech models (recognition and synthesis): such a folder is
/// never a chat model even when its config looks like one (`WhisperForConditionalGeneration`,
/// `Qwen3TTSForConditionalGeneration`).
const AUDIO_TYPES: &[&str] = &[
    "qwen3_tts",
    "whisper",
    "parakeet",
    "kokoro",
    "chatterbox",
    "chatterbox_turbo",
    "dia",
    "sesame",
    "csm",
    "orpheus",
    "outetts",
    "spark",
    "voxcpm",
    "voxcpm2",
    "vibevoice",
    "voxtral",
    "voxtral_tts",
    "qwen3_asr",
    "qwen2_audio",
    "moonshine",
    "sensevoice",
    "indextts",
    "kitten_tts",
    "pocket_tts",
    "soprano",
    "melotts",
    "bark",
    "higgs_audio",
    "fish_qwen3_omni",
    "granite_speech",
    "canary",
    "nemotron_asr",
    "wav2vec",
    "mms",
];

/// Model card tasks of speech models.
const AUDIO_TASKS: &[&str] = &[
    "text-to-speech",
    "text-to-audio",
    "automatic-speech-recognition",
    "audio-to-audio",
];
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
    "--task",
];

/// What a launch without a `max_tokens` parameter gets: mlx-lm's own default (512) cuts
/// ordinary chat answers short.
const DEFAULT_MAX_TOKENS: u32 = 8192;

/// What an MLX engine serves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Flavour {
    /// `mlx`: chat (mlx-lm).
    #[default]
    Text,
    /// `mlx-vlm`: chat with images.
    Vision,
    /// `mlx-audio`: text to speech or speech recognition.
    Audio,
}

/// An MLX engine: the text one (`mlx`), the vision one (`mlx-vlm`) or the audio one
/// (`mlx-audio`).
#[derive(Debug, Clone, Copy, Default)]
pub struct MlxEngine {
    flavour: Flavour,
}

impl MlxEngine {
    /// `mlx` (mlx-lm).
    pub const TEXT: MlxEngine = MlxEngine {
        flavour: Flavour::Text,
    };
    /// `mlx-vlm`.
    pub const VISION: MlxEngine = MlxEngine {
        flavour: Flavour::Vision,
    };
    /// `mlx-audio`.
    pub const AUDIO: MlxEngine = MlxEngine {
        flavour: Flavour::Audio,
    };

    fn env(&self) -> &'static str {
        match self.flavour {
            Flavour::Text => ENV,
            Flavour::Vision | Flavour::Audio => VLM_ENV,
        }
    }

    /// Whether `path` is a folder this flavour runs.
    pub fn runs(&self, path: &Path) -> bool {
        match self.flavour {
            Flavour::Text => is_model_dir(path),
            Flavour::Vision => is_vlm_dir(path),
            Flavour::Audio => is_audio_dir(path),
        }
    }
}

/// The parsed catalog of a flavour (`params_mlx.toml`, `params_mlx_vlm.toml`,
/// `params_mlx_audio.toml`).
pub fn catalog(flavour: Flavour) -> &'static ParamCatalog {
    static TEXT: OnceLock<ParamCatalog> = OnceLock::new();
    static VISION: OnceLock<ParamCatalog> = OnceLock::new();
    static AUDIO: OnceLock<ParamCatalog> = OnceLock::new();
    let parse = |text: &str| -> ParamCatalog {
        toml::from_str(text).expect("the MLX catalogs are valid (unit tested)")
    };
    match flavour {
        Flavour::Text => TEXT.get_or_init(|| parse(include_str!("../params_mlx.toml"))),
        Flavour::Vision => VISION.get_or_init(|| parse(include_str!("../params_mlx_vlm.toml"))),
        Flavour::Audio => AUDIO.get_or_init(|| parse(include_str!("../params_mlx_audio.toml"))),
    }
}

/// Capabilities as documented: every flag of the catalog. The worker has no `--help` to probe
/// (its flags are the catalog's own), and without capabilities nothing would be rendered.
fn documented_caps(flavour: Flavour) -> &'static RuntimeCapabilities {
    static TEXT: OnceLock<RuntimeCapabilities> = OnceLock::new();
    static VISION: OnceLock<RuntimeCapabilities> = OnceLock::new();
    static AUDIO: OnceLock<RuntimeCapabilities> = OnceLock::new();
    let cell = match flavour {
        Flavour::Text => &TEXT,
        Flavour::Vision => &VISION,
        Flavour::Audio => &AUDIO,
    };
    cell.get_or_init(|| {
        let help: String = catalog(flavour)
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

/// `model_type` of a folder's config.
fn model_type(dir: &Path) -> Option<String> {
    read_json(&dir.join("config.json"))?
        .get("model_type")?
        .as_str()
        .map(str::to_ascii_lowercase)
}

/// Whether a folder is a speech model by its card (`library_name: mlx-audio`, a speech task) or
/// its `model_type`: never a chat model then.
pub fn is_audio_folder(dir: &Path) -> bool {
    card_field(dir, "library_name").is_some_and(|l| l.eq_ignore_ascii_case("mlx-audio"))
        || card_field(dir, "pipeline_tag").is_some_and(|t| AUDIO_TASKS.contains(&t.as_str()))
        || model_type(dir).is_some_and(|t| AUDIO_TYPES.contains(&t.as_str()))
}

/// Whether an MLX folder has a vision tower, `None` when `path` is not an MLX chat folder.
fn mlx_vision(path: &Path) -> Option<bool> {
    (super::mistralrs::is_checkpoint_dir(path) && is_mlx_checkpoint(path) && !is_audio_folder(path))
        .then(|| {
            read_json(&path.join("config.json")).is_some_and(|c| c.get("vision_config").is_some())
        })
}

/// Whether `path` is a folder the `mlx-audio` engine runs: an mlx-audio text-to-speech model
/// of a tested architecture ([`TTS_TYPES`]), with `config.json` and safetensors weights.
pub fn is_tts_dir(path: &Path) -> bool {
    path.is_dir()
        && !super::mistralrs::weight_files(path).is_empty()
        && model_type(path).is_some_and(|t| TTS_TYPES.contains(&t.as_str()))
        && (card_field(path, "library_name")
            .is_some_and(|l| l.eq_ignore_ascii_case("mlx-audio") || l.eq_ignore_ascii_case("mlx"))
            || read_json(&path.join("config.json")).is_some_and(|c| quant_bits(&c).is_some()))
}

/// Whether a config is NeMo's for a Parakeet (RNNT / TDT / CTC) recognizer: no `model_type`, a
/// `target` of NeMo's ASR models.
fn is_parakeet_config(cfg: &serde_json::Value) -> bool {
    cfg.get("model_type").is_none()
        && cfg
            .get("target")
            .and_then(|t| t.as_str())
            .is_some_and(|t| t.starts_with("nemo.collections.asr.models.") && t.contains("EncDec"))
}

/// Whether `path` is a speech recognition folder the `mlx-audio` engine runs: a Whisper or
/// Parakeet model in MLX's format (its card says `library_name: mlx` / `mlx-audio`, or its
/// config has MLX's quantization), with `config.json` and safetensors weights. A transformers
/// Whisper checkpoint (no MLX card) stays the Python engine's.
pub fn is_stt_dir(path: &Path) -> bool {
    if !path.is_dir() || super::mistralrs::weight_files(path).is_empty() {
        return false;
    }
    let Some(cfg) = read_json(&path.join("config.json")) else {
        return false;
    };
    let arch = cfg
        .get("model_type")
        .and_then(|t| t.as_str())
        .is_some_and(|t| STT_TYPES.contains(&t.to_ascii_lowercase().as_str()))
        || is_parakeet_config(&cfg);
    arch && (card_field(path, "library_name")
        .is_some_and(|l| l.eq_ignore_ascii_case("mlx-audio") || l.eq_ignore_ascii_case("mlx"))
        || quant_bits(&cfg).is_some())
}

/// Whether `path` is a folder of the `mlx-audio` engine (text to speech or recognition).
pub fn is_audio_dir(path: &Path) -> bool {
    is_tts_dir(path) || is_stt_dir(path)
}

/// The named voices of an mlx-audio text-to-speech folder (Qwen3-TTS: the keys of
/// `talker_config.spk_id`), sorted; empty when it has none (one default voice).
pub fn tts_voices(path: &Path) -> Vec<String> {
    let Some(config) = read_json(&path.join("config.json")) else {
        return Vec::new();
    };
    let ids = config
        .get("talker_config")
        .and_then(|t| t.get("spk_id"))
        .or_else(|| config.get("spk_id"))
        .and_then(|v| v.as_object());
    let mut voices: Vec<String> = ids
        .map(|m| m.keys().filter(|k| !k.is_empty()).cloned().collect())
        .unwrap_or_default();
    voices.sort();
    voices
}

/// Whether `path` is a folder the `mlx` engine runs: an MLX text generator (not a vision model).
pub fn is_model_dir(path: &Path) -> bool {
    mlx_vision(path) == Some(false)
}

/// Whether `path` is a folder the `mlx-vlm` engine runs: an MLX vision-language model.
pub fn is_vlm_dir(path: &Path) -> bool {
    mlx_vision(path) == Some(true)
}

/// Whether `path` is an MLX folder of any MLX engine.
pub fn is_any_model_dir(path: &Path) -> bool {
    mlx_vision(path).is_some() || is_audio_dir(path)
}

/// The flavour that runs an MLX folder, `None` for other paths.
pub fn flavour_of(path: &Path) -> Option<Flavour> {
    match mlx_vision(path) {
        Some(true) => Some(Flavour::Vision),
        Some(false) => Some(Flavour::Text),
        None => is_audio_dir(path).then_some(Flavour::Audio),
    }
}

/// What the files of an MLX chat folder say (chat template, vision tower...); `None` for other
/// paths (speech folders included).
pub fn inspect(dir: &Path) -> Option<super::mistralrs::ModelDirInfo> {
    mlx_vision(dir)?;
    super::mistralrs::inspect_checkpoint(dir)
}

/// The library's view of an MLX folder: a chat model like mistral.rs's, or (speech) a pipeline
/// model like the Python engine's, with MLX's bits as its quantization (`4bit`).
fn to_local_model(dir: &Path, root: &Path) -> Option<crate::models::LocalModel> {
    let stt = is_stt_dir(dir);
    // A recognition config is no text generator's (Whisper's MLX port, NeMo's Parakeet): its
    // folder is described by its weights and `model_type` alone.
    let info = match super::mistralrs::inspect_checkpoint(dir) {
        Some(info) => info,
        None if stt => audio_dir_info(dir),
        None => return None,
    };
    let mut model = super::mistralrs::to_local_model(dir, &info, root);
    if is_audio_dir(dir) {
        model.kind = crate::domain::ModelKind::Pipeline;
        model.context_length = None;
    }
    if stt {
        model.task = Some("automatic-speech-recognition".into());
    }
    if let Some(bits) = read_json(&dir.join("config.json")).and_then(|c| quant_bits(&c)) {
        model.quantization = format!("{bits}bit");
    }
    Some(model)
}

/// What the files of a speech recognition folder say: its weights and `model_type`.
fn audio_dir_info(dir: &Path) -> super::mistralrs::ModelDirInfo {
    let weights = super::mistralrs::weight_files(dir);
    super::mistralrs::ModelDirInfo {
        weights_bytes: weights
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum(),
        weights,
        architecture: model_type(dir).or_else(|| Some("parakeet".into())),
        complete: true,
        ..Default::default()
    }
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

/// Removes an MLX model folder (everything in it: config, tokenizer, weights); refuses anything
/// that is not one strictly inside one of `roots`.
pub fn delete_model_dir(dir: &Path, roots: &[PathBuf]) -> Result<(), String> {
    if !is_any_model_dir(dir) {
        return Err(err("mlx.model_unsupported")
            .with("path", dir.display())
            .into());
    }
    super::mistralrs::remove_folder_within(dir, roots)
}

/// The family that runs an MLX folder (`mlx`, `mlx-vlm`, `mlx-audio`), `None` for other paths.
pub fn family_of(path: &Path) -> Option<&'static str> {
    Some(match flavour_of(path)? {
        Flavour::Text => MLX,
        Flavour::Vision => MLX_VLM,
        Flavour::Audio => MLX_AUDIO,
    })
}

// ---- the environment -----------------------------------------------------------------------

/// The worker script of this version, written next to its environment (rewritten when it
/// differs, so an app update brings its worker along).
pub fn worker_script(flavour: Flavour) -> Result<PathBuf, String> {
    let (env, name, body) = match flavour {
        Flavour::Text => (ENV, "mlx_worker.py", WORKER),
        Flavour::Vision => (VLM_ENV, "mlx_vlm_worker.py", VLM_WORKER),
        Flavour::Audio => (VLM_ENV, "mlx_audio_worker.py", AUDIO_WORKER),
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

/// [`binary`] of the audio engine (the `mlx-vlm` environment too).
pub fn audio_binary() -> Result<PathBuf, String> {
    MlxEngine::AUDIO.binary()
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
        match self.flavour {
            Flavour::Text => MLX,
            Flavour::Vision => MLX_VLM,
            Flavour::Audio => MLX_AUDIO,
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

    /// Folders are told apart by [`is_model_dir`] / [`is_vlm_dir`] / [`is_tts_dir`], not by a
    /// file format.
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
        catalog(self.flavour)
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
        build_args(config, &worker_script(self.flavour)?, self.flavour)
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
fn build_args(
    config: &LaunchConfig,
    script: &Path,
    flavour: Flavour,
) -> Result<BuildOutput, String> {
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
    if flavour == Flavour::Audio && is_stt_dir(&config.model_path) {
        args.push("--task".into());
        args.push("transcription".into());
    }
    let ctx = RenderContext {
        host: config.host.clone(),
        ..Default::default()
    };
    let rendered = crate::params::render_params(
        catalog(flavour),
        Some(documented_caps(flavour)),
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
    if flavour != Flavour::Audio
        && !args
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
pub(crate) mod tests {
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
        for flavour in [Flavour::Text, Flavour::Vision, Flavour::Audio] {
            assert!(!catalog(flavour).params.is_empty());
        }
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
        let out = build_args(&config, Path::new("/env/mlx_worker.py"), Flavour::Text).unwrap();
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
        set.params.insert(
            "kv_bits".into(),
            crate::params::ParamValue::Text("8".into()),
        );
        let out = build_args(&set, Path::new("w.py"), Flavour::Text).unwrap();
        assert!(out.skipped.is_empty(), "{:?}", out.skipped);
        let args = out.args.join(" ");
        assert!(args.contains("--max-tokens 512"), "{args}");
        assert!(args.contains("--prompt-cache-size 4"), "{args}");
        assert!(args.contains("--kv-bits 8"), "{args}");
        assert_eq!(args.matches("--max-tokens").count(), 1, "{args}");

        let mut custom = config.clone();
        custom.custom_args = Some("--max-tokens 100".into());
        let out = build_args(&custom, Path::new("w.py"), Flavour::Text).unwrap();
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
            let e = build_args(&c, Path::new("w.py"), Flavour::Text).unwrap_err();
            assert!(e.contains("params.protected_flag"), "{bad}: {e}");
            let e = build_args(&c, Path::new("w.py"), Flavour::Vision).unwrap_err();
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
        let out = build_args(
            &config,
            Path::new("/env/mlx_vlm_worker.py"),
            Flavour::Vision,
        )
        .unwrap();
        assert_eq!(out.args[0], "/env/mlx_vlm_worker.py");
        assert!(out.skipped.is_empty(), "{:?}", out.skipped);
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
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("mlx_worker.py");
        std::fs::write(&script, WORKER).unwrap();
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
        let (ok, text) = check(&["--model", "/m", "--port", "1", "--max-tokens", "9"]).unwrap();
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

    /// The system's Python 3 (`python3`, or `python` on Windows), `None` when there is none. On
    /// Windows without Python, `python` is the Store's stand-in: it runs and fails ("Python
    /// bulunamadı"), so the version it prints decides, not whether it starts.
    pub(crate) fn system_python() -> Option<&'static str> {
        static FOUND: std::sync::OnceLock<Option<&'static str>> = std::sync::OnceLock::new();
        *FOUND.get_or_init(|| {
            let python = if cfg!(windows) { "python" } else { "python3" };
            let out = std::process::Command::new(python)
                .args(["-I", "-c", "import sys; print(sys.version_info[0])"])
                .output()
                .ok()?;
            (out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "3")
                .then_some(python)
        })
    }

    /// Runs `code` with a system Python (`None` when there is none) next to the vision worker
    /// (it needs neither MLX nor the environment for its checks): `w` is the worker module.
    fn vision_worker_py(code: &str) -> Option<(bool, String)> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mlx_vlm_worker.py"), VLM_WORKER).unwrap();
        let python = system_python()?;
        let code = format!(
            "import sys, json, asyncio; sys.path.insert(0, {:?}); import mlx_vlm_worker as w\n{code}",
            dir.path().to_string_lossy()
        );
        let out = std::process::Command::new(python)
            .args(["-I", "-c", &code])
            .env("HARNESS_WORKER_API_KEY", "k")
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        Some((out.status.success(), text))
    }

    /// The vision worker's request check: only inline media of the right kind; input_audio is
    /// rewritten into a data: URL; no adapters or other models.
    #[test]
    fn the_vision_worker_takes_only_inline_media() {
        let image = |url: &str| {
            serde_json::json!({"messages": [{"role": "user", "content": [
                {"type": "text", "text": "hi"},
                {"type": "image_url", "image_url": {"url": url, "detail": "auto"}}
            ]}]})
        };
        let audio = |data: &str| {
            serde_json::json!({"messages": [{"role": "user", "content": [
                {"type": "input_audio", "input_audio": {"data": data, "format": "wav"}}
            ]}]})
        };
        let check = |body: &serde_json::Value| {
            vision_worker_py(&format!(
                "e, out = w.check_body({:?}.encode())\nprint(json.dumps([e, out and json.loads(out)]))",
                body.to_string()
            ))
            .map(|(ok, text)| {
                assert!(ok, "{text}");
                serde_json::from_str::<serde_json::Value>(text.trim()).unwrap()
            })
        };
        let Some(ok) = check(&image("data:image/png;base64,iVBORw0K")) else {
            eprintln!("no system python, skipping");
            return;
        };
        assert!(ok[0].is_null(), "{ok}");
        // Base64 audio becomes a data: URL, so mlx-vlm never takes it for a path ("/9j..." is
        // valid base64).
        let out = check(&audio("/9j/AAAA")).unwrap();
        assert!(out[0].is_null(), "{out}");
        assert_eq!(
            out[1]["messages"][0]["content"][0]["input_audio"]["data"],
            "data:audio/wav;base64,/9j/AAAA"
        );
        let out = check(&audio("data:audio/wav;base64,AAAA")).unwrap();
        assert!(out[0].is_null(), "{out}");
        for bad in [
            image("http://example.com/x.png"),
            image("/etc/hosts"),
            image("file:///etc/hosts"),
            image("data:text/plain;base64,AAAA"),
            image("data:image/png,notbase64"),
            image("data:image/png;base64,***"),
            audio("/etc/hosts"),
            audio("http://x"),
            audio("file:///etc/hosts"),
            audio("./a.wav"),
            audio("data:image/png;base64,AAAA"),
            serde_json::json!({"messages": [{"role": "user", "content": [
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA", "detail": "../x"}}]}]}),
            serde_json::json!({"messages": [{"role": "user", "content": [
                {"type": "image_url", "image_url": "png"}]}]}),
            serde_json::json!({"messages": [{"role": "user", "content": [
                {"type": "video_url", "video_url": "/v.mp4"}]}]}),
            serde_json::json!({"messages": [{"role": "user", "content": [
                {"type": "video_url", "video_url": {"url": "data:image/png;base64,AAAA"}}]}]}),
            serde_json::json!({"messages": [{"role": "user", "content": [
                {"type": "file", "file": {"file_id": "x"}}]}]}),
            serde_json::json!({"messages": [], "adapter_path": "/x"}),
            serde_json::json!({"messages": [], "model_path": "/x"}),
        ] {
            let out = check(&bad).unwrap();
            assert!(out[0].is_string(), "{bad} was accepted: {out}");
        }
    }

    /// The vision worker's ASGI guard around a stand-in app: only its three routes, the key,
    /// the body limit, and the (checked) body replayed to the app.
    #[test]
    fn the_vision_worker_guard_lets_through_only_what_it_checked() {
        let script = r#"
seen = []
async def inner(scope, receive, send):
    body = b""
    while True:
        m = await receive()
        body += m.get("body", b"")
        if not m.get("more_body"):
            break
    seen.append((scope["path"], body, dict(scope["headers"]).get(b"content-length")))
    await send({"type": "http.response.start", "status": 200, "headers": []})
    await send({"type": "http.response.body", "body": b"inner"})

app = w.guard(inner, "alias")

async def call(method, path, headers=(), chunks=(b"",), kind="http"):
    sent = []
    queue = [{"type": "http.request", "body": c, "more_body": i < len(chunks) - 1}
             for i, c in enumerate(chunks)]
    async def receive():
        return queue.pop(0) if queue else {"type": "http.disconnect"}
    async def send(m):
        sent.append(m)
    scope = {"type": kind, "method": method, "path": path,
             "headers": [(k.lower().encode(), v.encode()) for k, v in headers]}
    await app(scope, receive, send)
    if kind == "websocket":
        return sent[0]["type"], sent[0].get("code")
    return sent[0]["status"], b"".join(m.get("body", b"") for m in sent[1:])

KEY = [("Authorization", "Bearer k")]
async def main():
    r = {}
    for m, p in [("HEAD", "/v1/models"), ("OPTIONS", "/v1/chat/completions"),
                 ("GET", "/v1/models/"), ("GET", "//v1/models"), ("POST", "/unload"),
                 ("GET", "/v1/chat/completions"), ("POST", "/v1/chat/completions/")]:
        r[m + " " + p] = (await call(m, p, KEY))[0]
    r["ws"] = await call("GET", "/v1/chat/completions", KEY, kind="websocket")
    r["health"] = await call("GET", "/health")
    r["no key"] = (await call("GET", "/v1/models"))[0]
    r["wrong key"] = (await call("GET", "/v1/models", [("Authorization", "Bearer x")]))[0]
    r["models"] = await call("GET", "/v1/models", KEY)
    big = b"x" * (1024 * 1024)
    r["big"] = (await call("POST", "/v1/chat/completions", KEY, [big] * 65))[0]
    r["bad"] = (await call("POST", "/v1/chat/completions", KEY,
                           [b'{"messages":[{"role":"user","content":[{"type":"input_audio",',
                            b'"input_audio":{"data":"/etc/hosts","format":"wav"}}]}]}']))[0]
    good = b'{"messages":[{"role":"user","content":"hi"}]}'
    r["good"] = await call("POST", "/v1/chat/completions", KEY, [good[:10], good[10:]])
    audio = b'{"messages":[{"role":"user","content":[{"type":"input_audio","input_audio":{"data":"AAAA"}}]}]}'
    r["audio"] = (await call("POST", "/v1/chat/completions", KEY + [("Content-Length", str(len(audio)))], [audio]))[0]
    return r

r = asyncio.run(main())
for k, v in r.items():
    print(k, "=", v)
print("seen", [(p, b.decode()[:120], cl) for p, b, cl in seen])
"#;
        let Some((ok, text)) = vision_worker_py(script) else {
            eprintln!("no system python, skipping");
            return;
        };
        assert!(ok, "{text}");
        for line in [
            "HEAD /v1/models = 404",
            "OPTIONS /v1/chat/completions = 404",
            "GET /v1/models/ = 404",
            "GET //v1/models = 404",
            "POST /unload = 404",
            "GET /v1/chat/completions = 404",
            "POST /v1/chat/completions/ = 404",
            "ws = ('websocket.close', 1008)",
            "health = (200, b'{\"status\": \"ok\"}')",
            "no key = 401",
            "wrong key = 401",
            "big = 413",
            "bad = 400",
            "good = (200, b'inner')",
            "audio = 200",
        ] {
            assert!(text.contains(line), "{line} missing:\n{text}");
        }
        assert!(text.contains("\"id\": \"alias\""), "{text}");
        // The app saw the body whole, and the rewritten one with its new length.
        assert!(
            text.contains("('/v1/chat/completions', '{\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}', b'45')"),
            "{text}"
        );
        assert!(text.contains("data:audio/wav;base64,AAAA"), "{text}");
        assert_eq!(text.matches("('/v1/chat/completions'").count(), 2, "{text}");
    }

    /// `mlx-community/Qwen3-TTS-12Hz-0.6B-CustomVoice-8bit` (config trimmed) and its card.
    const TTS_CONFIG: &str = r#"{"architectures":["Qwen3TTSForConditionalGeneration"],"model_type":"qwen3_tts","quantization":{"group_size":64,"bits":8,"mode":"affine"},"talker_config":{"spk_id":{"serena":3066,"ryan":3061}}}"#;
    const TTS_CARD: &str =
        "---\nlicense: apache-2.0\nlibrary_name: mlx-audio\ntags:\n- mlx\n- text-to-speech\n---\n";

    #[test]
    fn speech_folders_are_mlx_audio_and_never_chat_models() {
        let tts = folder(&[
            ("config.json", TTS_CONFIG),
            ("model.safetensors", "x"),
            ("README.md", TTS_CARD),
            ("speech_tokenizer/model.safetensors", "x"),
        ]);
        assert!(is_tts_dir(tts.path()));
        assert_eq!(tts_voices(tts.path()), ["ryan", "serena"]);
        assert!(!is_model_dir(tts.path()) && !is_vlm_dir(tts.path()));
        assert!(!super::super::mistralrs::is_model_dir(tts.path()));
        assert_eq!(flavour_of(tts.path()), Some(Flavour::Audio));
        assert_eq!(family_of(tts.path()), Some(MLX_AUDIO));
        assert!(MlxEngine::AUDIO.runs(tts.path()));
        assert!(inspect(tts.path()).is_none());
        let m = to_local_model(tts.path(), tts.path().parent().unwrap()).unwrap();
        assert_eq!(m.kind, crate::domain::ModelKind::Pipeline);
        assert_eq!(m.quantization, "8bit");

        // A whisper conversion (an MLX card, a transformers-like config) is a speech model:
        // not a chat model; mlx-audio recognizes with it (see the next test).
        let whisper = folder(&[
            (
                "config.json",
                r#"{"architectures":["WhisperForConditionalGeneration"],"model_type":"whisper"}"#,
            ),
            ("model.safetensors", "x"),
            ("README.md", "---\nlibrary_name: mlx\ntags:\n- mlx\n---\n"),
        ]);
        assert!(is_audio_folder(whisper.path()));
        assert!(!is_model_dir(whisper.path()) && !is_vlm_dir(whisper.path()));
        assert!(!super::super::mistralrs::is_model_dir(whisper.path()));
        // Kokoro (an untested architecture) is not offered.
        let kokoro = folder(&[
            ("config.json", r#"{"model_type":"kokoro","istftnet":{}}"#),
            ("kokoro-v1_0.safetensors", "x"),
            (
                "README.md",
                "---\nlibrary_name: mlx\npipeline_tag: text-to-speech\n---\n",
            ),
        ]);
        assert_eq!(flavour_of(kokoro.path()), None);
        // A chat model whose card names a speech task is not a chat model either.
        let tagged = folder(&[
            ("config.json", MLX_CONFIG),
            ("model.safetensors", "x"),
            ("README.md", "---\npipeline_tag: text-to-speech\n---\n"),
        ]);
        assert_eq!(flavour_of(tagged.path()), None);
        assert!(is_tts_repo("mlx-community/Qwen3-TTS-12Hz-0.6B-Base-bf16"));
        assert!(!is_tts_repo("mlx-community/Kokoro-82M-bf16"));
    }

    /// Speech recognition folders in MLX's format (mlx-audio's Whisper port with a transformers
    /// config, mlx-whisper's own config, NeMo's Parakeet config) are mlx-audio's, launched for
    /// transcription; the same folders without an MLX card stay with the Python engine, and
    /// nothing else of mlx-audio's recognizers is offered.
    #[test]
    fn mlx_recognition_folders_are_mlx_audio_transcription_models() {
        let hf_whisper = r#"{"architectures":["WhisperForConditionalGeneration"],"model_type":"whisper","d_model":384}"#;
        let ported = folder(&[
            ("config.json", hf_whisper),
            ("model.safetensors", "x"),
            ("preprocessor_config.json", "{}"),
            (
                "README.md",
                "---\npipeline_tag: automatic-speech-recognition\nlibrary_name: mlx-audio\n---\n",
            ),
        ]);
        let turbo = folder(&[
            (
                "config.json",
                r#"{"n_mels":128,"n_audio_ctx":1500,"n_text_layer":4,"model_type":"whisper"}"#,
            ),
            ("weights.safetensors", "x"),
            (
                "README.md",
                "---\nlibrary_name: mlx\npipeline_tag: automatic-speech-recognition\n---\n",
            ),
        ]);
        let parakeet = folder(&[
            (
                "config.json",
                r#"{"sample_rate":16000,"encoder":{},"decoder":{},"target":"nemo.collections.asr.models.rnnt_bpe_models.EncDecRNNTBPEModel"}"#,
            ),
            ("model.safetensors", "x"),
            ("tokenizer.model", "x"),
            ("README.md", "---\nlibrary_name: mlx\n---\n"),
        ]);
        for dir in [&ported, &turbo, &parakeet] {
            let path = dir.path();
            assert!(is_stt_dir(path), "{}", path.display());
            assert!(!is_tts_dir(path));
            assert_eq!(flavour_of(path), Some(Flavour::Audio));
            assert_eq!(family_of(path), Some(MLX_AUDIO));
            assert!(MlxEngine::AUDIO.runs(path) && is_any_model_dir(path));
            assert!(!is_model_dir(path) && !is_vlm_dir(path));
            assert!(!super::super::mistralrs::is_model_dir(path));
            assert_eq!(super::super::python_hf::detect(path), None);
            let m = to_local_model(path, path.parent().unwrap()).unwrap();
            assert_eq!(m.kind, crate::domain::ModelKind::Pipeline);
            assert_eq!(m.task.as_deref(), Some("automatic-speech-recognition"));
            assert!(m.is_ready && m.size_bytes == 1);
            assert_eq!(
                super::super::server_for_model(path).map(|e| e.family()),
                Some(MLX_AUDIO)
            );
        }
        assert_eq!(
            to_local_model(turbo.path(), turbo.path())
                .unwrap()
                .architecture
                .as_deref(),
            Some("whisper")
        );
        assert_eq!(
            to_local_model(parakeet.path(), parakeet.path())
                .unwrap()
                .architecture
                .as_deref(),
            Some("parakeet")
        );

        // A transformers Whisper checkpoint (no MLX card) is the Python engine's.
        let hf = folder(&[
            ("config.json", hf_whisper),
            ("model.safetensors", "x"),
            (
                "README.md",
                "---\npipeline_tag: automatic-speech-recognition\n---\n",
            ),
        ]);
        assert!(!is_stt_dir(hf.path()));
        assert_eq!(flavour_of(hf.path()), None);
        assert_eq!(
            super::super::python_hf::detect(hf.path()).as_deref(),
            Some("automatic-speech-recognition")
        );
        // Old mlx-whisper weights (`weights.npz`) and untested recognizers are not offered.
        let npz = folder(&[
            ("config.json", r#"{"n_mels":80,"model_type":"whisper"}"#),
            ("weights.npz", "x"),
            ("README.md", "---\nlibrary_name: mlx\n---\n"),
        ]);
        assert!(!is_stt_dir(npz.path()));
        let moonshine = folder(&[
            ("config.json", r#"{"model_type":"moonshine"}"#),
            ("model.safetensors", "x"),
            ("README.md", "---\nlibrary_name: mlx-audio\n---\n"),
        ]);
        assert!(!is_stt_dir(moonshine.path()));
        assert_eq!(flavour_of(moonshine.path()), None);

        // Launched for transcription; a custom line cannot change the task.
        let mut config = LaunchConfig {
            model_path: turbo.path().to_path_buf(),
            host: "127.0.0.1".into(),
            port: 11453,
            ..Default::default()
        };
        config
            .params
            .insert("max_seconds".into(), crate::params::ParamValue::Int(300));
        let script = Path::new("/env/mlx_audio_worker.py");
        let args = build_args(&config, script, Flavour::Audio).unwrap().args;
        let args = args.join(" ");
        assert!(args.contains(" --task transcription"), "{args}");
        assert!(args.contains("--max-seconds 300"), "{args}");
        assert!(!args.contains("--max-tokens"), "{args}");
        config.custom_args = Some("--task speech".into());
        let e = build_args(&config, script, Flavour::Audio).unwrap_err();
        assert!(e.contains("params.protected_flag"), "{e}");
        // A text-to-speech folder is launched without a task (the worker speaks by default).
        let tts = folder(&[
            ("config.json", TTS_CONFIG),
            ("model.safetensors", "x"),
            ("README.md", TTS_CARD),
        ]);
        config.model_path = tts.path().to_path_buf();
        config.custom_args = None;
        let args = build_args(&config, script, Flavour::Audio).unwrap().args;
        assert!(!args.iter().any(|a| a == "--task"), "{args:?}");
    }

    #[test]
    fn audio_launch_args_have_no_answer_limit() {
        let dir = folder(&[
            ("config.json", TTS_CONFIG),
            ("model.safetensors", "x"),
            ("README.md", TTS_CARD),
        ]);
        let mut config = LaunchConfig {
            model_path: dir.path().to_path_buf(),
            host: "127.0.0.1".into(),
            port: 11452,
            ..Default::default()
        };
        config
            .params
            .insert("max_chars".into(), crate::params::ParamValue::Int(900));
        config
            .params
            .insert("max_seconds".into(), crate::params::ParamValue::Int(45));
        let out = build_args(
            &config,
            Path::new("/env/mlx_audio_worker.py"),
            Flavour::Audio,
        )
        .unwrap();
        assert!(out.skipped.is_empty(), "{:?}", out.skipped);
        let args = out.args.join(" ");
        assert!(
            args.starts_with("/env/mlx_audio_worker.py --model "),
            "{args}"
        );
        assert!(args.contains("--max-chars 900"), "{args}");
        assert!(args.contains("--max-seconds 45"), "{args}");
        assert!(!args.contains("--max-tokens"), "{args}");
        assert_eq!(MlxEngine::AUDIO.family(), MLX_AUDIO);
        assert_eq!(MlxEngine::AUDIO.env(), VLM_ENV);
        let e = MlxEngine::TEXT
            .build_launch(&config, &ServerCaps::default(), None)
            .unwrap_err();
        assert!(e.contains("mlx.model_unsupported"), "{e}");
    }

    /// The audio worker's checks and routes around a stand-in speaker (system Python, no MLX).
    #[test]
    fn the_audio_worker_speaks_only_checked_requests() {
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mlx_audio_worker.py"), AUDIO_WORKER).unwrap();
        let script = format!(
            r#"
import sys, asyncio; sys.path.insert(0, {:?}); import mlx_audio_worker as w
spoken = []
def speak(text, voice, speed, language):
    spoken.append((text, voice, speed, language))
    if text == "slow":
        raise w.TooSlow()
    return b"RIFFfake", text == "long run"
app = w.app_for(speak, "tts", ["serena", "ryan"], 50)
async def call(method, path, body=b"", key=True, kind="http"):
    sent, queue = [], [{{"type": "http.request", "body": body, "more_body": False}}]
    async def receive():
        return queue.pop(0) if queue else {{"type": "http.disconnect"}}
    async def send(m):
        sent.append(m)
    headers = [(b"authorization", b"Bearer k")] if key else []
    await app({{"type": kind, "method": method, "path": path, "headers": headers}}, receive, send)
    if kind == "websocket":
        return sent[0].get("code")
    status = sent[0]["status"]
    if dict(sent[0]["headers"]).get(b"x-harness-truncated"):
        status = (status, "truncated")
    return status, b"".join(m.get("body", b"") for m in sent[1:]).decode(errors="replace")
async def main():
    S = "/v1/audio/speech"
    for name, args in [
        ("health", ("GET", "/health", b"", False)),
        ("nokey", ("GET", "/v1/models", b"", False)),
        ("models", ("GET", "/v1/models")),
        ("stt", ("POST", "/v1/audio/transcriptions")),
        ("voices", ("GET", "/v1/audio/voices")),
        ("slash", ("POST", S + "/")),
        ("ok", ("POST", S, b'{{"input":"hello","voice":"ryan","speed":1.5}}')),
        ("openai", ("POST", S, b'{{"input":"hello","voice":"alloy","model":"tts"}}')),
        ("ref", ("POST", S, b'{{"input":"x","ref_audio":"/etc/hosts"}}')),
        ("path", ("POST", S, b'{{"input":"x","voice":"../etc/passwd"}}')),
        ("unknown", ("POST", S, b'{{"input":"x","voice":"bob"}}')),
        ("long", ("POST", S, b'{{"input":"' + b"a" * 60 + b'"}}')),
        ("speed", ("POST", S, b'{{"input":"x","speed":9}}')),
        ("mp3", ("POST", S, b'{{"input":"x","response_format":"mp3"}}')),
        ("empty", ("POST", S, b'{{"input":"  "}}')),
        ("slow", ("POST", S, b'{{"input":"slow"}}')),
        ("cut", ("POST", S, b'{{"input":"long run"}}')),
    ]:
        print(name, await call(*args))
    print("ws", await call("GET", S, kind="websocket"))
    print("spoken", spoken)
asyncio.run(main())
"#,
            dir.path().to_string_lossy()
        );
        let out = std::process::Command::new(python)
            .args(["-I", "-c", &script])
            .env("HARNESS_WORKER_API_KEY", "k")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{text}");
        for line in [
            "health (200, '{\"status\": \"ok\"}')",
            "nokey (401,",
            "models (200, '{\"object\": \"list\", \"data\": [{\"id\": \"tts\"",
            "\"voices\": [\"serena\", \"ryan\"]",
            "stt (404,",
            "voices (404,",
            "slash (404,",
            "ok (200, 'RIFFfake')",
            "openai (200, 'RIFFfake')",
            "ref (400,",
            "path (400,",
            "unknown (400,",
            "long (400,",
            "speed (400,",
            "mp3 (400,",
            "empty (400,",
            "slow (504,",
            "cut ((200, 'truncated'), 'RIFFfake')",
            "ws 1008",
            "spoken [('hello', 'ryan', 1.5, None), ('hello', None, 1.0, None), ('slow', None, 1.0, None), ('long run', None, 1.0, None)]",
        ] {
            assert!(text.contains(line), "{line} missing:\n{text}");
        }
    }

    /// Python that calls an ASGI worker `app` as a browser would (`report(name, app, method,
    /// path)` prints one line per case: status, Access-Control-Allow-Origin, -Allow-Headers).
    /// The workers run with `HARNESS_WORKER_ORIGINS=tauri://localhost` and key `k`.
    pub(crate) const CORS_DRIVER: &str = r#"
import asyncio
async def _call(app, method, path, origin=None, key=True, kind="http"):
    body = b'{"messages": [{"role": "user", "content": "hi"}]}'
    first = {"type": "websocket.connect"} if kind == "websocket" else {
        "type": "http.request", "body": body, "more_body": False}
    sent, queue = [], [first]
    async def receive():
        return queue.pop(0) if queue else {"type": "http.disconnect"}
    async def send(m):
        sent.append(m)
    headers = [(b"authorization", b"Bearer k")] if key else []
    if origin:
        headers.append((b"origin", origin.encode()))
    await app({"type": kind, "method": method, "path": path, "headers": headers}, receive, send)
    if kind == "websocket":
        return sent[0]["type"], sent[0].get("code")
    h = dict(sent[0].get("headers") or [])
    return (sent[0]["status"], h.get(b"access-control-allow-origin", b"-").decode(),
            h.get(b"access-control-allow-headers", b"-").decode())
def report(name, app, method, path):
    T, E = "tauri://localhost", "https://evil.example"
    for label, args in [("preflight", ("OPTIONS", T, False)), ("preflight foreign", ("OPTIONS", E, False)),
                        ("foreign", (method, E)), ("foreign nokey", (method, E, False)),
                        ("allowed nokey", (method, T, False)), ("allowed", (method, T)),
                        ("plain", (method, None)), ("options plain", ("OPTIONS", None, False)),
                        # Browsers send "null", lowercase and no slash: anything else is foreign.
                        ("null", (method, "null")), ("upper", (method, "TAURI://LOCALHOST")),
                        ("slash", (method, T + "/")),
                        ("ws foreign", ("GET", E, True, "websocket"))]:
        print(name, label, asyncio.run(_call(app, args[0], path, *args[1:])))
"#;

    /// What [`CORS_DRIVER`] prints for a worker that answers `path` with 200 (`plain` is the
    /// header a request without Origin gets: the inner app's own, unchanged; `options` the
    /// status of an OPTIONS without Origin, also the worker's own answer).
    pub(crate) fn cors_expected(name: &str, plain: &str, options: u16) -> Vec<String> {
        [
            "preflight (204, 'tauri://localhost', 'authorization, content-type')",
            "preflight foreign (403, '-', '-')",
            "foreign (403, '-', '-')",
            "foreign nokey (403, '-', '-')",
            "allowed nokey (401, 'tauri://localhost', '-')",
            "allowed (200, 'tauri://localhost', '-')",
            "null (403, '-', '-')",
            "upper (403, '-', '-')",
            "slash (403, '-', '-')",
            "ws foreign ('websocket.close', 1008)",
        ]
        .iter()
        .map(|l| format!("{name} {l}"))
        .chain([
            format!("{name} plain (200, '{plain}', '-')"),
            format!("{name} options plain ({options}, "),
        ])
        .collect()
    }

    /// Browsers: the vision and speech workers answer the Harness window's preflight (no key
    /// needed), refuse every other Origin with 403 before the key, keep the key for real
    /// requests, and replace the inner app's own CORS headers.
    #[test]
    fn the_vision_and_audio_workers_answer_only_harness_origins() {
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mlx_vlm_worker.py"), VLM_WORKER).unwrap();
        std::fs::write(dir.path().join("mlx_audio_worker.py"), AUDIO_WORKER).unwrap();
        let script = format!(
            r#"
import os, sys; sys.path.insert(0, {:?})
env = dict(os.environ)
import mlx_vlm_worker as v
os.environ.update(env)  # each worker takes its key and origins out of the environment
import mlx_audio_worker as a
{CORS_DRIVER}
async def inner(scope, receive, send):
    await send({{"type": "http.response.start", "status": 200,
                "headers": [(b"access-control-allow-origin", b"*")]}})
    await send({{"type": "http.response.body", "body": b"inner"}})
report("vlm", v.cors(v.guard(inner, "alias")), "POST", "/v1/chat/completions")
report("audio", a.cors(a.app_for(lambda *x: b"RIFF", "tts", [], 50)), "GET", "/v1/models")
"#,
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
        for line in cors_expected("vlm", "*", 404)
            .into_iter()
            .chain(cors_expected("audio", "-", 404))
        {
            assert!(text.contains(&line), "{line} missing:\n{text}");
        }
    }

    /// The text worker (mlx-lm's http.server handler, here over a stand-in mlx_lm) follows the
    /// same browser rules over real HTTP.
    #[test]
    fn the_text_worker_answers_only_harness_origins() {
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mlx_worker.py"), WORKER).unwrap();
        let stub = dir.path().join("mlx_lm");
        std::fs::create_dir_all(&stub).unwrap();
        std::fs::write(stub.join("__init__.py"), "").unwrap();
        // Just enough of mlx_lm.server for the worker: its handler answers 200 with "*" CORS
        // headers (as mlx-lm does), and the server is handed to the test instead of run.
        std::fs::write(
            stub.join("server.py"),
            r#"
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import types
HANDLER = []
class APIHandler(BaseHTTPRequestHandler):
    created = 0
    def _set_cors_headers(self):
        self.send_header("Access-Control-Allow-Origin", "*")
    def _set_completion_headers(self, status_code=200):
        self.send_response(status_code)
        self.send_header("Content-type", "application/json")
        self._set_cors_headers()
    def _ok(self):
        self._set_completion_headers(200)
        self.send_header("Content-Length", "2")
        self.end_headers()
        self.wfile.write(b"ok")
    def do_GET(self):
        self._ok()
    def do_POST(self):
        self._ok()
    def do_OPTIONS(self):
        self._set_completion_headers(204)
        self.end_headers()
class ModelProvider:
    def __init__(self, cli_args):
        self.cli_args = cli_args
    def load(self, *a):
        pass
class LRUPromptCache:
    def __init__(self, size):
        pass
class ResponseGenerator:
    def __init__(self, *a):
        pass
mx = types.SimpleNamespace(distributed=types.SimpleNamespace(
    init=lambda: types.SimpleNamespace(rank=lambda: 0)))
def _run_http_server(host, port, generator, server_class, handler):
    HANDLER.append(handler)
def main():
    run(None, 0, ModelProvider(types.SimpleNamespace(prompt_cache_size=1)))
"#,
        )
        .unwrap();
        let script = format!(
            r#"
import sys, threading, http.client; sys.path.insert(0, {:?})
import mlx_worker as w
from mlx_lm import server
w.main(["--model", "m", "--port", "0"])
httpd = server.ThreadingHTTPServer(("127.0.0.1", 0), server.HANDLER[0])
threading.Thread(target=httpd.serve_forever, daemon=True).start()
def call(method, path, origin=None, key=True, upgrade=False):
    c = http.client.HTTPConnection("127.0.0.1", httpd.server_port)
    h = {{"Authorization": "Bearer k"}} if key else {{}}
    if upgrade:
        h.update({{"Upgrade": "websocket", "Connection": "Upgrade"}})
    if origin:
        h["Origin"] = origin
    c.request(method, path, body=b"{{}}" if method == "POST" else None, headers=h)
    r = c.getresponse()
    r.read()
    return (r.status, r.getheader("Access-Control-Allow-Origin", "-"),
            r.getheader("Access-Control-Allow-Headers", "-"))
T, E = "tauri://localhost", "https://evil.example"
for label, args in [("preflight", ("OPTIONS", T, False)), ("preflight foreign", ("OPTIONS", E, False)),
                    ("foreign", ("POST", E)), ("foreign nokey", ("POST", E, False)),
                    ("allowed nokey", ("POST", T, False)), ("allowed", ("POST", T)),
                    ("plain", ("POST", None)), ("options plain", ("OPTIONS", None, False)),
                    ("null", ("POST", "null")), ("upper", ("POST", "TAURI://LOCALHOST")),
                    ("slash", ("POST", T + "/")), ("ws foreign", ("GET", E, True, True))]:
    print("text", label, call(args[0], "/v1/chat/completions", *args[1:]))
print("text health foreign", call("GET", "/health", E, False))
"#,
            dir.path().to_string_lossy()
        );
        let out = std::process::Command::new(python)
            .args(["-I", "-c", &script])
            .env("HARNESS_WORKER_API_KEY", "k")
            .env(
                "HARNESS_WORKER_ORIGINS",
                "tauri://localhost,http://a.example",
            )
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{text}");
        // mlx-lm has no websockets: an upgrade from a foreign page is a GET, refused the same.
        for line in cors_expected("text", "-", 204)
            .into_iter()
            .filter(|l| !l.contains("ws foreign"))
            .chain([
                "text health foreign (403, '-', '-')".to_string(),
                "text ws foreign (403, '-', '-')".to_string(),
            ])
        {
            assert!(text.contains(&line), "{line} missing:\n{text}");
        }
    }

    /// A speech request cannot run away: `max_tokens` grows with the text within a floor and a
    /// ceiling, generation streams and stops at the time limit with the audio made so far, and
    /// nothing made by then is `TooSlow` (504). A model without `stream` / `max_tokens` is called
    /// without them.
    #[test]
    fn the_audio_worker_caps_frames_and_time() {
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mlx_audio_worker.py"), AUDIO_WORKER).unwrap();
        let script = format!(
            r#"
import sys, types; sys.path.insert(0, {:?}); import mlx_audio_worker as w
print("budget short", w.frame_budget(8, 1.0))
print("budget long", w.frame_budget(100, 1.0))
print("budget slow", w.frame_budget(80, 0.5))
print("budget cap", w.frame_budget(100000, 1.0))
now = [0.0]
clock = lambda: now[0]
class Talker:
    # Talks forever: one chunk per call, 4 s of (fake) clock each, until closed.
    tokenizer = types.SimpleNamespace(encode=lambda t: t.split())
    def __init__(self):
        self.calls, self.closed = [], False
    def generate(self, text, voice=None, speed=1.0, lang_code="auto", max_tokens=4096, stream=False):
        self.calls.append(dict(voice=voice, max_tokens=max_tokens, stream=stream))
        try:
            while True:
                now[0] += 4.0
                yield types.SimpleNamespace(audio=[0.0] * 10)
        finally:
            self.closed = True
encode = lambda parts, rate: "wav of %d chunks" % len(parts)
t = Talker()
speak = w.make_speak(t, ["ryan"], 24000, 10, encode, clock)
print("runaway", speak("one two three", None, 1.0, None), t.calls, "closed", t.closed)
class Silent:
    # Spends the whole time limit and makes nothing.
    def generate(self, text, max_tokens=4096, stream=False, **kw):
        now[0] += 30
        return iter(())
speak = w.make_speak(Silent(), [], 24000, 10, encode, clock)
try:
    speak("one", None, 1.0, None)
except w.TooSlow:
    print("too slow")
class Plain:
    def generate(self, text, voice=None, speed=1.0):
        yield types.SimpleNamespace(audio=[0.0])
speak = w.make_speak(Plain(), [], 24000, 10, encode, clock)
print("plain", speak("hi", None, 1.0, None))
"#,
            dir.path().to_string_lossy()
        );
        let out = std::process::Command::new(python)
            .args(["-I", "-c", &script])
            .env("HARNESS_WORKER_API_KEY", "k")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{text}");
        for line in [
            "budget short 75",
            "budget long 800",
            "budget slow 1280",
            "budget cap 1500",
            // 3 tokens -> the floor; stops after the chunk that passed 10 s (4 + 4 + 4).
            "runaway ('wav of 3 chunks', True) [{'voice': 'ryan', 'max_tokens': 75, 'stream': True}] closed True",
            "too slow",
            "plain ('wav of 1 chunks', False)",
        ] {
            assert!(text.contains(line), "{line} missing:\n{text}");
        }
    }

    /// The transcription app: the key, its one route, a multipart body of known fields only, a
    /// body cap (by Content-Length too), audio decoded from the body (never a path) and the
    /// worker's time limit as a 504.
    #[test]
    fn the_audio_worker_transcribes_only_checked_requests() {
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mlx_audio_worker.py"), AUDIO_WORKER).unwrap();
        let script = format!(
            r#"
import sys, asyncio; sys.path.insert(0, {:?}); import mlx_audio_worker as w
heard = []
def decode(audio):
    if audio == b"junk":
        raise ValueError("not audio")
    return audio
def transcribe(samples, language, task):
    heard.append((samples, language, task))
    if samples == b"slow":
        raise w.TooSlow()
    if samples == b"long":
        raise w.Refused("the audio is longer than 10 seconds")
    return "hello world"
app = w.transcription_app_for(transcribe, decode, "stt")
def form(*fields, boundary="XyZ"):
    body = b""
    for name, value in fields:
        disposition = b'form-data; name="%s"' % name.encode()
        if name == "file":
            disposition += b'; filename="a.wav"'
        body += b"--" + boundary.encode() + b"\r\nContent-Disposition: " + disposition + b"\r\n\r\n" + value + b"\r\n"
    return body + b"--" + boundary.encode() + b"--\r\n"
CT = b"multipart/form-data; boundary=XyZ"
async def call(method, path, body=b"", ct=CT, key=True, length=None):
    sent, queue = [], [{{"type": "http.request", "body": body, "more_body": False}}]
    async def receive():
        return queue.pop(0) if queue else {{"type": "http.disconnect"}}
    async def send(m):
        sent.append(m)
    headers = [(b"content-type", ct)]
    if key:
        headers.append((b"authorization", b"Bearer k"))
    if length is not None:
        headers.append((b"content-length", str(length).encode()))
    await app({{"type": "http", "method": method, "path": path, "headers": headers}}, receive, send)
    return sent[0]["status"], b"".join(m.get("body", b"") for m in sent[1:]).decode(errors="replace")
async def main():
    T = "/v1/audio/transcriptions"
    binary = bytes(range(256)) * 4 + b"\r\n--Xy\r\n"
    for name, args in [
        ("health", ("GET", "/health")),
        ("nokey", ("POST", T, form(("file", b"RIFF")), CT, False)),
        ("models", ("GET", "/v1/models")),
        ("speech", ("POST", "/v1/audio/speech", b'{{"input":"x"}}')),
        ("ok", ("POST", T, form(("file", binary), ("language", b"tr"), ("model", b"stt")))),
        ("translate", ("POST", T, form(("file", b"RIFF"), ("task", b"translate")))),
        ("json", ("POST", T, form(("file", b"RIFF"), ("response_format", b"json")))),
        ("srt", ("POST", T, form(("file", b"RIFF"), ("response_format", b"srt")))),
        ("nofile", ("POST", T, form(("language", b"en")))),
        ("emptyfile", ("POST", T, form(("file", b"")))),
        ("path", ("POST", T, form(("file", b"RIFF"), ("path", b"/etc/hosts")))),
        ("twice", ("POST", T, form(("file", b"RIFF"), ("file", b"RIFF")))),
        ("task", ("POST", T, form(("file", b"RIFF"), ("task", b"summarize")))),
        ("lang", ("POST", T, form(("file", b"RIFF"), ("language", b"../x")))),
        ("json body", ("POST", T, b'{{"file":"/etc/hosts"}}', b"application/json")),
        ("noboundary", ("POST", T, form(("file", b"RIFF")), b"multipart/form-data")),
        ("malformed", ("POST", T, b"--XyZ\r\nno headers--XyZ--\r\n")),
        ("big", ("POST", T, b"x", CT, True, w.MAX_AUDIO_BODY + 1)),
        ("junk", ("POST", T, form(("file", b"junk")))),
        ("slow", ("POST", T, form(("file", b"slow")))),
        ("long", ("POST", T, form(("file", b"long")))),
    ]:
        print(name, await call(*args))
    print("heard", [(s if len(s) < 10 else (len(s), s == binary), l, t) for s, l, t in heard])
asyncio.run(main())
"#,
            dir.path().to_string_lossy()
        );
        let out = std::process::Command::new(python)
            .args(["-I", "-c", &script])
            .env("HARNESS_WORKER_API_KEY", "k")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{text}");
        for line in [
            "health (200, '{\"status\": \"ok\"}')",
            "nokey (401,",
            "models (200, '{\"object\": \"list\", \"data\": [{\"id\": \"stt\"",
            "speech (404,",
            "ok (200, '{\"text\": \"hello world\"}')",
            "translate (200,",
            "json (200,",
            "srt (400,",
            "nofile (400,",
            "emptyfile (400,",
            "path (400,",
            "twice (400,",
            "task (400,",
            "lang (400,",
            "json body (400,",
            "noboundary (400,",
            "malformed (400,",
            "big (413,",
            "junk (400,",
            "slow (504,",
            "long (400,",
            // The body's bytes reach the decoder unchanged (CRLF and boundary-like bytes too).
            "heard [((1032, True), 'tr', 'transcribe'), (b'RIFF', None, 'translate'), (b'RIFF', None, 'transcribe'), (b'slow', None, 'transcribe'), (b'long', None, 'transcribe')]",
        ] {
            assert!(text.contains(line), "{line} missing:\n{text}");
        }
    }

    /// A transcription cannot run away: audio over the length limit is refused, Whisper is
    /// stopped before its next 30 s window once the time is up (its `decode`), Parakeet after
    /// its next chunk (`chunk_callback`, with chunking for long audio); Parakeet does not
    /// translate.
    #[test]
    fn the_audio_worker_caps_transcriptions() {
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mlx_audio_worker.py"), AUDIO_WORKER).unwrap();
        let script = format!(
            r#"
import sys, types; sys.path.insert(0, {:?}); import mlx_audio_worker as w
now = [0.0]
clock = lambda: now[0]
class Whisper:
    # Decodes one 30 s window per call; each takes 4 s of (fake) clock.
    def __init__(self):
        self.calls = []
    def decode(self, segment, options=None):
        now[0] += 4.0
        return "w"
    def generate(self, audio, *, verbose=None, language=None, task="transcribe", **kw):
        self.calls.append(dict(language=language, task=task))
        windows = max(1, len(audio) // (30 * w.SAMPLE_RATE))
        return types.SimpleNamespace(text=" " + "".join(self.decode(None) for _ in range(windows)) + " ")
class Parakeet:
    def __init__(self):
        self.calls = []
    def generate(self, audio, *, dtype=None, chunk_duration=None, overlap_duration=None, chunk_callback=None, stream=False, **kw):
        self.calls.append(dict(chunk_duration=chunk_duration))
        step = int((chunk_duration or 1e9) * w.SAMPLE_RATE)
        for end in range(step, len(audio) + step, step):
            now[0] += 4.0
            if chunk_callback:
                chunk_callback(min(end, len(audio)), len(audio))
        return types.SimpleNamespace(text="p")
second = [0.0] * w.SAMPLE_RATE
def run(name, transcribe, samples, language=None, task="transcribe"):
    try:
        print(name, repr(transcribe(samples, language, task)))
    except w.TooSlow:
        print(name, "too slow")
    except w.Refused as e:
        print(name, "refused", e)
whisper = Whisper()
t = w.make_transcribe(whisper, 10, 600, clock)
run("whisper short", t, second * 60, "tr", "translate")
run("whisper runaway", t, second * 300)
run("whisper too long", t, second * 601)
print("whisper calls", whisper.calls)
now[0] = 0.0
parakeet = Parakeet()
t = w.make_transcribe(parakeet, 10, 600, clock)
run("parakeet short", t, second * 60)
run("parakeet runaway", t, second * 600)
run("parakeet translate", t, second, None, "translate")
print("parakeet calls", parakeet.calls)
# Whisper's made-up segments past the end of the audio are dropped; without start times the
# text is kept as it is.
seg = lambda start, text, end=None, score=-0.2: dict(start=start, end=start + 1 if end is None else end, text=text, avg_logprob=score)
heard = types.SimpleNamespace(text=" Hello there. pop pop", segments=[
    seg(0.0, " Hello"), seg(1.2, " there."), seg(2.95, " pop"), seg(4.0, " pop"),
    seg(2.0, " K", end=1.0), seg(2.5, " My?", end=9.0)])
nan = types.SimpleNamespace(segments=[seg(0.0, " Real", score=float("nan")), seg(1.0, " speech.", score=float("nan"))])
print("nan scores", repr(w.heard_text(nan, 3.0)))
print("tail", repr(w.heard_text(heard, 3.0)))
print("no starts", repr(w.heard_text(types.SimpleNamespace(text=" as is ", segments=[dict(text="x")]), 3.0)))
print("no segments", repr(w.heard_text(types.SimpleNamespace(text=" p "), 3.0)))
print("parakeet options", w.stt_options({:?}))
"#,
            dir.path().to_string_lossy(),
            dir.path().join("parakeet").to_string_lossy()
        );
        let parakeet = dir.path().join("parakeet");
        std::fs::create_dir_all(&parakeet).unwrap();
        std::fs::write(
            parakeet.join("config.json"),
            r#"{"target":"nemo.collections.asr.models.rnnt_bpe_models.EncDecRNNTBPEModel"}"#,
        )
        .unwrap();
        let out = std::process::Command::new(python)
            .args(["-I", "-c", &script])
            .env("HARNESS_WORKER_API_KEY", "k")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{text}");
        for line in [
            "whisper short 'ww'",
            // 10 windows of 4 s: stopped before the fourth (12 s > 10 s).
            "whisper runaway too slow",
            "whisper too long refused the audio is longer than 600 seconds",
            "whisper calls [{'language': 'tr', 'task': 'translate'}, {'language': None, 'task': 'transcribe'}]",
            "parakeet short 'p'",
            "parakeet runaway too slow",
            "parakeet translate refused this model only transcribes",
            "parakeet calls [{'chunk_duration': None}, {'chunk_duration': 120.0}]",
            "parakeet options {'model_type': 'parakeet'}",
            "tail 'Hello there.'",
            "nan scores 'Real speech.'",
            "no starts 'as is'",
            "no segments 'p'",
        ] {
            assert!(text.contains(line), "{line} missing:\n{text}");
        }
    }

    /// Uploaded audio is never decoded past the length limit: a container header declaring more
    /// is refused before decoding, a header that says less (or nothing) is caught while
    /// streaming, one step past the limit; only WAV, FLAC, MP3 and Ogg Vorbis are taken, and
    /// other rates are resampled to 16 kHz.
    #[test]
    fn the_audio_worker_caps_decoding() {
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mlx_audio_worker.py"), AUDIO_WORKER).unwrap();
        let script = format!(
            r#"
import sys, types; sys.path.insert(0, {:?})
# Stand-ins for numpy (lists) and miniaudio (a header and a stream of chunks).
np = types.ModuleType("numpy")
np.float32 = "f"
np.asarray = lambda x, dtype=None: list(x)
np.concatenate = lambda parts: [v for p in parts for v in p]
np.zeros = lambda n, dtype=None: [0.0] * n
sys.modules["numpy"] = np
import mlx_audio_worker as w
w.DECODE_FRAMES = 1000
class Mini:
    SampleFormat = types.SimpleNamespace(FLOAT32="f32")
    def __init__(self, rate, declared, actual):
        self.rate, self.declared, self.actual, self.read, self.closed = rate, declared, actual, 0, False
    def info(self, data):
        return types.SimpleNamespace(sample_rate=self.rate, nchannels=2, num_frames=self.declared)
    wav_get_info = flac_get_info = mp3_get_info = vorbis_get_info = info
    def stream_memory(self, data, fmt, nchannels, rate, frames):
        assert (fmt, nchannels, rate, frames) == ("f32", 1, self.rate, 1000), (fmt, nchannels, rate, frames)
        def gen():
            try:
                while self.read < self.actual:
                    n = min(frames, self.actual - self.read)
                    self.read += n
                    yield [0.0] * n
            finally:
                self.closed = True
        return gen()
resampled = []
def resample(samples, src, dst):
    resampled.append((len(samples), src, dst))
    return samples[: len(samples) * dst // src]
FLAC = b"fLaC" + b"\0" * 20
def run(name, data, mini, limit=10):
    try:
        out = w.decode_capped(data, limit, mini, resample)
        print(name, "ok", len(out), "read", mini.read, "closed", mini.closed)
    except w.Refused as e:
        print(name, "refused", e, "read", mini.read, "closed", mini.closed)
# 30 minutes declared: refused before any decoding.
run("declared", FLAC, Mini(16000, 30 * 60 * 16000, 30 * 60 * 16000))
# The header says 1 s (or nothing) but the stream goes on: stopped one step past 10 s.
run("lying", FLAC, Mini(16000, 16000, 30 * 60 * 16000))
run("unknown length", FLAC, Mini(16000, 0, 30 * 60 * 16000))
run("fits", b"RIFF\0\0\0\0WAVEfmt ", Mini(16000, 5 * 16000, 5 * 16000))
run("44.1k", b"ID3\3\0\0", Mini(44100, 0, 2 * 44100))
run("ogg", b"OggS\0\2" + b"\0" * 8, Mini(16000, 0, 100))
run("opus-like junk", b"\x1a\x45\xdf\xa3webm", Mini(16000, 0, 100))
run("text", b"hello world!", Mini(16000, 0, 100))
print("resampled", resampled)
"#,
            dir.path().to_string_lossy()
        );
        let out = std::process::Command::new(python)
            .args(["-I", "-c", &script])
            .env("HARNESS_WORKER_API_KEY", "k")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{text}");
        for line in [
            "declared refused the audio is longer than 10 seconds read 0 closed False",
            "lying refused the audio is longer than 10 seconds read 161000 closed True",
            "unknown length refused the audio is longer than 10 seconds read 161000 closed True",
            "fits ok 80000 read 80000 closed True",
            "44.1k ok 32000 read 88200 closed True",
            "ogg ok 100",
            "opus-like junk refused the audio must be WAV, FLAC, MP3 or Ogg Vorbis read 0",
            "text refused the audio must be WAV, FLAC, MP3 or Ogg Vorbis read 0",
            "resampled [(88200, 44100, 16000)]",
        ] {
            assert!(text.contains(line), "{line} missing:\n{text}");
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
        assert!(VLM_WORKER.contains("kw[\"trust_remote_code\"] = False"));
        assert!(VLM_WORKER.contains("MLX_TRUST_REMOTE_CODE\"] = \"false\""));
        assert!(VLM_WORKER.contains("allow_abbrev=False"));
        assert!(AUDIO_WORKER.contains("os.environ.pop(\"HARNESS_WORKER_API_KEY\""));
        assert!(AUDIO_WORKER.contains("HOST = \"127.0.0.1\""));
        assert!(AUDIO_WORKER.contains("kw[\"trust_remote_code\"] = False"));
        assert!(AUDIO_WORKER.contains("HF_HUB_OFFLINE"));
    }
}
