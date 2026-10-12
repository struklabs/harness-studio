use std::path::Path;

use crate::hardware::{is_rdna3_or_newer, parse_gpu_vendor, GpuInfo, GpuVendor};

/// AMD card to name when neither the OS nor the runtime reports an RDNA3/RDNA4 GPU.
pub fn rocm_unsupported_gpu(gpus: &[GpuInfo], device_names: &[String]) -> Option<String> {
    let amd: Vec<_> = gpus
        .iter()
        .map(|g| (g.name.as_str(), g.vendor.clone(), g.is_discrete))
        .chain(device_names.iter().map(|name| {
            (
                name.as_str(),
                parse_gpu_vendor(name),
                crate::gpu::is_discrete_gpu(name, None),
            )
        }))
        .filter(|(_, vendor, _)| *vendor == GpuVendor::Amd)
        .collect();
    if amd.iter().any(|(name, _, _)| is_rdna3_or_newer(name))
        || device_names.iter().any(|name| is_rdna3_or_newer(name))
    {
        return None;
    }
    amd.iter()
        .find(|(_, _, discrete)| *discrete)
        .or_else(|| amd.first())
        .map(|(name, _, _)| (*name).to_string())
}

/// Registry installs of the driver-dependent builds (ROCm/HIP, SYCL) must list a device of
/// their backend, as the managed install checks in `RuntimeManager::verify_stage1_devices`;
/// otherwise `runtime.backend_no_device` (with the DLLs the backend library cannot find).
pub fn verify_backend_devices(binary: &Path, backend: &str) -> Result<(), String> {
    if !matches!(backend, "rocm" | "sycl") {
        return Ok(());
    }
    let devices = crate::devices::probe(binary).unwrap_or_default();
    let dir = binary.parent().unwrap_or_else(|| Path::new("."));
    verify_backend_device_ids(backend, devices.iter().map(|d| d.id.as_str()), || {
        crate::runtime::missing_backend_libraries(dir, backend)
    })
}

fn verify_backend_device_ids<'a>(
    backend: &str,
    devices: impl IntoIterator<Item = &'a str>,
    missing: impl FnOnce() -> Vec<String>,
) -> Result<(), String> {
    let prefix = match backend {
        "rocm" => "ROCm",
        "sycl" => "SYCL",
        _ => return Ok(()),
    };
    if devices.into_iter().any(|id| id.starts_with(prefix)) {
        return Ok(());
    }
    let missing = missing();
    if missing.is_empty() {
        Err(format!("runtime.backend_no_device: {backend}"))
    } else {
        Err(format!(
            "runtime.backend_no_device: {backend} (missing: {})",
            missing.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(name: &str, vendor: GpuVendor, is_discrete: bool) -> GpuInfo {
        GpuInfo {
            index: 0,
            name: name.into(),
            vendor,
            vram_bytes: None,
            driver_version: None,
            is_discrete,
            is_default: true,
        }
    }

    #[test]
    fn backend_devices_require_matching_probe_ids() {
        for (backend, id) in [("rocm", "ROCm0"), ("sycl", "SYCL0")] {
            assert!(
                verify_backend_device_ids(backend, [id], || panic!("device is present")).is_ok()
            );
            assert_eq!(
                verify_backend_device_ids(backend, ["Vulkan0"], Vec::new),
                Err(format!("runtime.backend_no_device: {backend}"))
            );
        }
        assert!(verify_backend_device_ids("cpu", [], Vec::new).is_ok());
        assert_eq!(
            verify_backend_device_ids("rocm", [], || vec!["hipblas.dll".into()]),
            Err("runtime.backend_no_device: rocm (missing: hipblas.dll)".into())
        );
    }

    #[test]
    fn rocm_unsupported_gpu_table() {
        for name in [
            "AMD Radeon RX 6700 XT",
            "AMD Radeon RX 580",
            "AMD Radeon RX 5700 XT",
        ] {
            assert_eq!(
                rocm_unsupported_gpu(&[gpu(name, GpuVendor::Amd, true)], &[]).as_deref(),
                Some(name)
            );
        }
        let gre = gpu("AMD Radeon RX 7900 GRE", GpuVendor::Amd, true);
        let integrated = gpu("AMD Radeon(TM) Graphics", GpuVendor::Amd, false);
        assert_eq!(rocm_unsupported_gpu(std::slice::from_ref(&gre), &[]), None);
        assert_eq!(rocm_unsupported_gpu(&[gre, integrated.clone()], &[]), None);
        assert_eq!(
            rocm_unsupported_gpu(
                &[gpu("NVIDIA GeForce RTX 4090", GpuVendor::Nvidia, true)],
                &[]
            ),
            None
        );
        assert_eq!(
            rocm_unsupported_gpu(
                &[
                    integrated.clone(),
                    gpu("AMD Radeon RX 6600", GpuVendor::Amd, true)
                ],
                &[]
            )
            .as_deref(),
            Some("AMD Radeon RX 6600")
        );
        assert_eq!(
            rocm_unsupported_gpu(&[integrated], &[]).as_deref(),
            Some("AMD Radeon(TM) Graphics")
        );
    }

    #[test]
    fn rocm_unsupported_gpu_uses_runtime_names() {
        let generic = [gpu("AMD Radeon GPU", GpuVendor::Amd, true)];
        assert_eq!(
            rocm_unsupported_gpu(&generic, &["AMD Radeon RX 7900 GRE".into()]),
            None
        );
        assert_eq!(rocm_unsupported_gpu(&generic, &["gfx1100".into()]), None);
        assert_eq!(
            rocm_unsupported_gpu(&[], &["AMD Radeon RX 6600".into()]).as_deref(),
            Some("AMD Radeon RX 6600")
        );
    }
}
