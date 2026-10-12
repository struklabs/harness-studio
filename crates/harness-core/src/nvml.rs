//! NVML (NVIDIA Management Library), loaded at run time from the driver: `libnvidia-ml.so.1`
//! on Linux, `nvml.dll` from System32 on Windows. It answers in-process what `nvidia-smi`
//! answers by spawning a process: the memory of a GPU and the memory of each process on it.
//!
//! * Linux: the telemetry tick splits the VRAM bar per process with it (the proprietary
//!   driver has no DRM `fdinfo`); `nvidia-smi` stays the fallback where NVML cannot load.
//! * Windows: PDH `GPU Process Memory` stays the source. Under WDDM NVML lists the processes
//!   but reports their memory as `NVML_VALUE_NOT_AVAILABLE`, so [`NvmlDevice::process_memory`]
//!   returns `None` there.
//!
//! NVML is initialised once and never shut down; device handles stay valid for the process.

use std::collections::HashMap;
use std::ffi::{c_char, c_uint, c_void, CString};
use std::sync::OnceLock;

type Ret = c_uint;
type Device = *mut c_void;

const SUCCESS: Ret = 0;
const ERROR_INSUFFICIENT_SIZE: Ret = 7;
/// `NVML_VALUE_NOT_AVAILABLE` (`(unsigned long long)-1`).
const VALUE_NOT_AVAILABLE: u64 = u64::MAX;

/// `nvmlProcessInfo_t` of the `_v3` process queries (r510+).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct ProcessInfo {
    pid: c_uint,
    used_gpu_memory: u64,
    gpu_instance_id: c_uint,
    compute_instance_id: c_uint,
}

/// `nvmlMemory_t`.
#[repr(C)]
#[derive(Default)]
struct Memory {
    total: u64,
    free: u64,
    used: u64,
}

type Init = unsafe extern "C" fn() -> Ret;
type Count = unsafe extern "C" fn(*mut c_uint) -> Ret;
type ByIndex = unsafe extern "C" fn(c_uint, *mut Device) -> Ret;
type ByUuid = unsafe extern "C" fn(*const c_char, *mut Device) -> Ret;
type MemoryInfo = unsafe extern "C" fn(Device, *mut Memory) -> Ret;
type ProcessQuery = unsafe extern "C" fn(Device, *mut c_uint, *mut ProcessInfo) -> Ret;

struct Api {
    count: Count,
    by_index: ByIndex,
    by_uuid: ByUuid,
    memory: MemoryInfo,
    compute: ProcessQuery,
    graphics: ProcessQuery,
}

fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(load).as_ref()
}

fn load() -> Option<Api> {
    let lib = platform::open()?;
    let sym = |name: &str| platform::symbol(lib, name);
    // SAFETY: the symbols are NVML's exported C functions with these signatures (nvml.h).
    unsafe {
        use std::mem::transmute;
        type P = *mut c_void;
        let init = transmute::<P, Init>(sym("nvmlInit_v2")?);
        if init() != SUCCESS {
            return None;
        }
        Some(Api {
            count: transmute::<P, Count>(sym("nvmlDeviceGetCount_v2")?),
            by_index: transmute::<P, ByIndex>(sym("nvmlDeviceGetHandleByIndex_v2")?),
            by_uuid: transmute::<P, ByUuid>(sym("nvmlDeviceGetHandleByUUID")?),
            memory: transmute::<P, MemoryInfo>(sym("nvmlDeviceGetMemoryInfo")?),
            compute: transmute::<P, ProcessQuery>(sym("nvmlDeviceGetComputeRunningProcesses_v3")?),
            graphics: transmute::<P, ProcessQuery>(sym(
                "nvmlDeviceGetGraphicsRunningProcesses_v3",
            )?),
        })
    }
}

/// Whether NVML loaded and initialised (an NVIDIA driver is installed).
pub fn available() -> bool {
    api().is_some()
}

/// One NVIDIA GPU as NVML addresses it.
#[derive(Debug, Clone, Copy)]
pub struct NvmlDevice(Device);

