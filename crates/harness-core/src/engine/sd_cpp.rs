//! stable-diffusion.cpp as an [`Engine`]: `sd-server`, a long-lived process that
//! loads one image / video model (SD1.x / 2.x, SDXL, SD3.5, FLUX, Qwen-Image, Z-Image, Wan...)
//! and generates on request.
//!
//! What the official releases (`leejet/stable-diffusion.cpp`, tags `master-<n>-<commit>`) really
//! ship (checked 2026-10-11 on `master-956-1b0ba10`): Windows x64 CPU, Vulkan (~30 MB), CUDA 12
//! (`...-win-cuda12-x64.zip` plus the `cudart-sd-bin-win-cu12-x64.zip` runtime archive) and ROCm;
//! Linux x86_64 CPU, Vulkan and ROCm (`...-Linux-Ubuntu-24.04-x86_64[-vulkan|-rocm-<v>].zip`);
//! macOS arm64 (Metal). Every archive holds `sd-cli` and `sd-server` next to the ggml backends.
//!
//! `sd-server` loads the model before it listens (no readiness endpoint of its own: the port
//! opening is readiness; `/v1/models` is the cheapest answer), has no authentication and speaks
//! three dialects: OpenAI (`/v1/images/generations`), A1111 (`/sdapi/v1/*`) and its own async
//! job API (`/sdcpp/v1/img_gen` + `/sdcpp/v1/jobs/{id}`), which the gateway uses. Harness binds
//! it to loopback only and puts the OpenAI route (`/v1/images/generations`) in the gateway.
//!
//! A model is not one file: a full checkpoint (SD1.x / SDXL / SD3 single file) goes to `-m`;
//! a standalone diffusion model (FLUX, Qwen-Image, Wan...) goes to `--diffusion-model` and
//! needs its VAE and text encoders (`--vae`, `--clip_l`, `--t5xxl`, `--llm`). The components
//! live in the definition's `load` settings under the catalog ids of `params_sdcpp.toml`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use super::{Engine, MetricsSource, ModelFormat, ReleaseSource, RuntimeKind, ServerEngine};
use crate::capabilities::RuntimeCapabilities;
use crate::error_key::err;
use crate::gguf::GgufMetadata;
use crate::params::{ParamCatalog, ParamValue, ParamValues, RenderContext};
use crate::process::{BuildOutput, LaunchConfig, ServerCaps};
use crate::runtime::ReleaseAsset;
use crate::telemetry::Metrics;
use crate::vram::{
    EstimateSource, FitCategory, FitInput, FitResult, Measurement, MemoryBudget, MemoryEstimate,
};

/// Family id of stable-diffusion.cpp builds.
pub const SD_CPP: &str = "stable-diffusion.cpp";

/// Catalog ids of the files of a model besides its main file, with the flag each one is
/// given to (see `params_sdcpp.toml`). Directories (`lora_model_dir`, `embd_dir`) are not
/// listed: they hold optional files, not a part of the model.
pub const COMPONENTS: &[(&str, &str)] = &[
    ("vae", "--vae"),
    ("clip_l", "--clip_l"),
    ("clip_g", "--clip_g"),
    ("t5xxl", "--t5xxl"),
    ("llm", "--llm"),
    ("llm_vision", "--llm_vision"),
    ("high_noise_diffusion_model", "--high-noise-diffusion-model"),
    ("taesd", "--taesd"),
    ("control_net", "--control-net"),
    ("upscale_model", "--upscale-model"),
];

/// Flags a custom argument line (or a catalog value) may never set: the ones Harness's own
/// fields and the gateway depend on. `--host` / `--port` are the generic protected ones.
const PROTECTED_FLAGS: &[&str] = &[
    "-m",
    "--model",
    "--diffusion-model",
    "-l",
    "--listen-ip",
    "--listen-port",
    "--serve-html-path",
];

/// `sd-server` builds from the official releases (or a local folder).
#[derive(Debug, Clone, Copy, Default)]
pub struct SdCppEngine;

/// The parsed catalog (`params_sdcpp.toml`).
pub fn catalog() -> &'static ParamCatalog {
    static CATALOG: OnceLock<ParamCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        toml::from_str(include_str!("../params_sdcpp.toml"))
            .expect("params_sdcpp.toml is valid (unit tested)")
    })
}

impl Engine for SdCppEngine {
    fn family(&self) -> &'static str {
        SD_CPP
    }

    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Server
    }

    fn release_source(&self) -> ReleaseSource {
        ReleaseSource {
            repo: "leejet/stable-diffusion.cpp",
            binary: "sd-server",
        }
    }

    /// stable-diffusion.cpp has no managed "official" runtime: its builds are registered
    /// runtimes (`runtimes.json`) installed from a release.
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

    /// Safetensors and `.ckpt` always are image models; a GGUF is one only by its tensors
    /// (see [`is_image_gguf`]), which a format alone cannot tell.
    fn supports(&self, format: ModelFormat) -> bool {
        matches!(format, ModelFormat::Safetensors | ModelFormat::Ckpt)
    }

    fn version_arg(&self) -> &'static str {
        // `--version` is an unknown argument of sd-server.
        "--help"
    }

    fn detect_capabilities(&self, binary: &Path) -> Arc<RuntimeCapabilities> {
        crate::capabilities::registry()
            .get_or_probe_with(binary, crate::capabilities::sdcpp::probe_binary)
    }

    fn cached_capabilities(&self, binary: &Path) -> Option<Arc<RuntimeCapabilities>> {
        crate::capabilities::registry().cached(binary)
    }

    fn probe_capabilities(&self, binary: &Path) -> RuntimeCapabilities {
        crate::capabilities::sdcpp::probe_binary(binary)
    }

    fn param_catalog(&self) -> &'static ParamCatalog {
        catalog()
    }

    fn as_server(&self) -> Option<&dyn ServerEngine> {
        Some(self)
    }
}

/// How the main file is given to `sd-server`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainRole {
    /// A full checkpoint (diffusion model, VAE and text encoder in one file): `-m`.
    Checkpoint,
    /// A standalone diffusion model: `--diffusion-model` (its VAE / text encoders come from
    /// the component params).
    Diffusion,
}

impl MainRole {
    pub fn flag(self) -> &'static str {
        match self {
            MainRole::Checkpoint => "-m",
            MainRole::Diffusion => "--diffusion-model",
        }
    }
}

fn text_param<'a>(params: &'a ParamValues, id: &str) -> Option<&'a str> {
    match params.get(id) {
        Some(ParamValue::Text(t)) if !t.trim().is_empty() => Some(t.trim()),
        _ => None,
    }
}

