//! audio.cpp's resident TTS server. Memory is estimated as weights × 1.5 for
//! activations and compute buffers; no llama KV cache is involved.
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use super::{Engine, MetricsSource, ModelFormat, ReleaseSource, RuntimeKind, ServerEngine};
use crate::capabilities::RuntimeCapabilities;
use crate::gguf::GgufMetadata;
use crate::params::{ParamCatalog, RenderContext};
use crate::process::{BuildOutput, LaunchConfig, ServerCaps};
use crate::runtime::ReleaseAsset;
use crate::telemetry::Metrics;
use crate::vram::{
    EstimateSource, FitCategory, FitInput, FitResult, Measurement, MemoryBudget, MemoryEstimate,
};

pub const AUDIO_CPP: &str = "audio.cpp";
pub const IDLE_SECONDS: u64 = 300;

pub fn require_enabled() -> Result<(), String> {
    if crate::config::AppSettings::load().audio_cpp_enabled {
        Ok(())
    } else {
        Err(
            "audio.cpp is experimental and disabled. Enable it in Settings > Installed Runtime > Other Runtimes before use."
                .into(),
        )
    }
}

/// Ask the OS for an ephemeral loopback port, never a Harness service port.
pub fn random_port() -> Result<u16, String> {
    loop {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        if ![11435, 11440, 11500].contains(&port) {
            return Ok(port);
        }
    }
}
pub const TTS_FAMILIES: &[&str] = &[
    "pocket_tts",
    "kokoro_tts",
    "qwen3_tts",
    "chatterbox",
    "chatterbox_turbo",
    "omnivoice",
    "vieneu_v3_turbo",
    "f5_tts",
    "voxcpm2",
    "voxcpm1",
    "vibevoice",
    "sanotts",
    "higgs_audio_tts",
    "cosyvoice3",
    "auk",
    "moss_tts_nano",
    "moss_tts_local",
    "moss_tts_v15",
    "moss_ttsd",
    "moss_voicegen",
    "miotts",
    "echo_tts",
    "soprano_tts",
    "fish_audio",
    "sopro_tts",
    "index_tts2",
    "zipvoice",
    "audio8_tts",
    "supertonic",
    "kitten_tts",
    "kitten_tts2",
    "breeze_tts",
    "magpie_tts",
    "maya1",
    "neutts",
    "kugelaudio",
    "irodori_tts",
    "confucius4_tts",
    "dots_tts",
    "dramabox",
    "fireredtts3",
    "voxtral_tts",
    "outetts",
    "piper_tts",
    "inflect_v2",
    "glm_tts",
    "firered_audio",
    "minimax_h3",
    "lfm2_audio",
    "mira_tts",
];

/// Shared phonemizer consumers audited in audio.cpp v0.9.1 sources.
pub fn needs_espeak(family: &str) -> bool {
    matches!(
        family,
        "piper_tts" | "kitten_tts" | "sanotts" | "inflect_v2" | "zipvoice" | "kokoro_tts"
    )
}

pub fn espeak_unavailable(family: &str) -> bool {
    cfg!(windows)
        && needs_espeak(family)
        && !crate::runtimes::RuntimeRegistry::global()
            .entries()
            .unwrap_or_default()
            .iter()
            .filter(|r| r.family == AUDIO_CPP)
            .any(|r| {
                r.dir.join("espeak-ng-data.bin").is_file()
                    || r.dir.join("espeak-ng-data.gguf").is_file()
                    || ((r.dir.join("libespeak-ng.dll").is_file()
                        || r.dir.join("espeak-ng.dll").is_file())
                        && r.dir.join("espeak-ng-data").is_dir())
            })
}

pub fn task(family: &str) -> Option<&'static str> {
    if TTS_FAMILIES.contains(&family) {
        Some("tts")
    } else {
        processing_task(family)
    }
}

/// Only self-contained GGUF packages audited against v0.9.1 model specs.
pub const PROCESS_FAMILIES: &[&str] = &[
    "bs_roformer",
    "mel_band_roformer",
    "sam_audio",
    "sidon",
    "apollo",
    "audiosr",
    "universr",
];

pub fn processing_task(family: &str) -> Option<&'static str> {
    match family {
        "bs_roformer" | "mel_band_roformer" => Some("sep"),
        "sam_audio" | "sidon" | "apollo" | "audiosr" | "universr" => Some("s2s"),
        _ => None,
    }
}

