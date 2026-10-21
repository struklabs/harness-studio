//! Engine abstraction (private hs-internal architecture notes).
//!
//! A runtime family (llama.cpp today; whisper.cpp, stable-diffusion.cpp later) is one
//! [`Engine`]. It names its binaries and release source, detects a build's capabilities,
//! owns its parameter catalog and says which model formats it can run. The rest depends on
//! how the family runs:
//! - [`ServerEngine`]: a long-lived process with an HTTP endpoint (llama-server,
//!   whisper-server): launch arguments, readiness, OpenAI base URL, telemetry and the
//!   memory estimate.
//! - [`JobEngine`]: one process per request (sd.cpp, llama-bench): arguments, progress
//!   lines, cancellation and output files; [`job::run_job`] drives it.
//!
//! The llama.cpp layers keep their own modules (`process`, `capabilities`, `params`,
//! `vram`, `telemetry`); [`LlamaCppEngine`] is the seam the callers go through, and the
//! golden tests (`engine_golden_tests.rs`) pin its output to what those modules produced
//! before the refactor.

pub mod audio_cpp;
pub mod audio_process;
pub use audio_cpp::{AudioCppEngine, AUDIO_CPP};
pub mod crispasr;
pub mod crispasr_helpers;
pub mod crispasr_tts;
pub use crispasr::{CrispAsrEngine, CRISPASR};
pub mod job;
mod lemonade;
mod llama_bench;
mod llama_cpp;
pub mod mistralrs;
pub mod mlx;
mod onnx;
pub mod python_hf;
mod sd_cpp;
mod sherpa_onnx;
pub mod transcribe_cpp;
mod whisper_cpp;
#[cfg(test)]
mod whisper_e2e;

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::capabilities::RuntimeCapabilities;
use crate::gguf::GgufMetadata;
use crate::hardware::AssetQuery;
use crate::params::ParamCatalog;
use crate::process::{BuildOutput, LaunchConfig, ServerCaps};
use crate::runtime::{ReleaseAsset, ReleaseAssetInfo};
use crate::telemetry::Metrics;
use crate::vram::{FitInput, FitResult, Measurement, MemoryBudget};

pub use job::{JobCommand, JobEngine, JobEvent, JobOutcome, JobProgress};
pub use lemonade::LemonadeEngine;
pub use llama_bench::{LlamaBenchEngine, LLAMA_BENCH};
pub use llama_cpp::LlamaCppEngine;
pub(crate) use mistralrs::delete_model_dir as mistralrs_delete_model_dir;
pub use mistralrs::{
    concurrency as mistralrs_concurrency, inspect as mistralrs_inspect,
    is_library_model as is_mistralrs_library_model, is_model_dir as is_mistralrs_dir,
    trained_context as mistralrs_trained_context, upstream_body as mistralrs_upstream_body,
    MistralRsEngine, ModelDirInfo as MistralRsDirInfo, MISTRALRS,
};
pub use mlx::{MlxEngine, MFLUX, MLX, MLX_AUDIO, MLX_EMBED, MLX_VLM};
pub use onnx::{
    provider_of as onnx_provider_of, vram_need as onnx_vram_need, OnnxEngine, ONNX_FAMILY,
    PROVIDER_PARAM as ONNX_PROVIDER_PARAM,
};
pub use python_hf::{PythonHfEngine, PYTHON_HF};
pub use sd_cpp::{
    component_files as sd_component_files, detect_main_role as sd_detect_main_role,
    footprint_bytes as sd_footprint_bytes, installed_build_has_gpu as sd_installed_build_has_gpu,
    is_image_arch, is_image_gguf, is_image_launch, is_image_path, is_image_safetensors,
    is_video_arch, main_role as sd_main_role, min_vram_need as sd_min_vram_need,
    vram_need as sd_vram_need, weights_bytes as sd_weights_bytes, MainRole, SdCppEngine,
    COMPONENTS as SD_COMPONENTS, SD_CPP,
};
pub use sherpa_onnx::{SherpaOnnxEngine, SHERPA_ONNX, SHERPA_ONNX_REPO};
pub use transcribe_cpp::{TranscribeCppEngine, TRANSCRIBE_CPP};
pub(crate) use whisper_cpp::gpu_fallback_note as whisper_gpu_fallback_note;
pub use whisper_cpp::{
    footprint_bytes as whisper_footprint_bytes,
    installed_build_has_gpu as whisper_installed_build_has_gpu, is_ggml_file,
    vram_need as whisper_vram_need, WhisperCppEngine, WHISPER_CPP,
};

/// Family id of llama.cpp builds (official, system, GitHub forks such as PrismML).
pub const LLAMA_CPP: &str = "llama.cpp";

