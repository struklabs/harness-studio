//! Managed CrispASR v0.8.43 native ASR and preset TTS server (MIT).
use std::path::Path;
use std::sync::Arc;

use super::{Engine, MetricsSource, ModelFormat, ReleaseSource, RuntimeKind, ServerEngine};
use crate::capabilities::RuntimeCapabilities;
use crate::gguf::GgufMetadata;
use crate::params::ParamCatalog;
use crate::process::{BuildOutput, LaunchConfig, ServerCaps};
use crate::runtime::ReleaseAsset;
use crate::telemetry::Metrics;
use crate::vram::{FitInput, FitResult, Measurement, MemoryBudget};

pub const CRISPASR: &str = "crispasr";
pub const VERSION: &str = "v0.8.43";

#[derive(Debug, Clone, Copy)]
pub struct CrispAsrEngine;

/// Only standalone ASR architectures from the pinned converter/backend table.
pub fn is_asr_arch(arch: &str) -> bool {
    matches!(
        arch,
        "parakeet"
            | "parakeet-tdt"
            | "parakeet-ja"
            | "parakeet_ja"
            | "canary"
            | "canary_qwen"
            | "canary-qwen"
            | "cohere"
            | "cohere-transcribe"
            | "qwen3asr"
            | "qwen3-asr"
            | "qwen3_asr"
            | "voxtral"
            | "voxtral4b"
            | "nemotron"
            | "nemotron-asr"
            | "nemotron-streaming"
            | "gigaam"
            | "granitespeech"
            | "granite-nle"
            | "granite_nle"
            | "granitenle"
            | "granite_speech"
            | "granite-speech"
            | "voxtral-4b"
            | "voxtral_4b"
            | "vibevoice-asr"
            | "vibevoice_asr"
            | "vibevoice"
            | "moonshine"
            | "moonshine-tiny"
            | "moonshine-base"
            | "moonshine_streaming"
            | "sensevoice"
            | "wav2vec2"
            | "wav2vec2-ctc"
            | "hubert"
            | "hubert-ctc"
            | "data2vec"
            | "data2vec-audio"
            | "data2vec_audio"
    )
}

/// Repository provenance is a format boundary: transcribe.cpp exports are not interchangeable.
pub fn supports_repo(repo: &str, task: Option<&str>) -> bool {
    let repo = repo.to_ascii_lowercase();
    repo == "handy-computer/canary-180m-flash-gguf"
        || (repo.starts_with("cstr/")
            && repo.ends_with("-gguf")
            && task == Some("automatic-speech-recognition"))
}

pub fn supports_metadata(meta: &GgufMetadata) -> bool {
    if !meta.architecture.as_deref().is_some_and(is_asr_arch) {
        return false;
    }
    let provenance = [
        &meta.url,
        &meta.repo_url,
        &meta.source_url,
        &meta.source_repo_url,
    ];
    !provenance.into_iter().flatten().any(|url| {
        let url = url.to_ascii_lowercase();
        url.contains("handy-computer/") && !url.contains("handy-computer/canary-180m-flash-gguf")
    })
}

/// ASR container evidence, including exports refused by the format check.
pub fn is_asr_gguf(path: &Path) -> bool {
    ModelFormat::of(path) == ModelFormat::Gguf
        && crate::models::cached_metadata(path)
            .as_ref()
            .and_then(|m| m.architecture.as_deref())
            .is_some_and(|a| is_asr_arch(a) || a == "cohere_asr")
}

/// Architectures whose CrispASR backend translates (CAP_TRANSLATE); every other family only
/// transcribes.
pub fn arch_can_translate(arch: &str) -> bool {
    matches!(
        arch,
        "canary"
            | "cohere"
            | "cohere-transcribe"
            | "voxtral"
            | "voxtral4b"
            | "voxtral-4b"
            | "voxtral_4b"
            | "granitespeech"
            | "granite-speech"
            | "granite_speech"
    )
}

/// Whether the model file at `path` can translate (decided from its GGUF architecture).
pub fn can_translate(path: &Path) -> bool {
    crate::models::cached_metadata(path)
        .as_ref()
        .and_then(|m| m.architecture.as_deref())
        .is_some_and(arch_can_translate)
}

