//! GPU enumeration and system-wide VRAM usage, per platform (the OS view; the runtime's
//! view with real free memory is [`crate::devices`]).
//!
//! * Windows: adapters via DXGI (exact 64-bit dedicated VRAM, no 4 GB WMI cap);
//!   live usage via the vendor-agnostic `GPU Adapter Memory` performance counters,
//!   falling back to `nvidia-smi`; per-process usage via `GPU Process Memory`.
//! * Linux: `nvidia-smi` lists NVIDIA cards, [`crate::nvml`] reads their memory and the
//!   per-process usage (`nvidia-smi` without NVML); DRM sysfs (`/sys/class/drm/cardN/device`) for AMD and
//!   Intel, named through `lspci -mm`; VRAM usage from amdgpu `mem_info_vram_*`; load from
//!   `gpu_busy_percent` (amdgpu) and temperature from hwmon; per-process usage from DRM
//!   `fdinfo` (`drm-memory-vram`, kernel `drm-usage-stats`).
//! * macOS: Apple Silicon (unified memory, see `hardware::macos_gpu_budget`) or, on Intel
//!   Macs, the adapters `system_profiler SPDisplaysDataType` lists.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuAdapter {
    pub index: u32,
    pub name: String,
    pub vendor_id: Option<u32>,
    pub dedicated_vram: Option<u64>,
    pub driver_version: Option<String>,
    /// Stable NVIDIA identity used internally; deliberately omitted from the IPC shape.
    #[serde(skip)]
    uuid: Option<String>,
    /// Linux DRM device directory (`/sys/class/drm/cardN/device`).
    #[serde(skip)]
    sysfs: Option<PathBuf>,
    /// Windows adapter LUID, used to match performance counter instances.
    pub luid: Option<(u32, i32)>,
    pub is_discrete: bool,
}

impl GpuAdapter {
    /// Linux DRM device directory of this adapter.
    pub fn sysfs_dir(&self) -> Option<&Path> {
        self.sysfs.as_deref()
    }
}

pub const VENDOR_NVIDIA: u32 = 0x10DE;
pub const VENDOR_AMD: u32 = 0x1002;
pub const VENDOR_INTEL: u32 = 0x8086;
pub const VENDOR_APPLE: u32 = 0x106B;

/// lspci names AMD's integrated GPUs after the APU's codename (`Renoir [Radeon RX Vega 6
/// (Ryzen 4000/5000 Mobile Series)]`, `Rembrandt [Radeon 680M]`, `Phoenix1`); the bracketed
/// marketing name may still say "Radeon RX".
const AMD_APU_CODENAMES: &[&str] = &[
    "raven",
    "picasso",
    "dali",
    "pollock",
    "renoir",
    "lucienne",
    "cezanne",
    "barcelo",
    "rembrandt",
    "mendocino",
    "van gogh",
    "raphael",
    "phoenix",
    "hawk point",
    "hawkpoint",
    "granite ridge",
    "strix",
    "krackan",
];

pub fn is_discrete_gpu(name: &str, vram: Option<u64>) -> bool {
    let lower = name.to_lowercase();
    if AMD_APU_CODENAMES.iter().any(|c| lower.starts_with(c)) || lower.contains("(ryzen ") {
        return false;
    }
    if lower.contains("basic display")
        || lower.contains("basic render")
        || lower.contains("intel(r) uhd")
        || lower.contains("intel(r) hd")
        || lower.contains("intel(r) iris")
        || lower.contains("intel(r) graphics")
        || (lower.contains("radeon(tm) graphics") && !lower.contains("rx "))
        || lower.contains("apple")
    {
        return false;
    }
    if let Some(bytes) = vram {
        if bytes >= 2 * 1024 * 1024 * 1024 {
            return true;
        }
    }
    lower.contains("geforce")
        || lower.contains("rtx")
        || lower.contains("gtx")
        || lower.contains("radeon rx")
        || lower.contains("radeon pro")
        || lower.contains("arc ")
        || lower.contains("arc(tm)")
}

/// NVIDIA's driver number from the Windows driver version: the last five digits of the third
/// and fourth parts (`32.0.16.1692` → `616.92`, `32.0.15.8129` → `581.29`).
pub fn nvidia_driver_version(windows: &str) -> Option<String> {
    let parts: Vec<&str> = windows.split('.').collect();
    let [_, _, third, fourth] = parts.as_slice() else {
        return None;
    };
    let (third, fourth) = (third.parse::<u32>().ok()?, fourth.parse::<u32>().ok()?);
    if fourth > 9999 {
        return None;
    }
    let digits = format!("{third}{fourth:04}");
    let last = &digits[digits.len().saturating_sub(5)..];
    if last.len() < 5 {
        return None;
    }
    Some(format!("{}.{}", &last[..3], &last[3..]))
}

/// Lists physical GPUs, prioritized: discrete GPUs with largest dedicated VRAM first.
pub fn list_adapters() -> Vec<GpuAdapter> {
    let mut adapters = platform::list_adapters();
    adapters.sort_by(|a, b| {
        b.is_discrete.cmp(&a.is_discrete).then_with(|| {
            b.dedicated_vram
                .unwrap_or(0)
                .cmp(&a.dedicated_vram.unwrap_or(0))
        })
    });
    adapters
}

/// Returns the physical adapter index of the primary (recommended) GPU.
pub fn primary_gpu_index() -> Option<u32> {
    list_adapters().first().map(|a| a.index)
}

/// System-wide (all processes) dedicated VRAM usage of the given adapter: `(used, total)`.
pub fn vram_usage(adapter: &GpuAdapter) -> Option<(u64, u64)> {
    let _stable_identity = adapter.uuid.as_deref();
    platform::vram_usage(adapter)
}

/// [`vram_usage`] for repeated sampling (the telemetry sampler, once a second): on Windows the
/// performance counter query stays open between samples instead of being opened, resolved and
/// closed every time. Falls back to [`vram_usage`] where no such query exists.
pub struct VramMonitor {
    adapter: GpuAdapter,
    probe: Option<platform::VramProbe>,
    opened: bool,
    processes: Option<ProcessVramProbe>,
    processes_opened: bool,
}

impl VramMonitor {
    pub fn new(adapter: GpuAdapter) -> Self {
        Self {
            adapter,
            probe: None,
            opened: false,
            processes: None,
            processes_opened: false,
        }
    }

    pub fn adapter(&self) -> &GpuAdapter {
        &self.adapter
    }

    /// `(used, total)` dedicated VRAM of the adapter, system-wide.
    pub fn usage(&mut self) -> Option<(u64, u64)> {
        if !self.opened {
            self.opened = true;
            self.probe = platform::VramProbe::open(&self.adapter);
        }
        if let Some(probe) = &mut self.probe {
            if let Some(v) = probe.sample() {
                return Some(v);
            }
        }
        vram_usage(&self.adapter)
    }

    /// Dedicated VRAM of each process on this adapter (`pid -> bytes`; processes that hold
    /// none are absent), one read of the OS counter. `pids` are the processes the caller will
    /// look at (Linux reads only those; Windows lists all in one read). `None` where the OS
    /// has no per-process counter for this GPU: callers must then not split the bar.
    pub fn process_usage(&mut self, pids: &[u32]) -> Option<std::collections::HashMap<u32, u64>> {
        if !self.processes_opened {
            self.processes_opened = true;
            self.processes = ProcessVramProbe::open(&self.adapter);
        }
        self.processes.as_mut()?.sample(pids)
    }
}