/// Display name of an engine family in messages ("MLX", "Python (Transformers)"); an unknown family is
/// shown as it is. The GUI's `engineLabel` uses the same names.
pub fn display_name(family: &str) -> &str {
    match family {
        LLAMA_CPP => "llama.cpp",
        "mlx" => "MLX",
        "mlx-vlm" => "MLX-VLM",
        "mlx-audio" => "MLX Audio",
        "mlx-embed" => "MLX Embeddings",
        "mflux" => "mflux",
        "python-hf" => "Python (Transformers)",
        "liquid-audio" => "Liquid Audio",
        "audio.cpp" => "audio.cpp",
        "crispasr" => "CrispASR",
        "transcribe.cpp" => "transcribe.cpp",
        "mistral.rs" => "mistral.rs",
        "whisper.cpp" => "whisper.cpp",
        "stable-diffusion.cpp" => "sd.cpp",
        "onnxruntime" => "ONNX Runtime",
        "sherpa-onnx" => "sherpa-onnx",
        "lemonade" => "Lemonade",
        other => other,
    }
}

/// Family of runtimes recorded before families existed (`runtimes.json`, manifests).
pub fn default_family() -> String {
    LLAMA_CPP.to_string()
}

/// How a family runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum RuntimeKind {
    /// Always-on process with an HTTP endpoint.
    Server,
    /// One process per request: progress, cancellation, files as output.
    Job,
}

/// Model file formats, by what an engine has to parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFormat {
    Gguf,
    /// whisper.cpp's `ggml-*.bin`.
    GgmlBin,
    /// An ONNX model: sherpa-onnx speech folders (Kokoro, Piper / VITS, ASR) or a Hugging Face
    /// encoder (`model.onnx`) run by `harness-onnx`.
    Onnx,
    Safetensors,
    /// A PyTorch checkpoint (`.ckpt`), one of the single-file image model formats.
    Ckpt,
    Other,
}

impl ModelFormat {
    /// From the file name; the formats here are not told apart by content first.
    pub fn of(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("gguf") => Self::Gguf,
            Some("bin") => Self::GgmlBin,
            Some("onnx") => Self::Onnx,
            Some("safetensors") => Self::Safetensors,
            Some("ckpt") => Self::Ckpt,
            _ => Self::Other,
        }
    }
}

/// Where a family's managed builds come from and what they are called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseSource {
    /// GitHub `owner/repo` of the official releases.
    pub repo: &'static str,
    /// Executable stem of the process Harness runs (`llama-server`).
    pub binary: &'static str,
}

impl ReleaseSource {
    /// Executable file name on this OS.
    pub fn binary_file(&self) -> String {
        if cfg!(target_os = "windows") {
            format!("{}.exe", self.binary)
        } else {
            self.binary.to_string()
        }
    }

    /// GitHub API URL of the latest releases of the official repo.
    pub fn releases_api(&self, per_page: u32) -> String {
        self.releases_api_of(self.repo, per_page)
    }

    /// GitHub API URL of one release of the official repo.
    pub fn release_api(&self, tag: &str) -> String {
        self.release_api_of(self.repo, tag)
    }

    /// Prefix every archive URL of the official repo starts with.
    pub fn download_prefix(&self) -> String {
        self.download_prefix_of(self.repo)
    }

    /// [`Self::releases_api`] of a fork that follows the family's release layout.
    pub fn releases_api_of(&self, repo: &str, per_page: u32) -> String {
        format!(
            "https://api.github.com/repos/{}/releases?per_page={}",
            repo, per_page
        )
    }

    /// [`Self::release_api`] of a fork.
    pub fn release_api_of(&self, repo: &str, tag: &str) -> String {
        format!(
            "https://api.github.com/repos/{}/releases/tags/{}",
            repo, tag
        )
    }

    /// [`Self::download_prefix`] of a fork.
    pub fn download_prefix_of(&self, repo: &str) -> String {
        format!("https://github.com/{}/releases/download/", repo)
    }
}

/// Endpoints a server exposes for telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricsSource {
    /// Prometheus text exposition.
    pub metrics_path: &'static str,
    /// Per-slot JSON, if the family has slots.
    pub slots_path: Option<&'static str>,
}