pub fn is_model(path: &Path) -> bool {
    if ModelFormat::of(path) != ModelFormat::Gguf {
        return false;
    }
    let lower = path.to_string_lossy().to_ascii_lowercase();
    if lower.contains("handy-computer") && !lower.contains("canary-180m-flash") {
        return false;
    }
    let Some(meta) = crate::models::cached_metadata(path) else {
        return false;
    };
    if !supports_metadata(&meta) {
        return false;
    }
    // CrispASR's Cohere converter uses these names; transcribe.cpp uses a different table.
    if meta
        .architecture
        .as_deref()
        .is_some_and(|a| matches!(a, "cohere" | "cohere-transcribe"))
    {
        let names = crate::gguf::read_gguf_tensor_names(path, 4096).unwrap_or_default();
        return ["enc.pre.out.weight", "dec.emb.weight"]
            .iter()
            .all(|name| names.iter().any(|n| n == name));
    }
    true
}

/// Exact release names, with backend recorded even for the Metal/default CPU archives.
pub fn pick_assets_on(
    assets: &[ReleaseAsset],
    backends: &[String],
    os: &str,
    arch: &str,
) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
    let ext = if os == "windows" { "zip" } else { "tar.gz" };
    let prefix = format!("crispasr-{os}-{arch}");
    let mut order = backends.to_vec();
    if os == "macos" && arch == "arm64" {
        order.insert(0, "metal".into());
    }
    if !order.iter().any(|b| b == "cpu") {
        order.push("cpu".into());
    }
    for backend in order {
        let (suffix, chosen) = match backend.as_str() {
            "rocm" | "hip" if os == "linux" => ("-hip", "hip"),
            "cuda" if arch == "x86_64" => ("-cuda", "cuda"),
            "vulkan" if arch == "x86_64" && os != "macos" => ("-vulkan", "vulkan"),
            "metal" if os == "macos" && arch == "arm64" => ("", "metal"),
            "cpu" => (if os == "windows" { "-cpu" } else { "" }, "cpu"),
            _ => continue,
        };
        let name = format!("{prefix}{suffix}.{ext}");
        if let Some(asset) = assets.iter().find(|a| a.name == name) {
            return Some((vec![asset.clone()], Some(chosen.into())));
        }
    }
    None
}

