//! whisper.cpp as an [`Engine`]: `whisper-server`, a long-lived process
//! that loads one ggml model (`ggml-*.bin`) and transcribes the audio files POSTed to
//! `/inference`.
//!
//! What the official releases (`ggml-org/whisper.cpp`) really ship (checked 2026-10-02 on
//! `b5130`, `v1.9.2`, `v1.8.4`): Windows x64 CPU (`whisper-bin-x64.zip`), OpenBLAS
//! (`whisper-blas-bin-x64.zip`) and CUDA 11.8 / 12.4 (`whisper-cublas-<v>-bin-x64.zip`, from
//! `b5454` on `whisper-bin-win-cuda-<v>-x64.zip`). Only the 12.4 archive is self-contained
//! (`ggml-cuda.dll` plus cudart / cuBLAS); the 11.8 one lacks `cublas64_11.dll` and runs on the
//! CPU without saying so, so the picker takes CUDA 12 first; Windows arm64 CPU / CUDA /
//! OpenCL-Adreno; Linux x64 / arm64 (`whisper-bin-ubuntu-<arch>.tar.gz`, CPU). There is
//! **no Vulkan build and no ROCm build** on any OS, and macOS has only an XCFramework (no
//! `whisper-server`). A machine without an NVIDIA GPU therefore runs the CPU build; the
//! picker falls through `vulkan` / `rocm` to `cpu` and the install preview says which build
//! it chose. Some releases (`v1.9.4`) have no assets at all and are skipped.
//!
//! `whisper-server` is not OpenAI-compatible (`POST /inference`, multipart `file`), has no
//! authentication and no `/metrics`; Harness binds it to loopback only and puts the OpenAI
//! routes (`/v1/audio/transcriptions`) in the gateway.

use std::path::Path;
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

/// Family id of whisper.cpp builds.
pub const WHISPER_CPP: &str = "whisper.cpp";

/// Flags a custom argument line (or a catalog value) may never set: the ones Harness's own
/// fields and the gateway depend on. `--host` / `--port` are the generic protected ones.
const PROTECTED_FLAGS: &[&str] = &[
    "-m",
    "--model",
    "--host",
    "--port",
    "--inference-path",
    "--request-path",
    "--public",
    "--tmp-dir",
];

/// whisper-server builds from the official releases (or a local folder).
#[derive(Debug, Clone, Copy, Default)]
pub struct WhisperCppEngine;

/// The parsed catalog (`params_whisper.toml`).
pub fn catalog() -> &'static ParamCatalog {
    static CATALOG: OnceLock<ParamCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        toml::from_str(include_str!("../params_whisper.toml"))
            .expect("params_whisper.toml is valid (unit tested)")
    })
}

impl Engine for WhisperCppEngine {
    fn family(&self) -> &'static str {
        WHISPER_CPP
    }

    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Server
    }

    fn release_source(&self) -> ReleaseSource {
        ReleaseSource {
            repo: "ggml-org/whisper.cpp",
            binary: "whisper-server",
        }
    }

    /// whisper.cpp has no managed "official" runtime: its builds are registered runtimes
    /// (`runtimes.json`) installed from a release.
    fn release_asset(
        &self,
        _release: &serde_json::Value,
        _query: &crate::hardware::AssetQuery,
    ) -> Option<crate::runtime::ReleaseAssetInfo> {
        None
    }

    fn pick_release_assets(
        &self,
        assets: &[ReleaseAsset],
        backends: &[String],
    ) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
        if cfg!(windows) && backends.iter().any(|b| b == "cuda") {
            let hw = crate::hardware::detect_hardware();
            return pick_assets_for_driver(
                assets,
                backends,
                hw.driver_info.cuda_driver_version.as_deref(),
            );
        }
        pick_assets(assets, backends)
    }

    fn supports(&self, format: ModelFormat) -> bool {
        format == ModelFormat::GgmlBin
    }

    fn version_arg(&self) -> &'static str {
        // `--version` is an unknown argument of whisper-server.
        "--help"
    }

    fn detect_capabilities(&self, binary: &Path) -> Arc<RuntimeCapabilities> {
        crate::capabilities::registry()
            .get_or_probe_with(binary, crate::capabilities::whisper::probe_binary)
    }

    fn cached_capabilities(&self, binary: &Path) -> Option<Arc<RuntimeCapabilities>> {
        crate::capabilities::registry().cached(binary)
    }

    fn probe_capabilities(&self, binary: &Path) -> RuntimeCapabilities {
        crate::capabilities::whisper::probe_binary(binary)
    }

    fn param_catalog(&self) -> &'static ParamCatalog {
        catalog()
    }

    fn as_server(&self) -> Option<&dyn ServerEngine> {
        Some(self)
    }
}