pub fn is_processing(path: &Path) -> bool {
    metadata(path)
        .and_then(|m| m.audiocpp_family)
        .is_some_and(|f| processing_task(&f).is_some())
}

/// v0.9.1 model specs have no runtime-style capability field. PocketTTS's
/// framework session rejects speaking_rate, even when it is explicitly 1.0.
pub fn supports_speed(family: &str) -> bool {
    family != "pocket_tts"
}

/// Package folders use display names (PocketTTS-GGUF, Breeze-TTS-GGUF).
/// Match complete normalized names only: unknown/non-TTS families stay blocked.
pub fn family_from_package_path(path: &str) -> Option<&'static str> {
    // Display-name/version aliases in audio-cpp/audio.cpp-gguf. A boundary
    // avoids matching similarly named ASR or codec packages as TTS models.
    const PACKAGES: &[(&str, &str)] = &[
        ("bs-roformer", "bs_roformer"),
        ("mel-band-roformer", "mel_band_roformer"),
        ("sam-audio", "sam_audio"),
        ("sidon", "sidon"),
        ("apollo", "apollo"),
        ("audiosr", "audiosr"),
        ("universr", "universr"),
        ("breeze-tts", "breeze_tts"),
        ("dottts", "dots_tts"),
        ("fireredtts3", "fireredtts3"),
        ("fish-audio", "fish_audio"),
        ("higgs-audio-v3-tts", "higgs_audio_tts"),
        ("indextts2.5", "index_tts2"),
        ("inflect-micro-v2", "inflect_v2"),
        ("irodori-tts", "irodori_tts"),
        ("kokoro-82m", "kokoro_tts"),
        ("kugelaudio", "kugelaudio"),
        ("moss-tts-local", "moss_tts_local"),
        ("moss-tts-nano", "moss_tts_nano"),
        ("moss-voicegenerator", "moss_voicegen"),
        ("magpietts", "magpie_tts"),
        ("minimax-h3", "minimax_h3"),
        ("miotts", "miotts"),
        ("neutts", "neutts"),
        ("qwen3-tts", "qwen3_tts"),
        ("supertonic", "supertonic"),
        ("vibevoice-1.5b", "vibevoice"),
        ("voxtral-4b-tts", "voxtral_tts"),
    ];
    let folder = path.split('/').next()?.to_ascii_lowercase();
    let folder = folder.trim_end_matches("-gguf");
    if let Some((_, family)) = PACKAGES.iter().find(|(name, _)| {
        folder == *name
            || folder
                .strip_prefix(name)
                .is_some_and(|tail| tail.starts_with('-'))
    }) {
        return Some(family);
    }
    fn normalized(name: &str) -> String {
        name.to_ascii_lowercase()
            .replace("gguf", "")
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect()
    }
    let folder = normalized(folder);
    TTS_FAMILIES.iter().copied().find(|family| {
        let name = normalized(family);
        folder == name || folder == name.trim_end_matches("tts")
    })
}

/// Static audio.cpp builds have no ggml backend DLL to inspect.
pub fn backend_from_archive(name: &str) -> Option<String> {
    let name = name.to_ascii_lowercase();
    ["vulkan", "cuda", "metal", "cpu"]
        .into_iter()
        .find(|backend| {
            name.split('-').any(|part| {
                part == *backend
                    || part.strip_prefix(backend).is_some_and(|tail| {
                        tail.starts_with('.')
                            || tail.chars().next().is_some_and(|c| c.is_ascii_digit())
                    })
            })
        })
        .map(str::to_string)
}

/// Metadata, rather than a filename, is the authority for the engine family.
pub fn metadata(path: &Path) -> Option<GgufMetadata> {
    let file = if path.is_dir() {
        let mut files: Vec<_> = std::fs::read_dir(path)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| ModelFormat::of(p) == ModelFormat::Gguf)
            .collect();
        files.sort();
        files.into_iter().find(|p| metadata(p).is_some())?
    } else {
        path.to_path_buf()
    };
    crate::models::cached_metadata(&file).filter(|m| m.architecture.as_deref() == Some("audiocpp"))
}

pub fn is_model(path: &Path) -> bool {
    metadata(path).is_some()
}
pub fn is_tts(path: &Path) -> bool {
    metadata(path)
        .and_then(|m| m.audiocpp_family)
        .is_some_and(|f| task(&f) == Some("tts"))
}

pub fn model_dir(path: &Path) -> &Path {
    if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    }
}