// SAFETY: an NVML device handle is an opaque, process-wide token; NVML is thread-safe.
unsafe impl Send for NvmlDevice {}
unsafe impl Sync for NvmlDevice {}

/// The GPU with this UUID (`GPU-…`, as `nvidia-smi` prints it), else NVML index `index`
/// (the same PCI bus order `nvidia-smi` uses).
pub fn device(index: u32, uuid: Option<&str>) -> Option<NvmlDevice> {
    let api = api()?;
    let mut handle: Device = std::ptr::null_mut();
    if let Some(uuid) = uuid.and_then(|u| CString::new(u).ok()) {
        // SAFETY: valid C string and out pointer.
        if unsafe { (api.by_uuid)(uuid.as_ptr(), &mut handle) } == SUCCESS {
            return Some(NvmlDevice(handle));
        }
    }
    // SAFETY: valid out pointer.
    (unsafe { (api.by_index)(index, &mut handle) } == SUCCESS).then_some(NvmlDevice(handle))
}

/// Every NVIDIA GPU NVML sees.
pub fn devices() -> Vec<NvmlDevice> {
    let Some(api) = api() else {
        return Vec::new();
    };
    let mut count: c_uint = 0;
    // SAFETY: valid out pointer.
    if unsafe { (api.count)(&mut count) } != SUCCESS {
        return Vec::new();
    }
    (0..count).filter_map(|i| device(i, None)).collect()
}

impl NvmlDevice {
    /// `(used, total)` bytes of the GPU's memory, all processes.
    pub fn memory(&self) -> Option<(u64, u64)> {
        let api = api()?;
        let mut m = Memory::default();
        // SAFETY: valid handle and out pointer.
        (unsafe { (api.memory)(self.0, &mut m) } == SUCCESS).then_some((m.used, m.total))
    }

    /// GPU memory per process (`pid -> bytes`, compute and graphics contexts added up; a
    /// process without memory is absent). `None` when the query fails or the driver lists
    /// processes without their memory (Windows WDDM): the caller must not split the bar then.
    pub fn process_memory(&self) -> Option<HashMap<u32, u64>> {
        let api = api()?;
        let mut all = processes(api.compute, self.0)?;
        all.extend(processes(api.graphics, self.0)?);
        per_pid(&all)
    }
}

/// One `_v3` process query, growing the buffer while the driver reports more processes.
fn processes(query: ProcessQuery, device: Device) -> Option<Vec<ProcessInfo>> {
    let mut count: c_uint = 0;
    // SAFETY: a count of 0 with a null buffer asks for the size.
    match unsafe { query(device, &mut count, std::ptr::null_mut()) } {
        SUCCESS => return Some(Vec::new()),
        ERROR_INSUFFICIENT_SIZE => {}
        _ => return None,
    }
    for _ in 0..4 {
        // Room for processes that start between the two calls.
        let mut buf = vec![ProcessInfo::default(); count as usize + 8];
        let mut n = buf.len() as c_uint;
        // SAFETY: `buf` has room for `n` entries.
        match unsafe { query(device, &mut n, buf.as_mut_ptr()) } {
            SUCCESS => {
                buf.truncate(n as usize);
                return Some(buf);
            }
            ERROR_INSUFFICIENT_SIZE => count = n,
            _ => return None,
        }
    }
    None
}

/// Adds up the entries of each pid. `None` when there are entries but none carries a value.
fn per_pid(entries: &[ProcessInfo]) -> Option<HashMap<u32, u64>> {
    let mut out = HashMap::new();
    let mut known = entries.is_empty();
    for e in entries {
        if e.used_gpu_memory == VALUE_NOT_AVAILABLE {
            continue;
        }
        known = true;
        if e.used_gpu_memory > 0 {
            *out.entry(e.pid).or_insert(0u64) += e.used_gpu_memory;
        }
    }
    known.then_some(out)
}