impl ServerEngine for WhisperCppEngine {
    fn build_launch(
        &self,
        config: &LaunchConfig,
        _caps: &ServerCaps,
        runtime: Option<&RuntimeCapabilities>,
    ) -> Result<BuildOutput, String> {
        // No authentication of its own: only loopback, the gateway is the front door.
        if !crate::network::is_loopback_host(&config.host) {
            return Err(err("process.whisper_loopback_only")
                .with("host", &config.host)
                .into());
        }
        let mut args: Vec<String> = vec![
            "-m".into(),
            config.model_path.to_string_lossy().to_string(),
            "--host".into(),
            config.host.clone(),
            "--port".into(),
            config.port.to_string(),
        ];
        if let Some(threads) = config.threads.filter(|&n| n > 0) {
            args.push("-t".into());
            args.push(threads.to_string());
        }
        if config.gpu_layers == 0 {
            args.push("--no-gpu".into());
        }
        // whisper-server's own default language is English; a transcription service detects
        // it (a request's `language` field still wins).
        if !config.params.contains_key("language") {
            args.push("--language".into());
            args.push("auto".into());
        }
        let ctx = RenderContext {
            host: config.host.clone(),
            ..Default::default()
        };
        let rendered = crate::params::render_params(catalog(), runtime, &config.params, &ctx)?;
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

    fn health_path(&self) -> &'static str {
        "/health"
    }

    /// `{"status":"ok"}` once the model is loaded (the server only starts listening after
    /// that; newer builds answer 503 while loading).
    fn health(&self, status: u16, body: &str) -> bool {
        status == 200
            && serde_json::from_str::<serde_json::Value>(body)
                .ok()
                .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(|s| s == "ok"))
                .unwrap_or(false)
    }

    /// whisper-server has no OpenAI surface; the base is the server itself (`/inference`).
    /// The OpenAI routes live in the gateway.
    fn openai_base(&self, host_url: &str) -> String {
        host_url.trim_end_matches('/').to_string()
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
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|m| m.len())
                .unwrap_or(0)
        };
        let gpu = budget.vram_bytes > 0 && input.gpu_layers > 0;
        let (weights, mut compute) = (model, compute_bytes(model));
        if gpu {
            compute += CUDA_CONTEXT;
        }
        let total = weights + compute;
        let estimate = MemoryEstimate {
            vram_bytes: if gpu { total } else { 0 },
            ram_bytes: if gpu { 0 } else { total },
            model_gpu_bytes: if gpu { weights } else { 0 },
            model_host_bytes: if gpu { 0 } else { weights },
            compute_gpu_bytes: if gpu { compute } else { 0 },
            compute_host_bytes: if gpu { 0 } else { compute },
            slots: 1,
            exact: false,
            ..Default::default()
        };
        let category = match (gpu, total) {
            (true, t) if t * 10 <= budget.vram_bytes * 9 => FitCategory::FullGpu,
            (true, t) if t <= budget.vram_bytes => FitCategory::Tight,
            (false, t) if t * 10 <= budget.ram_bytes * 9 => FitCategory::CpuHeavy,
            _ => FitCategory::OomRisk,
        };
        let pct = |n: u64, d: u64| (n * 100).checked_div(d).unwrap_or(0) as u32;
        FitResult {
            category,
            vram_percentage: pct(estimate.vram_bytes, budget.vram_bytes),
            ram_percentage: pct(estimate.ram_bytes, budget.ram_bytes),
            estimate,
            budget: *budget,
            source: EstimateSource::Estimate,
            measured_at: None,
            // `--no-gpu` or not: the whole model is one unit.
            recommended_gpu_layers: if gpu { 999 } else { 0 },
            slots: None,
            kv: None,
        }
    }
}