/// PocketTTS stores one embedding per speaker alongside its GGUF.
pub fn voices(path: &Path) -> Vec<String> {
    let mut voices: Vec<_> = std::fs::read_dir(model_dir(path).join("embeddings"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let p = e.path();
            (p.extension()?.to_str()? == "safetensors")
                .then(|| p.file_stem()?.to_str().map(String::from))
                .flatten()
        })
        .collect();
    voices.sort();
    voices.dedup();
    voices
}

pub fn language(path: &Path) -> Option<String> {
    const LANGUAGES: &[&str] = &[
        "english",
        "french",
        "german",
        "italian",
        "portuguese",
        "spanish",
    ];
    model_dir(path)
        .components()
        .rev()
        .filter_map(|c| c.as_os_str().to_str())
        .map(str::to_ascii_lowercase)
        .find(|s| LANGUAGES.contains(&s.as_str()))
}

/// `auto` is resolved by Harness; audio.cpp's server requires a concrete backend.
pub fn backend(config: &LaunchConfig) -> String {
    backend_in(config, &crate::runtimes::RuntimeRegistry::global())
}

fn backend_in(config: &LaunchConfig, registry: &crate::runtimes::RuntimeRegistry) -> String {
    if let Some(crate::params::ParamValue::Text(value)) = config.params.get("backend") {
        if value != "auto" && !value.is_empty() {
            return value.clone();
        }
    }
    if config.gpu_layers == 0 {
        return "cpu".into();
    }
    let binary = if config.binary_path.as_os_str().is_empty() {
        crate::runtimes::resolve_family_binary_in(registry, AUDIO_CPP, config.runtime.as_deref())
            .ok()
    } else {
        Some(config.binary_path.clone())
    };
    binary
        .as_deref()
        .and_then(|binary| {
            registry
                .entries()
                .ok()?
                .into_iter()
                .find(|entry| entry.binary() == binary)
                .and_then(|entry| entry.effective_backend())
        })
        .or_else(|| {
            binary
                .as_deref()
                .and_then(Path::parent)
                .and_then(crate::runtime::detect_backend_in_dir)
        })
        .unwrap_or_else(|| "cpu".into())
}

fn has_companions(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| {
            let p = e.path();
            p.is_dir()
                || p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                    matches!(
                        e,
                        "safetensors" | "bin" | "json" | "npy" | "npz" | "onnx" | "txt"
                    )
                })
        })
}

pub fn weights_bytes(path: &Path) -> u64 {
    if path.is_file() {
        return std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| !e.file_type().is_ok_and(|t| t.is_symlink()))
        .map(|e| weights_bytes(&e.path()))
        .sum()
}

pub fn server_config(config: &LaunchConfig) -> Result<serde_json::Value, String> {
    if config.host != "127.0.0.1" {
        return Err(crate::error_key::err("audio_cpp.loopback_only").into());
    }
    let meta = metadata(&config.model_path)
        .ok_or_else(|| String::from(crate::error_key::err("audio_cpp.model_unsupported")))?;
    let family = meta.audiocpp_family.unwrap_or_default();
    if task(&family).is_none() {
        return Err(crate::error_key::err("audio_cpp.family_unsupported")
            .with("family", family)
            .into());
    }
    // v0.9.1's CPU backend aborts on AudioSR (ggml-cpu GGML_ASSERT nb10); Vulkan works.
    if family == "audiosr" && backend(config) == "cpu" {
        return Err(crate::error_key::err("audio_cpp.audiosr_needs_gpu").into());
    }
    let dir = model_dir(&config.model_path);
    let path = if has_companions(dir) {
        dir
    } else {
        &config.model_path
    };
    let voice = voices(path).into_iter().next();
    let mut model = serde_json::json!({
        "id": crate::process::served_alias(config).unwrap_or_else(|| "audio-cpp".into()),
        "family": family, "path": path, "task": task(&family), "mode": "offline"
    });
    if family == "pocket_tts" {
        if let Some(language) = language(path) {
            model["load_options"] = serde_json::json!({"language": language});
        }
    }
    if let Some(voice) = voice {
        model["default_voice_preset"] = serde_json::json!({"voice_id": voice});
    }
    Ok(
        serde_json::json!({"host": "127.0.0.1", "port": config.port, "backend": backend(config), "device": 0, "lazy_load": true, "ui": false, "max_loaded_models": 1, "models": [model]}),
    )
}

pub fn write_config(config: &LaunchConfig, dir: &Path) -> Result<PathBuf, String> {
    let path = dir.join("server.json");
    let body = serde_json::to_vec_pretty(&server_config(config)?).map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| {
        String::from(
            crate::error_key::err("fs.write_failed")
                .with("path", path.display())
                .with("error", e),
        )
    })?;
    Ok(path)
}