/// What every runtime family provides.
pub trait Engine: Send + Sync {
    /// Family id stored with runtimes (`llama.cpp`).
    fn family(&self) -> &'static str;
    fn kind(&self) -> RuntimeKind;
    fn release_source(&self) -> ReleaseSource;
    /// The archive (plus companions) of one GitHub release that fits `query`; `None` when
    /// the release has none (or is not a build release of this family).
    fn release_asset(
        &self,
        release: &serde_json::Value,
        query: &AssetQuery,
    ) -> Option<ReleaseAssetInfo>;
    /// Picks the archive (plus companions) of a release's assets by the family's naming
    /// scheme, trying `backends` in order (`vulkan`, `cuda`, `rocm`, `sycl`, `cpu`,
    /// `metal`). Returns the archives and the backend that matched; `None` when the release
    /// has no build for this OS/arch.
    fn pick_release_assets(
        &self,
        assets: &[ReleaseAsset],
        backends: &[String],
    ) -> Option<(Vec<ReleaseAsset>, Option<String>)>;
    /// Whether a GitHub release object is one to install from (default: every release).
    /// Lemonade publishes `candidate-` pre-releases whose assets look like a stable one's.
    fn accepts_release(&self, _release: &serde_json::Value) -> bool {
        true
    }
    fn supports(&self, format: ModelFormat) -> bool;
    /// Argument that makes the binary print something and exit 0: used to see that a freshly
    /// installed build runs and to read its version (llama.cpp: `--version`).
    fn version_arg(&self) -> &'static str {
        "--version"
    }
    /// The version line to record from what [`Engine::version_arg`] printed, for a family
    /// other than llama.cpp whose binary prints one (mistral.rs); `None` = the binary prints
    /// none and the release tag is the version (whisper.cpp, stable-diffusion.cpp).
    fn version_text(&self, _output: &str) -> Option<String> {
        None
    }
    /// Capabilities of a build (flags from `--help`, version, devices), cached by file
    /// identity. Blocking: runs the binary on a cache miss.
    fn detect_capabilities(&self, binary: &Path) -> Arc<RuntimeCapabilities>;
    /// The cached capabilities of a build; never runs the binary.
    fn cached_capabilities(&self, binary: &Path) -> Option<Arc<RuntimeCapabilities>>;
    /// A fresh probe that is not cached or written to disk (`harness doctor` is read-only).
    /// Blocking.
    fn probe_capabilities(&self, binary: &Path) -> RuntimeCapabilities;
    /// The family's parameter catalog (llama.cpp: `params.toml`).
    fn param_catalog(&self) -> &'static ParamCatalog;
    fn as_server(&self) -> Option<&dyn ServerEngine> {
        None
    }
    fn as_job(&self) -> Option<&dyn JobEngine> {
        None
    }
}

/// A family that runs as an always-on HTTP server.
pub trait ServerEngine: Engine {
    /// Command-line arguments of a launch for this build.
    fn build_launch(
        &self,
        config: &LaunchConfig,
        caps: &ServerCaps,
        runtime: Option<&RuntimeCapabilities>,
    ) -> Result<BuildOutput, String>;
    /// Readiness endpoint, polled until [`ServerEngine::health`] is true.
    fn health_path(&self) -> &'static str;
    /// Whether a readiness response means "loaded and serving".
    fn health(&self, status: u16, body: &str) -> bool;
    /// OpenAI-compatible base URL of a server at `host_url` (`http://host:port`).
    fn openai_base(&self, host_url: &str) -> String;
    fn metrics_source(&self) -> MetricsSource;
    fn parse_metrics(&self, text: &str) -> Metrics;
    /// Memory fit of a launch on a budget, corrected by earlier measurements.
    fn estimate_memory(
        &self,
        input: &FitInput,
        meta: Option<&GgufMetadata>,
        budget: &MemoryBudget,
        measured: &[Measurement],
    ) -> FitResult;
    /// A server that runs in a managed Python environment ([`crate::pyenv`]): Harness gives it
    /// an API key for every run (the user's, else a fresh one) and these environment variables;
    /// `None` for every other family.
    fn worker_env(&self, _api_key: &str) -> Option<Vec<(&'static str, std::ffi::OsString)>> {
        None
    }
    /// Whether the server can listen on a Unix socket ([`ServerEngine::listen_on_socket`]). A
    /// worker that can is started without network when [`crate::pyenv::isolation`] is
    /// available, and Harness relays its port to the socket; one that cannot runs on its
    /// loopback port, guarded by its key only.
    fn uses_socket(&self) -> bool {
        false
    }
    /// Rewrites a launch's arguments to listen on the Unix socket `socket` instead of its port.
    fn listen_on_socket(&self, _args: &mut Vec<String>, _socket: &Path) {}
    /// A local server that needs no network but only listens on TCP: on Linux it runs without
    /// network behind [`crate::netns_bridge`] when [`crate::pyenv::isolation`] is available,
    /// and as before (with a log line saying so) when it is not.
    fn netns_bridge(&self) -> bool {
        false
    }
}

/// An engine held by value in structs that derive `Default` (llama.cpp).
#[derive(Clone, Copy)]
pub struct EngineRef(pub &'static dyn Engine);

impl Default for EngineRef {
    fn default() -> Self {
        Self(&LLAMA)
    }
}

impl std::ops::Deref for EngineRef {
    type Target = dyn Engine;
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl std::fmt::Debug for EngineRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.family())
    }
}

