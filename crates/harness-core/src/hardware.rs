//! Machine facts (CPU, RAM, GPUs, drivers) and the llama.cpp backend that suits them.
//!
//! The runtime's own view of the devices (`--list-devices`, real free memory) lives in
//! [`crate::devices`]; this module is the OS view, available before any runtime exists.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use sysinfo::System;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Apple,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct GpuInfo {
    pub index: u32,
    pub name: String,
    pub vendor: GpuVendor,
    pub vram_bytes: Option<u64>,
    pub driver_version: Option<String>,
    pub is_discrete: bool,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DriverCompatibility {
    pub cuda_supported: bool,
    pub cuda_driver_version: Option<String>,
    pub vulkan_supported: bool,
    pub metal_supported: bool,
    /// AMD GPU usable by the official ROCm/HIP build: Windows with an AMD GPU (the release
    /// archive bundles the HIP runtime), Linux with the amdgpu KFD node (`/dev/kfd`).
    #[serde(default)]
    pub rocm_supported: bool,
    /// Running under WSL (CUDA through `/usr/lib/wsl/lib`).
    #[serde(default)]
    pub wsl: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum RecommendedBackend {
    Cuda12,
    Vulkan,
    Metal,
    Cpu,
    /// AMD ROCm/HIP (`llama-*-bin-{win,ubuntu}-rocm-*`).
    Rocm,
    /// Intel oneAPI SYCL (`llama-*-bin-win-sycl-x64`, `-ubuntu-sycl-fp16-x64`).
    Sycl,
}

impl RecommendedBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            RecommendedBackend::Cuda12 => "cuda",
            RecommendedBackend::Vulkan => "vulkan",
            RecommendedBackend::Metal => "metal",
            RecommendedBackend::Cpu => "cpu",
            RecommendedBackend::Rocm => "rocm",
            RecommendedBackend::Sycl => "sycl",
        }
    }

    /// Parses a backend token (`cuda`, `vulkan`, `rocm`, `hip`, `sycl`, `sycl-fp16`...).
    pub fn from_token(token: &str) -> Option<Self> {
        let t = token.trim().to_ascii_lowercase();
        Some(match t.as_str() {
            "cuda" | "cuda12" => RecommendedBackend::Cuda12,
            "vulkan" => RecommendedBackend::Vulkan,
            "metal" => RecommendedBackend::Metal,
            "cpu" => RecommendedBackend::Cpu,
            "rocm" | "hip" => RecommendedBackend::Rocm,
            _ if t.starts_with("sycl") => RecommendedBackend::Sycl,
            _ => return None,
        })
    }

    /// Describes which llama.cpp release asset fits this OS/arch/backend.
    pub fn asset_query(&self, os: &str, arch: &str) -> AssetQuery {
        let os_token = match os {
            "windows" => "win",
            "macos" => "macos",
            _ => "ubuntu",
        };
        let arch_token = if arch == "aarch64" { "arm64" } else { "x64" };
        // On Linux/macOS the plain CPU build carries no backend token; macOS builds include Metal.
        let backend = match (os_token, self) {
            ("macos", _) => None,
            (_, RecommendedBackend::Cuda12) => Some("cuda"),
            (_, RecommendedBackend::Vulkan) => Some("vulkan"),
            (_, RecommendedBackend::Rocm) => Some("rocm"),
            // Linux ships fp16 and fp32 SYCL builds; fp16 is the faster default.
            ("win", RecommendedBackend::Sycl) => Some("sycl"),
            (_, RecommendedBackend::Sycl) => Some("sycl-fp16"),
            ("win", _) => Some("cpu"),
            _ => None,
        };
        AssetQuery {
            os: os_token.to_string(),
            arch: arch_token.to_string(),
            backend: backend.map(String::from),
        }
    }
}