pub fn catalog() -> &'static ParamCatalog {
    static CATALOG: OnceLock<ParamCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        toml::from_str(include_str!("../params_audio_cpp.toml")).expect("valid audio.cpp catalog")
    })
}

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

#[derive(Debug, Clone, Copy, Default)]
pub struct AudioCppEngine;

/// Explicit platform arguments keep release selection testable on every host.
pub fn pick_assets(
    assets: &[ReleaseAsset],
    backends: &[String],
    os: &str,
    arch: &str,
) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
    let os = match os {
        "linux" => "ubuntu",
        "macos" => "macos",
        "windows" => "windows",
        _ => return None,
    };
    let arch = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => return None,
    };
    let choices = if os == "macos" {
        vec!["metal".to_string()]
    } else if os == "ubuntu" {
        if backends.iter().any(|b| b != "cpu") {
            vec!["vulkan".into(), "cpu".into()]
        } else {
            vec!["cpu".into()]
        }
    } else {
        let mut v = backends.to_vec();
        if v.iter().any(|b| b == "rocm" || b == "hip" || b == "sycl")
            && !v.iter().any(|b| b == "vulkan")
        {
            v.insert(0, "vulkan".into());
        }
        if !v.iter().any(|b| b == "cpu") {
            v.push("cpu".into());
        }
        v
    };
    for backend in choices {
        let token = format!("-bin-{os}-{arch}-{backend}");
        let mut hits: Vec<_> = assets
            .iter()
            .filter(|a| {
                a.name.contains(&token)
                    && !a.name.contains("cudart")
                    && (a.name.ends_with(".zip") || a.name.ends_with(".tar.gz"))
            })
            .collect();
        // Prefer standard archives to portable builds, then the lowest CUDA
        // version (12.4 before 13.3, for older NVIDIA drivers).
        hits.sort_by(|a, b| {
            a.name
                .contains("portable")
                .cmp(&b.name.contains("portable"))
                .then_with(|| a.name.cmp(&b.name))
        });
        if let Some(main) = hits.first() {
            let mut out = vec![(*main).clone()];
            if backend == "cuda" && os == "windows" {
                let suffix = main.name.split(&token).nth(1)?;
                let companion = assets.iter().find(|a| {
                    a.name.contains(&format!("-cudart-{os}-{arch}-cuda"))
                        && a.name.ends_with(suffix)
                });
                let Some(companion) = companion else { continue };
                out.push(companion.clone());
            }
            return Some((out, Some(backend)));
        }
    }
    None
}

impl Engine for AudioCppEngine {
    fn family(&self) -> &'static str {
        AUDIO_CPP
    }
    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Server
    }
    fn release_source(&self) -> ReleaseSource {
        ReleaseSource {
            repo: "0xShug0/audio.cpp",
            binary: "audiocpp_server",
        }
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
        pick_assets(
            assets,
            backends,
            std::env::consts::OS,
            std::env::consts::ARCH,
        )
    }
    fn supports(&self, format: ModelFormat) -> bool {
        format == ModelFormat::Gguf
    }
    fn version_arg(&self) -> &'static str {
        "--help"
    }
    fn detect_capabilities(&self, binary: &Path) -> Arc<RuntimeCapabilities> {
        crate::capabilities::registry()
            .get_or_probe_with(binary, crate::capabilities::audio_cpp::probe_binary)
    }
    fn cached_capabilities(&self, binary: &Path) -> Option<Arc<RuntimeCapabilities>> {
        crate::capabilities::registry().cached(binary)
    }
    fn probe_capabilities(&self, binary: &Path) -> RuntimeCapabilities {
        crate::capabilities::audio_cpp::probe_binary(binary)
    }
    fn param_catalog(&self) -> &'static ParamCatalog {
        catalog()
    }
    fn as_server(&self) -> Option<&dyn ServerEngine> {
        Some(self)
    }
}