/// Compute buffers on top of the weights: a floor for the small models (the conv / encoder /
/// cross / decoder buffers of `tiny.en` are 131 MiB) and a share of the weights for the large
/// ones. An estimate, never claimed exact.
fn compute_bytes(model: u64) -> u64 {
    const MIB: u64 = 1024 * 1024;
    (192 * MIB).max(model / 4)
}

/// Device memory the CUDA context of a whisper server takes on top of the weights and the
/// compute buffers. Measured on an RTX 3050 Ti (b5454, CUDA 12.4; per-process dedicated memory):
/// base.en 411, small.en 805, medium.en 1944, large-v3-turbo-q5_0 892 MiB, against 78-154 MiB
/// more than `weights + compute_bytes`.
const CUDA_CONTEXT: u64 = 128 * 1024 * 1024;

/// Device memory a loaded whisper model takes on a GPU build (host memory on a CPU build is
/// the model plus `compute_bytes`, see `estimate_memory`).
pub fn footprint_bytes(model_size_bytes: u64) -> u64 {
    model_size_bytes + compute_bytes(model_size_bytes) + CUDA_CONTEXT
}

/// Device memory a whisper launch takes: the model plus its compute buffers when the chosen
/// build has a GPU backend (CUDA...) and the launch uses it, else none (a CPU build keeps
/// everything in system memory, which is not VRAM). The binary is the one the launch runs
/// (`config.binary_path`, else the runtime the launch resolves to).
pub fn vram_need(config: &LaunchConfig) -> u64 {
    let binary = if config.binary_path.as_os_str().is_empty() {
        crate::runtimes::resolve_family_binary_in(
            &crate::runtimes::RuntimeRegistry::global(),
            WHISPER_CPP,
            config.runtime.as_deref(),
        )
        .ok()
    } else {
        Some(config.binary_path.clone())
    };
    let gpu_build = binary.as_deref().and_then(build_has_gpu).unwrap_or(false);
    if !gpu_build || config.gpu_layers == 0 {
        return 0;
    }
    let size = std::fs::metadata(&config.model_path)
        .map(|m| m.len())
        .unwrap_or(0);
    footprint_bytes(size)
}

/// Whether the build at `binary` has a GPU backend (`None`: its backend is not recognised).
fn build_has_gpu(binary: &Path) -> Option<bool> {
    binary
        .parent()
        .and_then(crate::runtime::detect_backend_in_dir)
        .map(|b| b != "cpu")
}

/// [`build_has_gpu`] of the installed whisper.cpp runtime (`None`: none installed or unknown).
pub fn installed_build_has_gpu() -> Option<bool> {
    let binary = crate::runtimes::resolve_family_binary_in(
        &crate::runtimes::RuntimeRegistry::global(),
        WHISPER_CPP,
        None,
    )
    .ok()?;
    build_has_gpu(&binary)
}

/// `-x`, `--x`, `--x=y`, `--x_y`: one of the flags Harness manages.
pub fn is_protected(arg: &str) -> bool {
    let flag = arg.split_once('=').map_or(arg, |(flag, _)| flag);
    PROTECTED_FLAGS.contains(&flag) || crate::params::is_protected(arg)
}

/// A whisper ggml file: the first four bytes are the `ggml` magic (`0x67676d6c`, stored
/// little-endian as `lmgg`). Other `.bin` files are not models.
pub fn is_ggml_file(path: &Path) -> bool {
    use std::io::Read;
    let mut magic = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .is_ok()
        && u32::from_le_bytes(magic) == 0x6767_6d6c
}

// --- release archive naming ---
//
// Two schemes exist side by side:
//   old (Windows x64):   whisper-bin-x64.zip, whisper-blas-bin-x64.zip,
//                        whisper-cublas-<ver>-bin-x64.zip
//   new (everything else, and the Windows arm64 builds):
//                        whisper-bin-<os>-<backend>[-<ver>]-<arch>.<ext>
//                        (`ubuntu`: no backend token = CPU)