static LLAMA: LlamaCppEngine = LlamaCppEngine;
static AUDIO: AudioCppEngine = AudioCppEngine;
static CRISP: CrispAsrEngine = CrispAsrEngine;
static WHISPER: WhisperCppEngine = WhisperCppEngine;
static SDCPP: SdCppEngine = SdCppEngine;
static SHERPA: SherpaOnnxEngine = SherpaOnnxEngine;
static ONNX: OnnxEngine = OnnxEngine;
static LEMONADE: LemonadeEngine = LemonadeEngine;
static MISTRAL: MistralRsEngine = MistralRsEngine;
static PYHF: PythonHfEngine = PythonHfEngine;
static TRANSCRIBE: TranscribeCppEngine = TranscribeCppEngine;
static MLX_ENGINE: MlxEngine = MlxEngine::TEXT;
static MLX_VLM_ENGINE: MlxEngine = MlxEngine::VISION;
static MLX_AUDIO_ENGINE: MlxEngine = MlxEngine::AUDIO;
static MFLUX_ENGINE: MlxEngine = MlxEngine::IMAGE;
static MLX_EMBED_ENGINE: MlxEngine = MlxEngine::RETRIEVAL;
static LLAMA_BENCH_ENGINE: LlamaBenchEngine = LlamaBenchEngine;

/// The llama.cpp engine.
pub fn llama_cpp() -> &'static dyn ServerEngine {
    &LLAMA
}

/// The llama-bench job engine (a tool of the llama.cpp family, not a runtime family).
pub fn llama_bench() -> &'static dyn JobEngine {
    &LLAMA_BENCH_ENGINE
}

/// The whisper.cpp engine.
pub fn whisper_cpp() -> &'static dyn ServerEngine {
    &WHISPER
}

/// The stable-diffusion.cpp engine.
pub fn sd_cpp() -> &'static dyn ServerEngine {
    &SDCPP
}

/// The sherpa-onnx (text-to-speech, speech recognition) job engine.
pub fn sherpa_onnx() -> &'static dyn JobEngine {
    &SHERPA
}

/// The ONNX Runtime engine (`harness-onnx`).
pub fn onnx() -> &'static dyn ServerEngine {
    &ONNX
}

/// The Lemonade engine (`lemond`).
pub fn lemonade() -> &'static dyn ServerEngine {
    &LEMONADE
}

/// The mistral.rs engine (safetensors LLMs and VLMs).
pub fn mistralrs() -> &'static dyn ServerEngine {
    &MISTRAL
}

/// The Python engine (`python-hf`: transformers, sentence-transformers, diffusers).
pub fn python_hf() -> &'static dyn ServerEngine {
    &PYHF
}

/// The audio.cpp resident text-to-speech engine.
pub fn audio_cpp() -> &'static dyn ServerEngine {
    &AUDIO
}

/// Every known family.
pub fn engines() -> [&'static dyn Engine; 16] {
    [
        &LLAMA,
        &WHISPER,
        &AUDIO,
        &TRANSCRIBE,
        &CRISP,
        &SDCPP,
        &SHERPA,
        &ONNX,
        &LEMONADE,
        &MISTRAL,
        &PYHF,
        &MLX_ENGINE,
        &MLX_VLM_ENGINE,
        &MLX_AUDIO_ENGINE,
        &MFLUX_ENGINE,
        &MLX_EMBED_ENGINE,
    ]
}

/// Whether a model path is on disk: a model file, or a mistral.rs or Python model folder.
pub fn model_on_disk(path: &Path) -> bool {
    path.is_file()
        || audio_cpp::is_model(path)
        || is_mistralrs_dir(path)
        || python_hf::is_model_dir(path)
        || mlx::is_any_model_dir(path)
}

/// Whether a mistral.rs runtime is registered (`runtimes.json`). Unit tests of this crate see
/// it registered unless a test says otherwise ([`tests::without_mistralrs`]), so no test reads
/// the registry of the machine it runs on.
pub fn mistralrs_registered() -> bool {
    registered_family(MISTRALRS)
}

#[cfg(test)]
fn registered_family(_family: &str) -> bool {
    tests::MISTRALRS_REGISTERED.with(|c| c.get())
}

#[cfg(not(test))]
fn registered_family(family: &str) -> bool {
    crate::runtimes::RuntimeRegistry::global()
        .entries()
        .map(|entries| entries.iter().any(|e| e.family == family))
        .unwrap_or(false)
}

/// The engine of a safetensors chat checkpoint folder ([`is_mistralrs_dir`]): mistral.rs, which
/// is the user's own build and so optional; without it the Python engine runs the folder with
/// transformers when it can ([`python_hf::chat_task`]).
fn chat_checkpoint_engine(path: &Path) -> &'static dyn ServerEngine {
    if !mistralrs_registered() && python_hf::chat_task(path).is_some() {
        &PYHF
    } else {
        &MISTRAL
    }
}