impl ServerEngine for AudioCppEngine {
    /// audio.cpp reads local models and voices only: no network on Linux.
    fn netns_bridge(&self) -> bool {
        true
    }
    fn build_launch(
        &self,
        config: &LaunchConfig,
        _: &ServerCaps,
        _: Option<&RuntimeCapabilities>,
    ) -> Result<BuildOutput, String> {
        server_config(config)?;
        let mut params = config.params.clone();
        params.insert(
            "idle_unload_ms".into(),
            crate::params::ParamValue::Int((IDLE_SECONDS * 1000) as i64),
        );
        params.insert(
            "backend".into(),
            crate::params::ParamValue::Text(backend(config)),
        );
        let mut out = crate::params::render_params(
            catalog(),
            Some(documented_caps()),
            &params,
            &RenderContext::default(),
        )?;
        let mut args = vec![
            "--config".into(),
            "server.json".into(),
            "--no-ui".into(),
            "--max-loaded-models".into(),
            "1".into(),
        ];
        if let Some(threads) = config.threads.filter(|t| *t > 0) {
            args.extend(["--threads".into(), threads.to_string()]);
        }
        if let Some(custom) = config
            .custom_args
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            if let Some(arg) = crate::process::split_args(custom)?.first() {
                return Err(crate::error_key::err("params.protected_flag")
                    .with("flag", arg)
                    .into());
            }
        }
        args.append(&mut out.args);
        Ok(BuildOutput {
            args,
            skipped: out.skipped,
        })
    }
    fn health_path(&self) -> &'static str {
        "/health"
    }
    fn health(&self, status: u16, body: &str) -> bool {
        status == 200
            && serde_json::from_str::<serde_json::Value>(body)
                .ok()
                .is_some_and(|v| v["status"] == "ok")
    }
    fn openai_base(&self, host_url: &str) -> String {
        format!("{}/v1", host_url.trim_end_matches('/'))
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
        _: Option<&GgufMetadata>,
        budget: &MemoryBudget,
        _: &[Measurement],
    ) -> FitResult {
        let weights = if input.model_size_bytes > 0 {
            input.model_size_bytes
        } else {
            input
                .model_path
                .as_deref()
                .map(Path::new)
                .map(weights_bytes)
                .unwrap_or(0)
        };
        let total = weights.saturating_add(weights / 2);
        let gpu = input.gpu_layers > 0 && budget.vram_bytes > 0;
        let capacity = if gpu {
            budget.vram_bytes
        } else {
            budget.ram_bytes
        };
        FitResult {
            category: if total > capacity {
                FitCategory::OomRisk
            } else if total > capacity / 10 * 9 {
                FitCategory::Tight
            } else if gpu {
                FitCategory::FullGpu
            } else {
                FitCategory::CpuHeavy
            },
            vram_percentage: if gpu {
                total.saturating_mul(100).checked_div(capacity).unwrap_or(0) as u32
            } else {
                0
            },
            ram_percentage: if gpu {
                0
            } else {
                total.saturating_mul(100).checked_div(capacity).unwrap_or(0) as u32
            },
            estimate: MemoryEstimate {
                vram_bytes: if gpu { total } else { 0 },
                ram_bytes: if gpu { 0 } else { total },
                model_gpu_bytes: if gpu { weights } else { 0 },
                model_host_bytes: if gpu { 0 } else { weights },
                compute_gpu_bytes: if gpu { weights / 2 } else { 0 },
                compute_host_bytes: if gpu { 0 } else { weights / 2 },
                slots: 1,
                ..Default::default()
            },
            budget: *budget,
            source: EstimateSource::Estimate,
            measured_at: None,
            recommended_gpu_layers: if gpu { 999 } else { 0 },
            slots: None,
            kv: None,
        }
    }
}

pub fn vram_need(config: &LaunchConfig) -> u64 {
    if backend(config) == "cpu" {
        return 0;
    }
    let size = weights_bytes(&config.model_path);
    size.saturating_add(size / 2)
}

#[cfg(test)]
mod tests {
    #[test]
    fn espeak_dependencies_do_not_block_self_contained_speech_or_processing() {
        for family in [
            "piper_tts",
            "kitten_tts",
            "sanotts",
            "inflect_v2",
            "zipvoice",
            "kokoro_tts",
        ] {
            assert!(super::needs_espeak(family));
        }
        for family in [
            "pocket_tts",
            "qwen3_tts",
            "kitten_tts2",
            "sam_audio",
            "bs_roformer",
        ] {
            assert!(!super::needs_espeak(family));
            assert!(!super::espeak_unavailable(family));
        }
    }

    use super::*;
    use crate::params::ParamValue;

    #[test]
    fn speed_capability_matches_pocket_session_and_speed_models() {
        assert!(!supports_speed("pocket_tts"));
        for family in ["kokoro_tts", "piper_tts", "supertonic"] {
            assert!(supports_speed(family));
        }
    }