/// Backends the official llama.cpp releases publish for an OS/arch (checked against the
/// GitHub release assets of b11236 and b11270: `win-{cpu,cuda-12.4,cuda-13.4,vulkan,
/// rocm-10.0,sycl,openvino}-x64`, `ubuntu-{x64,cuda-*,vulkan,rocm-10.0,sycl-fp16,sycl-fp32,
/// openvino}-x64`, `macos-{arm64,x64}`).
pub fn official_backends(os: &str, arch: &str) -> &'static [RecommendedBackend] {
    use RecommendedBackend::*;
    match (os, arch) {
        ("windows", "x86_64") | ("linux", "x86_64") => &[Cuda12, Vulkan, Rocm, Sycl, Cpu],
        ("windows", "aarch64") => &[Cuda12, Cpu],
        ("linux", "aarch64") => &[Cuda12, Vulkan, Cpu],
        ("macos", "aarch64") => &[Metal],
        ("macos", _) => &[Cpu],
        _ => &[Cpu],
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetQuery {
    pub os: String,
    pub arch: String,
    /// `None` = plain build without backend token (CPU on Linux, Metal on macOS).
    pub backend: Option<String>,
}

impl AssetQuery {
    /// The CPU-only fallback for the same OS/arch.
    pub fn cpu_fallback(&self) -> AssetQuery {
        AssetQuery {
            backend: if self.os == "win" {
                Some("cpu".to_string())
            } else {
                None
            },
            ..self.clone()
        }
    }

    /// Vulkan build of the same OS/arch (the portable GPU fallback of ROCm/SYCL/CUDA).
    pub fn vulkan_fallback(&self) -> Option<AssetQuery> {
        let gpu = self
            .backend
            .as_deref()
            .is_some_and(|b| b != "vulkan" && b != "cpu");
        (gpu && self.os != "macos").then(|| AssetQuery {
            backend: Some("vulkan".to_string()),
            ..self.clone()
        })
    }
}

/// Why a backend is recommended, for the UI (`reason_key` is an i18n key under `backend.`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BackendAdvice {
    pub recommended: RecommendedBackend,
    /// `backend.reason_nvidia_cuda`, `backend.reason_amd_rocm`, ...
    pub reason_key: String,
    /// GPU the advice is about.
    pub gpu_name: Option<String>,
    /// Other official builds that also run on this GPU.
    pub alternatives: Vec<RecommendedBackend>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SystemHardwareInfo {
    pub os: String,
    pub os_version: String,
    pub arch: String,
    pub cpu_name: String,
    pub cpu_cores: usize,
    pub total_ram_bytes: u64,
    pub available_ram_bytes: u64,
    pub gpus: Vec<GpuInfo>,
    pub primary_gpu_index: Option<u32>,
    pub driver_info: DriverCompatibility,
    pub recommended_backend: RecommendedBackend,
    pub backend_advice: BackendAdvice,
    /// Apple Silicon: memory the GPU may use (Metal `recommendedMaxWorkingSetSize`, estimated
    /// from `hw.memsize` / `iogpu.wired_limit_mb` without Metal; see [`macos_gpu_budget`]).
    pub unified_memory_budget_bytes: Option<u64>,
    /// Intel Mac (x86_64 macOS): no Metal build of llama.cpp; CPU only.
    #[serde(default)]
    pub intel_mac: bool,
}

static HARDWARE_CACHE: std::sync::OnceLock<SystemHardwareInfo> = std::sync::OnceLock::new();

/// Hardware info, detected once per process (GPU/driver probing is expensive).
/// `available_ram_bytes` is refreshed on every call.
pub fn detect_hardware() -> SystemHardwareInfo {
    let mut info = HARDWARE_CACHE.get_or_init(detect_hardware_uncached).clone();
    let mut sys = System::new();
    sys.refresh_memory();
    info.available_ram_bytes = sys.available_memory();
    info
}

fn detect_hardware_uncached() -> SystemHardwareInfo {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();

    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let os_version = System::os_version().unwrap_or_else(|| "unknown".to_string());

    let cpu_name = sys
        .cpus()
        .first()
        .map(|c| c.brand().to_string())
        .unwrap_or_else(|| "Unknown CPU".to_string());
    let cpu_cores = sys.cpus().len();
    let total_ram_bytes = sys.total_memory();
    let available_ram_bytes = sys.available_memory();

    let unified_memory_budget_bytes = if os == "macos" && arch == "aarch64" {
        macos_sysctl_budget().or(Some(macos_gpu_budget(total_ram_bytes, None)))
    } else {
        None
    };
    let mut gpus = detect_gpus();
    if let Some(budget) = unified_memory_budget_bytes {
        for g in gpus.iter_mut().filter(|g| g.vendor == GpuVendor::Apple) {
            g.vram_bytes = Some(budget);
        }
    }
    let primary_gpu_index = gpus.iter().find(|g| g.is_default).map(|g| g.index);
    let driver_info = detect_driver_compatibility(&gpus);
    let backend_advice = advise_backend(&os, &arch, &gpus, &driver_info, &[]);

    SystemHardwareInfo {
        intel_mac: os == "macos" && arch != "aarch64",
        os,
        os_version,
        arch,
        cpu_name,
        cpu_cores,
        total_ram_bytes,
        available_ram_bytes,
        gpus,
        primary_gpu_index,
        driver_info,
        recommended_backend: backend_advice.recommended,
        backend_advice,
        unified_memory_budget_bytes,
    }
}

fn detect_gpus() -> Vec<GpuInfo> {
    let adapters = crate::gpu::list_adapters();
    let primary_idx = adapters.first().map(|a| a.index);
    adapters
        .into_iter()
        .map(|a| {
            let vendor = match a.vendor_id {
                Some(crate::gpu::VENDOR_NVIDIA) => GpuVendor::Nvidia,
                Some(crate::gpu::VENDOR_AMD) => GpuVendor::Amd,
                Some(crate::gpu::VENDOR_INTEL) => GpuVendor::Intel,
                Some(crate::gpu::VENDOR_APPLE) => GpuVendor::Apple,
                _ => parse_gpu_vendor(&a.name),
            };
            let is_default = Some(a.index) == primary_idx;
            GpuInfo {
                index: a.index,
                name: a.name,
                vendor,
                vram_bytes: a.dedicated_vram,
                driver_version: a.driver_version,
                is_discrete: a.is_discrete,
                is_default,
            }
        })
        .collect()
}

pub fn parse_gpu_vendor(name: &str) -> GpuVendor {
    let lower = name.to_lowercase();
    if lower.contains("nvidia")
        || lower.contains("geforce")
        || lower.contains("rtx")
        || lower.contains("gtx")
    {
        GpuVendor::Nvidia
    } else if lower.contains("amd") || lower.contains("radeon") {
        GpuVendor::Amd
    } else if lower.contains("intel")
        || lower
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w == "arc")
        || lower.contains("iris")
    {
        GpuVendor::Intel
    } else if lower.contains("apple") {
        GpuVendor::Apple
    } else {
        GpuVendor::Other(name.to_string())
    }
}

