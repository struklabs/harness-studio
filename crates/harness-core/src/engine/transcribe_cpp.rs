//! The transcribe.cpp engine (`transcribe.cpp`, MIT, handy-computer/transcribe.cpp): speech
//! recognition GGUFs of its own converters (Parakeet, Nemotron, Canary, Cohere Transcribe,
//! Qwen3-ASR, Whisper, Moonshine, GigaAM, Granite Speech, SenseVoice...) served by
//! `python/transcribe_cpp_worker.py` through the transcribe-cpp Python binding.
//!
//! The upstream releases carry no command-line program or server, only the binding's native
//! library, so the worker is a small standard-library HTTP server in a managed environment
//! (`pyenv::envs::TRANSCRIBE_CPP`, its NVIDIA build `TRANSCRIBE_CPP_CUDA` with the CUDA
//! provider and the NVIDIA runtime wheels). A launch runs
//!
//! ```text
//! <env python> <env>/transcribe_cpp_worker.py --model <gguf> --host 127.0.0.1 --port <port>
//!     [--alias <alias>]
//! ```
//!
//! On Linux the worker runs without network (`pyenv::isolation`, a Unix socket Harness relays
//! the port to); every run has an API key, like the Python engine's.
//!
//! A model is a GGUF with `stt.variant` in its header: every transcribe.cpp converter writes
//! it and CrispASR's do not, so it decides between the two engines that share architecture
//! names (`parakeet`, `canary`...), and wins over CrispASR and llama.cpp (whose `whisper`
//! architecture it shares too). Voice activity, diarization and language identification exports
//! are not speech recognition models.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{Engine, MetricsSource, ModelFormat, ReleaseSource, RuntimeKind, ServerEngine};
use crate::capabilities::RuntimeCapabilities;
use crate::error_key::err;
use crate::gguf::GgufMetadata;
use crate::params::ParamCatalog;
use crate::process::{BuildOutput, LaunchConfig, ServerCaps};
use crate::pyenv::envs::{EnvSpec, ENVS, TRANSCRIBE_CPP as CPU_ENV};
use crate::runtime::ReleaseAsset;
use crate::telemetry::Metrics;
use crate::vram::{
    EstimateSource, FitCategory, FitInput, FitResult, Measurement, MemoryBudget, MemoryEstimate,
};

/// Family id of the engine.
pub const TRANSCRIBE_CPP: &str = "transcribe.cpp";

/// `load.task` of a definition of such a model (the Playground's speech pickers read it, like
/// CrispASR's `crispasr`).
pub const TASK: &str = "transcribe_cpp";

/// The worker script, written next to the environment ([`worker_script`]).
const WORKER: &str = include_str!("../../python/transcribe_cpp_worker.py");

/// Architectures of transcribe.cpp exports that are not speech recognition models (voice
/// activity, diarization, language identification): the worker's session refuses them.
const NOT_SPEECH_RECOGNITION: [&str; 3] = ["silero_vad", "sortformer", "ecapa_tdnn"];

/// Device memory the runtime takes next to the weights (CUDA context, cuBLAS workspace,
/// encoder activations of one window); a rough figure until it is measured.
const CUDA_OVERHEAD_BYTES: u64 = 600 << 20;

#[derive(Debug, Clone, Copy, Default)]
pub struct TranscribeCppEngine;

/// Whether a GGUF header is a transcribe.cpp speech recognition export.
pub fn is_export(meta: &GgufMetadata) -> bool {
    meta.stt_variant.is_some()
        && !meta
            .architecture
            .as_deref()
            .is_some_and(|a| NOT_SPEECH_RECOGNITION.contains(&a))
}

/// Whether the file at `path` is a transcribe.cpp speech recognition GGUF.
pub fn is_model(path: &Path) -> bool {
    ModelFormat::of(path) == ModelFormat::Gguf
        && crate::models::cached_metadata(path).is_some_and(|m| is_export(&m))
}

/// Whether the model at `path` translates (`stt.capability.translate`: Canary, Granite,
/// Voxtral, Whisper...).
pub fn can_translate(path: &Path) -> bool {
    crate::models::cached_metadata(path).is_some_and(|m| m.stt_translate)
}