    #[test]
    fn static_backend_survives_registry_roundtrip_and_legacy_archives() {
        use crate::runtimes::{RuntimeEntry, RuntimeRegistry, RuntimeSource};
        let dir = tempfile::tempdir().unwrap();
        let reg =
            RuntimeRegistry::with_paths(dir.path().join("runtimes.json"), dir.path().join("bin"));
        let binary = dir
            .path()
            .join(AudioCppEngine.release_source().binary_file());
        std::fs::write(&binary, b"static test binary").unwrap();
        let mut entry: RuntimeEntry = serde_json::from_value(serde_json::json!({
            "id":"audio", "name":"audio", "family":"audio.cpp", "dir":dir.path(),
            "source":{"kind":"github", "repo":"0xShug0/audio.cpp", "tag":"v0.9.1", "assets":[{
                "name":"audio-v0.9.1-bin-windows-x64-vulkan.zip", "url":"https://example.invalid/audio.zip", "size":1
            }]}, "backend":"vulkan", "trust":"verified"
        })).unwrap();
        let mut config = LaunchConfig {
            binary_path: binary,
            gpu_layers: 999,
            ..Default::default()
        };
        for persisted in [Some("vulkan".into()), None] {
            entry.backend = persisted;
            reg.save_entry(entry.clone()).unwrap();
            assert_eq!(
                reg.get("audio").unwrap().effective_backend().as_deref(),
                Some("vulkan")
            );
            assert_eq!(backend_in(&config, &reg), "vulkan");
        }
        config.gpu_layers = 0;
        assert_eq!(backend_in(&config, &reg), "cpu");
        config
            .params
            .insert("backend".into(), ParamValue::Text("metal".into()));
        assert_eq!(backend_in(&config, &reg), "metal");
        entry.backend = None;
        entry.source = RuntimeSource::Local {
            path: dir.path().into(),
            copied: false,
        };
        assert_eq!(entry.effective_backend(), None);
        for (name, expected) in [
            ("audio-bin-windows-x64-cuda12.4.zip", Some("cuda")),
            ("audio-bin-macos-arm64-metal.tar.gz", Some("metal")),
            ("audio-bin-ubuntu-x64-cpu.tar.gz", Some("cpu")),
            ("audio-bin-windows-x64-unknown.zip", None),
        ] {
            assert_eq!(backend_from_archive(name).as_deref(), expected);
        }
    }

    #[test]
    fn usage_synopsis_flags_are_visible_and_auto_is_resolved() {
        let caps = crate::capabilities::audio_cpp::from_help(Default::default(), "", "audiocpp_server [--backend <backend>]\n [--device <id>] [--threads <n>] [--idle-unload-ms <ms>]");
        for flag in ["--backend", "--device", "--threads", "--idle-unload-ms"] {
            assert!(caps.has_flag(flag), "{flag}");
        }
        let mut config = LaunchConfig::default();
        config
            .params
            .insert("backend".into(), ParamValue::Text("auto".into()));
        assert_eq!(backend(&config), "cpu");
    }

    fn gguf(path: &Path, family: &str) {
        use std::io::Write;
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(b"GGUF").unwrap();
        f.write_all(&3u32.to_le_bytes()).unwrap();
        f.write_all(&0u64.to_le_bytes()).unwrap();
        f.write_all(&2u64.to_le_bytes()).unwrap();
        for (key, value) in [
            ("general.architecture", "audiocpp"),
            ("audiocpp.model_spec.family", family),
        ] {
            f.write_all(&(key.len() as u64).to_le_bytes()).unwrap();
            f.write_all(key.as_bytes()).unwrap();
            f.write_all(&8u32.to_le_bytes()).unwrap();
            f.write_all(&(value.len() as u64).to_le_bytes()).unwrap();
            f.write_all(value.as_bytes()).unwrap();
        }
    }