/// The component files a launch names (VAE, text encoders...), in catalog order, empty values
/// left out.
pub fn component_files(params: &ParamValues) -> Vec<(&'static str, PathBuf)> {
    COMPONENTS
        .iter()
        .filter_map(|(id, _)| text_param(params, id).map(|p| (*id, PathBuf::from(p))))
        .collect()
}

/// `-m` or `--diffusion-model` for a launch: the definition's explicit `main_model_role`, else
/// [`detect_main_role`] from the file.
pub fn main_role(config: &LaunchConfig) -> MainRole {
    match text_param(&config.params, "main_model_role") {
        Some("checkpoint") => MainRole::Checkpoint,
        Some("diffusion") => MainRole::Diffusion,
        _ => detect_main_role(
            &config.model_path,
            !component_files(&config.params).is_empty(),
        ),
    }
}

/// Role of a main file without an explicit choice. A safetensors file is a checkpoint when its
/// tensors carry the single-file prefixes (`model.diffusion_model.`, `first_stage_model.`,
/// `cond_stage_model.`, `conditioner.`); a standalone transformer (FLUX `double_blocks.`...)
/// is not. A GGUF has no such table in reach cheaply: with companion files it is a standalone
/// model, without them a checkpoint.
pub fn detect_main_role(path: &Path, has_components: bool) -> MainRole {
    match ModelFormat::of(path) {
        ModelFormat::Safetensors => match safetensors_names(path) {
            Some(names) if is_checkpoint_names(names.iter().map(String::as_str)) => {
                MainRole::Checkpoint
            }
            Some(_) => MainRole::Diffusion,
            None if has_components => MainRole::Diffusion,
            None => MainRole::Checkpoint,
        },
        ModelFormat::Gguf => {
            if has_components {
                MainRole::Diffusion
            } else {
                MainRole::Checkpoint
            }
        }
        _ => MainRole::Checkpoint,
    }
}

/// Tensor-name prefixes of a full single-file checkpoint.
const CHECKPOINT_PREFIXES: &[&str] = &[
    "model.diffusion_model.",
    "first_stage_model.",
    "cond_stage_model.",
    "conditioner.",
    "text_encoders.",
];

fn is_checkpoint_names<'a>(names: impl Iterator<Item = &'a str>) -> bool {
    names
        .into_iter()
        .any(|n| CHECKPOINT_PREFIXES.iter().any(|p| n.starts_with(p)))
}

/// Tensor-name prefixes of an image / video generator (not a language model, VAE or text
/// encoder): the UNet / DiT / MM-DiT families of the models sd.cpp runs.
const IMAGE_PREFIXES: &[&str] = &[
    "model.diffusion_model.",
    "double_blocks.",
    "single_blocks.",
    "joint_blocks.",
    "input_blocks.",
    "down_blocks.",
    "transformer_blocks.",
    "patch_embedding.",
    "img_in.",
    "cap_embedder.",
    "x_embedder.",
    "cond_stage_model.",
    "first_stage_model.",
    "conditioner.",
];

/// A tensor table that belongs to an image generator.
pub fn is_image_names<'a>(mut names: impl Iterator<Item = &'a str>) -> bool {
    names.any(|n| IMAGE_PREFIXES.iter().any(|p| n.starts_with(p)))
}

/// GGUF `general.architecture` values of image / video generators (the ComfyUI-GGUF set plus
/// the newer ones stable-diffusion.cpp reads); not language models, llama-server cannot load
/// them.
pub const IMAGE_ARCHS: &[&str] = &[
    "flux",
    "flux2",
    "sd1",
    "sd2",
    "sd3",
    "sdxl",
    "aura",
    "ltxv",
    "hyvid",
    "wan",
    "hidream",
    "cosmos",
    "lumina2",
    "qwen_image",
    "qwen_image21",
    "krea2",
    "z_image",
    "chroma",
];

/// Whether a GGUF `general.architecture` names an image generator.
pub fn is_image_arch(arch: &str) -> bool {
    IMAGE_ARCHS.contains(&arch)
}

type FileId = (PathBuf, u64, u64);

fn file_id(path: &Path) -> Option<FileId> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    Some((path.to_path_buf(), meta.len(), mtime))
}

/// Tensor names of a safetensors file (the JSON header only; `None` when it cannot be read).
/// Cached by path, size and modification time.
pub fn safetensors_names(path: &Path) -> Option<Arc<Vec<String>>> {
    type Names = Option<Arc<Vec<String>>>;
    static CACHE: OnceLock<Mutex<HashMap<FileId, Names>>> = OnceLock::new();
    let id = file_id(path)?;
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().unwrap().get(&id) {
        return hit.clone();
    }
    let names = read_safetensors_names(path).map(Arc::new);
    let mut map = cache.lock().unwrap();
    if map.len() > 4096 {
        map.clear();
    }
    map.insert(id, names.clone());
    names
}

/// Largest JSON header read (FLUX / Wan checkpoints have 1-3 MB; anything bigger is not one).
const MAX_SAFETENSORS_HEADER: u64 = 64 * 1024 * 1024;

fn read_safetensors_names(path: &Path) -> Option<Vec<String>> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut len = [0u8; 8];
    f.read_exact(&mut len).ok()?;
    let len = u64::from_le_bytes(len);
    if len == 0 || len > MAX_SAFETENSORS_HEADER {
        return None;
    }
    let mut header = vec![0u8; len as usize];
    f.read_exact(&mut header).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&header).ok()?;
    Some(
        json.as_object()?
            .keys()
            .filter(|k| *k != "__metadata__")
            .cloned()
            .collect(),
    )
}

/// A safetensors file that is an image generator (a checkpoint or a standalone transformer;
/// not a VAE, text encoder or language model).
pub fn is_image_safetensors(path: &Path) -> bool {
    safetensors_names(path).is_some_and(|n| is_image_names(n.iter().map(String::as_str)))
}

/// A GGUF that is an image generator: its `general.architecture` names one, or it carries no
/// architecture at all (stable-diffusion.cpp's own conversions write no metadata) and its
/// tensors are a UNet / DiT. Cached by path, size and modification time.
pub fn is_image_gguf(path: &Path, meta: Option<&GgufMetadata>) -> bool {
    match meta.and_then(|m| m.architecture.as_deref()) {
        Some(arch) => return is_image_arch(arch),
        None if meta.is_none() => return false,
        None => {}
    }
    static CACHE: OnceLock<Mutex<HashMap<FileId, bool>>> = OnceLock::new();
    let Some(id) = file_id(path) else {
        return false;
    };
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().unwrap().get(&id) {
        return *hit;
    }
    let found = crate::gguf::read_gguf_tensor_names(path, 400)
        .map(|names| is_image_names(names.iter().map(String::as_str)))
        .unwrap_or(false);
    let mut map = cache.lock().unwrap();
    if map.len() > 4096 {
        map.clear();
    }
    map.insert(id, found);
    found
}

