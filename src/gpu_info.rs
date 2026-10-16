//! GPU model names for display.
//!
//! Names come from sources that don't wake a runtime-suspended GPU: the
//! NVIDIA driver's /proc information file, libdrm's amdgpu.ids database for
//! AMD (keyed by device id and revision, since one chip is sold under several
//! names), and otherwise the PCI vendor and device ids (cached by the kernel)
//! looked up in the system pci.ids database.

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
const AMDGPU_IDS_PATHS: [&str; 2] = [
    "/usr/share/libdrm/amdgpu.ids",
    "/usr/local/share/libdrm/amdgpu.ids",
];

/// Name databases, read once per detection.
struct IdDatabases {
    pci_ids: String,
    amdgpu_ids: String,
}

/// Every display controller on the PCI bus, in bus order.
pub fn detect_gpus() -> Vec<GpuInfo> {
    let read_first = |paths: &[&str]| {
        paths
            .iter()
            .find_map(|p| fs::read_to_string(p).ok())
            .unwrap_or_default()
    };
    let db = IdDatabases {
        pci_ids: read_first(&PCI_IDS_PATHS),
        amdgpu_ids: read_first(&AMDGPU_IDS_PATHS),
    };

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
            let revision = parse_hex(&read("revision")).unwrap_or_default();
            let address = dev.file_name()?.to_string_lossy().to_string();
            Some(gpu_info(&db, &address, vendor_id, device_id, revision))
        })
        .collect()
}

fn parse_hex(s: &str) -> Option<u16> {
    u16::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok()
}

fn gpu_info(
    db: &IdDatabases,
    address: &str,
    vendor_id: u16,
    device_id: u16,
    revision: u16,
) -> GpuInfo {
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

    // libdrm knows newer AMD cards than pci.ids, e.g. "AMD Radeon RX 9060 XT".
    if vendor == GpuVendor::Amd {
        if let Some(model) = lookup_amdgpu_name(&db.amdgpu_ids, device_id, revision) {
            return GpuInfo {
                vendor,
                name: model,
            };
        }
    }

    let name = match lookup_pci_name(&db.pci_ids, vendor_id, device_id) {
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

/// Product name for `device`/`revision` in a libdrm amdgpu.ids database, whose
/// lines look like "7590,\tC0,\tAMD Radeon RX 9060 XT". Only an exact revision
/// match counts: other revisions of the same chip can be different products.
fn lookup_amdgpu_name(amdgpu_ids: &str, device: u16, revision: u16) -> Option<String> {
    amdgpu_ids
        .lines()
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| {
            let mut fields = line.splitn(3, ',');
            let dev = parse_hex(fields.next()?)?;
            let rev = parse_hex(fields.next()?)?;
            let name = fields.next()?.trim();
            (dev == device && rev == revision && !name.is_empty()).then(|| name.to_string())
        })
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

    const AMDGPU_IDS: &str = "\
# List of AMDGPU IDs
#
# Syntax:
# device_id,\trevision_id,\tproduct_name        <-- single tab after comma

1.0.0
7590,\tC0,\tAMD Radeon RX 9060 XT
7590,\tC1,\tAMD Radeon RX 9060 XT LP
7590,\tC7,\tAMD Radeon RX 9060
";

    fn databases() -> IdDatabases {
        IdDatabases {
            pci_ids: PCI_IDS.to_string(),
            amdgpu_ids: AMDGPU_IDS.to_string(),
        }
    }

    #[test]
    fn names_use_bracketed_marketing_name() {
        let info = gpu_info(&databases(), "0000:00:02.0", 0x8086, 0x3e9b, 0x00);
        assert_eq!(info.vendor, GpuVendor::Intel);
        assert_eq!(info.name, "Intel UHD Graphics 630");

        let unknown = gpu_info(&databases(), "0000:03:00.0", 0x1002, 0x744c, 0xc8);
        assert_eq!(unknown.name, "AMD GPU (1002:744c)");
    }

    #[test]
    fn amd_names_come_from_amdgpu_ids_by_revision() {
        let info = gpu_info(&databases(), "0000:03:00.0", 0x1002, 0x7590, 0xc0);
        assert_eq!(info.vendor, GpuVendor::Amd);
        assert_eq!(info.name, "AMD Radeon RX 9060 XT");

        let info = gpu_info(&databases(), "0000:03:00.0", 0x1002, 0x7590, 0xc7);
        assert_eq!(info.name, "AMD Radeon RX 9060");

        // A revision libdrm doesn't list isn't guessed from its siblings.
        assert_eq!(lookup_amdgpu_name(AMDGPU_IDS, 0x7590, 0xc3), None);
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
