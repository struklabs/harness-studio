//! Devices as the runtime sees them: `llama-server --list-devices` is the primary source
//! (backend name, total and free memory in the backend's own numbering, which is what
//! `--main-gpu`, `--tensor-split` and `--device` refer to). The OS sensors stay the source of
//! live usage: the free value is corrected with the adapter's current usage where the
//! runtime reports a static budget (Vulkan on AMD/Windows prints the same "free" value with
//! and without a loaded model).
//!
//! Results are cached per binary (path + mtime + size) for [`FRESH_FOR`]; a changed binary is
//! probed again. Shared with `harness doctor` / `status` through the Control API.

use crate::capabilities::parse::{parse_list_devices, Device};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Free memory changes as models load; older reports are probed again.
pub const FRESH_FOR: Duration = Duration::from_secs(20);
/// `--list-devices` initializes every backend (Vulkan/CUDA/HIP), which can take a while.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// A GPU device of the runtime.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RuntimeDevice {
    /// `Vulkan0`, `CUDA1`, `ROCm0`, `Metal`, `SYCL0`...
    pub id: String,
    /// Position in the runtime's list (the value `--main-gpu` takes; `--tensor-split` order).
    pub index: u32,
    /// `Vulkan`, `CUDA`, `ROCm`, `Metal`, `SYCL`...
    pub backend: String,
    pub name: String,
    pub total_bytes: Option<u64>,
    /// Free memory as the runtime reports it.
    pub free_bytes: Option<u64>,
    /// Current usage of the matching OS adapter (all processes), when known.
    pub os_used_bytes: Option<u64>,
    /// What a new model can use: the smaller of the runtime's free value and
    /// `total - OS usage` (CUDA on Windows: `total - OS usage` alone, see `available_from`).
    pub available_bytes: Option<u64>,
    /// Integrated GPU (shares system memory); never part of a split suggestion.
    pub integrated: bool,
}

/// Main GPU and tensor split proportional to available memory (two or more discrete GPUs).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SplitSuggestion {
    /// Main GPU as an OS adapter index (`LaunchConfig.main_gpu`; see [`pair_adapters`]).
    pub main_gpu: u32,
    /// The main GPU's runtime device (`CUDA0`, `Vulkan1`...).
    pub main_device: String,
    /// `--tensor-split` over all runtime devices in order (0 for integrated ones), GiB with
    /// one decimal; llama.cpp only uses the proportions.
    pub tensor_split: String,
    /// `--split-mode`.
    pub split_mode: String,
    /// Devices that get a share.
    pub devices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DeviceReport {
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub binary: Option<PathBuf>,
    pub devices: Vec<RuntimeDevice>,
    /// Unix seconds of the `--list-devices` run.
    pub probed_at: u64,
    /// The probe failed (`devices` then come from the capability cache, if any).
    pub error: Option<String>,
    pub split: Option<SplitSuggestion>,
}

impl DeviceReport {
    /// The devices in the capability registry's shape, with the corrected free memory
    /// (Control API `RuntimeSummary.devices`, `harness doctor`).
    pub fn as_capability_devices(&self) -> Vec<Device> {
        const MIB: u64 = 1024 * 1024;
        self.devices
            .iter()
            .map(|d| Device {
                id: d.id.clone(),
                backend: d.backend.clone(),
                name: d.name.clone(),
                total_mib: d.total_bytes.map(|b| b / MIB),
                free_mib: d.available_bytes.or(d.free_bytes).map(|b| b / MIB),
            })
            .collect()
    }
}

fn from_parsed(parsed: Vec<Device>) -> Vec<RuntimeDevice> {
    const MIB: u64 = 1024 * 1024;
    parsed
        .into_iter()
        .enumerate()
        .map(|(i, d)| {
            let total = d.total_mib.map(|m| m * MIB);
            // Apple's Metal device is the whole SoC GPU: the only (and main) device. Newer
            // llama.cpp builds name it `MTL0` (backend "MTL").
            let metal = d.backend == "Metal" || d.backend == "MTL";
            let integrated = !metal && is_integrated_name(&d.name);
            RuntimeDevice {
                index: i as u32,
                id: d.id,
                backend: d.backend,
                total_bytes: total,
                free_bytes: d.free_mib.map(|m| m * MIB),
                available_bytes: d.free_mib.map(|m| m * MIB),
                os_used_bytes: None,
                integrated,
                name: d.name,
            }
        })
        .collect()
}

