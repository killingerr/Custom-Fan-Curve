//! GPU model names for display.
//!
//! Names come from sources that don't wake a runtime-suspended GPU: the
//! NVIDIA driver's /proc information file, and otherwise the PCI vendor and
//! device ids (cached by the kernel) looked up in the system pci.ids database.

use std::fs;
use std::path::Path;

/// GPU vendor, used to colour the name in the GUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuInfo {
    pub vendor: GpuVendor,
    pub name: String,
}

const PCI_IDS_PATHS: [&str; 2] = ["/usr/share/misc/pci.ids", "/usr/share/hwdata/pci.ids"];

/// Every display controller on the PCI bus, in bus order.
pub fn detect_gpus() -> Vec<GpuInfo> {
    let pci_ids = PCI_IDS_PATHS
        .iter()
        .find_map(|p| fs::read_to_string(p).ok())
        .unwrap_or_default();

    let Ok(entries) = fs::read_dir("/sys/bus/pci/devices") else {
        return Vec::new();
    };
    let mut devices: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    devices.sort();

    devices
        .iter()
        .filter_map(|dev| {
            let read = |f: &str| fs::read_to_string(dev.join(f)).unwrap_or_default();
            // PCI class 0x03xxxx: display controller
            if !read("class").starts_with("0x03") {
                return None;
            }
            let vendor_id = parse_hex(&read("vendor"))?;
            let device_id = parse_hex(&read("device"))?;
            let address = dev.file_name()?.to_string_lossy().to_string();
            Some(gpu_info(&pci_ids, &address, vendor_id, device_id))
        })
        .collect()
}

fn parse_hex(s: &str) -> Option<u16> {
    u16::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok()
}

fn gpu_info(pci_ids: &str, address: &str, vendor_id: u16, device_id: u16) -> GpuInfo {
    let (vendor, vendor_name) = match vendor_id {
        0x10de => (GpuVendor::Nvidia, "NVIDIA"),
        0x1002 => (GpuVendor::Amd, "AMD"),
        0x8086 => (GpuVendor::Intel, "Intel"),
        _ => (GpuVendor::Other, ""),
    };

    // The NVIDIA driver reports the marketing name, e.g. "NVIDIA GeForce RTX 2060".
    if vendor == GpuVendor::Nvidia {
        let info = Path::new("/proc/driver/nvidia/gpus")
            .join(address)
            .join("information");
        if let Some(model) = fs::read_to_string(info).ok().and_then(|s| nvidia_model(&s)) {
            return GpuInfo {
                vendor,
                name: model,
            };
        }
    }

    let name = match lookup_pci_name(pci_ids, vendor_id, device_id) {
        Some(device_name) => {
            // "CoffeeLake-H GT2 [UHD Graphics 630]" -> "UHD Graphics 630"
            let short = device_name
                .rfind('[')
                .and_then(|start| {
                    let rest = &device_name[start + 1..];
                    rest.find(']').map(|end| &rest[..end])
                })
                .unwrap_or(&device_name);
            if vendor_name.is_empty() {
                short.to_string()
            } else {
                format!("{} {}", vendor_name, short)
            }
        }
        None => format!(
            "{} GPU ({:04x}:{:04x})",
            if vendor_name.is_empty() {
                "Unknown"
            } else {
                vendor_name
            },
            vendor_id,
            device_id
        ),
    };
    GpuInfo { vendor, name }
}

/// The "Model:" line of an NVIDIA /proc/driver/nvidia/gpus/*/information file.
fn nvidia_model(information: &str) -> Option<String> {
    information
        .lines()
        .find_map(|line| line.strip_prefix("Model:"))
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
}

/// Device name for `vendor:device` in a pci.ids database. Device ids are only
/// unique within a vendor, so the lookup is scoped to the vendor's section.
fn lookup_pci_name(pci_ids: &str, vendor: u16, device: u16) -> Option<String> {
    let vendor_prefix = format!("{:04x}  ", vendor);
    let device_prefix = format!("\t{:04x}  ", device);
    let section = pci_ids
        .lines()
        .skip_while(|line| !line.starts_with(&vendor_prefix))
        .skip(1);
    for line in section {
        if let Some(name) = line.strip_prefix(&device_prefix) {
            return Some(name.trim().to_string());
        }
        // A line that isn't indented, a comment or blank starts the next vendor.
        if !line.starts_with('\t') && !line.starts_with('#') && !line.is_empty() {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const PCI_IDS: &str = "\
# comment
10de  NVIDIA Corporation
\t1f11  TU106M [GeForce RTX 2060 Mobile]
\t\t1558 1325  some subsystem
8086  Intel Corporation
\t1c20  6 Series/C200 Series Chipset Family High Definition Audio Controller
\t3e9b  CoffeeLake-H GT2 [UHD Graphics 630]
";

    #[test]
    fn lookup_is_scoped_to_vendor() {
        assert_eq!(
            lookup_pci_name(PCI_IDS, 0x8086, 0x3e9b).as_deref(),
            Some("CoffeeLake-H GT2 [UHD Graphics 630]")
        );
        // 1c20 exists under Intel but not under NVIDIA
        assert_eq!(lookup_pci_name(PCI_IDS, 0x10de, 0x1c20), None);
        assert_eq!(lookup_pci_name(PCI_IDS, 0x1002, 0x1234), None);
    }

    #[test]
    fn names_use_bracketed_marketing_name() {
        let info = gpu_info(PCI_IDS, "0000:00:02.0", 0x8086, 0x3e9b);
        assert_eq!(info.vendor, GpuVendor::Intel);
        assert_eq!(info.name, "Intel UHD Graphics 630");

        let unknown = gpu_info(PCI_IDS, "0000:03:00.0", 0x1002, 0x744c);
        assert_eq!(unknown.name, "AMD GPU (1002:744c)");
    }

    #[test]
    fn reads_nvidia_model_line() {
        let information = "Model: \t\t NVIDIA GeForce RTX 2060\nIRQ:   \t\t 156\n";
        assert_eq!(
            nvidia_model(information).as_deref(),
            Some("NVIDIA GeForce RTX 2060")
        );
    }
}