/// AMD RDNA3 / RDNA4 GPU (RX 7000 / RX 9000, Radeon PRO W7000 / AI PRO R9700, gfx11xx /
/// gfx12xx), the generations the official ROCm/HIP builds target.
pub fn is_rdna3_or_newer(name: &str) -> bool {
    let lower = name.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let rx_series = words.windows(2).any(|w| {
        w[0] == "rx"
            && w[1].len() == 4
            && (w[1].starts_with('7') || w[1].starts_with('9'))
            && w[1].bytes().all(|b| b.is_ascii_digit())
    }) || words.iter().any(|w| {
        // "RX7900" spelled together.
        w.len() == 6
            && w.starts_with("rx")
            && (w.as_bytes()[2] == b'7' || w.as_bytes()[2] == b'9')
            && w[2..].bytes().all(|b| b.is_ascii_digit())
    });
    let pro = words
        .iter()
        .any(|w| (w.starts_with("w7") && w.len() == 5) || *w == "r9700");
    let codename = lower.contains("navi 3")
        || lower.contains("navi3")
        || lower.contains("navi 4")
        || lower.contains("navi4");
    let gfx = words
        .iter()
        .any(|w| w.starts_with("gfx11") || w.starts_with("gfx12"));
    rx_series || pro || codename || gfx
}

/// Intel Arc (Alchemist / Battlemage) discrete GPU.
pub fn is_intel_arc(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w == "arc")
        || lower.contains("arc(tm)")
}

/// Picks the backend: CUDA for NVIDIA with a driver, ROCm/HIP for RDNA3/RDNA4 AMD GPUs,
/// SYCL for Intel Arc on Windows (Vulkan on Linux, where SYCL needs the oneAPI runtime),
/// Vulkan for other GPUs, Metal on Apple Silicon, CPU otherwise. Only backends of
/// [`official_backends`] are recommended. `extra_names` are device names the runtime reports
/// (more precise than the OS names on Linux).
pub fn advise_backend(
    os: &str,
    arch: &str,
    gpus: &[GpuInfo],
    driver: &DriverCompatibility,
    extra_names: &[String],
) -> BackendAdvice {
    use RecommendedBackend::*;
    let official = official_backends(os, arch);
    let advice =
        |recommended, key: &str, gpu: Option<&str>, alternatives: Vec<RecommendedBackend>| {
            BackendAdvice {
                recommended,
                reason_key: format!("backend.reason_{key}"),
                gpu_name: gpu.map(str::to_string),
                alternatives: alternatives
                    .into_iter()
                    .filter(|b| official.contains(b) && *b != recommended)
                    .collect(),
            }
        };
    if os == "macos" {
        return if arch == "aarch64" {
            advice(
                Metal,
                "apple_silicon",
                gpus.first().map(|g| g.name.as_str()),
                vec![],
            )
        } else {
            advice(Cpu, "intel_mac", None, vec![])
        };
    }
    let names: Vec<(String, GpuVendor, bool)> = gpus
        .iter()
        .map(|g| (g.name.clone(), g.vendor.clone(), g.is_discrete))
        .chain(extra_names.iter().map(|n| {
            (
                n.clone(),
                parse_gpu_vendor(n),
                crate::gpu::is_discrete_gpu(n, None),
            )
        }))
        .collect();
    let find = |pred: &dyn Fn(&(String, GpuVendor, bool)) -> bool| {
        names.iter().find(|g| pred(g)).map(|g| g.0.as_str())
    };

    if os == "windows"
        && windows_cuda12_driver_supported(driver.cuda_driver_version.as_deref()) == Some(false)
    {
        if let Some(gpu) = find(&|g| g.1 == GpuVendor::Nvidia) {
            return advice(
                if driver.vulkan_supported { Vulkan } else { Cpu },
                "nvidia_driver_old",
                Some(gpu),
                vec![Cuda12],
            );
        }
    }
    if driver.cuda_supported && official.contains(&Cuda12) {
        let gpu = find(&|g| g.1 == GpuVendor::Nvidia);
        return advice(Cuda12, "nvidia_cuda", gpu, vec![Vulkan]);
    }
    if let Some(gpu) = find(&|g| g.1 == GpuVendor::Amd && is_rdna3_or_newer(&g.0)) {
        if driver.rocm_supported && official.contains(&Rocm) {
            return advice(Rocm, "amd_rocm", Some(gpu), vec![Vulkan]);
        }
        if driver.vulkan_supported {
            // Windows: the official ROCm build needs the AMD HIP SDK (hipblas.dll), not /dev/kfd.
            let reason = if os == "windows" {
                "amd_no_hip_sdk"
            } else {
                "amd_no_kfd"
            };
            return advice(Vulkan, reason, Some(gpu), vec![Rocm]);
        }
    }
    if let Some(gpu) = find(&|g| g.1 == GpuVendor::Intel && is_intel_arc(&g.0)) {
        if os == "windows" && official.contains(&Sycl) {
            return advice(Sycl, "intel_arc_sycl", Some(gpu), vec![Vulkan]);
        }
        if driver.vulkan_supported {
            return advice(Vulkan, "intel_arc_vulkan", Some(gpu), vec![Sycl]);
        }
    }
    if !gpus.is_empty() && driver.vulkan_supported {
        let gpu = find(&|g| g.2).or(find(&|_| true));
        let alternatives = if find(&|g| g.1 == GpuVendor::Intel).is_some() {
            vec![Sycl]
        } else {
            vec![]
        };
        return advice(Vulkan, "vulkan", gpu, alternatives);
    }
    advice(Cpu, "no_gpu", None, vec![])
}