/// Whether a model file is an image model by itself (safetensors / `.ckpt`, or a GGUF told by
/// its architecture or tensors): what the library lists as [`crate::domain::ModelKind::Image`].
pub fn is_image_path(path: &Path) -> bool {
    match ModelFormat::of(path) {
        ModelFormat::Safetensors | ModelFormat::Ckpt => true,
        ModelFormat::Gguf => {
            path.is_file() && is_image_gguf(path, crate::models::cached_metadata(path).as_ref())
        }
        _ => false,
    }
}

/// Whether a launch runs on `sd-server`: a safetensors / `.ckpt` file, or a GGUF that is an
/// image model (explicit component or role settings, or its tensors).
pub fn is_image_launch(config: &LaunchConfig) -> bool {
    match ModelFormat::of(&config.model_path) {
        ModelFormat::Safetensors | ModelFormat::Ckpt => true,
        ModelFormat::Gguf => {
            text_param(&config.params, "main_model_role").is_some()
                || !component_files(&config.params).is_empty()
                || is_image_gguf(
                    &config.model_path,
                    crate::models::cached_metadata(&config.model_path).as_ref(),
                )
        }
        _ => false,
    }
}

impl ServerEngine for SdCppEngine {
    fn build_launch(
        &self,
        config: &LaunchConfig,
        _caps: &ServerCaps,
        runtime: Option<&RuntimeCapabilities>,
    ) -> Result<BuildOutput, String> {
        // No authentication of its own: only loopback, the gateway is the front door.
        if !crate::network::is_loopback_host(&config.host) {
            return Err(err("process.sdcpp_loopback_only")
                .with("host", &config.host)
                .into());
        }
        let mut args: Vec<String> = vec![
            main_role(config).flag().into(),
            config.model_path.to_string_lossy().to_string(),
            "-l".into(),
            config.host.clone(),
            "--listen-port".into(),
            config.port.to_string(),
        ];
        if let Some(threads) = config.threads.filter(|&n| n > 0) {
            args.push("-t".into());
            args.push(threads.to_string());
        }
        // `gpu_layers` 0 is "CPU only" for every family; sd.cpp names it a backend.
        if config.gpu_layers == 0
            && text_param(&config.params, "backend").is_none()
            && runtime.is_none_or(|r| r.has_flag("--backend"))
        {
            args.push("--backend".into());
            args.push("cpu".into());
        }
        let ctx = RenderContext {
            host: config.host.clone(),
            ..Default::default()
        };
        let mut rendered = crate::params::render_params(catalog(), runtime, &config.params, &ctx)?;
        // The typed settings (`threads`, `main_model_role`) are built above, not skipped.
        rendered.skipped.retain(|s| s.reason != "legacy_field");
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

    /// The port opening is readiness (the model loads before the server listens); the
    /// cheapest route that answers is the OpenAI model list.
    fn health_path(&self) -> &'static str {
        "/v1/models"
    }

    fn health(&self, status: u16, _body: &str) -> bool {
        status == 200
    }

    /// sd-server's OpenAI surface is images only; the base is the server itself.
    fn openai_base(&self, host_url: &str) -> String {
        host_url.trim_end_matches('/').to_string()
    }

    /// No `/metrics`, no slots (liveness comes from the model list).
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
        let (weights, compute) = (model, compute_bytes(model));
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
            // The whole model is one unit; sd.cpp's own auto-fit moves weights when it must.
            recommended_gpu_layers: if gpu { 999 } else { 0 },
            slots: None,
            kv: None,
        }
    }
}

/// Compute buffers on top of the weights: a quarter of the weights, between 256 MiB and 6 GiB
/// (the UNet / DiT activations and the VAE decode grow with the resolution, not the file size;
/// measured on an RTX 3050 Ti with sd-server master-956, SD-Turbo q8 at 512x512 peaked at
/// ~2.2 GiB of device memory for 1.9 GiB of weights, i.e. ~0.3 GiB of compute). Half of the
/// weights with a 512 MiB floor, plus the admission reserve, refused SD-Turbo on the 4 GB card
/// it runs on. A rough number, never claimed exact: sd.cpp's `--auto-fit` places weights by
/// what is free.
fn compute_bytes(weights: u64) -> u64 {
    const GIB: u64 = 1024 * 1024 * 1024;
    (weights / 4).clamp(GIB / 4, 6 * GIB)
}

/// Device memory a loaded image model takes on a GPU build: its files plus the compute buffers.
pub fn footprint_bytes(weights: u64) -> u64 {
    weights + compute_bytes(weights)
}

/// Size of the files of a launch: the main file and every component that exists.
pub fn weights_bytes(config: &LaunchConfig) -> u64 {
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    size(&config.model_path)
        + component_files(&config.params)
            .iter()
            .map(|(_, p)| size(p))
            .sum::<u64>()
}

/// Device memory an sd-server launch takes: the files plus compute buffers when the chosen
/// build has a GPU backend and the launch uses it, else none (a CPU build keeps everything in
/// system memory, which is not VRAM).
pub fn vram_need(config: &LaunchConfig) -> u64 {
    let gpu_build = launch_binary(config)
        .as_deref()
        .and_then(build_has_gpu)
        .unwrap_or(false);
    if !gpu_build || config.gpu_layers == 0 || cpu_backend(config) {
        return 0;
    }
    footprint_bytes(weights_bytes(config))
}

/// The sd-server binary a launch runs: its own, else the installed one of its runtime.
fn launch_binary(config: &LaunchConfig) -> Option<PathBuf> {
    if config.binary_path.as_os_str().is_empty() {
        crate::runtimes::resolve_family_binary_in(
            &crate::runtimes::RuntimeRegistry::global(),
            SD_CPP,
            config.runtime.as_deref(),
        )
        .ok()
    } else {
        Some(config.binary_path.clone())
    }
}

fn cpu_backend(config: &LaunchConfig) -> bool {
    text_param(&config.params, "backend").is_some_and(|b| b.to_ascii_lowercase().starts_with("cpu"))
}