/// Whether a Hub repo (a listing item or `/api/models/<id>`) holds transcribe.cpp speech
/// recognition GGUFs: its `library_name` (or tag) says transcribe.cpp and its task is speech
/// recognition. handy-computer's voice activity, diarization and language identification repos
/// carry other tasks.
pub fn is_hub_repo(item: &serde_json::Value) -> bool {
    let library = item
        .get("library_name")
        .and_then(|l| l.as_str())
        .is_some_and(|l| l.eq_ignore_ascii_case(TRANSCRIBE_CPP));
    let tagged = item
        .get("tags")
        .and_then(|t| t.as_array())
        .is_some_and(|tags| {
            tags.iter().any(|t| {
                t.as_str()
                    .is_some_and(|t| t.eq_ignore_ascii_case(TRANSCRIBE_CPP))
            })
        });
    let task = item.get("pipeline_tag").and_then(|t| t.as_str());
    (library || tagged) && task == Some("automatic-speech-recognition")
}

/// The environment the worker runs in: the NVIDIA build when it is ready for this version,
/// else the default one.
pub fn env() -> &'static EnvSpec {
    env_in(&crate::pyenv::envs::root())
}

fn env_in(root: &Path) -> &'static EnvSpec {
    ENVS.iter()
        .find(|s| {
            s.gpu_build_of == Some(CPU_ENV.name) && crate::pyenv::envs::status_in(root, s).ready
        })
        .unwrap_or(&CPU_ENV)
}

/// Whether the worker would run on the NVIDIA build (its env is ready and there is a CUDA
/// driver): then its model takes device memory.
fn runs_on_cuda() -> bool {
    env().gpu_build_of.is_some() && crate::pyenv::envs::cuda_present()
}