/// Windows CUDA 12 requires NVIDIA 528.33; unknown versions preserve DLL detection.
/// Source: https://docs.nvidia.com/cuda/archive/12.4.0/cuda-toolkit-release-notes/index.html,
/// Table 2 "CUDA Toolkit and Minimum Required Driver Version for CUDA Minor Version
/// Compatibility": CUDA 12.x on Windows x86_64 needs >=528.33 (Table 3: 12.4 GA ships >=551.61).
/// CUDA 13.x needs >=580 (13.0 release notes, same Table 2); `runtime::select_asset_for_driver`
/// accepts 13.x packages on Windows only from that driver on.
pub fn windows_cuda12_driver_supported(version: Option<&str>) -> Option<bool> {
    windows_driver_at_least(version, (528, 33))
}

/// `Some(driver >= floor)` for a parseable driver version (WMI or NVIDIA form), else `None`.
pub fn windows_driver_at_least(version: Option<&str>, floor: (u32, u32)) -> Option<bool> {
    let version = version?.trim();
    if version
        .split('.')
        .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let normalized = crate::gpu::nvidia_driver_version(version);
    let version = normalized.as_deref().unwrap_or(version);
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 2
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let major: u32 = parts[0].parse().ok()?;
    let minor: u32 = parts[1].parse().ok()?;
    (minor < 100).then_some((major, minor) >= floor)
}

fn detect_driver_compatibility(gpus: &[GpuInfo]) -> DriverCompatibility {
    let has_nvidia = gpus.iter().any(|g| g.vendor == GpuVendor::Nvidia);
    let has_amd = gpus.iter().any(|g| g.vendor == GpuVendor::Amd);
    let nvidia_driver = || {
        gpus.iter()
            .find(|g| g.vendor == GpuVendor::Nvidia)
            .and_then(|g| g.driver_version.clone())
    };
    let (cuda_supported, rocm_supported, wsl) = platform_drivers(has_nvidia, has_amd);
    let cuda_driver_version =
        nvidia_driver().map(|v| crate::gpu::nvidia_driver_version(&v).unwrap_or(v));
    let cuda_supported = cuda_supported
        && (!cfg!(windows)
            || windows_cuda12_driver_supported(cuda_driver_version.as_deref()) != Some(false));
    DriverCompatibility {
        cuda_supported,
        cuda_driver_version,
        vulkan_supported: check_vulkan_available(),
        metal_supported: cfg!(target_os = "macos"),
        rocm_supported,
        wsl,
    }
}