/// Whether mistral.rs is the engine that serves `path` (a checkpoint folder it runs, with the
/// runtime registered), as opposed to the Python engine's transformers fallback.
pub fn runs_on_mistralrs(path: &Path) -> bool {
    is_mistralrs_dir(path) && chat_checkpoint_engine(path).family() == MISTRALRS
}

/// The engine of a family id; `None` for families this build does not know (a newer
/// `runtimes.json`).
pub fn engine(family: &str) -> Option<&'static dyn Engine> {
    engines().into_iter().find(|e| e.family() == family)
}

/// The server engine for a launch, by its model file: a whisper ggml `.bin` runs on
/// whisper-server, an `.onnx` file on harness-onnx, an image model (safetensors / `.ckpt`, or a
/// GGUF that is a diffusion model, see [`is_image_launch`]) on sd-server, a safetensors model
/// folder (`config.json` + weights, see [`is_mistralrs_dir`]) on mistral.rs, everything else
/// (router directories included) on llama-server.
pub fn server_for(config: &LaunchConfig) -> &'static dyn ServerEngine {
    if audio_cpp::is_model(&config.model_path) {
        return &AUDIO;
    }
    if config.router_mode {
        return &LLAMA;
    }
    // Before CrispASR and llama.cpp, which share its architecture names.
    if transcribe_cpp::is_model(&config.model_path) {
        return &TRANSCRIBE;
    }
    if is_mistralrs_dir(&config.model_path) {
        return chat_checkpoint_engine(&config.model_path);
    }
    if python_hf::is_model_dir(&config.model_path) {
        return &PYHF;
    }
    if let Some(engine) = mlx_engine_for(&config.model_path) {
        return engine;
    }
    if crispasr::is_asr_gguf(&config.model_path) || crispasr_tts::is_tts_gguf(&config.model_path) {
        return &CRISP;
    }
    match ModelFormat::of(&config.model_path) {
        ModelFormat::GgmlBin => &WHISPER,
        ModelFormat::Onnx => &ONNX,
        _ if is_image_launch(config) => &SDCPP,
        _ => &LLAMA,
    }
}

/// Whether a server started for `model_path` is a llama-server (it answers `/props`,
/// `/slots` and `/metrics`); a whisper, ONNX or mistral.rs server only has `/health`. An image
/// model (sd-server) is told apart by its serve mode, see `ActiveServerInfo::is_llama`.
pub fn is_llama_server(model_path: &Path, router_mode: bool) -> bool {
    !audio_cpp::is_model(model_path)
        && (router_mode
            || !(matches!(
                ModelFormat::of(model_path),
                ModelFormat::GgmlBin | ModelFormat::Onnx
            ) || is_mistralrs_dir(model_path)
                || python_hf::is_model_dir(model_path)
                || mlx::is_any_model_dir(model_path)
                || transcribe_cpp::is_model(model_path)
                || crispasr::is_asr_gguf(model_path)
                || crispasr_tts::is_tts_gguf(model_path)))
}

/// Device memory a launch of a non-llama.cpp family takes (whisper.cpp, stable-diffusion.cpp,
/// ONNX Runtime or mistral.rs); zero when it stays on the CPU.
pub fn non_llama_vram_need(config: &LaunchConfig) -> u64 {
    match server_for(config).family() {
        AUDIO_CPP => audio_cpp::vram_need(config),
        SD_CPP => sd_vram_need(config),
        ONNX_FAMILY => onnx_vram_need(config),
        MISTRALRS => mistralrs::vram_need(config),
        PYTHON_HF => python_hf::vram_need(config),
        TRANSCRIBE_CPP => transcribe_cpp::vram_need(config),
        MLX | MLX_VLM | MLX_AUDIO | MFLUX | MLX_EMBED => mlx::vram_need(config),
        _ => whisper_vram_need(config),
    }
}

/// Whether a non-llama.cpp launch is known to take no GPU memory: a CPU build, backend or
/// provider the launch names. A zero [`non_llama_vram_need`] does not say so: the families
/// without an estimate of their own (Python, MLX, mistral.rs, the whisper-style fallback) report
/// none although they may run on the GPU.
pub fn non_llama_runs_on_cpu(config: &LaunchConfig) -> bool {
    match server_for(config).family() {
        SD_CPP => sd_cpp::runs_on_cpu(config),
        WHISPER_CPP => whisper_cpp::runs_on_cpu(config),
        ONNX_FAMILY => onnx::provider_of(config) == "cpu",
        AUDIO_CPP => audio_cpp::backend(config) == "cpu",
        _ => false,
    }
}

/// The least GPU memory a non-llama.cpp launch can load with when the engine places what does
/// not fit in system memory itself (stable-diffusion.cpp's auto-fit); `None`: it needs
/// [`non_llama_vram_need`] on the card.
pub fn non_llama_min_vram_need(config: &LaunchConfig) -> Option<u64> {
    match server_for(config).family() {
        SD_CPP => sd_min_vram_need(config),
        _ => None,
    }
}

