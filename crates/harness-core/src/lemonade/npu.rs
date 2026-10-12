//! AMD XDNA NPU detection (Ryzen AI), the hardware FastFlowLM and Ryzen AI backends of
//! Lemonade need.
//!
//! The NPU is a PCI device (vendor `1022`). The device ids Harness knows are the ones the
//! `amdxdna` driver binds (the Linux driver's table and the Windows `amdxdna.inf`): `1502`
//! (XDNA, Phoenix / Hawk Point) and `17F0` (XDNA 2, Strix Point / Strix Halo / Krackan). With
//! the Ryzen AI driver installed Windows names the device "NPU Compute Accelerator Device"
//! (class `ComputeAccelerator`); without it the device shows as an unknown PCI device. On
//! Windows the present devices come from SetupAPI (device enumeration, no process spawned);
//! on Linux from `/sys/bus/pci/devices`. A machine without the device, like a desktop Ryzen
//! without an NPU, reports `detected: false`.

use serde::{Deserialize, Serialize};

/// PCI vendor of AMD.
const AMD_VENDOR: &str = "1022";

/// `(PCI device id, generation)` of the NPUs the amdxdna driver supports.
const XDNA_DEVICES: &[(&str, &str)] = &[("1502", "xdna"), ("17F0", "xdna2")];

/// What the NPU detection found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct NpuInfo {
    /// An AMD XDNA NPU is present.
    pub detected: bool,
    /// `xdna` (Phoenix, Hawk Point) or `xdna2` (Strix, Krackan).
    pub generation: Option<String>,
    /// Device name as the OS reports it.
    pub name: Option<String>,
    /// The driver is installed (Windows: the device is named "NPU Compute Accelerator Device"
    /// / class `ComputeAccelerator`; Linux: the `amdxdna` module is bound). Without it the
    /// NPU exists but no backend can use it.
    pub driver_installed: bool,
    /// The hardware id that matched (`PCI\VEN_1022&DEV_17F0`).
    pub hardware_id: Option<String>,
}

/// One present device, as the OS enumerates it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PnpDevice {
    /// Hardware ids, most specific first (`PCI\VEN_1022&DEV_17F0&SUBSYS_...`).
    pub hardware_ids: Vec<String>,
    /// Setup class name (`ComputeAccelerator`, `System`, `Display`...); empty when unknown.
    pub class: String,
    pub name: String,
    pub manufacturer: String,
    /// The `amdxdna` driver is bound (Linux `driver` symlink); Windows derives it from the class.
    pub driver_bound: bool,
}

/// The `(VEN, DEV)` pair of a hardware id such as `PCI\VEN_1022&DEV_17F0&SUBSYS_...`.
fn pci_ids(hardware_id: &str) -> Option<(String, String)> {
    let upper = hardware_id.to_ascii_uppercase();
    if !upper.starts_with("PCI\\") {
        return None;
    }
    let field = |tag: &str| -> Option<String> {
        let start = upper.find(tag)? + tag.len();
        let value: String = upper[start..]
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        (value.len() == 4).then_some(value)
    };
    Some((field("VEN_")?, field("DEV_")?))
}

/// Picks the NPU out of the present devices.
pub fn classify(devices: &[PnpDevice]) -> NpuInfo {
    for d in devices {
        for id in &d.hardware_ids {
            let Some((ven, dev)) = pci_ids(id) else {
                continue;
            };
            if ven != AMD_VENDOR {
                continue;
            }
            let Some((_, generation)) = XDNA_DEVICES.iter().find(|(known, _)| *known == dev) else {
                continue;
            };
            let named = d
                .name
                .to_ascii_lowercase()
                .contains("npu compute accelerator");
            let class = d.class.eq_ignore_ascii_case("ComputeAccelerator");
            return NpuInfo {
                detected: true,
                generation: Some((*generation).to_string()),
                name: Some(d.name.clone()).filter(|n| !n.is_empty()),
                driver_installed: named || class || d.driver_bound,
                hardware_id: Some(format!("PCI\\VEN_{ven}&DEV_{dev}")),
            };
        }
    }
    NpuInfo::default()
}

/// Detects the NPU of this machine. Blocking (a device enumeration, a few milliseconds).
pub fn detect() -> NpuInfo {
    classify(&present_devices())
}