fn os_token() -> Option<&'static str> {
    if cfg!(windows) {
        Some("win")
    } else if cfg!(target_os = "linux") {
        Some("ubuntu")
    } else {
        None // macOS: only an XCFramework is published
    }
}

fn arch_token() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    }
}

fn parse_ver(v: &str) -> Vec<u32> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// `(archive, backend)` of the first of `backends` that has an archive for this OS / arch.
fn pick_assets(
    assets: &[ReleaseAsset],
    backends: &[String],
) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
    let os = os_token()?;
    let arch = arch_token();
    let ext = r"\.(?:zip|tar\.gz)";
    for backend in backends {
        // Each pattern captures an optional backend version (CUDA).
        let patterns: Vec<String> = match backend.as_str() {
            "vulkan" => vec![format!(r"^whisper-bin-{os}-vulkan-{arch}{ext}$")],
            "cuda" => {
                let mut v = vec![format!(
                    r"^whisper-bin-{os}-cuda-(\d+(?:\.\d+)*)-{arch}{ext}$"
                )];
                if cfg!(windows) && arch == "x64" {
                    v.push(r"^whisper-cublas-(\d+(?:\.\d+)*)-bin-x64\.zip$".to_string());
                }
                v
            }
            "rocm" | "hip" => vec![format!(
                r"^whisper-bin-{os}-(?:rocm|hip-radeon|hip)(?:-\d+(?:\.\d+)*)?-{arch}{ext}$"
            )],
            "sycl" => vec![format!(
                r"^whisper-bin-{os}-sycl(?:-\d+(?:\.\d+)*)?-{arch}{ext}$"
            )],
            "blas" | "openblas" => {
                if cfg!(windows) && arch == "x64" {
                    vec![r"^whisper-blas-bin-x64\.zip$".to_string()]
                } else {
                    vec![]
                }
            }
            "opencl" => vec![format!(r"^whisper-bin-{os}-opencl-[a-z]+-{arch}{ext}$")],
            "cpu" => {
                let mut v = vec![format!(r"^whisper-bin-{os}-cpu-{arch}{ext}$")];
                if cfg!(windows) && arch == "x64" {
                    v.push(r"^whisper-bin-x64\.zip$".to_string());
                } else if !cfg!(windows) {
                    v.push(format!(r"^whisper-bin-{os}-{arch}{ext}$"));
                }
                v
            }
            // Metal: the only macOS artifact is an XCFramework (no server binary).
            _ => vec![],
        };
        for pattern in patterns {
            let Ok(re) = regex::Regex::new(&pattern) else {
                continue;
            };
            let mut hits: Vec<(&ReleaseAsset, Vec<u32>)> = assets
                .iter()
                .filter_map(|a| {
                    let c = re.captures(&a.name)?;
                    Some((
                        a,
                        c.get(1).map(|m| parse_ver(m.as_str())).unwrap_or_default(),
                    ))
                })
                .collect();
            // Several CUDA versions: the lowest CUDA 12 build. The 11.8 archive does not ship
            // cuBLAS 11 (`ggml-cuda.dll` imports `cublas64_11.dll`), so the backend fails to
            // load and whisper silently runs on the CPU (measured on b5454, RTX 3050 Ti); the
            // 12.x archives are self-contained. CUDA 11 is only the last resort.
            hits.sort_by(|a, b| {
                let old = |v: &Vec<u32>| v.first().is_some_and(|major| *major < 12);
                old(&a.1).cmp(&old(&b.1)).then_with(|| a.1.cmp(&b.1))
            });
            if let Some((main, _)) = hits.first() {
                return Some((vec![(*main).clone()], Some(backend.clone())));
            }
        }
    }
    None
}

fn pick_assets_for_driver(
    assets: &[ReleaseAsset],
    backends: &[String],
    driver: Option<&str>,
) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
    if crate::hardware::windows_cuda12_driver_supported(driver) == Some(false) {
        pick_assets(assets, &["cpu".into()])
    } else {
        pick_assets(assets, backends)
    }
}