/// Names of integrated GPUs as the backends print them.
fn is_integrated_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("radeon(tm) graphics")
        || lower.contains("radeon graphics")
        || lower.contains("intel(r) uhd")
        || lower.contains("intel(r) hd")
        || lower.contains("intel(r) iris")
        || lower.contains("intel(r) graphics")
        || lower.contains("(radv gfx1103")
        || lower.contains("(radv phoenix")
        || lower.contains("(radv rembrandt")
        || lower.contains("780m")
        || lower.contains("890m")
        || lower.contains("880m")
}

/// Parses `--list-devices` output into runtime devices.
pub fn parse(text: &str) -> Vec<RuntimeDevice> {
    from_parsed(parse_list_devices(text))
}

fn normalize(name: &str) -> String {
    name.to_lowercase()
        .replace("(tm)", " ")
        .replace("(r)", " ")
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// What a new model can use of a device: the smaller of the runtime's free value and the OS
/// figure (`total - used`). Not on CUDA under Windows (WDDM): the `--list-devices` process does
/// not count its own CUDA context in "free" (measured 3295 MiB against 3962 MiB in nvidia-smi on
/// an RTX 3050 Ti 4 GB), so the OS figure stands alone there. Vulkan / ROCm keep the minimum.
fn available_from(backend: &str, windows: bool, free: Option<u64>, by_os: u64) -> u64 {
    if windows && backend.eq_ignore_ascii_case("CUDA") {
        return by_os;
    }
    free.map_or(by_os, |f| f.min(by_os))
}

/// Pairs runtime devices with OS adapters by name (`os` = `(name, used, total)`; identical
/// names are paired in order) and fills `os_used_bytes` / `available_bytes`.
pub fn apply_os_usage(devices: &mut [RuntimeDevice], os: &[(String, Option<u64>, Option<u64>)]) {
    let mut taken = vec![false; os.len()];
    for d in devices.iter_mut() {
        let dn = normalize(&d.name);
        let Some(i) = os.iter().enumerate().position(|(i, (name, _, _))| {
            let on = normalize(name);
            !taken[i] && !on.is_empty() && (dn.contains(&on) || on.contains(&dn))
        }) else {
            continue;
        };
        taken[i] = true;
        let (_, used, _) = &os[i];
        d.os_used_bytes = *used;
        if let (Some(total), Some(used)) = (d.total_bytes, used) {
            let by_os = total.saturating_sub(*used);
            d.available_bytes = Some(available_from(
                &d.backend,
                cfg!(windows),
                d.free_bytes,
                by_os,
            ));
        }
    }
}

/// OS adapters as `(index, name)` ([`crate::gpu::GpuAdapter::index`], DXGI order on Windows).
pub fn os_adapters() -> Vec<(u32, String)> {
    crate::gpu::list_adapters()
        .into_iter()
        .map(|a| (a.index, a.name))
        .collect()
}

/// Pairs OS adapters with runtime devices by name, as `(adapter index, runtime index)`; identical
/// names are paired in order (two RTX 3090s: adapter order ↔ runtime order).
///
/// `LaunchConfig.main_gpu` is an adapter index (what the GPU picker shows); `--main-gpu`,
/// `--tensor-split` and `--device` take the runtime's. The orders differ on hybrid laptops:
/// DXGI lists the Intel iGPU as 0 and the NVIDIA dGPU as 1, a CUDA build sees only the dGPU
/// (`CUDA0`). Without a pair the index passes through unchanged, both ways (the old behavior).
pub fn pair_adapters(devices: &[RuntimeDevice], adapters: &[(u32, String)]) -> Vec<(u32, u32)> {
    let mut ordered: Vec<&(u32, String)> = adapters.iter().collect();
    ordered.sort_by_key(|(index, _)| *index);
    let mut taken = vec![false; devices.len()];
    let mut pairs = Vec::new();
    for (index, name) in ordered {
        let on = normalize(name);
        if on.is_empty() {
            continue;
        }
        let Some(i) = devices.iter().enumerate().position(|(i, d)| {
            let dn = normalize(&d.name);
            !taken[i] && !dn.is_empty() && (dn.contains(&on) || on.contains(&dn))
        }) else {
            continue;
        };
        taken[i] = true;
        pairs.push((*index, devices[i].index));
    }
    pairs
}

/// Runtime index of adapter `adapter` (see [`pair_adapters`]).
pub fn runtime_index(devices: &[RuntimeDevice], adapters: &[(u32, String)], adapter: u32) -> u32 {
    pair_adapters(devices, adapters)
        .into_iter()
        .find(|&(a, _)| a == adapter)
        .map_or(adapter, |(_, r)| r)
}

/// Adapter index of runtime device `runtime` (see [`pair_adapters`]).
pub fn adapter_index(devices: &[RuntimeDevice], adapters: &[(u32, String)], runtime: u32) -> u32 {
    pair_adapters(devices, adapters)
        .into_iter()
        .find(|&(_, r)| r == runtime)
        .map_or(runtime, |(a, _)| a)
}

/// [`runtime_index`] for `binary`'s devices and the OS adapters. Blocking.
pub fn runtime_index_for_adapter(binary: &Path, adapter: u32) -> Result<u32, String> {
    checked_runtime_index(&report(binary, FRESH_FOR), &os_adapters(), adapter)
}

/// The value `--main-gpu` takes for OS adapter `adapter`, or an error when the runtime would
/// not use that GPU.
///
/// Never interpret an unmatched DXGI index as a backend index: on Optimus, Iris Xe's DXGI 0
/// would otherwise silently select CUDA0 (the NVIDIA GPU). `--main-gpu` also does not index
/// `--list-devices` itself but the devices llama.cpp loads the model on, and that list leaves
/// integrated GPUs out as soon as a discrete one exists (measured: Vulkan0 Iris Xe, Vulkan1
/// RTX 3050 Ti; `--main-gpu 1` fails with "available devices: 1" and `--main-gpu 0` runs
/// on the RTX). So the value is the position among the discrete devices, and an integrated
/// GPU next to a discrete one cannot be selected with `--main-gpu`.
pub fn checked_runtime_index(
    report: &DeviceReport,
    adapters: &[(u32, String)],
    adapter: u32,
) -> Result<u32, String> {
    let pairs = pair_adapters(&report.devices, adapters);
    let paired = pairs
        .iter()
        .find(|(index, _)| *index == adapter)
        .and_then(|(_, runtime)| report.devices.iter().find(|d| d.index == *runtime))
        .or_else(|| vendor_match(report, adapters, &pairs, adapter));
    if let Some(device) = paired {
        let discrete_exists = report.devices.iter().any(|d| !d.integrated);
        if !discrete_exists {
            let mut all: Vec<u32> = report.devices.iter().map(|d| d.index).collect();
            all.sort_unstable();
            return Ok(all.iter().position(|i| *i == device.index).unwrap_or(0) as u32);
        }
        if !device.integrated {
            let mut discrete: Vec<u32> = report
                .devices
                .iter()
                .filter(|d| !d.integrated)
                .map(|d| d.index)
                .collect();
            discrete.sort_unstable();
            return Ok(discrete
                .iter()
                .position(|i| *i == device.index)
                .unwrap_or(0) as u32);
        }
    } else if let Some(error) = &report.error {
        return Err(error.clone());
    }
    let name = adapters
        .iter()
        .find(|(index, _)| *index == adapter)
        .map(|(_, name)| name.as_str())
        .unwrap_or("?");
    Err(crate::error_key::err("runtime.gpu_unavailable")
        .with("name", name)
        .with("index", adapter)
        .into())
}

/// The one unpaired runtime device of the adapter's vendor, for an adapter whose name pairs
/// with nothing: Linux without `lspci` names an AMD card "AMD Radeon GPU" while Vulkan says
/// "AMD Radeon RX 7900 GRE". Two candidates (or none, as for the Intel iGPU of an Optimus
/// laptop next to a CUDA build) stay unmatched.
fn vendor_match<'a>(
    report: &'a DeviceReport,
    adapters: &[(u32, String)],
    pairs: &[(u32, u32)],
    adapter: u32,
) -> Option<&'a RuntimeDevice> {
    let name = normalize(&adapters.iter().find(|(i, _)| *i == adapter)?.1);
    let vendor = ["amd", "radeon", "nvidia", "geforce", "intel", "apple"]
        .into_iter()
        .find(|v| name.split(' ').any(|w| w == *v))?;
    let family = |n: &str| match vendor {
        "amd" | "radeon" => n.contains("amd") || n.contains("radeon"),
        "nvidia" | "geforce" => n.contains("nvidia") || n.contains("geforce") || n.contains("rtx"),
        "apple" => n.contains("apple"),
        _ => n.contains("intel"),
    };
    let mut candidates = report
        .devices
        .iter()
        .filter(|d| !pairs.iter().any(|(_, r)| *r == d.index) && family(&normalize(&d.name)));
    let one = candidates.next()?;
    candidates.next().is_none().then_some(one)
}