/// The serving mode a non-llama.cpp family runs in (its HTTP surface, see
/// [`crate::serve_mode::ServeMode`]); the ONNX family derives its own from the repo task.
pub fn serve_mode_of(family: &str) -> crate::serve_mode::ServeMode {
    match family {
        SD_CPP => crate::serve_mode::ServeMode::Image,
        MISTRALRS | MLX | MLX_VLM => crate::serve_mode::ServeMode::Chat,
        // Text to speech: the gateway's /v1/audio/speech posts to the worker.
        // Images of mflux: the gateway's /v1/images routes post to the worker.
        MLX_AUDIO | AUDIO_CPP | MFLUX => crate::serve_mode::ServeMode::Pipeline,
        _ => crate::serve_mode::ServeMode::Transcription,
    }
}

/// The MLX engine of an MLX model folder: mlx-vlm for a vision model, mlx-audio for a
/// text-to-speech one, mlx otherwise.
fn mlx_engine_for(path: &Path) -> Option<&'static MlxEngine> {
    Some(match mlx::flavour_of(path)? {
        mlx::Flavour::Text => &MLX_ENGINE,
        mlx::Flavour::Vision => &MLX_VLM_ENGINE,
        mlx::Flavour::Audio => &MLX_AUDIO_ENGINE,
        mlx::Flavour::Image => &MFLUX_ENGINE,
        mlx::Flavour::Retrieval => &MLX_EMBED_ENGINE,
    })
}

/// The server engine that runs a model file (or a mistral.rs model folder), by its format.
pub fn server_for_model(path: &Path) -> Option<&'static dyn ServerEngine> {
    if audio_cpp::is_model(path) {
        return Some(&AUDIO);
    }
    if transcribe_cpp::is_model(path) {
        return Some(&TRANSCRIBE);
    }
    if is_mistralrs_dir(path) {
        return Some(chat_checkpoint_engine(path));
    }
    if python_hf::is_model_dir(path) {
        return Some(&PYHF);
    }
    if let Some(engine) = mlx_engine_for(path) {
        return Some(engine);
    }
    if crispasr::is_model(path) || crispasr_tts::is_model(path) {
        return Some(&CRISP);
    }
    if crispasr::is_asr_gguf(path) || crispasr_tts::is_tts_gguf(path) {
        return None;
    }
    let format = ModelFormat::of(path);
    engines()
        .into_iter()
        .filter(|e| e.supports(format))
        .find_map(|e| e.as_server())
}