/// The worker script of this version, written next to the environment (rewritten when it
/// differs, so an app update brings its worker along).
pub fn worker_script() -> Result<PathBuf, String> {
    let dir = crate::pyenv::envs::env_dir_in(&crate::pyenv::envs::root(), env().name);
    let path = dir.join("transcribe_cpp_worker.py");
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
/// else `runtime.family_not_installed: transcribe.cpp` (the UI offers the install).
pub fn binary() -> Result<PathBuf, String> {
    let root = crate::pyenv::envs::root();
    let spec = env_in(&root);
    if !crate::pyenv::envs::status_in(&root, spec).ready {
        return Err(format!(
            "{}: {}",
            crate::runtimes::ERR_FAMILY_NOT_INSTALLED,
            TRANSCRIBE_CPP
        ));
    }
    Ok(crate::pyenv::envs::python_in(&root, spec.name))
}

/// Size of the model file (the weights the backend loads).
fn model_bytes(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Device memory of a launch: the weights and the runtime's share on the NVIDIA build; none on
/// the default build (CPU; its Vulkan module may still use a GPU, which is not counted).
pub fn vram_need(config: &LaunchConfig) -> u64 {
    if runs_on_cuda() {
        model_bytes(&config.model_path) + CUDA_OVERHEAD_BYTES
    } else {
        0
    }
}

/// The command line of a launch: the worker script first (the program is the interpreter).
fn build_args(config: &LaunchConfig, script: &Path) -> Vec<String> {
    let mut args = vec![
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
    args
}

// ---- requests -------------------------------------------------------------------------------

/// The sample rate every transcribe.cpp model takes.
pub const SAMPLE_RATE: u32 = 16_000;

/// `audio` as the worker takes it: a 16 kHz mono 16-bit PCM WAV, with its length in seconds.
/// A 16-bit mono WAV is resampled here; anything else (stereo or float WAV, MP3, M4A, Opus...)
/// goes through ffmpeg first, and without ffmpeg it is refused (`stt.needs_ffmpeg`).
pub async fn prepare_audio(audio: Vec<u8>) -> Result<(Vec<u8>, f64), String> {
    use crate::asr::run::{pcm16_mono, resample_16k, wav_of};
    fn to_16k(wav: &[u8]) -> Result<Option<(Vec<u8>, f64)>, String> {
        let Some((rate, data)) = pcm16_mono(wav) else {
            return Ok(None);
        };
        if !(1_000..=384_000).contains(&rate) {
            return Err("Invalid WAV sample rate: expected 1000–384000 Hz.".into());
        }
        // Bound the decoded duration (and thus the resampled allocation) before copying PCM.
        let projected_samples = data.len() as f64 / 2.0 * f64::from(SAMPLE_RATE) / f64::from(rate);
        if projected_samples > crate::asr::llm::MAX_AUDIO_SECS * f64::from(SAMPLE_RATE) {
            return Err("Audio exceeds the maximum duration of 2 hours.".into());
        }
        let pcm = if rate == SAMPLE_RATE {
            data.to_vec()
        } else {
            resample_16k(rate, data)
        };
        let secs = pcm.len() as f64 / 2.0 / f64::from(SAMPLE_RATE);
        Ok(Some((wav_of(SAMPLE_RATE, &[&pcm]), secs)))
    }
    if let Some(ready) = to_16k(&audio)? {
        return Ok(ready);
    }
    if crate::audio::ffmpeg_path().is_none() {
        return Err(err("stt.needs_ffmpeg")
            .with("format", crate::audio::sniff(&audio).name())
            .into());
    }
    let converted = crate::audio::to_wav(audio).await?;
    to_16k(&converted)?.ok_or_else(|| {
        String::from(err("audio.convert_failed").with("error", "not a 16-bit mono WAV"))
    })
}

/// The worker's answer (`{"text", "language", "duration", "segments": [{"start", "end",
/// "text"}]}`) as a transcript. A model without timestamps answers no segments: its text is
/// one segment over the whole audio, so subtitles still have a cue.
pub fn transcript_of(
    body: &[u8],
    model: &str,
    duration_secs: f64,
    elapsed_ms: u64,
) -> Option<crate::asr::Transcript> {
    use crate::asr::Segment;
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let text = v.get("text")?.as_str()?.trim().to_string();
    let mut segments: Vec<Segment> = v
        .get("segments")
        .and_then(|s| s.as_array())
        .map(|s| {
            s.iter()
                .filter_map(|s| {
                    Some(Segment {
                        start: s.get("start")?.as_f64()?,
                        end: s.get("end")?.as_f64()?,
                        text: s.get("text")?.as_str()?.trim().to_string(),
                        speaker: None,
                    })
                })
                .filter(|s| !s.text.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if segments.is_empty() && !text.is_empty() {
        segments.push(Segment {
            start: 0.0,
            end: duration_secs,
            text: text.clone(),
            speaker: None,
        });
    }
    let language = v
        .get("language")
        .and_then(|l| l.as_str())
        .filter(|l| !l.is_empty())
        .map(str::to_string);
    Some(crate::asr::Transcript {
        text,
        segments,
        duration_secs,
        language,
        model: model.to_string(),
        elapsed_ms,
    })
}

impl Engine for TranscribeCppEngine {
    fn family(&self) -> &'static str {
        TRANSCRIBE_CPP
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

    /// GGUFs are told apart by [`is_model`] (their header), not by the format alone: a GGUF in
    /// general belongs to llama.cpp.
    fn supports(&self, _format: ModelFormat) -> bool {
        false
    }

    fn detect_capabilities(&self, binary: &Path) -> Arc<RuntimeCapabilities> {
        Arc::new(self.probe_capabilities(binary))
    }

    fn cached_capabilities(&self, binary: &Path) -> Option<Arc<RuntimeCapabilities>> {
        Some(self.detect_capabilities(binary))
    }

    /// Nothing to probe: the worker's flags are its own.
    fn probe_capabilities(&self, _binary: &Path) -> RuntimeCapabilities {
        RuntimeCapabilities::default()
    }

    fn param_catalog(&self) -> &'static ParamCatalog {
        static CATALOG: ParamCatalog = ParamCatalog {
            schema_version: 1,
            params: Vec::new(),
        };
        &CATALOG
    }

    fn as_server(&self) -> Option<&dyn ServerEngine> {
        Some(self)
    }
}

impl ServerEngine for TranscribeCppEngine {
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
        if !is_model(&config.model_path) {
            return Err(err("transcribe_cpp.model_unsupported")
                .with("path", config.model_path.display())
                .into());
        }
        if config
            .custom_args
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
        {
            return Err(err("transcribe_cpp.custom_args_unsupported").into());
        }
        Ok(BuildOutput {
            args: build_args(config, &worker_script()?),
            skipped: Vec::new(),
        })
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

    fn worker_env(&self, api_key: &str) -> Option<Vec<(&'static str, std::ffi::OsString)>> {
        let mut env = crate::pyenv::python_env(&crate::pyenv::envs::root());
        env.push((super::python_hf::API_KEY_ENV, api_key.into()));
        Some(env)
    }

    fn uses_socket(&self) -> bool {
        true
    }

    fn listen_on_socket(&self, args: &mut Vec<String>, socket: &Path) {
        super::python_hf::listen_on_socket(args, socket);
    }

    /// The weights on the card on the NVIDIA build (plus the runtime's share), else in RAM;
    /// the interpreter and the binding take ~100 MB of RAM either way.
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
                .map(|p| model_bytes(Path::new(p)))
                .unwrap_or(0)
        };
        let host = 100u64 << 20;
        let cuda = runs_on_cuda() && budget.vram_bytes > 0;
        let (vram, ram) = if cuda {
            (model + CUDA_OVERHEAD_BYTES, host)
        } else {
            (0, host + model + model / 4)
        };
        let estimate = MemoryEstimate {
            vram_bytes: vram,
            ram_bytes: ram,
            model_host_bytes: if cuda { 0 } else { model },
            compute_host_bytes: host,
            slots: 1,
            exact: false,
            ..Default::default()
        };
        let category = if cuda {
            if vram * 10 <= budget.vram_bytes * 9 {
                FitCategory::FullGpu
            } else {
                FitCategory::OomRisk
            }
        } else if ram * 10 <= budget.ram_bytes * 9 {
            FitCategory::CpuHeavy
        } else {
            FitCategory::OomRisk
        };
        FitResult {
            category,
            vram_percentage: (vram * 100).checked_div(budget.vram_bytes).unwrap_or(0) as u32,
            ram_percentage: (ram * 100).checked_div(budget.ram_bytes).unwrap_or(0) as u32,
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A GGUF header with `general.architecture` and, when given, `stt.variant`.
    pub(crate) fn export(path: &Path, arch: &str, variant: Option<&str>) {
        use std::io::Write;
        fn kv(f: &mut std::fs::File, key: &str, value: &str) {
            f.write_all(&(key.len() as u64).to_le_bytes()).unwrap();
            f.write_all(key.as_bytes()).unwrap();
            f.write_all(&8u32.to_le_bytes()).unwrap();
            f.write_all(&(value.len() as u64).to_le_bytes()).unwrap();
            f.write_all(value.as_bytes()).unwrap();
        }
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(b"GGUF").unwrap();
        f.write_all(&3u32.to_le_bytes()).unwrap();
        f.write_all(&0u64.to_le_bytes()).unwrap();
        f.write_all(&(1 + u64::from(variant.is_some())).to_le_bytes())
            .unwrap();
        kv(&mut f, "general.architecture", arch);
        if let Some(v) = variant {
            kv(&mut f, "stt.variant", v);
        }
    }

    #[test]
    fn an_stt_variant_makes_a_transcribe_cpp_model_and_wins_the_routing() {
        let dir = tempfile::tempdir().unwrap();
        let launch = |path: &Path| {
            super::super::server_for(&LaunchConfig {
                model_path: path.to_path_buf(),
                ..Default::default()
            })
            .family()
        };
        // Architectures CrispASR (parakeet, canary) and llama.cpp (whisper) also know.
        for (name, arch) in [
            ("parakeet-tdt-0.6b-v3-Q4_K_M.gguf", "parakeet"),
            ("canary-180m-flash-Q8_0.gguf", "canary"),
            ("whisper-tiny-Q5_K_M.gguf", "whisper"),
            ("cohere-transcribe-03-2026-Q4_K_M.gguf", "cohere_asr"),
        ] {
            let path = dir.path().join(name);
            export(&path, arch, Some(name));
            assert!(is_model(&path), "{name}");
            assert!(!super::super::crispasr::is_model(&path), "{name}");
            // Nor its wider "speech recognition GGUF" check (the gateway scheduler refused
            // transcriptions with it as a CrispASR route).
            assert!(!super::super::crispasr::is_asr_gguf(&path), "{name}");
            assert_eq!(launch(&path), TRANSCRIBE_CPP, "{name}");
            assert_eq!(
                super::super::server_for_model(&path).map(|e| e.family()),
                Some(TRANSCRIBE_CPP),
                "{name}"
            );
            assert!(!super::super::is_llama_server(&path, false), "{name}");
        }
        // Without the key: CrispASR's (parakeet) and llama.cpp's (whisper is not a GGUF arch of
        // CrispASR) as before.
        let crisp = dir.path().join("parakeet-crisp.gguf");
        export(&crisp, "parakeet", None);
        assert!(!is_model(&crisp));
        assert!(super::super::crispasr::is_asr_gguf(&crisp));
        assert_eq!(launch(&crisp), super::super::CRISPASR);
        // Voice activity, diarization and language identification are not speech recognition.
        for arch in NOT_SPEECH_RECOGNITION {
            let path = dir.path().join(format!("{arch}.gguf"));
            export(&path, arch, Some(arch));
            assert!(!is_model(&path), "{arch}");
        }
    }

    #[test]
    fn hub_repos_need_the_library_and_the_speech_recognition_task() {
        let repo = |library: &str, task: &str| {
            serde_json::json!({
                "id": "handy-computer/x-gguf",
                "library_name": library,
                "pipeline_tag": task,
            })
        };
        assert!(is_hub_repo(&repo(
            "transcribe.cpp",
            "automatic-speech-recognition"
        )));
        assert!(!is_hub_repo(&repo(
            "transcribe.cpp",
            "voice-activity-detection"
        )));
        assert!(!is_hub_repo(&repo("gguf", "automatic-speech-recognition")));
        let tagged = serde_json::json!({
            "id": "someone/parakeet-gguf",
            "tags": ["gguf", "transcribe.cpp"],
            "pipeline_tag": "automatic-speech-recognition",
        });
        assert!(is_hub_repo(&tagged));
    }

    #[test]
    fn audio_becomes_16k_mono_wav_and_answers_become_transcripts() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        // One second of 8 kHz mono 16-bit PCM: resampled without ffmpeg.
        let wav8k = crate::asr::run::wav_of(8000, &[&vec![0u8; 16000]]);
        let (wav, secs) = rt.block_on(prepare_audio(wav8k)).unwrap();
        assert!((secs - 1.0).abs() < 1e-3, "{secs}");
        let (rate, data) = crate::asr::run::pcm16_mono(&wav).unwrap();
        assert_eq!((rate, data.len()), (SAMPLE_RATE, 32000));

        let body = br#"{"text":" hello world ","language":"en","duration":2.0,"segments":[{"start":0.0,"end":1.0,"text":" hello "},{"start":1.0,"end":2.0,"text":"world"}]}"#;
        let t = transcript_of(body, "parakeet", 2.0, 5).unwrap();
        assert_eq!(t.text, "hello world");
        assert_eq!(t.segments.len(), 2);
        assert_eq!(t.language.as_deref(), Some("en"));
        assert!(t.to_srt().contains("00:00:01,000 --> 00:00:02,000"));
        // No timestamps: the text is one cue over the audio.
        let t =
            transcript_of(br#"{"text":"hi","language":"","segments":[]}"#, "m", 3.0, 0).unwrap();
        assert_eq!((t.segments.len(), t.segments[0].end), (1, 3.0));
        assert_eq!(t.language, None);
        assert!(transcript_of(b"{}", "m", 1.0, 0).is_none());
    }

    #[tokio::test]
    async fn audio_rejects_invalid_sample_rates_before_resampling() {
        let pcm = vec![0; 1024 * 1024];
        for rate in [0, 1, 999, 384_001] {
            let wav = crate::asr::run::wav_of(rate, &[&pcm]);
            assert_eq!(
                prepare_audio(wav).await.unwrap_err(),
                "Invalid WAV sample rate: expected 1000–384000 Hz.",
                "rate {rate}"
            );
        }
    }

    #[tokio::test]
    async fn audio_rejects_excessive_projected_duration_before_resampling() {
        // Just over two hours at an accepted rate: resampling would allocate over 219 MiB.
        let pcm = vec![0; (crate::asr::llm::MAX_AUDIO_SECS as usize * 1_000 + 1) * 2];
        let wav = crate::asr::run::wav_of(1_000, &[&pcm]);
        assert_eq!(
            prepare_audio(wav).await.unwrap_err(),
            "Audio exceeds the maximum duration of 2 hours."
        );
    }

    #[tokio::test]
    async fn audio_accepts_sample_rate_boundaries() {
        for rate in [1_000, SAMPLE_RATE, 384_000] {
            let wav = crate::asr::run::wav_of(rate, &[&vec![0; rate as usize * 2]]);
            let (wav, secs) = prepare_audio(wav).await.unwrap();
            let (out_rate, pcm) = crate::asr::run::pcm16_mono(&wav).unwrap();
            assert_eq!((out_rate, pcm.len()), (SAMPLE_RATE, 32_000));
            assert_eq!(secs, 1.0);
        }
    }

    #[test]
    fn launch_args_and_socket_rewrite() {
        let config = LaunchConfig {
            model_path: "m.gguf".into(),
            host: "127.0.0.1".into(),
            port: 9000,
            ..Default::default()
        };
        let mut args = build_args(&config, Path::new("w.py"));
        assert_eq!(
            args[..7],
            [
                "w.py",
                "--model",
                "m.gguf",
                "--host",
                "127.0.0.1",
                "--port",
                "9000"
            ]
        );
        TranscribeCppEngine.listen_on_socket(&mut args, Path::new("/run/w.sock"));
        assert!(!args.iter().any(|a| a == "--host" || a == "--port"));
        assert!(args.windows(2).any(|w| w == ["--uds", "/run/w.sock"]));
    }

    #[test]
    fn the_worker_serves_transcriptions_behind_its_key() {
        use super::super::mlx::tests::system_python;
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let worker = dir.path().join("transcribe_cpp_worker.py");
        std::fs::write(&worker, WORKER).unwrap();
        let script = dir.path().join("test_transcribe_cpp_worker.py");
        std::fs::write(
            &script,
            include_str!("../../python/test_transcribe_cpp_worker.py"),
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
            "{stdout}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