/// Whether a launch is known to keep everything in system memory: CPU only (`gpu_layers` 0),
/// a CPU backend, or a build without a GPU backend. An unrecognised build is not known.
pub fn runs_on_cpu(config: &LaunchConfig) -> bool {
    config.gpu_layers == 0
        || cpu_backend(config)
        || launch_binary(config).as_deref().and_then(build_has_gpu) == Some(false)
}

/// Device memory an sd-server launch cannot do without when sd.cpp places the weights itself
/// (`--auto-fit`, on by default: compute GPU, then system memory): the compute buffers. Slower
/// than a model on the card, but it loads (measured on a 4 GB RTX 3050 Ti, sd-server b964:
/// Z-Image Turbo Q4_0 with its Qwen3-4B text encoder, 5.8 GB of weights, kept 2.4 GB on the
/// card and generated 512x512 in 57 s). `None` when the launch takes no GPU memory or its
/// placement is fixed (`--auto-fit off`, `--params-backend`): the full need stays.
pub fn min_vram_need(config: &LaunchConfig) -> Option<u64> {
    if !auto_fit(config) {
        return None;
    }
    let need = vram_need(config);
    if need == 0 {
        return None;
    }
    // An older sd-server without `--auto-fit` (or one whose help could not be read) puts the
    // whole model on the card: the full need stays.
    let binary = launch_binary(config)?;
    let caps = crate::capabilities::registry()
        .get_or_probe_with(&binary, crate::capabilities::sdcpp::probe_binary);
    supports_auto_fit(&caps).then(|| compute_bytes(weights_bytes(config)).min(need))
}

/// Whether an sd-server build places the weights itself: its help lists `--auto-fit`.
fn supports_auto_fit(caps: &crate::capabilities::RuntimeCapabilities) -> bool {
    caps.help_parsed && caps.has_flag("--auto-fit")
}

/// Whether sd.cpp's `--auto-fit` places the weights of a launch (the typed settings or the
/// extra arguments may turn it off; an explicit `--params-backend` disables it).
fn auto_fit(config: &LaunchConfig) -> bool {
    let off = match config.params.get("auto_fit") {
        Some(ParamValue::Text(t)) => t.trim().eq_ignore_ascii_case("off"),
        Some(ParamValue::Bool(b)) => !b,
        _ => false,
    };
    if off || text_param(&config.params, "params_backend").is_some() {
        return false;
    }
    let custom = config
        .custom_args
        .as_deref()
        .and_then(|s| crate::process::split_args(s).ok())
        .unwrap_or_default();
    let flag = |a: &str| a.split_once('=').map_or(a, |(f, _)| f).to_string();
    !custom
        .iter()
        .enumerate()
        .any(|(i, a)| match flag(a).as_str() {
            "--params-backend" => true,
            "--auto-fit" => a
                .split_once('=')
                .map(|(_, v)| v)
                .or_else(|| custom.get(i + 1).map(String::as_str))
                .is_some_and(|v| v.eq_ignore_ascii_case("off")),
            _ => false,
        })
}

/// Whether the build at `binary` has a GPU backend (`None`: its backend is not recognised).
fn build_has_gpu(binary: &Path) -> Option<bool> {
    binary
        .parent()
        .and_then(crate::runtime::detect_backend_in_dir)
        .map(|b| b != "cpu")
}

/// [`build_has_gpu`] of the installed stable-diffusion.cpp runtime (`None`: none installed or
/// unknown).
pub fn installed_build_has_gpu() -> Option<bool> {
    let binary = crate::runtimes::resolve_family_binary_in(
        &crate::runtimes::RuntimeRegistry::global(),
        SD_CPP,
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

// --- release archive naming ---
//
// Windows:  sd-master-<commit>-bin-win-<cpu|vulkan|cuda12|rocm-<v>>-x64.zip
//           cudart-sd-bin-win-cu12-x64.zip (the CUDA 12 runtime, a companion of `cuda12`)
// Linux:    sd-master-<commit>-bin-Linux-Ubuntu-24.04-x86_64[-vulkan|-rocm-<v>].zip
// macOS:    sd-master-<commit>-bin-Darwin-macOS-<v>-arm64.zip (Metal)

fn is_x86_64() -> bool {
    cfg!(target_arch = "x86_64")
}

/// `(archive [+ companion], backend)` of the first of `backends` that has an archive for this
/// OS / arch.
fn pick_assets(
    assets: &[ReleaseAsset],
    backends: &[String],
) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
    let find = |pattern: &str| -> Option<(&ReleaseAsset, Option<String>)> {
        let re = regex::Regex::new(pattern).ok()?;
        let mut hits: Vec<(&ReleaseAsset, Option<String>)> = assets
            .iter()
            .filter_map(|a| {
                let c = re.captures(&a.name)?;
                Some((a, c.get(1).map(|m| m.as_str().to_string())))
            })
            .collect();
        // Several versions of a backend (ROCm): the lowest, the most compatible.
        hits.sort_by(|a, b| a.0.name.cmp(&b.0.name));
        hits.into_iter().next()
    };
    for backend in backends {
        let picked: Option<(&ReleaseAsset, Option<String>)> = if cfg!(windows) && is_x86_64() {
            match backend.as_str() {
                "cpu" => find(r"^sd-.+-bin-win-cpu-x64\.zip$"),
                "vulkan" => find(r"^sd-.+-bin-win-vulkan-x64\.zip$"),
                "cuda" => find(r"^sd-.+-bin-win-cuda(\d+)-x64\.zip$"),
                "rocm" | "hip" => find(r"^sd-.+-bin-win-rocm-(\d+(?:\.\d+)*)-x64\.zip$"),
                _ => None,
            }
        } else if cfg!(target_os = "linux") && is_x86_64() {
            let base = r"^sd-.+-bin-Linux-[^-]+-[\d.]+-x86_64";
            match backend.as_str() {
                "cpu" => find(&format!(r"{base}\.zip$")),
                "vulkan" => find(&format!(r"{base}-vulkan\.zip$")),
                "rocm" | "hip" => find(&format!(r"{base}-rocm-(\d+(?:\.\d+)*)\.zip$")),
                _ => None,
            }
        } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
            match backend.as_str() {
                "metal" => find(r"^sd-.+-bin-Darwin-.+-arm64\.zip$"),
                _ => None,
            }
        } else {
            None
        };
        let Some((main, version)) = picked else {
            continue;
        };
        let mut out = vec![main.clone()];
        if backend == "cuda" {
            // The CUDA runtime (cudart, cuBLAS) is a separate archive: `cu12` for `cuda12`.
            if let Some(rt) = version
                .and_then(|v| {
                    let name = format!("cudart-sd-bin-win-cu{v}-x64.zip");
                    assets.iter().find(|a| a.name == name)
                })
                .cloned()
            {
                out.push(rt);
            }
        }
        return Some((out, Some(backend.clone())));
    }
    None
}