/// `<SystemRoot>\System32` (Windows may be installed outside `C:\Windows`).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn system32_dir() -> PathBuf {
    system32_of(std::env::var_os("SystemRoot"))
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn system32_of(system_root: Option<std::ffi::OsString>) -> PathBuf {
    system_root
        .filter(|r| !r.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
}

/// `(cuda, rocm, wsl)` of this OS.
#[cfg(target_os = "windows")]
fn platform_drivers(has_nvidia: bool, has_amd: bool) -> (bool, bool, bool) {
    let cuda = has_nvidia && system32_dir().join("nvcuda.dll").exists();
    // The official Windows ROCm build does not bundle hipblas.dll: it needs the AMD HIP SDK.
    let hip = has_amd
        && hip_sdk_present(
            std::env::var("HIP_PATH").ok().as_deref(),
            std::env::var_os("PATH").as_deref(),
            &|p| p.exists(),
        );
    (cuda, hip, false)
}

/// Whether the AMD HIP SDK the official ROCm build needs is installed: `hipblas.dll` (the
/// only DLL `ggml-hip.dll` imports that the build does not bundle) is in `%HIP_PATH%\bin`
/// or a `PATH` directory (no disk scan). `HIP_PATH` alone is not enough: a ROCm install
/// can set it and ship only `libhipblas.dll`.
pub fn hip_sdk_present(
    hip_path: Option<&str>,
    path_var: Option<&std::ffi::OsStr>,
    exists: &dyn Fn(&Path) -> bool,
) -> bool {
    let hip_path = hip_path.map(str::trim).filter(|p| !p.is_empty());
    hip_path.is_some_and(|dir| exists(&Path::new(dir).join("bin").join("hipblas.dll")))
        || path_var.is_some_and(|paths| {
            std::env::split_paths(paths).any(|dir| exists(&dir.join("hipblas.dll")))
        })
}

#[cfg(target_os = "linux")]
fn platform_drivers(has_nvidia: bool, has_amd: bool) -> (bool, bool, bool) {
    let wsl = std::fs::read_to_string("/proc/version")
        .map(|v| is_wsl(&v))
        .unwrap_or(false);
    let driver = linux_cuda_driver(&ldconfig_cache(), &|p| Path::new(p).exists());
    // WSL exposes the Windows driver's GPUs through /usr/lib/wsl/lib, without sysfs/lspci.
    let cuda = driver.is_some() && (has_nvidia || wsl);
    let rocm = has_amd && Path::new("/dev/kfd").exists();
    (cuda, rocm, wsl)
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn platform_drivers(_has_nvidia: bool, _has_amd: bool) -> (bool, bool, bool) {
    (false, false, false)
}

/// `/proc/version` of a WSL kernel (`...-microsoft-standard-WSL2`).
pub fn is_wsl(proc_version: &str) -> bool {
    proc_version.to_lowercase().contains("microsoft")
}

/// Whether `ldconfig -p` output lists a library (`libcuda.so.1 (libc6,x86-64) => /path`).
pub fn ldconfig_has(ldconfig: &str, lib: &str) -> bool {
    ldconfig.lines().any(|l| {
        l.split_whitespace()
            .next()
            .is_some_and(|name| name == lib || name.starts_with(&format!("{lib}.")))
    })
}

/// Where the Linux CUDA driver library is: `ldconfig -p`, the usual distro paths, the WSL
/// driver directory, or a CUDA toolkit under `/usr/local/cuda` (compat driver).
pub fn linux_cuda_driver(ldconfig: &str, exists: &dyn Fn(&str) -> bool) -> Option<String> {
    if ldconfig_has(ldconfig, "libcuda.so") {
        return Some("ldconfig".into());
    }
    [
        "/usr/lib/wsl/lib/libcuda.so.1",
        "/usr/lib/x86_64-linux-gnu/libcuda.so.1",
        "/usr/lib/x86_64-linux-gnu/libcuda.so",
        "/usr/lib64/libcuda.so.1",
        "/usr/lib64/libcuda.so",
        "/usr/lib/aarch64-linux-gnu/libcuda.so.1",
        "/usr/local/cuda/compat/libcuda.so.1",
        "/usr/local/cuda/lib64/libcuda.so.1",
    ]
    .into_iter()
    .find(|p| exists(p))
    .map(str::to_string)
}

#[cfg(target_os = "linux")]
fn ldconfig_cache() -> String {
    for bin in ["ldconfig", "/sbin/ldconfig", "/usr/sbin/ldconfig"] {
        let mut cmd = crate::process::quiet_command(bin);
        cmd.arg("-p");
        if let Ok(out) = crate::runtime::run_command_with_timeout(
            &mut cmd,
            std::time::Duration::from_secs(5),
            4 << 20,
        ) {
            if out.status.success() {
                return String::from_utf8_lossy(&out.stdout).into_owned();
            }
        }
    }
    String::new()
}

fn check_vulkan_available() -> bool {
    #[cfg(target_os = "windows")]
    {
        system32_dir().join("vulkan-1.dll").exists()
    }
    #[cfg(target_os = "linux")]
    {
        ldconfig_has(&ldconfig_cache(), "libvulkan.so")
            || [
                "/usr/lib/x86_64-linux-gnu/libvulkan.so.1",
                "/usr/lib64/libvulkan.so.1",
                "/usr/lib/aarch64-linux-gnu/libvulkan.so.1",
                "/usr/lib/libvulkan.so.1",
            ]
            .iter()
            .any(|p| Path::new(p).exists())
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        false
    }
}

/// GPU share of unified memory on Apple Silicon without asking Metal. `iogpu.wired_limit_mb`
/// (set by the user, macOS 14+) wins; otherwise Metal's `recommendedMaxWorkingSetSize`
/// default is approximated as 2/3 of RAM up to 36 GB and 3/4 above (what Apple Silicon Macs
/// report). The runtime's `--list-devices` (Metal: exact value) is preferred when available.
pub fn macos_gpu_budget(memsize: u64, wired_limit_mb: Option<u64>) -> u64 {
    if let Some(mb) = wired_limit_mb.filter(|&m| m > 0) {
        return (mb * 1024 * 1024).min(memsize);
    }
    if memsize > 36 * 1024 * 1024 * 1024 {
        memsize / 4 * 3
    } else {
        memsize / 3 * 2
    }
}

/// `sysctl -n hw.memsize` and `iogpu.wired_limit_mb` (macOS only).
#[cfg(target_os = "macos")]
fn macos_sysctl_budget() -> Option<u64> {
    let read = |key: &str| -> Option<u64> {
        let out = crate::process::quiet_command("sysctl")
            .args(["-n", key])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    };
    let memsize = read("hw.memsize")?;
    Some(macos_gpu_budget(memsize, read("iogpu.wired_limit_mb")))
}

#[cfg(not(target_os = "macos"))]
fn macos_sysctl_budget() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system32_follows_system_root() {
        assert_eq!(
            system32_of(Some(r"D:\Win".into())),
            PathBuf::from(r"D:\Win").join("System32")
        );
        let default = PathBuf::from(r"C:\Windows").join("System32");
        assert_eq!(system32_of(None), default);
        assert_eq!(system32_of(Some("".into())), default);
    }

    #[test]
    fn windows_cuda12_real_driver_versions() {
        for (raw, expected) in [
            ("32.0.16.1742", true),
            ("31.0.15.5161", true),
            ("31.0.15.2833", true),
            ("31.0.15.2824", false),
            ("30.0.14.7212", false),
            ("617.42", true),
            ("528.24", false),
        ] {
            assert_eq!(
                windows_cuda12_driver_supported(Some(raw)),
                Some(expected),
                "{raw}"
            );
            let mut driver = compat(true, true, false);
            driver.cuda_driver_version = Some(raw.into());
            let advice = advise_backend(
                "windows",
                "x86_64",
                &[gpu("RTX", GpuVendor::Nvidia, true)],
                &driver,
                &[],
            );
            assert_eq!(
                advice.recommended,
                if expected {
                    RecommendedBackend::Cuda12
                } else {
                    RecommendedBackend::Vulkan
                }
            );
            if !expected {
                assert_eq!(advice.reason_key, "backend.reason_nvidia_driver_old");
                assert!(advice.alternatives.contains(&RecommendedBackend::Cuda12));
            }
        }
        for raw in [
            None,
            Some(""),
            Some("broken"),
            Some("32.0.x.1742"),
            Some("x.0.16.1742"),
            Some("528.333"),
        ] {
            assert_eq!(windows_cuda12_driver_supported(raw), None);
            let mut driver = compat(true, true, false);
            driver.cuda_driver_version = raw.map(str::to_owned);
            assert_eq!(
                advise_backend(
                    "windows",
                    "x86_64",
                    &[gpu("RTX", GpuVendor::Nvidia, true)],
                    &driver,
                    &[]
                )
                .recommended,
                RecommendedBackend::Cuda12
            );
        }
    }

    fn gpu(name: &str, vendor: GpuVendor, discrete: bool) -> GpuInfo {
        GpuInfo {
            index: 0,
            name: name.to_string(),
            vendor,
            vram_bytes: Some(16 * 1024 * 1024 * 1024),
            driver_version: None,
            is_discrete: discrete,
            is_default: true,
        }
    }

    fn compat(cuda: bool, vulkan: bool, rocm: bool) -> DriverCompatibility {
        DriverCompatibility {
            cuda_supported: cuda,
            cuda_driver_version: None,
            vulkan_supported: vulkan,
            metal_supported: false,
            rocm_supported: rocm,
            wsl: false,
        }
    }

    #[test]
    fn test_hardware_detection() {
        let info = detect_hardware();
        println!("Detected System Hardware: {:?}", info);
        assert!(!info.os.is_empty());
        assert!(info.total_ram_bytes > 0);
        assert!(info.cpu_cores > 0);
        assert_eq!(info.recommended_backend, info.backend_advice.recommended);
    }

    #[test]
    fn rdna3_and_rdna4_names() {
        for yes in [
            "AMD Radeon RX 7900 GRE",
            "AMD Radeon RX 7600",
            "AMD Radeon RX 9070 XT",
            "AMD Radeon RX 7900 XTX (RADV NAVI31)",
            "Navi 31 [Radeon RX 7900 XT/7900 XTX/7900 GRE/7900M]",
            "AMD Radeon PRO W7900",
            "AMD Radeon AI PRO R9700",
            "gfx1100",
        ] {
            assert!(is_rdna3_or_newer(yes), "{yes}");
        }
        for no in [
            "AMD Radeon RX 6800 XT",
            "AMD Radeon(TM) Graphics",
            "AMD Radeon RX 580",
            "AMD Radeon 780M",
            "NVIDIA GeForce RTX 4090",
        ] {
            assert!(!is_rdna3_or_newer(no), "{no}");
        }
    }

    #[test]
    fn hip_sdk_detection() {
        // Relative dirs: a "C:" drive letter is a separator in a Unix PATH.
        let hip_bin = Path::new("HIP").join("bin");
        let path = std::env::join_paths([Path::new("Tools"), hip_bin.as_path()]).unwrap();
        // HIP_PATH set but no hipblas.dll (ROCm 7.1 ships only libhipblas.dll): not enough.
        assert!(!hip_sdk_present(Some("C:/AMD/ROCm"), None, &|_| false));
        assert!(!hip_sdk_present(Some("C:/AMD/ROCm"), None, &|p| p
            == Path::new("C:/AMD/ROCm").join("bin").join("libhipblas.dll")));
        assert!(hip_sdk_present(Some("C:/AMD/ROCm"), None, &|p| p
            == Path::new("C:/AMD/ROCm").join("bin").join("hipblas.dll")));
        assert!(hip_sdk_present(None, Some(&path), &|p| p == hip_bin.join("hipblas.dll")));
        assert!(!hip_sdk_present(None, Some(&path), &|_| false));
        assert!(!hip_sdk_present(Some("  "), None, &|_| true));
        assert!(!hip_sdk_present(None, None, &|_| true));
    }

    #[test]
    fn advice_per_vendor() {
        let amd = [gpu("AMD Radeon RX 7900 GRE", GpuVendor::Amd, true)];
        let a = advise_backend("windows", "x86_64", &amd, &compat(false, true, true), &[]);
        assert_eq!(a.recommended, RecommendedBackend::Rocm);
        assert_eq!(a.reason_key, "backend.reason_amd_rocm");
        assert_eq!(a.alternatives, vec![RecommendedBackend::Vulkan]);
        assert_eq!(a.gpu_name.as_deref(), Some("AMD Radeon RX 7900 GRE"));

        // Windows without the AMD HIP SDK: Vulkan with the HIP SDK reason; RDNA2 unchanged.
        let a = advise_backend("windows", "x86_64", &amd, &compat(false, true, false), &[]);
        assert_eq!(a.recommended, RecommendedBackend::Vulkan);
        assert_eq!(a.reason_key, "backend.reason_amd_no_hip_sdk");
        assert_eq!(a.alternatives, vec![RecommendedBackend::Rocm]);
        let rdna2 = [gpu("AMD Radeon RX 6800 XT", GpuVendor::Amd, true)];
        let a = advise_backend(
            "windows",
            "x86_64",
            &rdna2,
            &compat(false, true, false),
            &[],
        );
        assert_eq!(a.recommended, RecommendedBackend::Vulkan);
        assert_eq!(a.reason_key, "backend.reason_vulkan");

        // Linux without /dev/kfd: Vulkan, ROCm as the alternative.
        let a = advise_backend("linux", "x86_64", &amd, &compat(false, true, false), &[]);
        assert_eq!(a.recommended, RecommendedBackend::Vulkan);
        assert_eq!(a.alternatives, vec![RecommendedBackend::Rocm]);

        // Older AMD: Vulkan.
        let old = [gpu("AMD Radeon RX 6800 XT", GpuVendor::Amd, true)];
        let a = advise_backend("windows", "x86_64", &old, &compat(false, true, true), &[]);
        assert_eq!(a.recommended, RecommendedBackend::Vulkan);

        // Linux sysfs only knows "AMD Radeon GPU": the runtime's device name decides.
        let generic = [gpu("AMD Radeon GPU", GpuVendor::Amd, true)];
        let names = ["AMD Radeon RX 9070 XT (RADV GFX1201)".to_string()];
        let a = advise_backend(
            "linux",
            "x86_64",
            &generic,
            &compat(false, true, true),
            &names,
        );
        assert_eq!(a.recommended, RecommendedBackend::Rocm);

        let arc = [gpu(
            "Intel(R) Arc(TM) A770 Graphics",
            GpuVendor::Intel,
            true,
        )];
        let a = advise_backend("windows", "x86_64", &arc, &compat(false, true, false), &[]);
        assert_eq!(a.recommended, RecommendedBackend::Sycl);
        assert_eq!(a.alternatives, vec![RecommendedBackend::Vulkan]);
        let a = advise_backend("linux", "x86_64", &arc, &compat(false, true, false), &[]);
        assert_eq!(a.recommended, RecommendedBackend::Vulkan);
        assert_eq!(a.alternatives, vec![RecommendedBackend::Sycl]);

        let nv = [gpu("NVIDIA GeForce RTX 4090", GpuVendor::Nvidia, true)];
        let a = advise_backend("linux", "x86_64", &nv, &compat(true, true, false), &[]);
        assert_eq!(a.recommended, RecommendedBackend::Cuda12);

        let a = advise_backend("macos", "x86_64", &[], &compat(false, false, false), &[]);
        assert_eq!(a.recommended, RecommendedBackend::Cpu);
        assert_eq!(a.reason_key, "backend.reason_intel_mac");
        let a = advise_backend("macos", "aarch64", &[], &compat(false, false, false), &[]);
        assert_eq!(a.recommended, RecommendedBackend::Metal);

        let a = advise_backend("windows", "x86_64", &[], &compat(false, false, false), &[]);
        assert_eq!(a.recommended, RecommendedBackend::Cpu);
    }

    #[test]
    fn asset_queries_match_real_release_names() {
        // Asset names of the real b11236 release (GitHub API, 2026-09-30).
        let names = [
            "llama-b11236-bin-win-rocm-10.0-x64.zip",
            "llama-b11236-bin-win-sycl-x64.zip",
            "llama-b11236-bin-win-vulkan-x64.zip",
            "llama-b11236-bin-win-cpu-x64.zip",
            "llama-b11236-bin-ubuntu-rocm-10.0-x64.tar.gz",
            "llama-b11236-bin-ubuntu-sycl-fp16-x64.tar.gz",
            "llama-b11236-bin-ubuntu-sycl-fp32-x64.tar.gz",
            "llama-b11236-bin-ubuntu-vulkan-x64.tar.gz",
            "llama-b11236-bin-macos-x64.tar.gz",
        ];
        let pick = |b: RecommendedBackend, os: &str, arch: &str| {
            crate::runtime::select_asset(&names, &b.asset_query(os, arch)).map(|(n, _)| n)
        };
        use RecommendedBackend::*;
        assert_eq!(
            pick(Rocm, "windows", "x86_64"),
            Some("llama-b11236-bin-win-rocm-10.0-x64.zip")
        );
        assert_eq!(
            pick(Rocm, "linux", "x86_64"),
            Some("llama-b11236-bin-ubuntu-rocm-10.0-x64.tar.gz")
        );
        assert_eq!(
            pick(Sycl, "windows", "x86_64"),
            Some("llama-b11236-bin-win-sycl-x64.zip")
        );
        assert_eq!(
            pick(Sycl, "linux", "x86_64"),
            Some("llama-b11236-bin-ubuntu-sycl-fp16-x64.tar.gz")
        );
        assert_eq!(
            pick(Cpu, "macos", "x86_64"),
            Some("llama-b11236-bin-macos-x64.tar.gz")
        );
        assert_eq!(RecommendedBackend::from_token("sycl-fp16"), Some(Sycl));
        assert_eq!(RecommendedBackend::from_token("hip"), Some(Rocm));
    }

    /// `ldconfig -p` output in the format glibc prints (Ubuntu 24.04 with the NVIDIA driver).
    const LDCONFIG: &str = "1523 libs found in cache `/etc/ld.so.cache'
\tlibvulkan.so.1 (libc6,x86-64) => /lib/x86_64-linux-gnu/libvulkan.so.1
\tlibcudart.so.12 (libc6,x86-64) => /usr/local/cuda-12.8/targets/x86_64-linux/lib/libcudart.so.12
\tlibcuda.so.1 (libc6,x86-64) => /lib/x86_64-linux-gnu/libcuda.so.1
\tlibcuda.so (libc6,x86-64) => /lib/x86_64-linux-gnu/libcuda.so
";

    #[test]
    fn linux_cuda_detection_sources() {
        assert!(ldconfig_has(LDCONFIG, "libcuda.so"));
        assert!(ldconfig_has(LDCONFIG, "libvulkan.so"));
        assert!(!ldconfig_has(LDCONFIG, "libamdhip64.so"));
        // libcudart alone (toolkit, no driver) is not the driver.
        assert!(!ldconfig_has(
            "\tlibcudart.so.12 (libc6,x86-64) => /x\n",
            "libcuda.so"
        ));
        assert_eq!(
            linux_cuda_driver(LDCONFIG, &|_| false).as_deref(),
            Some("ldconfig")
        );
        // WSL: no ldconfig entry, driver in /usr/lib/wsl/lib.
        let wsl = linux_cuda_driver("", &|p| p == "/usr/lib/wsl/lib/libcuda.so.1");
        assert_eq!(wsl.as_deref(), Some("/usr/lib/wsl/lib/libcuda.so.1"));
        let toolkit = linux_cuda_driver("", &|p| p.starts_with("/usr/local/cuda/compat"));
        assert!(toolkit.is_some());
        assert_eq!(linux_cuda_driver("", &|_| false), None);
        assert!(is_wsl(
            "Linux version 5.15.167.4-microsoft-standard-WSL2 (root@f9c826d3017f) (gcc (GCC) 11.2.0)"
        ));
        assert!(!is_wsl(
            "Linux version 6.8.0-45-generic (buildd@lcy02-amd64-075)"
        ));
    }

    #[test]
    fn macos_budget_rule() {
        const GB: u64 = 1024 * 1024 * 1024;
        assert_eq!(macos_gpu_budget(16 * GB, None), 16 * GB / 3 * 2);
        assert_eq!(macos_gpu_budget(64 * GB, None), 48 * GB);
        assert_eq!(macos_gpu_budget(64 * GB, Some(56 * 1024)), 56 * GB);
        assert_eq!(macos_gpu_budget(16 * GB, Some(0)), 16 * GB / 3 * 2);
    }
}