#[cfg(windows)]
fn present_devices() -> Vec<PnpDevice> {
    use windows::core::PCWSTR;
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
        SetupDiGetDeviceRegistryPropertyW, DIGCF_ALLCLASSES, DIGCF_PRESENT, HDEVINFO,
        SETUP_DI_REGISTRY_PROPERTY, SPDRP_CLASS, SPDRP_DEVICEDESC, SPDRP_FRIENDLYNAME,
        SPDRP_HARDWAREID, SPDRP_MFG, SP_DEVINFO_DATA,
    };

    /// A registry property of a device as UTF-16 text; multi-string values are split at NUL.
    unsafe fn property(
        set: HDEVINFO,
        data: &SP_DEVINFO_DATA,
        which: SETUP_DI_REGISTRY_PROPERTY,
    ) -> Vec<String> {
        let mut buf = vec![0u8; 2048];
        let mut required = 0u32;
        let ok = SetupDiGetDeviceRegistryPropertyW(
            set,
            data,
            which,
            None,
            Some(&mut buf),
            Some(&mut required),
        )
        .is_ok();
        if !ok {
            return Vec::new();
        }
        let len = (required as usize).min(buf.len());
        let wide: Vec<u16> = buf[..len]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        wide.split(|&c| c == 0)
            .filter(|s| !s.is_empty())
            .map(String::from_utf16_lossy)
            .collect()
    }

    let mut out = Vec::new();
    // SAFETY: plain SetupAPI enumeration; the handle is released below, buffers are owned.
    unsafe {
        let Ok(set) =
            SetupDiGetClassDevsW(None, PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_ALLCLASSES)
        else {
            return out;
        };
        let mut index = 0u32;
        loop {
            let mut data = SP_DEVINFO_DATA {
                cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
                ..Default::default()
            };
            if SetupDiEnumDeviceInfo(set, index, &mut data).is_err() {
                break;
            }
            index += 1;
            let ids = property(set, &data, SPDRP_HARDWAREID);
            if ids.is_empty() {
                continue;
            }
            let first =
                |which: SETUP_DI_REGISTRY_PROPERTY| property(set, &data, which).into_iter().next();
            let name = first(SPDRP_FRIENDLYNAME)
                .or_else(|| first(SPDRP_DEVICEDESC))
                .unwrap_or_default();
            out.push(PnpDevice {
                hardware_ids: ids,
                class: first(SPDRP_CLASS).unwrap_or_default(),
                name,
                manufacturer: first(SPDRP_MFG).unwrap_or_default(),
                driver_bound: false,
            });
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    out
}

#[cfg(target_os = "linux")]
fn present_devices() -> Vec<PnpDevice> {
    let Ok(dirs) = std::fs::read_dir("/sys/bus/pci/devices") else {
        return Vec::new();
    };
    let hex = |p: std::path::PathBuf| -> Option<String> {
        let t = std::fs::read_to_string(p).ok()?;
        Some(t.trim().trim_start_matches("0x").to_ascii_uppercase())
    };
    let mut out = Vec::new();
    for entry in dirs.flatten() {
        let dir = entry.path();
        let (Some(ven), Some(dev)) = (hex(dir.join("vendor")), hex(dir.join("device"))) else {
            continue;
        };
        let driver = std::fs::read_link(dir.join("driver"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_default();
        out.push(PnpDevice {
            hardware_ids: vec![format!("PCI\\VEN_{ven}&DEV_{dev}")],
            class: String::new(),
            name: if driver.is_empty() {
                String::new()
            } else {
                format!("PCI device ({driver})")
            },
            manufacturer: String::new(),
            driver_bound: driver == "amdxdna",
        });
    }
    out
}

#[cfg(not(any(windows, target_os = "linux")))]
fn present_devices() -> Vec<PnpDevice> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(ids: &[&str], class: &str, name: &str) -> PnpDevice {
        PnpDevice {
            hardware_ids: ids.iter().map(|s| s.to_string()).collect(),
            class: class.into(),
            name: name.into(),
            ..Default::default()
        }
    }

    #[test]
    fn reads_pci_ids() {
        assert_eq!(
            pci_ids("PCI\\VEN_1022&DEV_17F0&SUBSYS_00000000&REV_10"),
            Some(("1022".into(), "17F0".into()))
        );
        assert_eq!(
            pci_ids("pci\\ven_1002&dev_744c"),
            Some(("1002".into(), "744C".into()))
        );
        assert_eq!(pci_ids("ACPI\\VEN_1022&DEV_17F0"), None);
        assert_eq!(pci_ids("PCI\\VEN_10&DEV_1"), None);
    }

    #[test]
    fn a_desktop_without_an_npu_reports_not_detected() {
        // What a Ryzen 7 9700X + Radeon RX 7900 GRE desktop enumerates (no XDNA device).
        let devices = vec![
            dev(
                &["PCI\\VEN_1002&DEV_744C&SUBSYS_E4161DA2&REV_C8"],
                "Display",
                "AMD Radeon RX 7900 GRE",
            ),
            dev(
                &["PCI\\VEN_1022&DEV_14D8&SUBSYS_14531022"],
                "System",
                "PCI-to-PCI bridge",
            ),
            dev(&["PCI\\VEN_1022&DEV_15B8"], "USB", "USB Root Hub"),
            dev(
                &["ACPI\\VEN_AMDI&DEV_0010"],
                "System",
                "AMD GPIO Controller",
            ),
        ];
        assert_eq!(classify(&devices), NpuInfo::default());
        assert_eq!(classify(&[]), NpuInfo::default());
    }

    #[test]
    fn an_npu_with_its_driver_is_found() {
        let devices = vec![
            dev(
                &["PCI\\VEN_1002&DEV_1900"],
                "Display",
                "AMD Radeon(TM) 890M",
            ),
            dev(
                &[
                    "PCI\\VEN_1022&DEV_17F0&SUBSYS_17F01022&REV_10",
                    "PCI\\VEN_1022&DEV_17F0",
                ],
                "ComputeAccelerator",
                "NPU Compute Accelerator Device",
            ),
        ];
        let npu = classify(&devices);
        assert!(npu.detected && npu.driver_installed);
        assert_eq!(npu.generation.as_deref(), Some("xdna2"));
        assert_eq!(npu.name.as_deref(), Some("NPU Compute Accelerator Device"));
        assert_eq!(npu.hardware_id.as_deref(), Some("PCI\\VEN_1022&DEV_17F0"));
    }

    #[test]
    fn an_npu_without_a_driver_is_detected_but_not_usable() {
        let devices = vec![dev(&["PCI\\VEN_1022&DEV_1502&REV_00"], "", "PCI Device")];
        let npu = classify(&devices);
        assert!(npu.detected);
        assert!(!npu.driver_installed);
        assert_eq!(npu.generation.as_deref(), Some("xdna"));
        // A device of another vendor with the same device id is not an NPU.
        assert!(!classify(&[dev(&["PCI\\VEN_8086&DEV_1502"], "", "")]).detected);
    }

    #[test]
    fn this_machine_enumerates_without_failing() {
        // Whatever the machine has, the enumeration runs and the result is consistent.
        let npu = detect();
        assert_eq!(npu.detected, npu.generation.is_some());
    }
}