/// A CUDA build can silently continue on CPU; expose the warning in the server log.
pub fn gpu_fallback_note(line: &str, language: &str) -> Option<&'static str> {
    line.to_ascii_lowercase().contains("no gpu found").then_some(
        if language == "tr" {
            "[Harness] Whisper GPU bulamadı; CPU üzerinde çalışıyor. NVIDIA sürücüsünü güncelleyin veya CPU build seçin."
        } else {
            "[Harness] Whisper found no GPU and is running on CPU. Update the NVIDIA driver or select the CPU build."
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::BinaryKey;
    use crate::params::ParamValue;
    use crate::runtime::AssetIntegrity;

    /// Release objects of ggml-org/whisper.cpp as the GitHub API returned them on 2026-10-02
    /// (`v1.9.4` has no assets, `b5130` is the newest CI release, `v1.9.2` / `v1.8.4` the stable
    /// ones).
    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    fn old_windows_driver_selects_small_cpu_archive() {
        let assets = release("b5130");
        for driver in ["31.0.15.2824", "30.0.14.7212"] {
            let (selected, backend) =
                pick_assets_for_driver(&assets, &["cuda".into()], Some(driver)).unwrap();
            assert_eq!(backend.as_deref(), Some("cpu"));
            assert_eq!(selected[0].name, "whisper-bin-x64.zip");
        }
        for driver in [
            None,
            Some("broken"),
            Some("31.0.15.2833"),
            Some("32.0.16.1742"),
        ] {
            assert_eq!(
                pick_assets_for_driver(&assets, &["cuda".into()], driver)
                    .unwrap()
                    .1
                    .as_deref(),
                Some("cuda")
            );
        }
    }

    #[test]
    fn gpu_fallback_is_visible_and_localized() {
        assert!(
            gpu_fallback_note("whisper_backend_init_gpu: no GPU found", "tr")
                .unwrap()
                .contains("CPU")
        );
        assert!(gpu_fallback_note("no GPU found", "en")
            .unwrap()
            .contains("running on CPU"));
        assert!(gpu_fallback_note("CUDA device found", "tr").is_none());
    }

    fn releases() -> Vec<serde_json::Value> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/github-releases-whisper-b5130.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn assets_of(release: &serde_json::Value) -> Vec<ReleaseAsset> {
        release["assets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| ReleaseAsset {
                name: a["name"].as_str().unwrap().into(),
                download_url: a["browser_download_url"].as_str().unwrap().into(),
                size: a["size"].as_u64().unwrap(),
                digest: a["digest"].as_str().map(str::to_string),
                integrity: AssetIntegrity::Verified,
            })
            .collect()
    }

    fn release(tag: &str) -> Vec<ReleaseAsset> {
        let all = releases();
        assets_of(all.iter().find(|r| r["tag_name"] == tag).unwrap())
    }

    fn pick(tag: &str, backends: &[&str]) -> Option<(String, Option<String>)> {
        let b: Vec<String> = backends.iter().map(|s| s.to_string()).collect();
        pick_assets(&release(tag), &b).map(|(a, b)| (a[0].name.clone(), b))
    }

    #[test]
    fn engine_identity() {
        let e = WhisperCppEngine;
        assert_eq!(e.family(), "whisper.cpp");
        assert_eq!(e.kind(), RuntimeKind::Server);
        assert_eq!(e.release_source().repo, "ggml-org/whisper.cpp");
        assert_eq!(e.release_source().binary, "whisper-server");
        assert!(e.supports(ModelFormat::GgmlBin));
        assert!(!e.supports(ModelFormat::Gguf));
        assert!(e.as_server().is_some() && e.as_job().is_none());
        assert_eq!(e.version_arg(), "--help");
        assert_eq!(
            crate::engine::engine("whisper.cpp").unwrap().family(),
            WHISPER_CPP
        );
    }

    /// Real asset names: the CPU build on Windows x64, and `cpu` is what Vulkan / ROCm fall
    /// through to (the official releases have no such build).
    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    fn windows_x64_builds() {
        for tag in ["b5130", "v1.9.2", "v1.8.4"] {
            assert_eq!(
                pick(tag, &["cpu"]),
                Some(("whisper-bin-x64.zip".into(), Some("cpu".into()))),
                "{tag}"
            );
            // No Vulkan / ROCm anywhere: the caller's next backend is tried.
            assert_eq!(pick(tag, &["vulkan"]), None, "{tag}");
            assert_eq!(pick(tag, &["rocm"]), None, "{tag}");
            assert_eq!(
                pick(tag, &["vulkan", "rocm", "cpu"]),
                Some(("whisper-bin-x64.zip".into(), Some("cpu".into())))
            );
            // CUDA: the lowest CUDA 12 build (12.4.0), not 11.8.0 (no cuBLAS 11 inside).
            assert_eq!(
                pick(tag, &["cuda"]),
                Some((
                    "whisper-cublas-12.4.0-bin-x64.zip".into(),
                    Some("cuda".into())
                ))
            );
            assert_eq!(
                pick(tag, &["blas"]),
                Some(("whisper-blas-bin-x64.zip".into(), Some("blas".into())))
            );
            assert_eq!(pick(tag, &["metal"]), None);
        }
        // The newer CI release also has arm64 builds under the new naming; x64 is unaffected.
        let (a, _) = pick_assets(&release("b5130"), &["cuda".to_string()]).unwrap();
        assert!(a[0].name.contains("cublas"), "{}", a[0].name);
        // The empty stable release matches nothing.
        let empty = serde_json::json!({"tag_name": "v1.9.4", "assets": []});
        assert!(pick_assets(&assets_of(&empty), &["cpu".to_string()]).is_none());
    }

    /// `b5454` (2026-10-06, the first release with `whisper-bin-win-cuda-<v>-x64.zip`): the CUDA 12.4
    /// archive is the pick on Windows x64, CUDA 11.8 only when nothing else exists.
    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    fn windows_x64_new_cuda_naming() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/github-releases-whisper-b5454.json");
        let all: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let assets = assets_of(all.iter().find(|r| r["tag_name"] == "b5454").unwrap());
        let name = |b: &str| pick_assets(&assets, &[b.to_string()]).map(|(a, _)| a[0].name.clone());
        assert_eq!(
            name("cuda").as_deref(),
            Some("whisper-bin-win-cuda-12.4.0-x64.zip")
        );
        assert_eq!(name("cpu").as_deref(), Some("whisper-bin-x64.zip"));
        let only_11: Vec<ReleaseAsset> = assets
            .iter()
            .filter(|a| !a.name.contains("12.4"))
            .cloned()
            .collect();
        assert_eq!(
            pick_assets(&only_11, &["cuda".to_string()]).map(|(a, _)| a[0].name.clone()),
            Some("whisper-bin-win-cuda-11.8.0-x64.zip".to_string())
        );
    }

    /// Real asset names on Linux: the Ubuntu archive has no backend token and is the CPU
    /// build; there is no GPU build, so Vulkan / ROCm / CUDA fall through to it.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_builds() {
        let want = format!("whisper-bin-ubuntu-{}.tar.gz", arch_token());
        for tag in ["b5130", "v1.9.2"] {
            assert_eq!(
                pick(tag, &["cpu"]),
                Some((want.clone(), Some("cpu".into()))),
                "{tag}"
            );
            assert_eq!(pick(tag, &["vulkan"]), None, "{tag}");
            assert_eq!(pick(tag, &["cuda"]), None, "{tag}");
            assert_eq!(pick(tag, &["blas"]), None, "{tag}");
            assert_eq!(
                pick(tag, &["cuda", "vulkan", "cpu"]),
                Some((want.clone(), Some("cpu".into()))),
                "{tag}"
            );
        }
        // v1.8.4 published no Linux build.
        assert_eq!(pick("v1.8.4", &["cpu"]), None);
    }

    /// macOS: only an XCFramework is published, so no release can be installed.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_has_no_server_build() {
        for tag in ["b5130", "v1.9.2", "v1.8.4"] {
            assert_eq!(pick(tag, &["metal", "cpu"]), None, "{tag}");
        }
    }

    /// The new naming scheme (`whisper-bin-<os>-<backend>[-<ver>]-<arch>`), as the arm64
    /// builds and the Linux archives already use it.
    #[test]
    fn new_naming_scheme() {
        let mk = |n: &str| ReleaseAsset {
            name: n.into(),
            download_url: format!(
                "https://github.com/ggml-org/whisper.cpp/releases/download/x/{n}"
            ),
            size: 1,
            digest: None,
            integrity: AssetIntegrity::Missing,
        };
        let os = os_token();
        let arch = arch_token();
        let Some(os) = os else { return };
        let names = [
            format!("whisper-bin-{os}-vulkan-{arch}.zip"),
            format!("whisper-bin-{os}-cuda-13.4-{arch}.zip"),
            format!("whisper-bin-{os}-cuda-12.4.0-{arch}.zip"),
            format!("whisper-bin-{os}-cpu-{arch}.zip"),
            format!("whisper-bin-{os}-{arch}.tar.gz"),
            format!("whisper-bin-{os}-rocm-7.2-{arch}.tar.gz"),
            "whisper-v1.9.2-xcframework.zip".to_string(),
        ];
        let assets: Vec<ReleaseAsset> = names.iter().map(|n| mk(n)).collect();
        let name = |b: &str| pick_assets(&assets, &[b.to_string()]).map(|(a, _)| a[0].name.clone());
        assert_eq!(
            name("vulkan"),
            Some(format!("whisper-bin-{os}-vulkan-{arch}.zip"))
        );
        // Lowest CUDA version.
        assert_eq!(
            name("cuda"),
            Some(format!("whisper-bin-{os}-cuda-12.4.0-{arch}.zip"))
        );
        assert_eq!(
            name("rocm"),
            Some(format!("whisper-bin-{os}-rocm-7.2-{arch}.tar.gz"))
        );
        assert!(name("cpu").unwrap().contains("whisper-bin"));
        assert!(name("metal").is_none());
    }

    #[test]
    fn launch_arguments() {
        let caps = crate::capabilities::whisper::from_help(
            BinaryKey::default(),
            &std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/data/whisper-server-b5130.help.txt"),
            )
            .unwrap(),
        );
        let mut cfg = LaunchConfig {
            model_path: "C:/m/ggml-base.bin".into(),
            host: "127.0.0.1".into(),
            port: 11440,
            gpu_layers: 999,
            threads: Some(6),
            ..Default::default()
        };
        cfg.params
            .insert("translate".into(), ParamValue::Bool(true));
        cfg.params.insert("beam_size".into(), ParamValue::Int(5));
        cfg.params.insert("vad".into(), ParamValue::Bool(true));
        cfg.params
            .insert("vad_model".into(), ParamValue::Text("C:/m/vad.bin".into()));
        cfg.params
            .insert("prompt".into(), ParamValue::Text("Harness Studio".into()));
        let out = WhisperCppEngine
            .build_launch(&cfg, &ServerCaps::default(), Some(&caps))
            .unwrap();
        let a = out.args.join(" ");
        assert!(
            a.starts_with("-m C:/m/ggml-base.bin --host 127.0.0.1 --port 11440 -t 6"),
            "{a}"
        );
        assert!(a.contains("--language auto"), "{a}");
        assert!(!a.contains("--no-gpu"), "{a}");
        assert!(
            a.contains("-tr") && a.contains("-bs 5") && a.contains("--vad"),
            "{a}"
        );
        assert!(
            a.contains("-vm C:/m/vad.bin") && a.contains("--prompt Harness Studio"),
            "{a}"
        );
        // CPU only: no GPU layers.
        cfg.gpu_layers = 0;
        // An explicit language replaces the default.
        cfg.params
            .insert("language".into(), ParamValue::Text("tr".into()));
        let a = WhisperCppEngine
            .build_launch(&cfg, &ServerCaps::default(), Some(&caps))
            .unwrap()
            .args
            .join(" ");
        assert!(
            a.contains("--no-gpu") && a.contains("-l tr") && !a.contains("auto"),
            "{a}"
        );
        // Without a probe nothing from the catalog is sent (the build is unknown).
        let out = WhisperCppEngine
            .build_launch(&cfg, &ServerCaps::default(), None)
            .unwrap();
        assert!(out.skipped.iter().any(|s| s.reason == "runtime_unknown"));
        // Network binds and protected flags are refused.
        cfg.host = "0.0.0.0".into();
        assert!(WhisperCppEngine
            .build_launch(&cfg, &ServerCaps::default(), Some(&caps))
            .is_err());
        cfg.host = "127.0.0.1".into();
        for bad in [
            "--port 1",
            "--inference-path /x",
            "-m other.bin",
            "--public .",
        ] {
            cfg.custom_args = Some(bad.into());
            assert!(
                WhisperCppEngine
                    .build_launch(&cfg, &ServerCaps::default(), Some(&caps))
                    .is_err(),
                "{bad}"
            );
        }
        cfg.custom_args = Some("--print-special".into());
        assert!(WhisperCppEngine
            .build_launch(&cfg, &ServerCaps::default(), Some(&caps))
            .unwrap()
            .args
            .contains(&"--print-special".to_string()));
    }

    #[test]
    fn health_is_the_status_field() {
        let e = WhisperCppEngine;
        assert!(e.health(200, r#"{"status":"ok"}"#));
        assert!(!e.health(200, r#"{"status":"loading model"}"#));
        assert!(!e.health(503, r#"{"status":"ok"}"#));
        assert!(!e.health(200, "<html>"));
        assert_eq!(e.openai_base("http://127.0.0.1:1/"), "http://127.0.0.1:1");
    }

    #[test]
    fn catalog_flags_exist_in_the_real_help() {
        let caps = crate::capabilities::whisper::from_help(
            BinaryKey::default(),
            &std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/data/whisper-server-b5130.help.txt"),
            )
            .unwrap(),
        );
        let cat = catalog();
        let mut ids = std::collections::BTreeSet::new();
        for p in &cat.params {
            assert!(ids.insert(p.id.clone()), "duplicate {}", p.id);
            for f in &p.flags {
                assert!(caps.has_flag(f), "{}: {} not in the real help", p.id, f);
                assert!(!is_protected(f), "{}: catalog emits protected {}", p.id, f);
            }
            assert!(p.label_key.starts_with("wparam.") && p.help_key.ends_with("_help"));
            for r in p.requires.iter().chain(&p.conflicts) {
                assert!(cat.get(r).is_some(), "{}: unknown reference {}", p.id, r);
            }
            // The kind matches the flag's shape (a switch takes no value).
            let takes_value = caps.flag(&p.flags[0]).unwrap().value_hint.is_some();
            let is_switch = p.kind == crate::params::ParamKind::Bool;
            assert_eq!(takes_value, !is_switch, "{} ({:?})", p.id, p.kind);
        }
    }

    #[test]
    fn memory_estimate_and_ggml_magic() {
        let budget = MemoryBudget {
            vram_bytes: 8 << 30,
            ram_bytes: 16 << 30,
            ..Default::default()
        };
        let input = FitInput {
            model_size_bytes: 574 << 20, // ggml-large-v3-turbo-q5_0.bin
            gpu_layers: 999,
            ..Default::default()
        };
        let r = WhisperCppEngine.estimate_memory(&input, None, &budget, &[]);
        assert_eq!(r.category, FitCategory::FullGpu);
        assert_eq!(r.estimate.vram_bytes, footprint_bytes(574 << 20));
        assert_eq!(r.estimate.ram_bytes, 0);
        assert!(!r.estimate.exact);
        let cpu = FitInput {
            gpu_layers: 0,
            ..input.clone()
        };
        let r = WhisperCppEngine.estimate_memory(&cpu, None, &budget, &[]);
        assert_eq!(r.estimate.vram_bytes, 0);
        assert!(r.estimate.ram_bytes > 574 << 20);
        // On the CPU it is not "fits in VRAM".
        assert_eq!(r.category, FitCategory::CpuHeavy);
        let tiny = MemoryBudget {
            vram_bytes: 100 << 20,
            ..budget
        };
        assert_eq!(
            WhisperCppEngine
                .estimate_memory(&input, None, &tiny, &[])
                .category,
            FitCategory::OomRisk
        );

        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("ggml-tiny.bin");
        std::fs::write(&model, [0x6c, 0x6d, 0x67, 0x67, 1, 2, 3]).unwrap();
        assert!(is_ggml_file(&model));
        let other = dir.path().join("weights.bin");
        std::fs::write(&other, b"not a model").unwrap();
        assert!(!is_ggml_file(&other));
        assert!(!is_ggml_file(&dir.path().join("missing.bin")));
    }
}