/// `http://host:port`, with IPv6 hosts bracketed and wildcard binds made connectable.
pub fn host_url(host: &str, port: u16) -> String {
    let host = crate::process::connect_host(host);
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{}]", host)
    } else {
        host
    };
    format!("http://{}:{}", host, port)
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        /// What [`mistralrs_registered`] says in this test thread (registered by default).
        pub(super) static MISTRALRS_REGISTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
    }

    /// Runs `f` as if no mistral.rs runtime were registered.
    pub(crate) fn without_mistralrs<T>(f: impl FnOnce() -> T) -> T {
        MISTRALRS_REGISTERED.with(|c| c.set(false));
        let out = f();
        MISTRALRS_REGISTERED.with(|c| c.set(true));
        out
    }

    #[test]
    fn families_resolve_and_old_records_are_llama_cpp() {
        assert_eq!(engine(LLAMA_CPP).unwrap().family(), "llama.cpp");
        assert_eq!(engine("whisper.cpp").unwrap().family(), "whisper.cpp");
        assert_eq!(
            engine("stable-diffusion.cpp").unwrap().family(),
            "stable-diffusion.cpp"
        );
        assert_eq!(engine("sherpa-onnx").unwrap().kind(), RuntimeKind::Job);
        assert_eq!(engine(ONNX_FAMILY).unwrap().family(), "onnxruntime");
        assert_eq!(engine("lemonade").unwrap().family(), "lemonade");
        assert!(engine("future-engine.cpp").is_none());
        assert_eq!(default_family(), LLAMA_CPP);
        let e = engine(LLAMA_CPP).unwrap();
        assert_eq!(e.kind(), RuntimeKind::Server);
        assert!(e.as_server().is_some() && e.as_job().is_none());
        assert_eq!(e.release_source().repo, "ggml-org/llama.cpp");
        assert_eq!(e.release_source().binary, "llama-server");
    }

    #[test]
    fn formats_by_extension() {
        assert_eq!(
            ModelFormat::of(Path::new("a/Qwen3-4B.Q4_K_M.GGUF")),
            ModelFormat::Gguf
        );
        assert_eq!(
            ModelFormat::of(Path::new("ggml-base.en.bin")),
            ModelFormat::GgmlBin
        );
        assert_eq!(
            ModelFormat::of(Path::new("model.safetensors")),
            ModelFormat::Safetensors
        );
        assert_eq!(
            ModelFormat::of(Path::new("vits/tr_TR-fettah-medium.onnx")),
            ModelFormat::Onnx
        );
        assert_eq!(
            ModelFormat::of(Path::new("onnx/model_quantized.ONNX")),
            ModelFormat::Onnx
        );
        assert_eq!(ModelFormat::of(Path::new("README")), ModelFormat::Other);
        let llama = llama_cpp();
        assert!(llama.supports(ModelFormat::Gguf));
        assert!(!llama.supports(ModelFormat::GgmlBin));
        assert!(!llama.supports(ModelFormat::Safetensors));
    }

    #[test]
    fn mlx_folders_route_by_their_vision_tower() {
        let root = tempfile::Builder::new()
            .prefix(&crate::paths::test_dir_prefix("mlx-route"))
            .tempdir()
            .unwrap();
        let folder = |name: &str, config: &str, readme: Option<&str>| {
            let dir = root.path().join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("config.json"), config).unwrap();
            std::fs::write(dir.join("model.safetensors"), [0u8; 16]).unwrap();
            if let Some(r) = readme {
                std::fs::write(dir.join("README.md"), r).unwrap();
            }
            dir
        };
        let q = r#""quantization":{"group_size":64,"bits":4}"#;
        let text = folder(
            "Qwen3-4B-4bit",
            &format!(r#"{{"architectures":["Qwen3ForCausalLM"],"model_type":"qwen3",{q}}}"#),
            None,
        );
        let vision = folder(
            "Qwen3-VL-2B-4bit",
            &format!(
                r#"{{"architectures":["Qwen3VLForConditionalGeneration"],"model_type":"qwen3_vl","vision_config":{{}},{q}}}"#
            ),
            None,
        );
        let plain = folder(
            "Qwen2.5-0.5B",
            r#"{"architectures":["Qwen2ForCausalLM"],"model_type":"qwen2"}"#,
            None,
        );
        let plain_vlm = folder(
            "Qwen2.5-VL-3B",
            r#"{"architectures":["Qwen2_5_VLForConditionalGeneration"],"model_type":"qwen2_5_vl","vision_config":{}}"#,
            None,
        );
        let blip = folder(
            "blip-captioning",
            r#"{"architectures":["BlipForConditionalGeneration"],"model_type":"blip"}"#,
            Some("---\npipeline_tag: image-to-text\nlibrary_name: transformers\n---\n"),
        );
        let launch = |dir: &Path| {
            server_for(&LaunchConfig {
                model_path: dir.to_path_buf(),
                ..Default::default()
            })
            .family()
        };
        let model = |dir: &Path| server_for_model(dir).map(|e| e.family());
        assert_eq!(launch(&text), MLX);
        assert_eq!(model(&text), Some(MLX));
        assert_eq!(launch(&vision), MLX_VLM);
        assert_eq!(model(&vision), Some(MLX_VLM));
        // Not MLX's: mistral.rs and the Python engine keep theirs.
        assert_eq!(launch(&plain), MISTRALRS);
        assert_eq!(model(&plain), Some(MISTRALRS));
        assert_eq!(launch(&plain_vlm), MISTRALRS);
        assert_eq!(launch(&blip), PYTHON_HF);
        assert_eq!(model(&blip), Some(PYTHON_HF));
        for dir in [&text, &vision] {
            assert!(model_on_disk(dir));
            assert!(!is_llama_server(dir, false));
        }
        assert_eq!(serve_mode_of(MLX_VLM), crate::serve_mode::ServeMode::Chat);
        let tts = folder(
            "Qwen3-TTS-0.6B-8bit",
            &format!(
                r#"{{"architectures":["Qwen3TTSForConditionalGeneration"],"model_type":"qwen3_tts",{q}}}"#
            ),
            Some("---\nlibrary_name: mlx-audio\n---\n"),
        );
        assert_eq!(launch(&tts), MLX_AUDIO);
        assert_eq!(model(&tts), Some(MLX_AUDIO));
        assert!(model_on_disk(&tts) && !is_llama_server(&tts, false));
        assert_eq!(
            serve_mode_of(MLX_AUDIO),
            crate::serve_mode::ServeMode::Pipeline
        );
        assert_eq!(engine(MLX_VLM).unwrap().family(), MLX_VLM);
        assert_eq!(engine(MLX).unwrap().family(), MLX);
    }

    /// Without a registered mistral.rs the Python engine runs a safetensors chat checkpoint
    /// (text or vision) with transformers; a folder it cannot run (remote code) stays mistral.rs's.
    #[test]
    fn chat_checkpoints_fall_back_to_the_python_engine() {
        let root = tempfile::Builder::new()
            .prefix(&crate::paths::test_dir_prefix("pyhf-chat-route"))
            .tempdir()
            .unwrap();
        let make = |name: &str, config: &str| {
            let dir = root.path().join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("config.json"), config).unwrap();
            std::fs::write(dir.join("model.safetensors"), [0u8; 16]).unwrap();
            dir
        };
        let text = make(
            "qwen",
            r#"{"architectures":["Qwen2ForCausalLM"],"model_type":"qwen2"}"#,
        );
        let vision = make(
            "smolvlm",
            r#"{"architectures":["Idefics3ForConditionalGeneration"],"model_type":"idefics3","vision_config":{}}"#,
        );
        let remote = make(
            "remote",
            r#"{"architectures":["OddForCausalLM"],"auto_map":{"AutoModel":"x.Y"}}"#,
        );
        let launch = |dir: &Path| {
            server_for(&LaunchConfig {
                model_path: dir.to_path_buf(),
                ..Default::default()
            })
            .family()
        };
        // Registered: mistral.rs keeps every chat checkpoint.
        for dir in [&text, &vision, &remote] {
            assert_eq!(launch(dir), MISTRALRS);
            assert!(runs_on_mistralrs(dir));
        }
        without_mistralrs(|| {
            assert_eq!(launch(&text), PYTHON_HF);
            assert_eq!(launch(&vision), PYTHON_HF);
            assert_eq!(server_for_model(&text).unwrap().family(), PYTHON_HF);
            assert!(!runs_on_mistralrs(&text));
            // Remote code is never run: the folder stays with mistral.rs (which then asks to be set up).
            assert_eq!(launch(&remote), MISTRALRS);
            assert!(!is_llama_server(&text, false));
            assert!(model_on_disk(&text));
            assert_eq!(python_hf::chat_task(&text), Some("text-generation"));
            assert_eq!(python_hf::chat_task(&vision), Some("image-text-to-text"));
            assert_eq!(python_hf::chat_task(&remote), None);
            assert_eq!(
                python_hf::serve_mode(&text),
                crate::serve_mode::ServeMode::Chat
            );
            // A chat model, not a pipeline: the library keeps it a language model.
            assert_eq!(
                python_hf::kind_of("text-generation"),
                crate::domain::ModelKind::Llm
            );
        });
    }

    #[test]
    fn safetensors_model_folders_route_to_mistralrs() {
        let root = tempfile::Builder::new()
            .prefix(&crate::paths::test_dir_prefix("mrs-route"))
            .tempdir()
            .unwrap();
        let dir = root.path().join("Qwen2.5-0.5B-Instruct");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.json"),
            r#"{"architectures":["Qwen2ForCausalLM"],"model_type":"qwen2"}"#,
        )
        .unwrap();
        std::fs::write(dir.join("model.safetensors"), [0u8; 16]).unwrap();
        let mut c = LaunchConfig {
            model_path: dir.clone(),
            ..Default::default()
        };
        assert_eq!(server_for(&c).family(), MISTRALRS);
        assert_eq!(engine(MISTRALRS).unwrap().kind(), RuntimeKind::Server);
        assert_eq!(
            engine("mistral.rs").unwrap().release_source().binary,
            "mistralrs"
        );
        assert!(!is_llama_server(&dir, false));
        assert_eq!(serve_mode_of(MISTRALRS), crate::serve_mode::ServeMode::Chat);
        assert_eq!(server_for_model(&dir).unwrap().family(), MISTRALRS);
        assert!(model_on_disk(&dir) && !model_on_disk(&root.path().join("nope")));
        // The single weights file of the folder is not a launch of its own here: an image
        // checkpoint by itself goes to sd-server as before.
        assert_eq!(
            server_for_model(&dir.join("model.safetensors"))
                .unwrap()
                .family(),
            SD_CPP
        );
        // Router mode and folders without a transformers config stay llama-server's.
        c.router_mode = true;
        assert_eq!(server_for(&c).family(), LLAMA_CPP);
        let plain = root.path().join("ggufs");
        std::fs::create_dir_all(&plain).unwrap();
        let c = LaunchConfig {
            model_path: plain.clone(),
            ..Default::default()
        };
        assert_eq!(server_for(&c).family(), LLAMA_CPP);
        assert!(is_llama_server(&plain, false));
        assert_eq!(
            server_for(&LaunchConfig {
                model_path: "m/Qwen3-4B-Q4_K_M.gguf".into(),
                ..Default::default()
            })
            .family(),
            LLAMA_CPP
        );
    }

    #[test]
    fn host_urls() {
        assert_eq!(host_url("127.0.0.1", 8080), "http://127.0.0.1:8080");
        assert_eq!(host_url("0.0.0.0", 8080), "http://127.0.0.1:8080");
        assert_eq!(host_url("::1", 8080), "http://[::1]:8080");
        assert_eq!(
            llama_cpp().openai_base(&host_url("127.0.0.1", 11440)),
            "http://127.0.0.1:11440/v1"
        );
    }
}