/// One process's memory on every NVIDIA GPU. `None` where NVML is missing or does not report
/// per-process memory.
pub fn process_bytes(pid: u32) -> Option<u64> {
    let mut seen = false;
    let mut total = 0u64;
    for d in devices() {
        if let Some(map) = d.process_memory() {
            seen = true;
            total += map.get(&pid).copied().unwrap_or(0);
        }
    }
    seen.then_some(total)
}

#[cfg(windows)]
mod platform {
    use std::ffi::{c_void, CString};
    use windows::core::{w, PCSTR};
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::{
        GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
    };

    #[derive(Clone, Copy)]
    pub struct Lib(HMODULE);

    /// `nvml.dll` from System32 only (the driver installs it there; no search of the
    /// working directory or PATH).
    pub fn open() -> Option<Lib> {
        // SAFETY: plain library load; the module stays loaded for the process.
        unsafe { LoadLibraryExW(w!("nvml.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
            .ok()
            .map(Lib)
    }

    pub fn symbol(lib: Lib, name: &str) -> Option<*mut c_void> {
        let name = CString::new(name).ok()?;
        // SAFETY: valid module handle and C string.
        unsafe { GetProcAddress(lib.0, PCSTR(name.as_ptr().cast())) }.map(|f| f as *mut c_void)
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::ffi::{c_void, CString};

    #[derive(Clone, Copy)]
    pub struct Lib(*mut c_void);

    pub fn open() -> Option<Lib> {
        let name = CString::new("libnvidia-ml.so.1").ok()?;
        // SAFETY: plain library load; never closed.
        let h = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        (!h.is_null()).then_some(Lib(h))
    }

    pub fn symbol(lib: Lib, name: &str) -> Option<*mut c_void> {
        let name = CString::new(name).ok()?;
        // SAFETY: valid handle and C string.
        let p = unsafe { libc::dlsym(lib.0, name.as_ptr()) };
        (!p.is_null()).then_some(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(pid: u32, bytes: u64) -> ProcessInfo {
        ProcessInfo {
            pid,
            used_gpu_memory: bytes,
            ..Default::default()
        }
    }

    #[test]
    fn process_struct_matches_nvml_h() {
        // unsigned int, unsigned long long, unsigned int, unsigned int.
        assert_eq!(std::mem::size_of::<ProcessInfo>(), 24);
        assert_eq!(std::mem::size_of::<Memory>(), 24);
    }

    #[test]
    fn per_process_memory_adds_up_and_skips_unknown() {
        let map = per_pid(&[info(10, 100), info(10, 50), info(11, 0), info(12, 7)]).unwrap();
        assert_eq!(map.get(&10), Some(&150));
        assert_eq!(map.get(&11), None);
        assert_eq!(map.get(&12), Some(&7));
        // WDDM: processes listed, memory not available -> no split.
        assert_eq!(
            per_pid(&[info(10, VALUE_NOT_AVAILABLE), info(11, VALUE_NOT_AVAILABLE)]),
            None
        );
        // No process on the GPU is a real answer.
        assert_eq!(per_pid(&[]), Some(HashMap::new()));
    }

    /// Real NVML on this machine (needs an NVIDIA driver): prints the memory of each GPU and of
    /// each process, next to the OS counter. `cargo test -p harness-core --lib nvml -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_nvml_report() {
        assert!(available(), "NVML did not load");
        for d in devices() {
            let (used, total) = d.memory().expect("memory");
            println!("GPU used {} MiB / {} MiB", used >> 20, total >> 20);
            let api = api().unwrap();
            for (kind, q) in [("compute", api.compute), ("graphics", api.graphics)] {
                for p in processes(q, d.0).unwrap_or_default() {
                    let os = crate::gpu::process_vram_bytes(p.pid).map(|b| b >> 20);
                    let nvml = (p.used_gpu_memory != VALUE_NOT_AVAILABLE)
                        .then_some(p.used_gpu_memory >> 20);
                    println!(
                        "  {kind} pid {} nvml {:?} MiB, os {:?} MiB",
                        p.pid, nvml, os
                    );
                }
            }
            println!("  per pid: {:?}", d.process_memory());
        }
    }
}