/// Splits PDH `GPU Process Memory` instances (`pid_1234_luid_0x..._0x..._phys_0`, lower case)
/// of the adapter with `luid_prefix` into `pid -> bytes`. Several engines of one process on
/// the adapter add up; instances of other adapters are ignored.
pub fn sum_by_pid(
    instances: impl IntoIterator<Item = (String, u64)>,
    luid_prefix: &str,
) -> std::collections::HashMap<u32, u64> {
    let mut out = std::collections::HashMap::new();
    for (name, bytes) in instances {
        let Some((pid, luid)) = parse_gpu_process_instance(&name) else {
            continue;
        };
        if luid == luid_prefix && bytes > 0 {
            *out.entry(pid).or_insert(0u64) += bytes;
        }
    }
    out
}

/// Splits PDH `GPU Process Memory` instances into `pid -> bytes`, adding up every adapter
/// (shared GPU memory is read for the whole machine, not per adapter).
pub fn sum_by_pid_all(
    instances: impl IntoIterator<Item = (String, u64)>,
) -> std::collections::HashMap<u32, u64> {
    let mut out = std::collections::HashMap::new();
    for (name, bytes) in instances {
        let Some((pid, _)) = parse_gpu_process_instance(&name) else {
            continue;
        };
        if bytes > 0 {
            *out.entry(pid).or_insert(0u64) += bytes;
        }
    }
    out
}

/// `pid_1234_luid_0x00000000_0x0000a1b2_phys_0` -> `(1234, "luid_0x00000000_0x0000a1b2")`.
pub fn parse_gpu_process_instance(name: &str) -> Option<(u32, String)> {
    let name = name.trim().to_lowercase();
    let rest = name.strip_prefix("pid_")?;
    let (pid, rest) = rest.split_once('_')?;
    let pid: u32 = pid.parse().ok()?;
    let rest = rest.strip_prefix("luid_")?;
    // luid_<high>_<low>_phys_<n>
    let mut parts = rest.splitn(3, '_');
    let (high, low) = (parts.next()?, parts.next()?);
    if !high.starts_with("0x") || !low.starts_with("0x") {
        return None;
    }
    Some((pid, format!("luid_{high}_{low}")))
}

/// Per-process dedicated VRAM on the monitored adapter, read on the telemetry tick
/// (see [`VramMonitor::process_usage`]).
pub use platform::ProcessVramProbe;

/// Shared GPU memory (system RAM the GPU driver allocated for a process) per process, read
/// on the telemetry tick: Windows `GPU Process Memory\Shared Usage`. Not part of a process's
/// working set, so it is added to the RAM split. Nothing on other platforms.
#[cfg(windows)]
pub use platform::SharedProbe;

#[cfg(not(windows))]
pub struct SharedProbe;

#[cfg(not(windows))]
impl SharedProbe {
    pub fn open() -> Option<Self> {
        None
    }

    pub fn sample(&mut self) -> Option<std::collections::HashMap<u32, u64>> {
        None
    }
}

/// Dedicated GPU memory one process uses (all adapters), as the OS reports it.
/// `None` where the platform has no per-process counter (macOS) or nothing matched.
pub fn process_vram_bytes(pid: u32) -> Option<u64> {
    platform::process_vram_bytes(pid).filter(|&b| b > 0)
}

/// Parses `nvidia-smi --query-gpu=memory.used,memory.total --format=csv,noheader,nounits`
/// output (MiB) and returns the first GPU's `(used, total)` in bytes.
pub fn parse_nvidia_smi_memory(text: &str) -> Option<(u64, u64)> {
    let line = text.lines().find(|l| !l.trim().is_empty())?;
    let mut parts = line.split(',').map(|p| p.trim().parse::<u64>());
    let used = parts.next()?.ok()?;
    let total = parts.next()?.ok()?;
    Some((used * 1024 * 1024, total * 1024 * 1024))
}

/// Sums `nvidia-smi --query-compute-apps=pid,used_memory --format=csv,noheader,nounits`
/// rows (MiB) of one process.
pub fn parse_nvidia_smi_apps(text: &str, pid: u32) -> Option<u64> {
    let mut total = None;
    for line in text.lines() {
        let mut parts = line.split(',').map(str::trim);
        let (Some(p), Some(mib)) = (parts.next(), parts.next()) else {
            continue;
        };
        if p.parse::<u32>().ok() == Some(pid) {
            if let Ok(m) = mib.parse::<u64>() {
                *total.get_or_insert(0) += m * 1024 * 1024;
            }
        }
    }
    total
}

/// VRAM bytes of one DRM client from a `/proc/<pid>/fdinfo/<fd>` file, with its client id
/// (several fds may share one client). Keys per the kernel's `drm-usage-stats`:
/// `drm-memory-vram` (amdgpu), `drm-resident-vram*` / `drm-total-vram*` (xe),
/// `drm-resident-local*` / `drm-total-local*` (discrete i915). A client without any local
/// memory region is an integrated GPU: its buffers live in system RAM, so
/// `drm-resident-system*` counts (i915 on an HD 6000: `drm-resident-system0`, the model and
/// KV of a fully offloaded llama-server). Values carry a `KiB`/`MiB` unit or none (bytes).
pub fn parse_drm_fdinfo(text: &str) -> Option<(String, u64)> {
    parse_drm_fdinfo_in(text, false)
}

/// [`parse_drm_fdinfo`]; `with_gtt` adds the client's GTT (`drm-resident-gtt`, older amdgpu
/// `drm-memory-gtt`). On an AMD APU the BIOS carve-out ("VRAM") is small and a model's buffers
/// live in GTT, system RAM the GPU maps: without it a fully offloaded llama-server shows a few
/// MB. Not for discrete cards, where GTT is staging memory outside the card.
pub fn parse_drm_fdinfo_in(text: &str, with_gtt: bool) -> Option<(String, u64)> {
    let mut client = None;
    let (mut memory, mut resident, mut total) = (None::<u64>, None::<u64>, None::<u64>);
    let mut system = None::<u64>;
    let (mut gtt_memory, mut gtt_resident) = (None::<u64>, None::<u64>);
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key == "drm-client-id" {
            client = Some(value.to_string());
            continue;
        }
        let bytes = || -> Option<u64> {
            let mut it = value.split_whitespace();
            let n: u64 = it.next()?.parse().ok()?;
            Some(match it.next() {
                Some("KiB") => n * 1024,
                Some("MiB") => n * 1024 * 1024,
                Some("GiB") => n * 1024 * 1024 * 1024,
                _ => n,
            })
        };
        let add = |slot: &mut Option<u64>| {
            if let Some(b) = bytes() {
                *slot.get_or_insert(0) += b;
            }
        };
        if key == "drm-memory-vram" {
            add(&mut memory);
        } else if key.starts_with("drm-resident-vram") || key.starts_with("drm-resident-local") {
            add(&mut resident);
        } else if key.starts_with("drm-total-vram") || key.starts_with("drm-total-local") {
            add(&mut total);
        } else if key.starts_with("drm-resident-system") {
            add(&mut system);
        } else if key == "drm-memory-gtt" {
            add(&mut gtt_memory);
        } else if key == "drm-resident-gtt" {
            add(&mut gtt_resident);
        }
    }
    let gtt = if with_gtt {
        gtt_resident.or(gtt_memory)
    } else {
        None
    };
    let bytes = match (memory.or(resident).or(total).or(system), gtt) {
        (None, None) => return None,
        (local, gtt) => local.unwrap_or(0) + gtt.unwrap_or(0),
    };
    Some((client.unwrap_or_default(), bytes))
}