    #[test]
    fn processing_models_use_pipeline_kind_and_their_own_server_task() {
        let dir = tempfile::tempdir().unwrap();
        for family in PROCESS_FAMILIES {
            let path = dir.path().join(format!("{family}.gguf"));
            gguf(&path, family);
            assert!(is_model(&path));
            assert!(is_processing(&path));
            assert!(!is_tts(&path));
            assert_eq!(
                crate::domain::detect_kind(metadata(&path).as_ref()),
                crate::domain::ModelKind::Pipeline
            );
            let mut c = LaunchConfig {
                model_path: path,
                host: "127.0.0.1".into(),
                ..Default::default()
            };
            c.params
                .insert("backend".into(), ParamValue::Text("vulkan".into()));
            assert_eq!(
                server_config(&c).unwrap()["models"][0]["task"],
                processing_task(family).unwrap()
            );
            assert_eq!(super::super::server_for(&c).family(), AUDIO_CPP);
            c.params
                .insert("backend".into(), ParamValue::Text("cpu".into()));
            assert_eq!(
                server_config(&c)
                    .err()
                    .is_some_and(|e| e.contains("audio_cpp.audiosr_needs_gpu")),
                *family == "audiosr"
            );
        }
        for family in [
            "builtin_audio_utils",
            "mossformer2",
            "tf_gridnet",
            "speechbrain",
            "vocoder",
        ] {
            assert!(processing_task(family).is_none());
        }
    }