/// Suggests `--main-gpu` and `--tensor-split` when two or more discrete devices have known
/// available memory: the device with the most available memory is the main GPU and every
/// device gets a share proportional to its available memory (integrated GPUs get 0).
/// `main_gpu` is the runtime index here; [`report`] turns it into the adapter index.
pub fn suggest_split(devices: &[RuntimeDevice]) -> Option<SplitSuggestion> {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let usable: Vec<&RuntimeDevice> = devices
        .iter()
        .filter(|d| !d.integrated && d.available_bytes.is_some_and(|b| b > 0))
        .collect();
    if usable.len() < 2 {
        return None;
    }
    let main = usable
        .iter()
        .max_by_key(|d| (d.available_bytes.unwrap_or(0), std::cmp::Reverse(d.index)))?;
    let split = devices
        .iter()
        .map(|d| {
            if usable.iter().any(|u| u.id == d.id) {
                let gib = d.available_bytes.unwrap_or(0) as f64 / GIB;
                format!("{:.1}", (gib * 10.0).round() / 10.0)
            } else {
                "0".to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    Some(SplitSuggestion {
        main_gpu: main.index,
        main_device: main.id.clone(),
        tensor_split: split,
        split_mode: "layer".into(),
        devices: usable.iter().map(|d| d.id.clone()).collect(),
    })
}

fn os_usage() -> Vec<(String, Option<u64>, Option<u64>)> {
    crate::gpu::list_adapters()
        .iter()
        .map(|a| {
            let usage = crate::gpu::vram_usage(a);
            (
                a.name.clone(),
                usage.map(|u| u.0),
                usage.map(|u| u.1).or(a.dedicated_vram),
            )
        })
        .collect()
}

/// Runs `binary --list-devices` (no console window, time-limited). Blocking.
pub fn probe(binary: &Path) -> Result<Vec<RuntimeDevice>, String> {
    let cwd = binary.parent().unwrap_or_else(|| Path::new("."));
    let mut cmd = crate::process::quiet_command(binary);
    cmd.arg("--list-devices").current_dir(cwd);
    let out = crate::runtime::run_command_with_timeout(&mut cmd, PROBE_TIMEOUT, 256 * 1024)
        .map_err(|e| {
            String::from(
                crate::error_key::err("process.spawn_failed")
                    .with("program", binary.display())
                    .with("error", e),
            )
        })?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !text.contains("Available devices") {
        return Err(crate::error_key::err("runtime.devices_unexpected_output")
            .with("status", out.status)
            .into());
    }
    Ok(parse(&text))
}

type Key = (PathBuf, Option<std::time::SystemTime>, u64);

fn cache() -> &'static Mutex<HashMap<Key, (Instant, DeviceReport)>> {
    static CACHE: OnceLock<Mutex<HashMap<Key, (Instant, DeviceReport)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn probe_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn key_of(binary: &Path) -> Key {
    let meta = std::fs::metadata(binary).ok();
    (
        binary.to_path_buf(),
        meta.as_ref().and_then(|m| m.modified().ok()),
        meta.map(|m| m.len()).unwrap_or(0),
    )
}

/// Devices of `binary`, from the cache while younger than `max_age` (`Duration::ZERO` =
/// always probe). A failed probe falls back to the capability registry's devices (probe
/// time values) and reports the error. Blocking.
pub fn report(binary: &Path, max_age: Duration) -> DeviceReport {
    let key = key_of(binary);
    let fresh = |c: &HashMap<Key, (Instant, DeviceReport)>| {
        c.get(&key)
            .filter(|(at, _)| at.elapsed() < max_age)
            .map(|(_, r)| r.clone())
    };
    if let Some(hit) = fresh(&cache().lock().unwrap_or_else(|e| e.into_inner())) {
        return hit;
    }
    let _probe = probe_lock().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(hit) = fresh(&cache().lock().unwrap_or_else(|e| e.into_inner())) {
        return hit;
    }
    let (mut devices, error) = match probe(binary) {
        Ok(d) => (d, None),
        Err(e) => (
            crate::capabilities::registry()
                .cached(binary)
                .map(|c| from_parsed(c.devices.clone()))
                .unwrap_or_default(),
            Some(e),
        ),
    };
    apply_os_usage(&mut devices, &os_usage());
    let adapters = os_adapters();
    let report = DeviceReport {
        binary: Some(binary.to_path_buf()),
        split: suggest_split(&devices).map(|mut s| {
            s.main_gpu = adapter_index(&devices, &adapters, s.main_gpu);
            s
        }),
        devices,
        probed_at: crate::capabilities::now_secs(),
        error,
    };
    let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
    c.retain(|k, _| k.0 != key.0);
    c.insert(key, (Instant::now(), report.clone()));
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;

    fn data(name: &str) -> String {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/data")
                .join(name),
        )
        .unwrap()
    }

    #[test]
    fn parses_real_vulkan_output() {
        let d = parse(&data("llama-server-b11236.list-devices.txt"));
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].id, "Vulkan0");
        assert_eq!(d[0].index, 0);
        assert_eq!(d[0].backend, "Vulkan");
        assert_eq!(d[0].total_bytes, Some(16368 * 1024 * 1024));
        assert!(!d[0].integrated);
        assert_eq!(d[1].name, "AMD Radeon(TM) Graphics");
        assert!(d[1].integrated);
        // One discrete GPU: nothing to split.
        assert_eq!(suggest_split(&d), None);
    }

    #[test]
    fn apple_m1_mtl0_is_main_gpu_and_blas_is_ignored() {
        let text =
            "Available devices:\n  MTL0: Apple M1 (5461 MiB, 5461 MiB free)\n  BLAS: Accelerate\n";
        let d = parse(text);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].backend, "MTL");
        assert!(!d[0].integrated);
        let report = DeviceReport {
            devices: d,
            ..Default::default()
        };
        assert_eq!(
            checked_runtime_index(&report, &[(0, "Apple M1 GPU".into())], 0),
            Ok(0)
        );
        assert_eq!(
            checked_runtime_index(&report, &[(0, "Apple Silicon GPU".into())], 0),
            Ok(0)
        );
    }

    #[test]
    fn cuda_on_windows_takes_the_os_figure_not_the_probes_free_value() {
        // RTX 3050 Ti 4 GB laptop, WDDM: probe free 3295 MiB, nvidia-smi 3962 MiB free.
        let (free, by_os) = (3295 * MIB, 3962 * MIB);
        assert_eq!(available_from("CUDA", true, Some(free), by_os), by_os);
        // Vulkan / ROCm (AMD) and CUDA elsewhere keep the minimum.
        assert_eq!(available_from("Vulkan", true, Some(free), by_os), free);
        assert_eq!(available_from("ROCm", true, Some(free), by_os), free);
        assert_eq!(available_from("CUDA", false, Some(free), by_os), free);
        assert_eq!(available_from("CUDA", true, None, by_os), by_os);
    }

    #[test]
    fn os_usage_corrects_static_free_value() {
        // Real numbers: `--list-devices` said 15557 MiB free while a 3.7 GiB model was loaded
        // (Vulkan, RX 7900 GRE); the adapter counter showed the usage.
        let mut d = parse(&data("llama-server-b11236.list-devices.txt"));
        let os = vec![
            (
                "AMD Radeon RX 7900 GRE".to_string(),
                Some(5 * GIB),
                Some(16 * GIB),
            ),
            ("AMD Radeon(TM) Graphics".to_string(), None, None),
        ];
        apply_os_usage(&mut d, &os);
        assert_eq!(d[0].os_used_bytes, Some(5 * GIB));
        assert_eq!(d[0].available_bytes, Some(16368 * 1024 * 1024 - 5 * GIB));
        assert_eq!(d[1].available_bytes, d[1].free_bytes);
        let caps = DeviceReport {
            devices: d,
            ..Default::default()
        }
        .as_capability_devices();
        assert_eq!(caps[0].free_mib, Some(16368 - 5 * 1024));
    }

    #[test]
    fn split_proportional_to_available_memory() {
        // Two discrete CUDA GPUs plus an iGPU through Vulkan, in llama.cpp's print format.
        let text = "Available devices:\n  CUDA0: NVIDIA GeForce RTX 4090 (24080 MiB, 22528 MiB free)\n  CUDA1: NVIDIA GeForce RTX 3090 (24068 MiB, 11264 MiB free)\n  Vulkan2: Intel(R) UHD Graphics 770 (16000 MiB, 15000 MiB free)\n";
        let d = parse(text);
        assert!(d[2].integrated);
        let s = suggest_split(&d).unwrap();
        assert_eq!(s.main_gpu, 0);
        assert_eq!(s.main_device, "CUDA0");
        assert_eq!(s.tensor_split, "22.0,11.0,0");
        assert_eq!(s.devices, vec!["CUDA0", "CUDA1"]);
        assert_eq!(s.split_mode, "layer");

        // The emptier GPU becomes the main one.
        let text = "Available devices:\n  ROCm0: AMD Radeon RX 7900 XTX (24560 MiB, 4096 MiB free)\n  ROCm1: AMD Radeon RX 7900 XTX (24560 MiB, 20480 MiB free)\n";
        let s = suggest_split(&parse(text)).unwrap();
        assert_eq!(s.main_gpu, 1);
        assert_eq!(s.tensor_split, "4.0,20.0");
    }

    #[test]
    fn adapter_and_runtime_indices_pair_by_name() {
        // Real Optimus laptop (RTX 3050 Ti Laptop + Iris Xe, b11327): DXGI lists the iGPU as
        // adapter 0 and the dGPU as 1; the CUDA build sees only the dGPU, as CUDA0.
        let adapters = vec![
            (0, "Intel(R) Iris(R) Xe Graphics".to_string()),
            (1, "NVIDIA GeForce RTX 3050 Ti Laptop GPU".to_string()),
        ];
        let cuda = parse("Available devices:\n  CUDA0: NVIDIA GeForce RTX 3050 Ti Laptop GPU (4095 MiB, 3295 MiB free)\n");
        assert_eq!(pair_adapters(&cuda, &adapters), vec![(1, 0)]);
        let report = DeviceReport {
            devices: cuda.clone(),
            ..Default::default()
        };
        assert_eq!(checked_runtime_index(&report, &adapters, 1), Ok(0));
        assert!(checked_runtime_index(&report, &adapters, 0)
            .unwrap_err()
            .starts_with("runtime.gpu_unavailable"));
        let failed = DeviceReport {
            error: Some("runtime.devices_unexpected_output".into()),
            ..Default::default()
        };
        assert_eq!(
            checked_runtime_index(&failed, &adapters, 1),
            Err("runtime.devices_unexpected_output".into())
        );
        assert_eq!(runtime_index(&cuda, &adapters, 1), 0);
        assert_eq!(adapter_index(&cuda, &adapters, 0), 1);
        // Vulkan lists both, in its own order.
        let vulkan = parse("Available devices:\n  Vulkan0: NVIDIA GeForce RTX 3050 Ti Laptop GPU (4095 MiB, 3900 MiB free)\n  Vulkan1: Intel(R) Iris(R) Xe Graphics (8000 MiB, 7000 MiB free)\n");
        assert_eq!(runtime_index(&vulkan, &adapters, 1), 0);
        assert_eq!(runtime_index(&vulkan, &adapters, 0), 1);
        // `--main-gpu` counts the devices the model loads on: discrete ones only.
        let vulkan = DeviceReport {
            devices: vulkan,
            ..Default::default()
        };
        assert_eq!(checked_runtime_index(&vulkan, &adapters, 1), Ok(0));
        assert!(checked_runtime_index(&vulkan, &adapters, 0)
            .unwrap_err()
            .starts_with("runtime.gpu_unavailable"));
        // Real Optimus order (measured, b11445): Vulkan0 is the Iris Xe, Vulkan1 the RTX.
        let optimus = DeviceReport {
            devices: parse(
                "Available devices:
  Vulkan0: Intel(R) Iris(R) Xe Graphics (8064 MiB, 7000 MiB free)
  Vulkan1: NVIDIA GeForce RTX 3050 Ti Laptop GPU (3961 MiB, 3500 MiB free)
",
            ),
            ..Default::default()
        };
        assert_eq!(checked_runtime_index(&optimus, &adapters, 1), Ok(0));
        assert!(checked_runtime_index(&optimus, &adapters, 0).is_err());
        // Only an integrated GPU: it is the only device the model loads on.
        let igpu_only = DeviceReport {
            devices: parse(
                "Available devices:
  Vulkan0: Intel(R) Iris(R) Xe Graphics (8064 MiB, 7000 MiB free)
",
            ),
            ..Default::default()
        };
        assert_eq!(checked_runtime_index(&igpu_only, &adapters, 0), Ok(0));
        // Linux without `lspci`: the generic "AMD Radeon GPU" is the one AMD runtime device.
        let generic = [(0, "AMD Radeon GPU".to_string())];
        let one_amd = DeviceReport {
            devices: parse(
                "Available devices:
  Vulkan0: AMD Radeon RX 7900 GRE (16368 MiB, 15557 MiB free)
",
            ),
            ..Default::default()
        };
        assert_eq!(checked_runtime_index(&one_amd, &generic, 0), Ok(0));
        // Two AMD devices: which one is unknown, so no guess.
        let two_amd = DeviceReport {
            devices: parse(
                "Available devices:
  Vulkan0: AMD Radeon RX 7900 GRE (16368 MiB, 15557 MiB free)
  Vulkan1: AMD Radeon RX 6600 (8176 MiB, 8000 MiB free)
",
            ),
            ..Default::default()
        };
        assert!(checked_runtime_index(&two_amd, &generic, 0).is_err());
        // No pair: unchanged (unknown names, empty device list).
        assert_eq!(runtime_index(&cuda, &[(3, "Other GPU".into())], 3), 3);
        assert_eq!(adapter_index(&[], &adapters, 2), 2);
    }

    #[test]
    fn identical_gpus_pair_in_order() {
        let adapters = vec![
            (2, "NVIDIA GeForce RTX 3090".to_string()),
            (0, "Intel(R) UHD Graphics 770".to_string()),
            (1, "NVIDIA GeForce RTX 3090".to_string()),
        ];
        let cuda = parse("Available devices:\n  CUDA0: NVIDIA GeForce RTX 3090 (24068 MiB, 23000 MiB free)\n  CUDA1: NVIDIA GeForce RTX 3090 (24068 MiB, 11264 MiB free)\n");
        assert_eq!(pair_adapters(&cuda, &adapters), vec![(1, 0), (2, 1)]);
        // The split suggestion's main GPU (CUDA0, most free memory) is adapter 1.
        let split = suggest_split(&cuda).unwrap();
        assert_eq!(split.main_gpu, 0);
        assert_eq!(adapter_index(&cuda, &adapters, split.main_gpu), 1);
    }

    #[test]
    fn metal_device_is_the_budget() {
        // ggml-metal prints recommendedMaxWorkingSetSize as the device memory.
        let d = parse("Available devices:\n  Metal: Apple M2 Max (49152 MiB, 49152 MiB free)\n");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].backend, "Metal");
        assert!(!d[0].integrated);
        assert_eq!(d[0].total_bytes, Some(48 * GIB));
    }

    /// Real binary: `HS_LLAMA_SERVER=<llama-server> cargo test -p harness-core devices -- --ignored`.
    #[test]
    #[ignore]
    fn real_list_devices() {
        let bin = std::env::var("HS_LLAMA_SERVER").expect("HS_LLAMA_SERVER");
        let r = report(Path::new(&bin), Duration::ZERO);
        println!("{r:#?}");
        assert!(r.error.is_none(), "{:?}", r.error);
        assert!(!r.devices.is_empty());
        let again = report(Path::new(&bin), FRESH_FOR);
        assert_eq!(
            again.probed_at, r.probed_at,
            "second call is served from the cache"
        );
    }
}