/// PCI slot (without the domain, `03:00.0`) of the adapter a `/proc/<pid>/fdinfo/<fd>` file
/// belongs to (`drm-pdev: 0000:03:00.0`); None when the driver prints none.
pub fn parse_drm_pdev(text: &str) -> Option<String> {
    let value = text
        .lines()
        .find_map(|l| l.strip_prefix("drm-pdev:"))?
        .trim();
    Some(
        value
            .split_once(':')
            .map_or(value, |(_, rest)| rest)
            .to_string(),
    )
}

/// Load (percent) and temperature (Celsius) of a Linux DRM device directory
/// (`/sys/class/drm/cardN/device`): `gpu_busy_percent` (amdgpu) and the hwmon `temp*_input`
/// sensor (millidegrees) labelled `edge` (amdgpu) / `pkg` (xe), else the first one.
pub fn sysfs_gpu_live(device: &Path) -> (Option<f64>, Option<f64>) {
    let read = |p: PathBuf| std::fs::read_to_string(p).ok();
    let load = read(device.join("gpu_busy_percent")).and_then(|s| s.trim().parse::<f64>().ok());
    let mut temp = None;
    if let Ok(dirs) = std::fs::read_dir(device.join("hwmon")) {
        let mut hwmons: Vec<PathBuf> = dirs.flatten().map(|e| e.path()).collect();
        hwmons.sort();
        'outer: for hw in hwmons {
            let mut inputs: Vec<PathBuf> = std::fs::read_dir(&hw)
                .map(|d| {
                    d.flatten()
                        .map(|e| e.path())
                        .filter(|p| {
                            p.file_name()
                                .and_then(|n| n.to_str())
                                .is_some_and(|n| n.starts_with("temp") && n.ends_with("_input"))
                        })
                        .collect()
                })
                .unwrap_or_default();
            inputs.sort();
            let mut first = None;
            for input in inputs {
                let Some(milli) = read(input.clone()).and_then(|s| s.trim().parse::<f64>().ok())
                else {
                    continue;
                };
                let c = milli / 1000.0;
                let name = input.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let label = read(input.with_file_name(name.replace("_input", "_label")))
                    .map(|l| l.trim().to_lowercase());
                if matches!(label.as_deref(), Some("edge") | Some("pkg")) {
                    temp = Some(c);
                    break 'outer;
                }
                first.get_or_insert(c);
            }
            if temp.is_none() {
                temp = first;
            }
            if temp.is_some() {
                break;
            }
        }
    }
    (load, temp)
}

/// Parses `lspci -mm` output: PCI slot (`03:00.0`) -> device name for display controllers
/// (`"VGA compatible controller"`, `"3D controller"`, `"Display controller"`).
/// Format per `lspci(8)`: quoted fields `slot class vendor device [-r..] [-p..] svendor
/// sdevice`.
pub fn parse_lspci_mm(text: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut fields: Vec<String> = Vec::new();
        let mut rest = line.trim();
        // slot is unquoted
        let Some((slot, tail)) = rest.split_once(' ') else {
            continue;
        };
        rest = tail;
        while let Some(start) = rest.find('"') {
            let after = &rest[start + 1..];
            let Some(end) = after.find('"') else {
                break;
            };
            fields.push(after[..end].to_string());
            rest = &after[end + 1..];
        }
        if fields.len() < 3 {
            continue;
        }
        let class = fields[0].to_lowercase();
        if class.contains("vga") || class.contains("3d controller") || class.contains("display") {
            out.push((slot.to_string(), fields[1].clone(), fields[2].clone()));
        }
    }
    out
}