    #[test]
    fn audio_metadata_routes_before_gguf_and_never_to_llama() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.gguf");
        gguf(&path, "pocket_tts");
        let meta = crate::gguf::read_gguf_metadata(&path).unwrap();
        assert_eq!(meta.architecture.as_deref(), Some("audiocpp"));
        assert_eq!(meta.audiocpp_family.as_deref(), Some("pocket_tts"));
        assert_eq!(
            crate::domain::detect_kind(Some(&meta)),
            crate::domain::ModelKind::Tts
        );
        assert!(is_tts(&path));
        assert_eq!(
            super::super::server_for_model(&path).unwrap().family(),
            AUDIO_CPP
        );
        for router_mode in [false, true] {
            let c = LaunchConfig {
                model_path: path.clone(),
                router_mode,
                ..Default::default()
            };
            assert_eq!(super::super::server_for(&c).family(), AUDIO_CPP);
            assert!(!super::super::is_llama_server(&path, router_mode));
            assert!(super::super::llama_cpp()
                .build_launch(&c, &ServerCaps::default(), None)
                .unwrap_err()
                .contains("audio_cpp.llama_rejected"));
        }
        // Use a distinct fixture: same-size rewrites can share an mtime on Windows.
        let path = dir.path().join("asr.gguf");
        gguf(&path, "sensevoice");
        assert!(is_model(&path));
        assert!(!is_tts(&path));
        assert_eq!(
            crate::domain::detect_kind(metadata(&path).as_ref()),
            crate::domain::ModelKind::Unknown
        );
        assert!(server_config(&LaunchConfig {
            model_path: path,
            host: "127.0.0.1".into(),
            ..Default::default()
        })
        .unwrap_err()
        .contains("audio_cpp.family_unsupported"));
        for family in TTS_FAMILIES {
            assert_eq!(task(family), Some("tts"));
        }
        for family in ["whisper", "musicgen", "rvc", "demucs"] {
            assert_eq!(task(family), None);
        }
    }

    #[test]
    fn config_uses_alias_companions_language_and_sorted_voices() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("PocketTTS-GGUF/english");
        std::fs::create_dir_all(dir.join("embeddings")).unwrap();
        let path = dir.join("pocket-tts-english-q8_0.gguf");
        gguf(&path, "pocket_tts");
        for file in ["anna.safetensors", "alba.safetensors", "ignored.txt"] {
            std::fs::write(dir.join("embeddings").join(file), b"voice").unwrap();
        }
        assert_eq!(voices(&path), ["alba", "anna"]);
        assert_eq!(voices(&dir), ["alba", "anna"]);
        let mut c = LaunchConfig {
            model_path: path.clone(),
            alias: Some("pocket-en".into()),
            host: "127.0.0.1".into(),
            port: 18761,
            threads: Some(4),
            ..Default::default()
        };
        c.params
            .insert("backend".into(), ParamValue::Text("vulkan".into()));
        c.params.insert("device".into(), ParamValue::Int(1));
        c.params.insert("idle_unload_ms".into(), ParamValue::Int(0));
        let v = server_config(&c).unwrap();
        assert_eq!(v["host"], "127.0.0.1");
        assert_eq!(v["port"], 18761);
        assert_eq!(v["models"][0]["id"], "pocket-en");
        assert_eq!(v["models"][0]["path"], serde_json::json!(dir));
        assert_eq!(v["models"][0]["load_options"]["language"], "english");
        assert_eq!(v["models"][0]["default_voice_preset"]["voice_id"], "alba");
        let run = tempfile::tempdir().unwrap();
        let written = write_config(&c, run.path()).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(written).unwrap()).unwrap(),
            v
        );
        let args = AudioCppEngine
            .build_launch(&c, &ServerCaps::default(), None)
            .unwrap()
            .args;
        for (flag, value) in [
            ("--config", "server.json"),
            ("--backend", "vulkan"),
            ("--device", "1"),
            ("--threads", "4"),
            ("--idle-unload-ms", "300000"),
        ] {
            assert!(args.windows(2).any(|w| w == [flag, value]), "{args:?}");
        }
        assert!(args.contains(&"--no-ui".into()));
        assert!(!args.contains(&"--ui-management".into()));
        assert!(!args.contains(&"--host".into()));
        std::fs::remove_dir_all(dir.join("embeddings")).unwrap();
        assert_eq!(
            server_config(&c).unwrap()["models"][0]["path"],
            serde_json::json!(path)
        );
        for host in ["0.0.0.0", "::1", "localhost"] {
            c.host = host.into();
            assert!(server_config(&c)
                .unwrap_err()
                .contains("audio_cpp.loopback_only"));
        }
    }

    #[test]
    fn assets_select_platform_backend_and_matching_cuda_runtime() {
        let assets: Vec<ReleaseAsset> = [
            "audio-v0.9.1-bin-windows-x64-vulkan.zip", "audio-v0.9.1-bin-windows-x64-vulkan-portable.zip", "audio-v0.9.1-bin-windows-x64-cpu.zip",
            "audio-v0.9.1-bin-windows-x64-cuda12.4.zip", "audio-v0.9.1-bin-windows-x64-cuda13.3.zip",
            "audio-v0.9.1-cudart-windows-x64-cuda12.4.zip", "audio-v0.9.1-cudart-windows-x64-cuda13.3.zip",
            "audio-v0.9.1-bin-ubuntu-x64-vulkan.tar.gz", "audio-v0.9.1-bin-ubuntu-x64-cpu.tar.gz",
            "audio-v0.9.1-bin-macos-arm64-metal.tar.gz", "audio-v0.9.1-bin-macos-x64-metal.tar.gz",
        ].iter().map(|n| serde_json::from_value(serde_json::json!({"name":n,"download_url":"https://example.test/archive","size":1})).unwrap()).collect();
        for backend in ["vulkan", "rocm", "sycl"] {
            let (a, b) = pick_assets(
                &assets,
                &[backend.into(), "cpu".into()],
                "windows",
                "x86_64",
            )
            .unwrap();
            assert_eq!(b.as_deref(), Some("vulkan"));
            assert_eq!(a[0].name, "audio-v0.9.1-bin-windows-x64-vulkan.zip");
        }
        let (a, b) = pick_assets(&assets, &["cuda".into()], "windows", "x86_64").unwrap();
        assert_eq!(b.as_deref(), Some("cuda"));
        assert_eq!(a.len(), 2);
        assert!(
            a[0].name.ends_with("cuda12.4.zip")
                && a[1].name.contains("cudart-windows-x64-cuda12.4")
        );
        for arch in ["x86_64", "aarch64"] {
            assert_eq!(
                pick_assets(&assets, &[], "macos", arch)
                    .unwrap()
                    .1
                    .as_deref(),
                Some("metal")
            );
        }
        assert_eq!(
            pick_assets(&assets, &["vulkan".into()], "linux", "x86_64")
                .unwrap()
                .1
                .as_deref(),
            Some("vulkan")
        );
        assert_eq!(
            pick_assets(&assets, &["cpu".into()], "windows", "x86_64")
                .unwrap()
                .1
                .as_deref(),
            Some("cpu")
        );
        assert!(pick_assets(&assets, &["cpu".into()], "windows", "aarch64").is_none());
    }

    #[test]
    fn estimate_has_compute_margin_and_no_kv_cache() {
        let input = FitInput {
            model_size_bytes: 1000,
            gpu_layers: 999,
            ..Default::default()
        };
        let budget = MemoryBudget {
            vram_bytes: 2000,
            ram_bytes: 4000,
            ..Default::default()
        };
        let fit = AudioCppEngine.estimate_memory(&input, None, &budget, &[]);
        assert_eq!(fit.estimate.vram_bytes, 1500);
        assert!(fit.kv.is_none());
        let cpu = AudioCppEngine.estimate_memory(
            &FitInput {
                gpu_layers: 0,
                ..input
            },
            None,
            &budget,
            &[],
        );
        assert_eq!(cpu.estimate.ram_bytes, 1500);
        assert_eq!(cpu.estimate.vram_bytes, 0);
    }
}