/// [`pick_assets`] for a Windows machine whose NVIDIA driver is too old for CUDA 12: CUDA is
/// left out and the next backend (Vulkan) is tried.
fn pick_assets_for_driver(
    assets: &[ReleaseAsset],
    backends: &[String],
    driver: Option<&str>,
) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
    if crate::hardware::windows_cuda12_driver_supported(driver) == Some(false) {
        let rest: Vec<String> = backends.iter().filter(|b| *b != "cuda").cloned().collect();
        pick_assets(assets, &rest)
    } else {
        pick_assets(assets, backends)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::BinaryKey;
    use crate::runtime::AssetIntegrity;

    fn help() -> String {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/sd-server-master-956.help.txt"),
        )
        .unwrap()
    }

    fn caps() -> RuntimeCapabilities {
        crate::capabilities::sdcpp::from_help(BinaryKey::default(), &help())
    }

    /// Releases of leejet/stable-diffusion.cpp as the GitHub API returned them on 2026-10-11.
    fn releases() -> Vec<serde_json::Value> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/github-releases-sdcpp-master-956.json");
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

    fn pick(backends: &[&str]) -> Option<(Vec<String>, Option<String>)> {
        let all = releases();
        let b: Vec<String> = backends.iter().map(|s| s.to_string()).collect();
        pick_assets(&assets_of(&all[0]), &b)
            .map(|(a, b)| (a.into_iter().map(|a| a.name).collect(), b))
    }

    #[test]
    fn engine_identity() {
        let e = SdCppEngine;
        assert_eq!(e.family(), "stable-diffusion.cpp");
        assert_eq!(e.kind(), RuntimeKind::Server);
        assert_eq!(e.release_source().repo, "leejet/stable-diffusion.cpp");
        assert_eq!(e.release_source().binary, "sd-server");
        assert!(e.supports(ModelFormat::Safetensors) && e.supports(ModelFormat::Ckpt));
        assert!(!e.supports(ModelFormat::Gguf) && !e.supports(ModelFormat::GgmlBin));
        assert!(e.as_server().is_some() && e.as_job().is_none());
        assert_eq!(e.version_arg(), "--help");
        assert_eq!(crate::engine::engine(SD_CPP).unwrap().family(), SD_CPP);
    }

    /// Real asset names of `master-956-1b0ba10`: the Windows x64 archives, and the CUDA one
    /// brings its runtime archive.
    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    fn windows_x64_builds() {
        assert_eq!(
            pick(&["vulkan"]),
            Some((
                vec!["sd-master-1b0ba10-bin-win-vulkan-x64.zip".into()],
                Some("vulkan".into())
            ))
        );
        assert_eq!(
            pick(&["cpu"]).unwrap().0,
            vec!["sd-master-1b0ba10-bin-win-cpu-x64.zip"]
        );
        assert_eq!(
            pick(&["cuda"]).unwrap().0,
            vec![
                "sd-master-1b0ba10-bin-win-cuda12-x64.zip",
                "cudart-sd-bin-win-cu12-x64.zip"
            ]
        );
        assert_eq!(
            pick(&["rocm"]).unwrap().0,
            vec!["sd-master-1b0ba10-bin-win-rocm-10.1.0-x64.zip"]
        );
        // The first backend that exists wins; Metal is not a Windows build.
        assert_eq!(
            pick(&["metal", "vulkan", "cpu"]).unwrap().1.as_deref(),
            Some("vulkan")
        );
        assert_eq!(pick(&["metal"]), None);
        // A release without assets matches nothing.
        let empty = serde_json::json!({"tag_name": "master-1-x", "assets": []});
        assert!(pick_assets(&assets_of(&empty), &["vulkan".to_string()]).is_none());
    }

    /// An NVIDIA driver too old for CUDA 12 gets Vulkan, not the CUDA archive.
    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    fn old_driver_falls_back_to_vulkan() {
        let all = releases();
        let assets = assets_of(&all[0]);
        let backends: Vec<String> = vec!["cuda".into(), "vulkan".into(), "cpu".into()];
        let old = pick_assets_for_driver(&assets, &backends, Some("31.0.15.2824")).unwrap();
        assert_eq!(old.1.as_deref(), Some("vulkan"));
        let new = pick_assets_for_driver(&assets, &backends, Some("32.0.16.1742")).unwrap();
        assert_eq!(new.1.as_deref(), Some("cuda"));
        assert_eq!(new.0.len(), 2);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_builds() {
        assert_eq!(
            pick(&["vulkan"]).unwrap().0,
            vec!["sd-master-1b0ba10-bin-Linux-Ubuntu-24.04-x86_64-vulkan.zip"]
        );
        assert_eq!(
            pick(&["cpu"]).unwrap().0,
            vec!["sd-master-1b0ba10-bin-Linux-Ubuntu-24.04-x86_64.zip"]
        );
        assert_eq!(
            pick(&["rocm"]).unwrap().0,
            vec!["sd-master-1b0ba10-bin-Linux-Ubuntu-24.04-x86_64-rocm-10.1.0.zip"]
        );
        assert_eq!(pick(&["cuda"]), None);
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn macos_build_is_metal() {
        assert_eq!(
            pick(&["vulkan", "metal"]).unwrap().0,
            vec!["sd-master-1b0ba10-bin-Darwin-macOS-26.6.2-arm64.zip"]
        );
    }

    fn config() -> LaunchConfig {
        LaunchConfig {
            model_path: "C:/m/sd_turbo.gguf".into(),
            host: "127.0.0.1".into(),
            port: 11466,
            gpu_layers: 999,
            threads: Some(6),
            ..Default::default()
        }
    }

    #[test]
    fn launch_arguments_of_a_checkpoint() {
        let caps = caps();
        let mut cfg = config();
        cfg.params.insert("fa".into(), ParamValue::Bool(true));
        cfg.params
            .insert("cfg_scale".into(), ParamValue::Float(1.0));
        cfg.params.insert("steps".into(), ParamValue::Int(4));
        cfg.params
            .insert("sampling_method".into(), ParamValue::Text("euler_a".into()));
        cfg.params
            .insert("offload_to_cpu".into(), ParamValue::Bool(true));
        cfg.params
            .insert("vae_tiling".into(), ParamValue::Bool(false));
        let out = SdCppEngine
            .build_launch(&cfg, &ServerCaps::default(), Some(&caps))
            .unwrap();
        let a = out.args.join(" ");
        assert!(
            a.starts_with("-m C:/m/sd_turbo.gguf -l 127.0.0.1 --listen-port 11466 -t 6"),
            "{a}"
        );
        assert!(a.contains("--fa") && a.contains("--offload-to-cpu"), "{a}");
        assert!(
            a.contains("--cfg-scale 1") && a.contains("--steps 4"),
            "{a}"
        );
        assert!(a.contains("--sampling-method euler_a"), "{a}");
        assert!(
            !a.contains("--vae-tiling"),
            "an off switch emits nothing: {a}"
        );
        assert!(!a.contains("--backend"), "{a}");
        // The typed settings are built, not reported as skipped.
        assert!(out.skipped.is_empty(), "{:?}", out.skipped);
        // CPU only.
        cfg.gpu_layers = 0;
        let a = SdCppEngine
            .build_launch(&cfg, &ServerCaps::default(), Some(&caps))
            .unwrap()
            .args
            .join(" ");
        assert!(a.contains("--backend cpu"), "{a}");
        // An explicit backend wins over the CPU switch.
        cfg.params
            .insert("backend".into(), ParamValue::Text("vulkan1".into()));
        let a = SdCppEngine
            .build_launch(&cfg, &ServerCaps::default(), Some(&caps))
            .unwrap()
            .args
            .join(" ");
        assert!(
            a.contains("--backend vulkan1") && !a.contains("--backend cpu"),
            "{a}"
        );
    }

    #[test]
    fn a_bundle_launches_with_its_components() {
        let caps = caps();
        let mut cfg = LaunchConfig {
            model_path: "C:/m/flux1-schnell-Q4_K_S.gguf".into(),
            ..config()
        };
        for (id, path) in [
            ("vae", "C:/m/ae.safetensors"),
            ("clip_l", "C:/m/clip_l.safetensors"),
            ("t5xxl", "C:/m/t5xxl_fp8.safetensors"),
            ("lora_model_dir", "C:/m/loras"),
        ] {
            cfg.params.insert(id.into(), ParamValue::Text(path.into()));
        }
        cfg.params
            .insert("clip_g".into(), ParamValue::Text("  ".into()));
        // A GGUF with companions is a standalone diffusion model.
        assert_eq!(main_role(&cfg), MainRole::Diffusion);
        let a = SdCppEngine
            .build_launch(&cfg, &ServerCaps::default(), Some(&caps))
            .unwrap()
            .args
            .join(" ");
        assert!(
            a.starts_with("--diffusion-model C:/m/flux1-schnell-Q4_K_S.gguf -l 127.0.0.1"),
            "{a}"
        );
        assert!(
            a.contains("--vae C:/m/ae.safetensors")
                && a.contains("--clip_l C:/m/clip_l.safetensors")
                && a.contains("--t5xxl C:/m/t5xxl_fp8.safetensors")
                && a.contains("--lora-model-dir C:/m/loras"),
            "{a}"
        );
        assert!(
            !a.contains("--clip_g"),
            "an empty component is not sent: {a}"
        );
        let comps = component_files(&cfg.params);
        assert_eq!(
            comps.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            ["vae", "clip_l", "t5xxl"]
        );
        // An explicit role wins over the guess.
        cfg.params.insert(
            "main_model_role".into(),
            ParamValue::Text("checkpoint".into()),
        );
        assert_eq!(main_role(&cfg), MainRole::Checkpoint);
        let a = SdCppEngine
            .build_launch(&cfg, &ServerCaps::default(), Some(&caps))
            .unwrap()
            .args
            .join(" ");
        assert!(a.starts_with("-m C:/m/flux1"), "{a}");
        // Without a probe nothing from the catalog is sent (the build is unknown).
        let out = SdCppEngine
            .build_launch(&cfg, &ServerCaps::default(), None)
            .unwrap();
        assert!(out.skipped.iter().any(|s| s.reason == "runtime_unknown"));
    }

    #[test]
    fn network_binds_and_protected_flags_are_refused() {
        let caps = caps();
        let mut cfg = config();
        cfg.host = "0.0.0.0".into();
        assert!(SdCppEngine
            .build_launch(&cfg, &ServerCaps::default(), Some(&caps))
            .is_err());
        cfg.host = "127.0.0.1".into();
        for bad in [
            "--listen-port 1",
            "-l 0.0.0.0",
            "--listen-ip=0.0.0.0",
            "-m other.gguf",
            "--diffusion-model x",
            "--serve-html-path .",
            "--port 5",
        ] {
            cfg.custom_args = Some(bad.into());
            assert!(
                SdCppEngine
                    .build_launch(&cfg, &ServerCaps::default(), Some(&caps))
                    .is_err(),
                "{bad}"
            );
        }
        cfg.custom_args = Some("--disable-image-metadata --rng cpu".into());
        let args = SdCppEngine
            .build_launch(&cfg, &ServerCaps::default(), Some(&caps))
            .unwrap()
            .args;
        assert!(args.contains(&"--disable-image-metadata".to_string()));
    }

    #[test]
    fn health_is_the_model_list() {
        let e = SdCppEngine;
        assert_eq!(e.health_path(), "/v1/models");
        assert!(e.health(200, r#"{"data":[{"id":"sd-cpp-local"}]}"#));
        assert!(!e.health(404, ""));
        assert_eq!(e.openai_base("http://127.0.0.1:1/"), "http://127.0.0.1:1");
    }

    #[test]
    fn catalog_flags_exist_in_the_real_help() {
        let caps = caps();
        let cat = catalog();
        let mut ids = std::collections::BTreeSet::new();
        for p in &cat.params {
            assert!(ids.insert(p.id.clone()), "duplicate {}", p.id);
            for f in &p.flags {
                assert!(caps.has_flag(f), "{}: {} not in the real help", p.id, f);
                assert!(
                    !is_protected(f) || p.is_legacy(),
                    "{}: emits protected {}",
                    p.id,
                    f
                );
            }
            assert!(p.label_key.starts_with("sdparam.") && p.help_key.ends_with("_help"));
            for r in p.requires.iter().chain(&p.conflicts) {
                assert!(cat.get(r).is_some(), "{}: unknown reference {}", p.id, r);
            }
            // The kind matches the flag's shape (a switch takes no value).
            let takes_value = caps.flag(&p.flags[0]).unwrap().value_hint.is_some();
            let is_switch = p.kind == crate::params::ParamKind::Bool;
            assert_eq!(takes_value, !is_switch, "{} ({:?})", p.id, p.kind);
            // The enum values the catalog lists are real ones where the help lists them.
            if let Some(info) = caps.flag(&p.flags[0]).filter(|i| !i.enum_values.is_empty()) {
                for v in &p.values {
                    if p.values_from_help {
                        assert!(
                            info.enum_values.contains(v),
                            "{}: {} not in {:?}",
                            p.id,
                            v,
                            info.enum_values
                        );
                    }
                }
            }
        }
        // Every component the engine knows is a path param with the flag it is given to.
        for (id, flag) in COMPONENTS {
            let spec = cat.get(id).unwrap_or_else(|| panic!("component {id}"));
            assert_eq!(spec.kind, crate::params::ParamKind::Path);
            assert!(spec.flags.iter().any(|f| f == flag), "{id}");
        }
    }

    #[test]
    fn memory_estimate() {
        let budget = MemoryBudget {
            vram_bytes: 8 << 30,
            ram_bytes: 16 << 30,
            ..Default::default()
        };
        let input = FitInput {
            model_size_bytes: 2 << 30, // SD-Turbo q8 and its parts
            gpu_layers: 999,
            ..Default::default()
        };
        let r = SdCppEngine.estimate_memory(&input, None, &budget, &[]);
        assert_eq!(r.category, FitCategory::FullGpu);
        assert_eq!(r.estimate.vram_bytes, footprint_bytes(2 << 30));
        assert_eq!(r.estimate.ram_bytes, 0);
        assert!(!r.estimate.exact);
        let cpu = FitInput {
            gpu_layers: 0,
            ..input.clone()
        };
        let r = SdCppEngine.estimate_memory(&cpu, None, &budget, &[]);
        assert_eq!(r.estimate.vram_bytes, 0);
        assert_eq!(r.category, FitCategory::CpuHeavy);
        // A FLUX bundle (12 GiB of weights) does not fit an 8 GiB card.
        let flux = FitInput {
            model_size_bytes: 12 << 30,
            gpu_layers: 999,
            ..Default::default()
        };
        assert_eq!(
            SdCppEngine
                .estimate_memory(&flux, None, &budget, &[])
                .category,
            FitCategory::OomRisk
        );
        // Compute buffers are clamped.
        assert_eq!(compute_bytes(0), 256 << 20);
        // SD-Turbo q8 (1.9 GiB of weights) fits a 4 GB card with ~3.3 GiB free, with the
        // admission reserve of an image server on top.
        // (the 4 GB card is NVIDIA: no AMD driver reserve, whatever machine runs the test)
        let reserve = crate::vram::fixed_footprint_headroom_with(4 << 30, 0, 0);
        assert!(footprint_bytes(1929 << 20) + reserve < 3295 << 20);
        assert_eq!(compute_bytes(100 << 30), 6 << 30);
    }

    #[test]
    fn weights_are_the_main_file_and_the_components() {
        let dir = tempfile::tempdir().unwrap();
        let (main, vae, t5) = (
            dir.path().join("m.gguf"),
            dir.path().join("ae.safetensors"),
            dir.path().join("t5.safetensors"),
        );
        std::fs::write(&main, vec![0u8; 1000]).unwrap();
        std::fs::write(&vae, vec![0u8; 300]).unwrap();
        std::fs::write(&t5, vec![0u8; 50]).unwrap();
        let mut cfg = LaunchConfig {
            model_path: main,
            ..config()
        };
        cfg.params.insert(
            "vae".into(),
            ParamValue::Text(vae.to_string_lossy().to_string()),
        );
        cfg.params.insert(
            "t5xxl".into(),
            ParamValue::Text(t5.to_string_lossy().to_string()),
        );
        // A component that is not there counts for nothing instead of failing the estimate.
        cfg.params.insert(
            "clip_l".into(),
            ParamValue::Text(dir.path().join("missing").to_string_lossy().to_string()),
        );
        assert_eq!(weights_bytes(&cfg), 1350);
    }

    /// sd.cpp places the weights itself unless the launch turns `--auto-fit` off or fixes the
    /// placement with `--params-backend`, in the typed settings or the extra arguments.
    #[test]
    fn auto_fit_is_on_unless_the_launch_fixes_the_placement() {
        assert!(auto_fit(&config()));
        let with = |id: &str, v: ParamValue| {
            let mut c = config();
            c.params.insert(id.into(), v);
            auto_fit(&c)
        };
        assert!(!with("auto_fit", ParamValue::Text("off".into())));
        assert!(with("auto_fit", ParamValue::Text("on".into())));
        assert!(!with("auto_fit", ParamValue::Bool(false)));
        assert!(!with("params_backend", ParamValue::Text("cuda0".into())));
        let custom = |args: &str| {
            auto_fit(&LaunchConfig {
                custom_args: Some(args.into()),
                ..config()
            })
        };
        assert!(!custom("--auto-fit off"));
        assert!(!custom("--rng cpu --auto-fit=OFF"));
        assert!(!custom("--params-backend cpu"));
        assert!(custom("--auto-fit on --rng cpu"));
        // Measured (RTX 3050 Ti, 3873 MiB free): Z-Image Turbo Q4_0 + Qwen3-4B + ae, 5.8 GB of
        // weights, loads with auto-fit; its compute buffers and the reserve fit the card.
        let weights = 3_311_730_000 + 2_375_910_000 + 160_000_000;
        let reserve = crate::vram::fixed_footprint_headroom_with(4 << 30, 0, 0);
        assert!(footprint_bytes(weights) > 3873 << 20);
        assert!(compute_bytes(weights) + reserve < 3873 << 20);
    }

    /// Only an sd-server whose help lists `--auto-fit` places the weights itself; an older build
    /// or an unreadable help keeps the full need.
    #[test]
    fn auto_fit_needs_a_build_that_has_it() {
        assert!(supports_auto_fit(&caps()));
        let older = help()
            .lines()
            .filter(|l| !l.contains("--auto-fit"))
            .collect::<Vec<_>>()
            .join("\n");
        let older = crate::capabilities::sdcpp::from_help(BinaryKey::default(), &older);
        assert!(older.help_parsed && !supports_auto_fit(&older));
        let unread = crate::capabilities::sdcpp::from_help(BinaryKey::default(), "");
        assert!(!supports_auto_fit(&unread));
    }

    /// A launch is known to run on the CPU when it says so; a GPU launch of an unknown build
    /// is not.
    #[test]
    fn a_launch_on_the_cpu_is_known() {
        let cpu = LaunchConfig {
            gpu_layers: 0,
            ..config()
        };
        assert!(runs_on_cpu(&cpu));
        let mut backend = config();
        backend
            .params
            .insert("backend".into(), ParamValue::Text("cpu".into()));
        assert!(runs_on_cpu(&backend));
        let unknown = LaunchConfig {
            binary_path: "C:/nowhere/sd-server.exe".into(),
            ..config()
        };
        assert!(!runs_on_cpu(&unknown));
    }

    /// A safetensors file's header tells a checkpoint from a standalone transformer, a VAE and
    /// a text encoder.
    #[test]
    fn safetensors_roles_come_from_the_tensor_names() {
        fn write(path: &Path, names: &[&str]) {
            let mut header = serde_json::Map::new();
            header.insert("__metadata__".into(), serde_json::json!({"format": "pt"}));
            for n in names {
                header.insert(
                    n.to_string(),
                    serde_json::json!({"dtype": "F16", "shape": [1], "data_offsets": [0, 2]}),
                );
            }
            let json = serde_json::to_vec(&header).unwrap();
            let mut bytes = (json.len() as u64).to_le_bytes().to_vec();
            bytes.extend(json);
            bytes.extend([0u8, 0]);
            std::fs::write(path, bytes).unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let p = |n: &str| dir.path().join(n);
        write(
            &p("sd15.safetensors"),
            &[
                "model.diffusion_model.input_blocks.0.0.weight",
                "first_stage_model.decoder.conv_in.weight",
                "cond_stage_model.transformer.text_model.final_layer_norm.bias",
            ],
        );
        write(
            &p("flux.safetensors"),
            &["double_blocks.0.img_attn.qkv.weight", "img_in.weight"],
        );
        write(
            &p("ae.safetensors"),
            &[
                "decoder.conv_in.weight",
                "encoder.down.0.block.0.conv1.weight",
            ],
        );
        write(
            &p("clip_l.safetensors"),
            &["text_model.encoder.layers.0.self_attn.k_proj.weight"],
        );
        write(
            &p("llm.safetensors"),
            &["model.layers.0.self_attn.q_proj.weight", "lm_head.weight"],
        );
        assert_eq!(
            detect_main_role(&p("sd15.safetensors"), false),
            MainRole::Checkpoint
        );
        assert_eq!(
            detect_main_role(&p("flux.safetensors"), true),
            MainRole::Diffusion
        );
        assert_eq!(
            detect_main_role(&p("flux.safetensors"), false),
            MainRole::Diffusion
        );
        assert!(is_image_safetensors(&p("sd15.safetensors")));
        assert!(is_image_safetensors(&p("flux.safetensors")));
        assert!(
            !is_image_safetensors(&p("ae.safetensors")),
            "a VAE is a component"
        );
        assert!(!is_image_safetensors(&p("clip_l.safetensors")));
        assert!(!is_image_safetensors(&p("llm.safetensors")));
        // Not a safetensors file at all.
        std::fs::write(p("bad.safetensors"), b"nope").unwrap();
        assert!(!is_image_safetensors(&p("bad.safetensors")));
        assert_eq!(
            detect_main_role(&p("bad.safetensors"), false),
            MainRole::Checkpoint
        );
        assert_eq!(
            detect_main_role(&p("bad.safetensors"), true),
            MainRole::Diffusion
        );
        // Every `.safetensors` launch runs on sd-server, a GGUF only by its tensors.
        let launch = |path: PathBuf| LaunchConfig {
            model_path: path,
            ..config()
        };
        assert!(is_image_launch(&launch(p("whatever.safetensors"))));
        assert!(is_image_launch(&launch("x.ckpt".into())));
        assert!(!is_image_launch(&launch("qwen3.gguf".into())));
    }

    #[test]
    fn image_architectures() {
        for arch in [
            "flux",
            "sdxl",
            "sd3",
            "wan",
            "qwen_image",
            "qwen_image21",
            "krea2",
            "z_image",
        ] {
            assert!(is_image_arch(arch), "{arch}");
        }
        for arch in ["llama", "qwen3", "gemma3", "bert", "clip"] {
            assert!(!is_image_arch(arch), "{arch}");
        }
        assert!(!is_image_gguf(Path::new("missing.gguf"), None));
    }

    /// Writes a GGUF without metadata (what stable-diffusion.cpp's own conversions look like)
    /// whose tensors are a UNet: it is an image model although it names no architecture.
    #[test]
    fn a_metadata_less_gguf_is_told_by_its_tensors() {
        fn gguf(path: &Path, names: &[&str]) {
            let mut b: Vec<u8> = b"GGUF".to_vec();
            b.extend(3u32.to_le_bytes());
            b.extend((names.len() as u64).to_le_bytes());
            b.extend(0u64.to_le_bytes());
            for n in names {
                b.extend((n.len() as u64).to_le_bytes());
                b.extend(n.as_bytes());
                b.extend(1u32.to_le_bytes()); // dims
                b.extend(4u64.to_le_bytes());
                b.extend(0u32.to_le_bytes()); // f32
                b.extend(0u64.to_le_bytes()); // offset
            }
            b.extend(vec![0u8; 64]);
            std::fs::write(path, b).unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let sd = dir.path().join("sd_turbo-q8.gguf");
        gguf(
            &sd,
            &[
                "cond_stage_model.transformer.text_model.final_layer_norm.bias",
                "model.diffusion_model.input_blocks.0.0.weight",
            ],
        );
        let names = crate::gguf::read_gguf_tensor_names(&sd, 10).unwrap();
        assert_eq!(names.len(), 2);
        let meta = crate::gguf::read_gguf_metadata(&sd).unwrap();
        assert!(meta.architecture.is_none());
        assert!(is_image_gguf(&sd, Some(&meta)));
        let llm = dir.path().join("llm.gguf");
        gguf(&llm, &["token_embd.weight", "blk.0.attn_q.weight"]);
        let meta = crate::gguf::read_gguf_metadata(&llm).unwrap();
        assert!(!is_image_gguf(&llm, Some(&meta)));
        // A launch of the metadata-less GGUF runs on sd-server.
        let cfg = LaunchConfig {
            model_path: sd,
            ..config()
        };
        assert!(is_image_launch(&cfg));
        assert!(!is_image_launch(&LaunchConfig {
            model_path: llm,
            ..config()
        }));
    }
}