/// Parses `system_profiler SPDisplaysDataType` (Intel Macs): `(name, vendor id, vram bytes,
/// built-in)` per GPU. VRAM lines: `VRAM (Total): 8 GB`, `VRAM (Dynamic, Max): 1536 MB`.
pub fn parse_system_profiler_displays(text: &str) -> Vec<(String, Option<u32>, Option<u64>, bool)> {
    let mut out: Vec<(String, Option<u32>, Option<u64>, bool)> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        let Some((key, value)) = t.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key {
            "Chipset Model" => out.push((value.to_string(), None, None, false)),
            _ if key.starts_with("VRAM") => {
                if let Some(last) = out.last_mut() {
                    let mut it = value.split_whitespace();
                    let n: Option<u64> = it.next().and_then(|n| n.parse().ok());
                    let unit = it.next().unwrap_or("MB");
                    last.2 = n.map(|n| match unit {
                        "GB" => n * 1024 * 1024 * 1024,
                        _ => n * 1024 * 1024,
                    });
                }
            }
            "Vendor" => {
                if let Some(last) = out.last_mut() {
                    let lower = value.to_lowercase();
                    last.1 = if lower.contains("amd") || lower.contains("ati") {
                        Some(VENDOR_AMD)
                    } else if lower.contains("intel") {
                        Some(VENDOR_INTEL)
                    } else if lower.contains("nvidia") {
                        Some(VENDOR_NVIDIA)
                    } else if lower.contains("apple") {
                        Some(VENDOR_APPLE)
                    } else {
                        None
                    };
                }
            }
            "Bus" => {
                if let Some(last) = out.last_mut() {
                    last.3 = value == "Built-In";
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(any(test, target_os = "linux"))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct NvidiaSmiRow {
    index: u32,
    uuid: String,
    name: String,
    driver_version: String,
    total_bytes: u64,
}

#[cfg(any(test, target_os = "linux"))]
fn parse_nvidia_smi_rows(text: &str) -> Vec<NvidiaSmiRow> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split(',').map(str::trim);
            Some(NvidiaSmiRow {
                index: parts.next()?.parse().ok()?,
                uuid: parts.next()?.to_string(),
                name: parts.next()?.to_string(),
                driver_version: parts.next()?.to_string(),
                total_bytes: parts.next()?.parse::<u64>().ok()? * 1024 * 1024,
            })
        })
        .collect()
}

fn nvidia_smi(args: &[&str]) -> Option<String> {
    let out = crate::process::quiet_command("nvidia-smi")
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(target_os = "linux")]
fn query_nvidia_smi_for(index: u32, uuid: Option<&str>) -> Option<(u64, u64)> {
    let selector = uuid.map(str::to_owned).unwrap_or_else(|| index.to_string());
    let id = format!("--id={selector}");
    parse_nvidia_smi_memory(&nvidia_smi(&[
        &id,
        "--query-gpu=memory.used,memory.total",
        "--format=csv,noheader,nounits",
    ])?)
}

#[allow(dead_code)]
fn query_nvidia_smi() -> Option<(u64, u64)> {
    parse_nvidia_smi_memory(&nvidia_smi(&[
        "--query-gpu=memory.used,memory.total",
        "--format=csv,noheader,nounits",
    ])?)
}

#[allow(dead_code)]
fn nvidia_process_vram(pid: u32) -> Option<u64> {
    parse_nvidia_smi_apps(
        &nvidia_smi(&[
            "--query-compute-apps=pid,used_memory",
            "--format=csv,noheader,nounits",
        ])?,
        pid,
    )
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::collections::HashMap;
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIDevice, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
    };
    use windows::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
        PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_LARGE, PDH_HCOUNTER, PDH_HQUERY,
    };

    const PDH_MORE_DATA: u32 = 0x8000_07D2;

    pub fn list_adapters() -> Vec<GpuAdapter> {
        let mut result = Vec::new();
        unsafe {
            let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
                return result;
            };
            let mut i = 0;
            while let Ok(adapter) = factory.EnumAdapters1(i) {
                let adapter_idx = i;
                i += 1;
                let Ok(desc) = adapter.GetDesc1() else {
                    continue;
                };
                if desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                    continue; // Microsoft Basic Render Driver etc.
                }
                let len = desc
                    .Description
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(desc.Description.len());
                let name = String::from_utf16_lossy(&desc.Description[..len]);
                // UMD driver version as a.b.c.d
                let driver_version =
                    adapter
                        .CheckInterfaceSupport(&IDXGIDevice::IID)
                        .ok()
                        .map(|v| {
                            let v = v as u64;
                            format!(
                                "{}.{}.{}.{}",
                                v >> 48,
                                (v >> 32) & 0xFFFF,
                                (v >> 16) & 0xFFFF,
                                v & 0xFFFF
                            )
                        });
                // NVIDIA's own number (616.92) rather than the Windows one (32.0.16.1692).
                let driver_version = match driver_version {
                    Some(v) if desc.VendorId == super::VENDOR_NVIDIA => {
                        Some(super::nvidia_driver_version(&v).unwrap_or(v))
                    }
                    v => v,
                };
                // Skip duplicate LUIDs (same adapter enumerated twice).
                let luid = (desc.AdapterLuid.LowPart, desc.AdapterLuid.HighPart);
                if result.iter().any(|a: &GpuAdapter| a.luid == Some(luid)) {
                    continue;
                }
                let vram = Some(desc.DedicatedVideoMemory as u64).filter(|&v| v > 0);
                let is_discrete = is_discrete_gpu(&name, vram);
                result.push(GpuAdapter {
                    index: adapter_idx,
                    name,
                    vendor_id: Some(desc.VendorId),
                    dedicated_vram: vram,
                    driver_version,
                    uuid: None,
                    sysfs: None,
                    luid: Some(luid),
                    is_discrete,
                });
            }
        }
        result
    }

    pub fn vram_usage(adapter: &GpuAdapter) -> Option<(u64, u64)> {
        let total = adapter.dedicated_vram?;
        if let Some(luid) = adapter.luid {
            if let Some(used) = pdh_sum(ADAPTER_DEDICATED, &adapter_prefix(luid)) {
                return Some((used, total));
            }
        }
        if adapter.vendor_id == Some(VENDOR_NVIDIA) {
            return query_nvidia_smi();
        }
        None
    }

    /// `\GPU Process Memory(pid_<pid>_luid_..._phys_N)\Dedicated Usage`, all adapters.
    pub fn process_vram_bytes(pid: u32) -> Option<u64> {
        pdh_sum(
            "\\GPU Process Memory(*)\\Dedicated Usage",
            &format!("pid_{pid}_"),
        )
    }

    const ADAPTER_DEDICATED: &str = "\\GPU Adapter Memory(*)\\Dedicated Usage";
    const PROCESS_DEDICATED: &str = "\\GPU Process Memory(*)\\Dedicated Usage";
    const PROCESS_SHARED: &str = "\\GPU Process Memory(*)\\Shared Usage";

    /// Sums the instances of a wildcard counter whose name starts with `prefix` (one-shot
    /// query; the telemetry sampler keeps a [`VramProbe`] open instead).
    fn pdh_sum(counter_path: &str, prefix: &str) -> Option<u64> {
        PdhProbe::open(counter_path)?.sum(prefix)
    }

    /// An open PDH query with one wildcard counter. Opening the query and resolving the
    /// counter path is a large part of a sample, so a probe read every second stays open.
    struct PdhProbe {
        query: PDH_HQUERY,
        counter: PDH_HCOUNTER,
    }

    // SAFETY: PDH query handles are not bound to the thread that opened them; a probe is used
    // by one thread at a time (it sits behind a mutex).
    unsafe impl Send for PdhProbe {}

    impl PdhProbe {
        fn open(counter_path: &str) -> Option<Self> {
            unsafe {
                let mut query = PDH_HQUERY::default();
                if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                    return None;
                }
                let path: Vec<u16> = counter_path.encode_utf16().chain(Some(0)).collect();
                let mut counter = PDH_HCOUNTER::default();
                if PdhAddEnglishCounterW(query, PCWSTR(path.as_ptr()), 0, &mut counter) != 0 {
                    PdhCloseQuery(query);
                    return None;
                }
                Some(Self { query, counter })
            }
        }

        /// Collects a fresh sample and sums the instances whose name starts with `prefix`.
        fn sum(&self, prefix: &str) -> Option<u64> {
            let items = self.instances()?;
            let mut matched = items
                .iter()
                .filter(|(name, _)| name.starts_with(prefix))
                .peekable();
            matched.peek()?;
            Some(matched.map(|(_, v)| *v).sum())
        }

        /// Collects a fresh sample: `(lower-case instance name, value)` of every instance
        /// with a valid reading. `None` when the query or the counter is unavailable.
        fn instances(&self) -> Option<Vec<(String, u64)>> {
            unsafe {
                if PdhCollectQueryData(self.query) != 0 {
                    return None;
                }
                let mut size = 0u32;
                let mut count = 0u32;
                let status = PdhGetFormattedCounterArrayW(
                    self.counter,
                    PDH_FMT_LARGE,
                    &mut size,
                    &mut count,
                    None,
                );
                if status != PDH_MORE_DATA || size == 0 {
                    return None;
                }
                // Buffer holds the items followed by their name strings.
                let item_size = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
                let item_count = (size as usize).div_ceil(item_size);
                let mut buf = vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); item_count];
                if PdhGetFormattedCounterArrayW(
                    self.counter,
                    PDH_FMT_LARGE,
                    &mut size,
                    &mut count,
                    Some(buf.as_mut_ptr()),
                ) != 0
                {
                    return None;
                }
                Some(
                    buf.iter()
                        .take(count as usize)
                        .filter(|item| item.FmtValue.CStatus == 0)
                        .map(|item| {
                            (
                                item.szName.to_string().unwrap_or_default().to_lowercase(),
                                item.FmtValue.Anonymous.largeValue.max(0) as u64,
                            )
                        })
                        .collect(),
                )
            }
        }
    }

    impl Drop for PdhProbe {
        fn drop(&mut self) {
            unsafe {
                PdhCloseQuery(self.query);
            }
        }
    }

    fn adapter_prefix(luid: (u32, i32)) -> String {
        format!("luid_0x{:08x}_0x{:08x}", luid.1 as u32, luid.0).to_lowercase()
    }

    /// System-wide dedicated VRAM of one adapter, read from a query that stays open.
    pub struct VramProbe {
        probe: PdhProbe,
        prefix: String,
        total: u64,
    }

    impl VramProbe {
        pub fn open(adapter: &GpuAdapter) -> Option<Self> {
            Some(Self {
                total: adapter.dedicated_vram?,
                prefix: adapter_prefix(adapter.luid?),
                probe: PdhProbe::open(ADAPTER_DEDICATED)?,
            })
        }

        pub fn sample(&mut self) -> Option<(u64, u64)> {
            Some((self.probe.sum(&self.prefix)?, self.total))
        }
    }

    /// Dedicated GPU memory per process on one adapter, from a query that stays open
    /// (`\GPU Process Memory(*)\Dedicated Usage`: one instance per process and adapter).
    pub struct ProcessVramProbe {
        probe: PdhProbe,
        prefix: String,
    }

    impl ProcessVramProbe {
        pub fn open(adapter: &GpuAdapter) -> Option<Self> {
            Some(Self {
                prefix: adapter_prefix(adapter.luid?),
                probe: PdhProbe::open(PROCESS_DEDICATED)?,
            })
        }

        /// `pid -> bytes` of every process using the adapter (the counter lists them all in
        /// one read, so `_pids` does not narrow the work).
        pub fn sample(&mut self, _pids: &[u32]) -> Option<HashMap<u32, u64>> {
            Some(sum_by_pid(self.probe.instances()?, &self.prefix))
        }
    }

    /// Shared GPU memory per process on every adapter, from a query that stays open
    /// (`\GPU Process Memory(*)\Shared Usage`).
    pub struct SharedProbe {
        probe: PdhProbe,
    }

    impl SharedProbe {
        pub fn open() -> Option<Self> {
            Some(Self {
                probe: PdhProbe::open(PROCESS_SHARED)?,
            })
        }

        pub fn sample(&mut self) -> Option<HashMap<u32, u64>> {
            Some(sum_by_pid_all(self.probe.instances()?))
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::collections::HashMap;
    use std::fs;

    /// `/sys/class/drm/cardN/device` of AMD and Intel GPUs (NVIDIA comes from nvidia-smi).
    fn drm_cards() -> Vec<(PathBuf, u32)> {
        let mut cards: Vec<PathBuf> = fs::read_dir("/sys/class/drm")
            .map(|rd| rd.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        cards.retain(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.starts_with("card") && !name.contains('-')
        });
        cards.sort();
        cards
            .into_iter()
            .filter_map(|card| {
                let dev = card.join("device");
                let vendor = fs::read_to_string(dev.join("vendor")).ok()?;
                let vendor =
                    u32::from_str_radix(vendor.trim().trim_start_matches("0x"), 16).ok()?;
                matches!(vendor, VENDOR_AMD | VENDOR_INTEL).then_some((dev, vendor))
            })
            .collect()
    }

    fn read_u64(p: PathBuf) -> Option<u64> {
        fs::read_to_string(p).ok()?.trim().parse().ok()
    }

    /// PCI slot (`03:00.0`) of a DRM device directory (its symlink target `0000:03:00.0`).
    fn pci_slot(dev: &Path) -> Option<String> {
        let target = fs::canonicalize(dev).ok()?;
        let name = target.file_name()?.to_str()?;
        Some(
            name.split_once(':')
                .map_or(name, |(_, rest)| rest)
                .to_string(),
        )
    }

    fn lspci_names() -> HashMap<String, String> {
        let Ok(out) = crate::process::quiet_command("lspci").arg("-mm").output() else {
            return HashMap::new();
        };
        parse_lspci_mm(&String::from_utf8_lossy(&out.stdout))
            .into_iter()
            .map(|(slot, _vendor, device)| (slot, device))
            .collect()
    }

    pub fn list_adapters() -> Vec<GpuAdapter> {
        let mut result = Vec::new();
        let mut idx = 0;

        let nvidia_rows = nvidia_smi(&[
            "--query-gpu=index,uuid,name,driver_version,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .map(|text| parse_nvidia_smi_rows(&text))
        .unwrap_or_default();
        for row in nvidia_rows {
            let is_discrete = is_discrete_gpu(&row.name, Some(row.total_bytes));
            result.push(GpuAdapter {
                index: row.index,
                name: row.name,
                vendor_id: Some(VENDOR_NVIDIA),
                dedicated_vram: Some(row.total_bytes),
                driver_version: Some(row.driver_version),
                uuid: Some(row.uuid),
                sysfs: None,
                luid: None,
                is_discrete,
            });
            idx = idx.max(row.index.saturating_add(1));
        }

        let names = lspci_names();
        for (dev, vendor) in drm_cards() {
            let fallback = if vendor == VENDOR_AMD {
                "AMD Radeon GPU"
            } else {
                "Intel GPU"
            };
            let name = pci_slot(&dev)
                .and_then(|slot| names.get(&slot).cloned())
                .unwrap_or_else(|| fallback.to_string());
            let vram = read_u64(dev.join("mem_info_vram_total"));
            // amdgpu APUs report their BIOS carve-out as VRAM; a "Radeon RX" name or a large
            // pool marks a discrete card.
            let is_discrete = if vendor == VENDOR_AMD {
                is_discrete_gpu(&name, vram)
            } else {
                super::is_discrete_gpu(&name, None)
            };
            result.push(GpuAdapter {
                index: idx,
                name,
                vendor_id: Some(vendor),
                dedicated_vram: vram,
                driver_version: None,
                uuid: None,
                sysfs: Some(dev),
                luid: None,
                is_discrete,
            });
            idx += 1;
        }
        result
    }

    pub fn vram_usage(adapter: &GpuAdapter) -> Option<(u64, u64)> {
        match adapter.vendor_id {
            Some(VENDOR_NVIDIA) => crate::nvml::device(adapter.index, adapter.uuid.as_deref())
                .and_then(|d| d.memory())
                .or_else(|| query_nvidia_smi_for(adapter.index, adapter.uuid.as_deref())),
            Some(VENDOR_AMD) => {
                let dev = adapter.sysfs.as_ref()?;
                let vram = (
                    read_u64(dev.join("mem_info_vram_used"))?,
                    read_u64(dev.join("mem_info_vram_total"))?,
                );
                if adapter.is_discrete {
                    return Some(vram);
                }
                // An APU's GPU memory is its carve-out plus GTT (see `parse_drm_fdinfo_in`).
                let gtt = (
                    read_u64(dev.join("mem_info_gtt_used")).unwrap_or(0),
                    read_u64(dev.join("mem_info_gtt_total")).unwrap_or(0),
                );
                Some((vram.0 + gtt.0, vram.1 + gtt.1))
            }
            _ => None,
        }
    }

    /// NVIDIA through NVML (nvidia-smi without it); AMD/Intel through the DRM fdinfo of the
    /// process.
    pub fn process_vram_bytes(pid: u32) -> Option<u64> {
        if let Some(b) = crate::nvml::process_bytes(pid)
            .filter(|&b| b > 0)
            .or_else(|| nvidia_process_vram(pid))
        {
            return Some(b);
        }
        let dir = format!("/proc/{pid}/fdinfo");
        let mut clients: HashMap<String, u64> = HashMap::new();
        for entry in fs::read_dir(dir).ok()?.flatten() {
            if let Ok(text) = fs::read_to_string(entry.path()) {
                if let Some((client, bytes)) = parse_drm_fdinfo(&text) {
                    let slot = clients.entry(client).or_default();
                    *slot = (*slot).max(bytes);
                }
            }
        }
        (!clients.is_empty()).then(|| clients.values().sum())
    }

    /// No long-lived VRAM query on this platform ([`super::vram_usage`] is used instead).
    pub struct VramProbe;

    impl VramProbe {
        pub fn open(_adapter: &GpuAdapter) -> Option<Self> {
            None
        }

        pub fn sample(&mut self) -> Option<(u64, u64)> {
            None
        }
    }

    /// Per-process VRAM from the DRM `fdinfo` of the given processes (amdgpu, xe, i915). The
    /// proprietary NVIDIA driver has no such file; NVIDIA comes from NVML (in-process, all
    /// processes in one call). Without NVML the bar stays unsplit (`nvidia-smi` would be a
    /// process spawned every sample).
    pub struct ProcessVramProbe {
        /// PCI slot of the adapter: with several cards a descriptor of another card is not
        /// counted (descriptors without `drm-pdev` are).
        pci: Option<String>,
        nvml: Option<crate::nvml::NvmlDevice>,
        /// An AMD APU: GTT counts as GPU memory.
        with_gtt: bool,
    }

    impl ProcessVramProbe {
        pub fn open(adapter: &GpuAdapter) -> Option<Self> {
            match adapter.vendor_id {
                Some(VENDOR_NVIDIA) => crate::nvml::device(adapter.index, adapter.uuid.as_deref())
                    .map(|d| Self {
                        pci: None,
                        nvml: Some(d),
                        with_gtt: false,
                    }),
                Some(VENDOR_AMD | VENDOR_INTEL) => Some(Self {
                    pci: adapter.sysfs_dir().and_then(pci_slot),
                    nvml: None,
                    with_gtt: adapter.vendor_id == Some(VENDOR_AMD) && !adapter.is_discrete,
                }),
                _ => None,
            }
        }

        pub fn sample(&mut self, pids: &[u32]) -> Option<HashMap<u32, u64>> {
            if let Some(d) = &self.nvml {
                return d.process_memory();
            }
            let mut out = HashMap::new();
            for &pid in pids {
                let Ok(dir) = fs::read_dir(format!("/proc/{pid}/fdinfo")) else {
                    continue; // exited, or not ours to read
                };
                let mut clients: HashMap<String, u64> = HashMap::new();
                for entry in dir.flatten() {
                    let Ok(text) = fs::read_to_string(entry.path()) else {
                        continue;
                    };
                    // Only DRM descriptors carry usage keys.
                    if !text.contains("drm-") {
                        continue;
                    }
                    if let (Some(mine), Some(theirs)) = (&self.pci, parse_drm_pdev(&text)) {
                        if *mine != theirs {
                            continue;
                        }
                    }
                    if let Some((client, bytes)) = parse_drm_fdinfo_in(&text, self.with_gtt) {
                        let slot = clients.entry(client).or_default();
                        *slot = (*slot).max(bytes);
                    }
                }
                let total: u64 = clients.values().sum();
                if total > 0 {
                    out.insert(pid, total);
                }
            }
            Some(out)
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::collections::HashMap;

    fn sysctl(key: &str) -> Option<String> {
        let out = crate::process::quiet_command("sysctl")
            .args(["-n", key])
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    pub fn list_adapters() -> Vec<GpuAdapter> {
        if cfg!(target_arch = "aarch64") {
            // "Apple M2 Max": the GPU is part of the SoC; its budget is set in `hardware`.
            let chip = sysctl("machdep.cpu.brand_string").unwrap_or_else(|| "Apple Silicon".into());
            return vec![GpuAdapter {
                index: 0,
                name: format!("{chip} GPU"),
                vendor_id: Some(VENDOR_APPLE),
                dedicated_vram: None,
                driver_version: None,
                uuid: None,
                sysfs: None,
                luid: None,
                is_discrete: false,
            }];
        }
        // Intel Mac: integrated Intel and/or discrete AMD GPUs.
        let Ok(out) = crate::process::quiet_command("system_profiler")
            .args(["SPDisplaysDataType"])
            .output()
        else {
            return Vec::new();
        };
        parse_system_profiler_displays(&String::from_utf8_lossy(&out.stdout))
            .into_iter()
            .enumerate()
            .map(|(i, (name, vendor_id, vram, built_in))| GpuAdapter {
                index: i as u32,
                is_discrete: !built_in && is_discrete_gpu(&name, vram),
                name,
                vendor_id,
                dedicated_vram: vram.filter(|_| !built_in),
                driver_version: None,
                uuid: None,
                sysfs: None,
                luid: None,
            })
            .collect()
    }

    pub fn vram_usage(_adapter: &GpuAdapter) -> Option<(u64, u64)> {
        None
    }

    pub fn process_vram_bytes(_pid: u32) -> Option<u64> {
        None
    }

    /// No long-lived VRAM query on this platform ([`super::vram_usage`] is used instead).
    pub struct VramProbe;

    impl VramProbe {
        pub fn open(_adapter: &GpuAdapter) -> Option<Self> {
            None
        }

        pub fn sample(&mut self) -> Option<(u64, u64)> {
            None
        }
    }

    /// No per-process GPU memory counter on this platform.
    pub struct ProcessVramProbe;

    impl ProcessVramProbe {
        pub fn open(_adapter: &GpuAdapter) -> Option<Self> {
            None
        }

        pub fn sample(&mut self, _pids: &[u32]) -> Option<HashMap<u32, u64>> {
            None
        }
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod platform {
    use super::*;
    use std::collections::HashMap;

    pub fn list_adapters() -> Vec<GpuAdapter> {
        Vec::new()
    }

    pub fn vram_usage(_adapter: &GpuAdapter) -> Option<(u64, u64)> {
        None
    }

    pub fn process_vram_bytes(_pid: u32) -> Option<u64> {
        None
    }

    /// No long-lived VRAM query on this platform ([`super::vram_usage`] is used instead).
    pub struct VramProbe;

    impl VramProbe {
        pub fn open(_adapter: &GpuAdapter) -> Option<Self> {
            None
        }

        pub fn sample(&mut self) -> Option<(u64, u64)> {
            None
        }
    }

    /// No per-process GPU memory counter on this platform.
    pub struct ProcessVramProbe;

    impl ProcessVramProbe {
        pub fn open(_adapter: &GpuAdapter) -> Option<Self> {
            None
        }

        pub fn sample(&mut self, _pids: &[u32]) -> Option<HashMap<u32, u64>> {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_driver_number_from_windows_version() {
        // Real pairs from one laptop (DXGI UMD version vs. nvidia-smi), before and after an
        // update.
        assert_eq!(
            nvidia_driver_version("32.0.16.1692").as_deref(),
            Some("616.92")
        );
        assert_eq!(
            nvidia_driver_version("32.0.15.8129").as_deref(),
            Some("581.29")
        );
        // The fourth part is zero-padded: 31.0.15.0123 → 150123 → 501.23.
        assert_eq!(
            nvidia_driver_version("31.0.15.123").as_deref(),
            Some("501.23")
        );
        assert_eq!(nvidia_driver_version("616.92"), None);
        assert_eq!(nvidia_driver_version("32.0.x.1692"), None);
    }

    #[test]
    fn amd_apu_lspci_names_are_integrated() {
        // `lspci -mm` device names of AMD APUs; Renoir's still says "Radeon RX".
        let mib512 = Some(512 * 1024 * 1024);
        for name in [
            "Renoir [Radeon RX Vega 6 (Ryzen 4000/5000 Mobile Series)]",
            "Cezanne [Radeon Vega Series / Radeon Vega Mobile Series]",
            "Rembrandt [Radeon 680M]",
            "Phoenix1",
            "Strix [Radeon 880M / 890M]",
            "Raven Ridge [Radeon Vega Series / Radeon Vega Mobile Series]",
        ] {
            assert!(!is_discrete_gpu(name, mib512), "{name}");
        }
        // Discrete cards keep their verdict.
        assert!(is_discrete_gpu(
            "Navi 23 [Radeon RX 6600/6600 XT/6600M]",
            None
        ));
        assert!(is_discrete_gpu(
            "Vega 10 XL/XT [Radeon RX Vega 56/64]",
            None
        ));
        assert!(is_discrete_gpu("AMD Radeon RX 7900 XTX", Some(24 << 30)));
        assert!(!is_discrete_gpu("AMD Radeon(TM) Graphics", mib512));
    }

    #[test]
    fn test_parse_nvidia_smi_memory() {
        assert_eq!(
            parse_nvidia_smi_memory("1024, 8192\n"),
            Some((1024 * 1024 * 1024, 8192 * 1024 * 1024))
        );
        assert_eq!(parse_nvidia_smi_memory(""), None);
        assert_eq!(parse_nvidia_smi_memory("N/A, N/A"), None);
    }

    #[test]
    fn test_parse_all_nvidia_smi_rows() {
        let rows = parse_nvidia_smi_rows(
            "0, GPU-aaa, NVIDIA RTX 4090, 555.42, 24564\n2, GPU-bbb, NVIDIA RTX 6000, 555.42, 49140\n",
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].index, 0);
        assert_eq!(rows[1].index, 2);
        assert_eq!(rows[1].uuid, "GPU-bbb");
        assert_eq!(rows[1].total_bytes, 49140 * 1024 * 1024);
    }

    #[test]
    fn nvidia_compute_apps_per_process() {
        // `nvidia-smi --query-compute-apps=pid,used_memory --format=csv,noheader,nounits`
        let text = "4242, 9870\n777, 512\n4242, 100\n";
        assert_eq!(parse_nvidia_smi_apps(text, 4242), Some(9970 * 1024 * 1024));
        assert_eq!(parse_nvidia_smi_apps(text, 1), None);
    }

    /// Format of `Documentation/gpu/drm-usage-stats.rst` as amdgpu prints it (Linux 6.8).
    const AMDGPU_FDINFO: &str = "pos:\t0\nflags:\t02100002\nmnt_id:\t26\nino:\t1073\n\
drm-driver:\tamdgpu\ndrm-client-id:\t41\ndrm-pdev:\t0000:03:00.0\npasid:\t32773\n\
drm-memory-vram:\t9876543 KiB\ndrm-memory-gtt: \t2048 KiB\ndrm-memory-cpu: \t0 KiB\n\
amd-memory-visible-vram:\t9876543 KiB\ndrm-engine-gfx:\t123456789 ns\n";
    /// xe (Intel Arc, Linux 6.10): per-region `drm-total-*` / `drm-resident-*`.
    const XE_FDINFO: &str = "pos:\t0\nflags:\t02100002\ndrm-driver:\txe\ndrm-client-id:\t7\n\
drm-pdev:\t0000:03:00.0\ndrm-total-system:\t0\ndrm-shared-system:\t0\ndrm-active-system:\t0\n\
drm-resident-system:\t0\ndrm-purgeable-system:\t0\ndrm-total-gtt:\t4 KiB\n\
drm-total-vram0:\t6291456 KiB\ndrm-shared-vram0:\t0\ndrm-active-vram0:\t0\n\
drm-resident-vram0:\t6291456 KiB\ndrm-purgeable-vram0:\t0\ndrm-cycles-rcs:\t28257900\n";

    #[test]
    fn drm_fdinfo_vram() {
        assert_eq!(
            parse_drm_fdinfo(AMDGPU_FDINFO),
            Some(("41".to_string(), 9876543 * 1024))
        );
        assert_eq!(
            parse_drm_fdinfo(XE_FDINFO),
            Some(("7".to_string(), 6291456 * 1024))
        );
        assert_eq!(parse_drm_fdinfo("pos:\t0\nflags:\t02\n"), None);
    }

    #[test]
    fn sysfs_live_reads_amdgpu_layout() {
        // Files as amdgpu exposes them under /sys/class/drm/card1/device.
        let dir = tempfile::tempdir().unwrap();
        let dev = dir.path();
        std::fs::write(dev.join("gpu_busy_percent"), "37\n").unwrap();
        let hw = dev.join("hwmon").join("hwmon4");
        std::fs::create_dir_all(&hw).unwrap();
        std::fs::write(hw.join("temp1_input"), "52000\n").unwrap();
        std::fs::write(hw.join("temp1_label"), "edge\n").unwrap();
        std::fs::write(hw.join("temp2_input"), "61000\n").unwrap();
        std::fs::write(hw.join("temp2_label"), "junction\n").unwrap();
        assert_eq!(sysfs_gpu_live(dev), (Some(37.0), Some(52.0)));

        // Intel xe: no busy percent, `pkg` sensor.
        let dir = tempfile::tempdir().unwrap();
        let hw = dir.path().join("hwmon").join("hwmon2");
        std::fs::create_dir_all(&hw).unwrap();
        std::fs::write(hw.join("temp2_input"), "48500\n").unwrap();
        std::fs::write(hw.join("temp2_label"), "pkg\n").unwrap();
        assert_eq!(sysfs_gpu_live(dir.path()), (None, Some(48.5)));

        let empty = tempfile::tempdir().unwrap();
        assert_eq!(sysfs_gpu_live(empty.path()), (None, None));
    }

    #[test]
    fn lspci_machine_readable() {
        // `lspci -mm` (pciutils 3.10) on a desktop with an RX 7900 XTX and an Arc A770.
        let text = r#"00:00.0 "Host bridge" "Advanced Micro Devices, Inc. [AMD]" "Raphael/Granite Ridge Root Complex" "" ""
03:00.0 "VGA compatible controller" "Advanced Micro Devices, Inc. [AMD/ATI]" "Navi 31 [Radeon RX 7900 XT/7900 XTX/7900 GRE/7900M]" -rc8 "Sapphire Technology Limited" "Device 471e"
04:00.0 "VGA compatible controller" "Intel Corporation" "DG2 [Arc A770]" -r08 "Intel Corporation" "Device 1020"
0e:00.0 "Display controller" "Advanced Micro Devices, Inc. [AMD/ATI]" "Granite Ridge [Radeon Graphics]" -rc5 "ASUSTeK Computer Inc." "Device 8877"
"#;
        let gpus = parse_lspci_mm(text);
        assert_eq!(gpus.len(), 3);
        assert_eq!(gpus[0].0, "03:00.0");
        assert_eq!(
            gpus[0].2,
            "Navi 31 [Radeon RX 7900 XT/7900 XTX/7900 GRE/7900M]"
        );
        assert!(crate::hardware::is_rdna3_or_newer(&gpus[0].2));
        assert!(crate::hardware::is_intel_arc(&gpus[1].2));
    }

    #[test]
    fn system_profiler_intel_mac() {
        // `system_profiler SPDisplaysDataType` of a 16-inch MacBook Pro 2019.
        let text = "Graphics/Displays:

    Intel UHD Graphics 630:

      Chipset Model: Intel UHD Graphics 630
      Type: GPU
      Bus: Built-In
      VRAM (Dynamic, Max): 1536 MB
      Vendor: Intel
      Device ID: 0x3e9b
      Revision ID: 0x0002
      Automatic Graphics Switching: Supported
      gMux Version: 5.0.0
      Metal Support: Metal 3

    AMD Radeon Pro 5500M:

      Chipset Model: AMD Radeon Pro 5500M
      Type: GPU
      Bus: PCIe
      PCIe Lane Width: x16
      VRAM (Total): 8 GB
      Vendor: AMD (0x1002)
      Device ID: 0x7340
      Revision ID: 0x0040
      ROM Revision: 113-D3220E-190
      VBIOS Version: 113-D32206U1-019
      Option ROM Version: 113-D32206U1-019
      EFI Driver Version: 01.A1.190
      Automatic Graphics Switching: Supported
      gMux Version: 5.0.0
      Metal Support: Metal 3
";
        let gpus = parse_system_profiler_displays(text);
        assert_eq!(gpus.len(), 2);
        assert_eq!(gpus[0].0, "Intel UHD Graphics 630");
        assert_eq!(gpus[0].1, Some(VENDOR_INTEL));
        assert!(gpus[0].3);
        assert_eq!(gpus[1].1, Some(VENDOR_AMD));
        assert_eq!(gpus[1].2, Some(8 * 1024 * 1024 * 1024));
        assert!(!gpus[1].3);
    }

    #[test]
    fn test_list_adapters_sorted_by_vram() {
        let adapters = list_adapters();
        for pair in adapters.windows(2) {
            assert!(
                (pair[0].is_discrete && !pair[1].is_discrete)
                    || pair[0].dedicated_vram.unwrap_or(0) >= pair[1].dedicated_vram.unwrap_or(0)
            );
        }
        if let Some(a) = adapters.first() {
            println!(
                "Primary GPU: {} VRAM={:?} usage={:?}",
                a.name,
                a.dedicated_vram,
                vram_usage(a)
            );
        }
    }

    /// This process has no GPU allocations; the call must not fail or hang.
    #[test]
    fn process_vram_of_self() {
        let _ = process_vram_bytes(std::process::id());
    }

    #[test]
    fn drm_fdinfo_names_its_adapter() {
        assert_eq!(
            parse_drm_pdev("drm-driver:\tamdgpu\ndrm-pdev:\t0000:03:00.0\ndrm-client-id:\t4\n"),
            Some("03:00.0".to_string())
        );
        assert_eq!(parse_drm_pdev("drm-driver:\ti915\n"), None);
    }

    #[test]
    fn shared_usage_adds_up_every_adapter_per_pid() {
        let instances = vec![
            ("pid_10_luid_0x00000000_0x0000aaaa_phys_0".to_string(), 100),
            ("pid_10_luid_0x00000000_0x0000bbbb_phys_0".to_string(), 30),
            ("pid_11_luid_0x00000000_0x0000aaaa_phys_0".to_string(), 0),
            ("pid_12_luid_0x00000000_0x0000bbbb_phys_0".to_string(), 7),
            ("_total".to_string(), 999),
        ];
        let by_pid = sum_by_pid_all(instances);
        assert_eq!(by_pid.get(&10), Some(&130), "two adapters of one process");
        assert_eq!(by_pid.get(&11), None, "zero is absent");
        assert_eq!(by_pid.get(&12), Some(&7));
        assert_eq!(by_pid.len(), 2, "non-process instances are ignored");
    }

    #[test]
    fn gpu_process_instance_names_parse() {
        assert_eq!(
            parse_gpu_process_instance("pid_1234_luid_0x00000000_0x0000A1B2_phys_0"),
            Some((1234, "luid_0x00000000_0x0000a1b2".to_string()))
        );
        assert_eq!(
            parse_gpu_process_instance("pid_7_luid_0x00000000_0x0000a1b2_phys_0_eng_3_engtype_3d"),
            Some((7, "luid_0x00000000_0x0000a1b2".to_string()))
        );
        for bad in [
            "",
            "luid_0x0_0x1_phys_0",
            "pid_x_luid_0x0_0x1_phys_0",
            "pid_5_phys_0",
            "pid_5_luid_a_b_phys_0",
            "_total",
        ] {
            assert_eq!(parse_gpu_process_instance(bad), None, "{bad}");
        }
    }

    #[test]
    fn process_instances_sum_per_pid_on_one_adapter() {
        let a = "luid_0x00000000_0x0000a1b2";
        let rows = vec![
            (
                "pid_10_luid_0x00000000_0x0000a1b2_phys_0".to_string(),
                100u64,
            ),
            ("pid_10_luid_0x00000000_0x0000a1b2_phys_1".to_string(), 50),
            ("pid_10_luid_0x00000000_0x0000ffff_phys_0".to_string(), 999), // other adapter
            ("pid_20_luid_0x00000000_0x0000a1b2_phys_0".to_string(), 0),   // holds nothing
            ("pid_30_luid_0x00000000_0x0000a1b2_phys_0".to_string(), 7),
            ("garbage".to_string(), 5),
        ];
        let m = sum_by_pid(rows, a);
        assert_eq!(m.get(&10), Some(&150));
        assert_eq!(m.get(&20), None);
        assert_eq!(m.get(&30), Some(&7));
        assert_eq!(m.len(), 2);
    }

    /// amdgpu on a Renoir APU (Ryzen 7 4700U, Linux 7.0), a llama-server with every layer on
    /// the GPU: the model is in GTT, the 512 MiB carve-out holds almost nothing of it.
    #[test]
    fn drm_fdinfo_apu_counts_gtt() {
        let text = "drm-driver:\tamdgpu\ndrm-client-id:\t64\ndrm-pdev:\t0000:05:00.0\n\
drm-total-cpu:\t0\ndrm-resident-cpu:\t0\ndrm-total-gtt:\t407080 KiB\n\
drm-resident-gtt:\t7670244 KiB\ndrm-total-vram:\t7278464 KiB\n\
drm-shared-vram:\t0\ndrm-resident-vram:\t15300 KiB\n";
        assert_eq!(parse_drm_fdinfo(text), Some(("64".into(), 15300 * 1024)));
        assert_eq!(
            parse_drm_fdinfo_in(text, true),
            Some(("64".into(), (15300 + 7670244) * 1024))
        );
        // Older amdgpu keys.
        let old = "drm-client-id:\t3\ndrm-memory-vram:\t2048 KiB\ndrm-memory-gtt:\t1024 KiB\n";
        assert_eq!(
            parse_drm_fdinfo_in(old, true),
            Some(("3".into(), 3072 * 1024))
        );
        // A client with only GTT still counts on an APU.
        assert_eq!(
            parse_drm_fdinfo_in("drm-client-id:\t5\ndrm-resident-gtt:\t10 KiB\n", true),
            Some(("5".into(), 10 * 1024))
        );
        assert_eq!(
            parse_drm_fdinfo("drm-client-id:\t5\ndrm-resident-gtt:\t10 KiB\n"),
            None
        );
    }

    #[test]
    fn drm_fdinfo_integrated_i915_counts_system_memory() {
        // Real fdinfo of llama-server (Qwen2.5-0.5B, -ngl 99) on an HD 6000 (i915, no local
        // memory), shortened.
        let text = "drm-driver:\ti915\ndrm-client-id:\t71\ndrm-pdev:\t0000:00:02.0\n\
drm-total-system0:\t577052 KiB\ndrm-shared-system0:\t0\ndrm-active-system0:\t0\n\
drm-resident-system0:\t576984 KiB\ndrm-purgeable-system0:\t1200 KiB\n\
drm-total-stolen-system0:\t0\ndrm-resident-stolen-system0:\t0\n\
drm-engine-render:\t343168960 ns\n";
        assert_eq!(parse_drm_fdinfo(text), Some(("71".into(), 576_984 * 1024)));
        // With a local region (discrete), system memory is not VRAM.
        let dg2 = "drm-driver:\ti915\ndrm-client-id:\t5\ndrm-resident-system0:\t100 KiB\n\
drm-resident-local0:\t2048 KiB\n";
        assert_eq!(parse_drm_fdinfo(dg2), Some(("5".into(), 2048 * 1024)));
    }

    #[test]
    fn drm_fdinfo_units_and_client_dedup() {
        let text = "pos:\t0\ndrm-driver:\tamdgpu\ndrm-client-id:\t42\ndrm-memory-vram:\t2048 KiB\ndrm-memory-gtt: 10 KiB\n";
        assert_eq!(parse_drm_fdinfo(text), Some(("42".into(), 2 * 1024 * 1024)));
        assert_eq!(parse_drm_fdinfo("pos: 0\nflags: 02\n"), None);
    }
}