impl Engine for CrispAsrEngine {
    fn family(&self) -> &'static str {
        CRISPASR
    }
    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Server
    }
    fn release_source(&self) -> ReleaseSource {
        ReleaseSource {
            repo: "CrispStrobe/CrispASR",
            binary: "crispasr",
        }
    }
    fn accepts_release(&self, release: &serde_json::Value) -> bool {
        release.get("tag_name").and_then(|v| v.as_str()) == Some(VERSION)
    }
    fn release_asset(
        &self,
        _: &serde_json::Value,
        _: &crate::hardware::AssetQuery,
    ) -> Option<crate::runtime::ReleaseAssetInfo> {
        None
    }
    fn pick_release_assets(
        &self,
        assets: &[ReleaseAsset],
        backends: &[String],
    ) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
        pick_assets_on(
            assets,
            backends,
            std::env::consts::OS,
            if cfg!(target_arch = "aarch64") {
                "arm64"
            } else {
                "x86_64"
            },
        )
    }
    fn supports(&self, format: ModelFormat) -> bool {
        format == ModelFormat::Gguf
    }
    fn detect_capabilities(&self, binary: &Path) -> Arc<RuntimeCapabilities> {
        crate::capabilities::registry()
            .get_or_probe_with(binary, crate::capabilities::crispasr::probe_binary)
    }
    fn cached_capabilities(&self, binary: &Path) -> Option<Arc<RuntimeCapabilities>> {
        crate::capabilities::registry().cached(binary)
    }
    fn probe_capabilities(&self, binary: &Path) -> RuntimeCapabilities {
        crate::capabilities::crispasr::probe_binary(binary)
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

impl ServerEngine for CrispAsrEngine {
    /// CrispASR reads local models and helper files only: no network on Linux.
    fn netns_bridge(&self) -> bool {
        true
    }
    fn build_launch(
        &self,
        config: &LaunchConfig,
        _: &ServerCaps,
        _: Option<&RuntimeCapabilities>,
    ) -> Result<BuildOutput, String> {
        if config.host != "127.0.0.1" {
            return Err("CrispASR requires 127.0.0.1".into());
        }
        if !is_model(&config.model_path) && !super::crispasr_tts::is_model(&config.model_path) {
            return Err(
                "Only CrispASR-format ASR and supported preset TTS GGUF models are supported"
                    .into(),
            );
        }
        if config
            .custom_args
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
        {
            return Err("CrispASR custom launch arguments are not supported".into());
        }
        // Helper models come from Harness's verified downloader and travel as local paths; the
        // worker's own cache folder is ours, so nothing it might fetch lands in the home folder.
        let cache = super::crispasr_helpers::cache_dir();
        std::fs::create_dir_all(&cache).map_err(|e| format!("{}: {e}", cache.display()))?;
        let lid = super::crispasr_helpers::LID.installed();
        let punc = super::crispasr_helpers::PUNC.installed();
        super::crispasr_helpers::note_launch(config.port, lid.is_some(), punc.is_some());
        let mut args = vec![
            "--server".into(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            config.port.to_string(),
            "-m".into(),
            config.model_path.to_string_lossy().into(),
        ];
        args.extend(super::crispasr_helpers::launch_args(
            &cache,
            lid.as_deref(),
            punc.as_deref(),
        ));
        if super::crispasr_tts::is_model(&config.model_path) {
            args.extend(super::crispasr_tts::launch_args(&config.model_path)?);
        }
        Ok(BuildOutput {
            args,
            skipped: Vec::new(),
        })
    }
    fn worker_env(&self, api_key: &str) -> Option<Vec<(&'static str, std::ffi::OsString)>> {
        let mut env: Vec<(&'static str, std::ffi::OsString)> =
            vec![("CRISPASR_API_KEYS", api_key.into())];
        env.extend(super::crispasr_helpers::worker_env(
            &super::crispasr_helpers::cache_dir(),
        ));
        if let Some(dict) = super::crispasr_helpers::MISAKI.verified() {
            env.push(("CRISPASR_MISAKI_DICT_PATH", dict.into_os_string()));
        }
        if let Some(dict) = super::crispasr_helpers::CMUDICT.verified() {
            env.push(("CRISPASR_CMUDICT_PATH", dict.into_os_string()));
        }
        env.push(("CRISPASR_KOKORO_G2P", "builtin-only".into()));
        env.push((
            "CRISPASR_G2P_MODEL_PATH",
            super::crispasr_helpers::cache_dir()
                .join("disabled-g2p.json")
                .into_os_string(),
        ));
        Some(env)
    }
    fn health_path(&self) -> &'static str {
        "/health"
    }
    fn health(&self, status: u16, body: &str) -> bool {
        super::whisper_cpp().health(status, body)
    }
    fn openai_base(&self, host: &str) -> String {
        host.trim_end_matches('/').into()
    }
    fn metrics_source(&self) -> MetricsSource {
        MetricsSource {
            metrics_path: "",
            slots_path: None,
        }
    }
    fn parse_metrics(&self, _: &str) -> Metrics {
        Metrics::default()
    }
    fn estimate_memory(
        &self,
        input: &FitInput,
        meta: Option<&GgufMetadata>,
        budget: &MemoryBudget,
        measured: &[Measurement],
    ) -> FitResult {
        super::whisper_cpp().estimate_memory(input, meta, budget, measured)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn model(path: &Path, arch: &str, tensors: &[&str]) {
        let mut bytes = b"GGUF".to_vec();
        bytes.extend(3u32.to_le_bytes());
        bytes.extend((tensors.len() as u64).to_le_bytes());
        bytes.extend(1u64.to_le_bytes());
        let string = |bytes: &mut Vec<u8>, s: &str| {
            bytes.extend((s.len() as u64).to_le_bytes());
            bytes.extend(s.as_bytes());
        };
        string(&mut bytes, "general.architecture");
        bytes.extend(8u32.to_le_bytes());
        string(&mut bytes, arch);
        for tensor in tensors {
            string(&mut bytes, tensor);
            bytes.extend(1u32.to_le_bytes());
            bytes.extend(1u64.to_le_bytes());
            bytes.extend(0u32.to_le_bytes());
            bytes.extend(0u64.to_le_bytes());
        }
        bytes.resize(bytes.len().div_ceil(32) * 32 + 4, 0);
        std::fs::write(path, bytes).unwrap();
    }
    #[test]
    fn platform_assets_and_backend_fallbacks() {
        let names = [
            "windows-x86_64-cpu.zip",
            "windows-x86_64-vulkan.zip",
            "windows-x86_64-cuda.zip",
            "linux-x86_64.tar.gz",
            "linux-x86_64-hip.tar.gz",
            "linux-x86_64-vulkan.tar.gz",
            "linux-x86_64-cuda.tar.gz",
            "linux-arm64.tar.gz",
            "macos-arm64.tar.gz",
            "macos-x86_64.tar.gz",
        ];
        let assets: Vec<_> = names
            .iter()
            .map(|n| ReleaseAsset {
                name: format!("crispasr-{n}"),
                download_url: String::new(),
                size: 0,
                digest: None,
                integrity: crate::runtime::AssetIntegrity::Missing,
            })
            .collect();
        for (os, arch, requested, expected) in [
            ("windows", "x86_64", "vulkan", "vulkan"),
            ("windows", "x86_64", "cuda", "cuda"),
            ("linux", "x86_64", "rocm", "hip"),
            ("linux", "x86_64", "cuda", "cuda"),
            ("linux", "arm64", "cpu", "cpu"),
            ("macos", "arm64", "cpu", "metal"),
            ("macos", "x86_64", "cpu", "cpu"),
        ] {
            let picked =
                pick_assets_on(&assets, &[requested.into(), "vulkan".into()], os, arch).unwrap();
            assert_eq!(picked.1.as_deref(), Some(expected));
        }
        let no_hip: Vec<_> = assets
            .into_iter()
            .filter(|a| !a.name.contains("-hip"))
            .collect();
        assert_eq!(
            pick_assets_on(
                &no_hip,
                &["rocm".into(), "vulkan".into()],
                "linux",
                "x86_64"
            )
            .unwrap()
            .1
            .as_deref(),
            Some("vulkan")
        );
        assert!(!CrispAsrEngine.accepts_release(&serde_json::json!({"tag_name":"v0.8.44"})));
    }
    #[test]
    fn metadata_format_boundary_and_native_priority() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data2vec.gguf");
        model(&path, "wav2vec2", &[]);
        assert!(is_model(&path));
        assert_eq!(
            super::super::server_for_model(&path).unwrap().family(),
            CRISPASR
        );
        assert!(!super::super::is_llama_server(&path, false));
        for (arch, file) in [
            ("kokoro", "kokoro"),
            ("pocket-tts", "pocket-tts-english-q8_0"),
        ] {
            let tts = dir.path().join(format!("{file}.gguf"));
            model(&tts, arch, &[]);
            assert!(super::super::crispasr_tts::is_model(&tts));
            assert!(!is_asr_gguf(&tts));
            assert!(!super::super::is_llama_server(&tts, false));
            assert_eq!(
                crate::domain::detect_kind(crate::models::cached_metadata(&tts).as_ref()),
                crate::domain::ModelKind::Tts
            );
            assert_eq!(
                super::super::server_for_model(&tts).unwrap().family(),
                CRISPASR
            );
        }
        let unsupported = dir.path().join("qwen3tts.gguf");
        model(&unsupported, "qwen3tts", &[]);
        assert!(super::super::crispasr_tts::is_tts_gguf(&unsupported));
        assert!(super::super::server_for_model(&unsupported).is_none());
        assert!(!super::super::is_llama_server(&unsupported, false));
        let config = LaunchConfig {
            model_path: path.clone(),
            ..Default::default()
        };
        assert_eq!(super::super::server_for(&config).family(), CRISPASR);
        let cohere = dir.path().join("renamed.gguf");
        model(
            &cohere,
            "cohere",
            &[
                "encoder.pre_encode.out.weight",
                "transf_decoder._embedding.token_embedding.weight",
            ],
        );
        assert!(!is_model(&cohere));
        let handy_cohere = dir.path().join("handy-cohere.gguf");
        model(
            &handy_cohere,
            "cohere_asr",
            &["enc.pre_encode.conv.0.weight"],
        );
        assert!(!is_model(&handy_cohere));
        assert!(is_asr_gguf(&handy_cohere));
        let canonical = dir.path().join("cohere.gguf");
        model(
            &canonical,
            "cohere",
            &["enc.pre.out.weight", "dec.emb.weight"],
        );
        assert!(is_model(&canonical));
        let canary = dir.path().join("handy-computer-canary-180m-flash.gguf");
        model(&canary, "canary", &[]);
        assert!(is_model(&canary));
        for repo in [
            "handy-computer/cohere-transcribe-03-2026-gguf",
            "cstr/Qwen3-TTS-GGUF",
            "nvidia/parakeet-tdt",
        ] {
            assert!(!supports_repo(repo, Some("text-to-speech")));
        }
        assert!(supports_repo(
            "cstr/data2vec-audio-960h-GGUF",
            Some("automatic-speech-recognition")
        ));
        assert!(supports_repo("handy-computer/canary-180m-flash-gguf", None));
        let whisper = LaunchConfig {
            model_path: "ggml-base.bin".into(),
            ..Default::default()
        };
        assert_eq!(
            super::super::server_for(&whisper).family(),
            super::super::WHISPER_CPP
        );
        let onnx = LaunchConfig {
            model_path: "model.onnx".into(),
            ..Default::default()
        };
        assert_eq!(
            super::super::server_for(&onnx).family(),
            super::super::ONNX_FAMILY
        );
        assert_eq!(
            super::super::engine(super::super::SHERPA_ONNX)
                .unwrap()
                .kind(),
            RuntimeKind::Job
        );
    }
    #[test]
    fn translation_is_decided_by_architecture() {
        for arch in ["canary", "cohere", "voxtral", "granitespeech"] {
            assert!(arch_can_translate(arch), "{arch}");
        }
        for arch in [
            "wav2vec2",
            "hubert",
            "data2vec",
            "parakeet",
            "moonshine",
            "sensevoice",
            "gigaam",
            "qwen3asr",
            "unknown",
        ] {
            assert!(!arch_can_translate(arch), "{arch}");
        }
        let dir = tempfile::tempdir().unwrap();
        let canary = dir.path().join("canary.gguf");
        model(&canary, "canary", &[]);
        assert!(can_translate(&canary));
        let ctc = dir.path().join("ctc.gguf");
        model(&ctc, "wav2vec2", &[]);
        assert!(!can_translate(&ctc));
    }
    #[test]
    fn private_launch_has_only_managed_arguments_and_env_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data2vec.gguf");
        model(&path, "wav2vec2", &[]);
        let mut config = LaunchConfig {
            model_path: path,
            host: "127.0.0.1".into(),
            port: 32123,
            api_key: Some("secret".into()),
            ..Default::default()
        };
        let args = CrispAsrEngine
            .build_launch(&config, &ServerCaps::default(), None)
            .unwrap()
            .args;
        assert_eq!(
            &args[..5],
            ["--server", "--host", "127.0.0.1", "--port", "32123"]
        );
        let cache = super::super::crispasr_helpers::cache_dir();
        assert_eq!(args[7], "--cache-dir");
        assert_eq!(args[8], cache.to_string_lossy());
        assert!(cache.starts_with(crate::paths::data_root()));
        // No helper file in a fresh data folder: both features are off, nothing is fetched.
        assert!(args.windows(2).any(|w| w == ["--lid-backend", "off"]));
        assert!(args.contains(&"--no-punctuation".to_string()));
        assert!(!args
            .iter()
            .any(|a| a == "--lid-model" || a == "--punc-model"));
        assert!(!args.iter().any(|a| a.contains("secret")));
        let env = CrispAsrEngine.worker_env("secret").unwrap();
        assert_eq!(env[0], ("CRISPASR_API_KEYS", "secret".into()));
        assert!(env.iter().any(|e| e.0 == "CRISPASR_CACHE_DIR"));
        assert!(env
            .iter()
            .any(|e| e.0 == "HTTPS_PROXY" && e.1 == "http://127.0.0.1:9"));
        for flag in [
            "--api-keys secret",
            "--cors-origin *",
            "--load other.gguf",
            "--chat-model x",
            "--ws-port 1",
            "--wyoming-port 1",
            "--voice-dir x",
            "--auto-download",
            "-hfr cstr/model",
            "--hf-repo cstr/model",
        ] {
            config.custom_args = Some(flag.into());
            assert!(CrispAsrEngine
                .build_launch(&config, &ServerCaps::default(), None)
                .is_err());
        }
        config.custom_args = None;
        config.host = "0.0.0.0".into();
        assert!(CrispAsrEngine
            .build_launch(&config, &ServerCaps::default(), None)
            .is_err());
    }

    /// Real binary, real model, no network side effects. Run with:
    /// `CRISPASR_TEST_BIN=<crispasr[.exe]> CRISPASR_TEST_MODEL=<model.gguf> cargo test -p
    /// harness-core real_worker_writes_nothing_outside -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn real_worker_writes_nothing_outside_the_data_folder() {
        use std::process::{Command, Stdio};
        let (Some(bin), Some(model)) = (
            std::env::var_os("CRISPASR_TEST_BIN"),
            std::env::var_os("CRISPASR_TEST_MODEL"),
        ) else {
            panic!("set CRISPASR_TEST_BIN and CRISPASR_TEST_MODEL");
        };
        let home = tempfile::tempdir().unwrap();
        let cache = super::super::crispasr_helpers::cache_dir();
        let _ = std::fs::remove_dir_all(&cache);
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let config = LaunchConfig {
            model_path: model.into(),
            host: "127.0.0.1".into(),
            port,
            ..Default::default()
        };
        let args = CrispAsrEngine
            .build_launch(&config, &ServerCaps::default(), None)
            .unwrap()
            .args;
        let key = "real-test-key";
        let mut cmd = Command::new(&bin);
        cmd.args(&args)
            .envs(CrispAsrEngine.worker_env(key).unwrap())
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .env("XDG_CACHE_HOME", home.path().join(".cache"))
            .env("LOCALAPPDATA", home.path())
            .env("APPDATA", home.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().unwrap();
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let base = format!("http://127.0.0.1:{port}");
        let mut up = false;
        for _ in 0..300 {
            if http.get(format!("{base}/health")).send().await.is_ok() {
                up = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        assert!(up, "the worker never listened");
        let mut wav = b"RIFF".to_vec();
        let samples = 16_000u32;
        wav.extend((36 + samples * 2).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend([1, 0, 1, 0]);
        wav.extend(16_000u32.to_le_bytes());
        wav.extend(32_000u32.to_le_bytes());
        wav.extend([2, 0, 16, 0]);
        wav.extend(b"data");
        wav.extend((samples * 2).to_le_bytes());
        wav.extend(vec![0u8; (samples * 2) as usize]);
        let (ct, mut body) = crate::asr::onnx::multipart(&wav, "json", "x");
        let boundary = ct.split("boundary=").nth(1).unwrap().to_string();
        let close = format!("--{boundary}--\r\n");
        body.truncate(body.len() - close.len());
        body.extend(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"language\"\r\n\r\nen\r\n--{boundary}--\r\n"
            )
            .into_bytes(),
        );
        let res = http
            .post(format!("{base}/v1/audio/transcriptions"))
            .bearer_auth(key)
            .header("content-type", ct)
            .body(body)
            .send()
            .await
            .unwrap();
        let status = res.status();
        let text = res.text().await.unwrap();
        let _ = child.kill();
        let _ = child.wait();
        assert!(status.is_success(), "{status}: {text}");
        fn files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    files(&p, out);
                } else {
                    out.push(p);
                }
            }
        }
        let (mut in_cache, mut in_home) = (Vec::new(), Vec::new());
        files(&cache, &mut in_cache);
        files(home.path(), &mut in_home);
        // GPU drivers keep their own shader caches under the cache folder of the user.
        in_home.retain(|p| {
            !p.components().any(|c| {
                matches!(
                    c.as_os_str().to_str(),
                    Some("VkCache" | "GLCache" | "DxCache" | "mesa_shader_cache")
                )
            })
        });
        assert!(in_cache.is_empty(), "cache: {in_cache:?}");
        assert!(in_home.is_empty(), "home: {in_home:?}");
    }
}
