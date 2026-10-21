use crate::errors::{FanCurveError, Result};
use log::{info, warn};
use std::fs;
use std::path::{Path, PathBuf};

/// These are the hwmon devices whose fans this app may have switched to manual
/// control: Thelio Io boards, and System76 laptops (system76, system76_acpi),
/// which older versions of the app drove.
const SYSTEM76_FAN_HWMON: [&str; 4] = [
    "system76",
    "system76_acpi",
    "system76_io",
    "system76_thelio_io",
];

/// Return the fans of every System76 hwmon device under `root` (normally
/// /sys/class/hwmon) to automatic control by writing 2 to each pwmN_enable.
/// Returns the files written; boards without enable files are left alone.
pub fn restore_automatic(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut written = Vec::new();
    for hwmon in entries.flatten().map(|e| e.path()) {
        let is_system76 = fs::read_to_string(hwmon.join("name"))
            .is_ok_and(|name| SYSTEM76_FAN_HWMON.contains(&name.trim()));
        if !is_system76 {
            continue;
        }
        for n in 1..=8 {
            let enable = hwmon.join(format!("pwm{}_enable", n));
            if !enable.exists() {
                continue;
            }
            match fs::write(&enable, "2") {
                Ok(()) => written.push(enable),
                Err(e) => warn!("Failed to write {}: {}", enable.display(), e),
            }
        }
    }
    written
}

/// Fan sensor information
#[derive(Debug, Clone)]
pub struct FanSensor {
    pub fan_number: u8,
    pub hwmon_path: PathBuf,
    pub fan_label: String,
}

/// FanDetector reads the CPU fan's speed from a System76 hwmon device, for the
/// GUI when no daemon reports it.
#[derive(Default)]
pub struct FanDetector {
    fans: Vec<FanSensor>,
}

impl FanDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Find the CPU fan on the System76 hwmon device
    pub fn initialize(&mut self) -> Result<()> {
        let hwmon = fs::read_dir("/sys/class/hwmon")?
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                let name = fs::read_to_string(p.join("name")).unwrap_or_default();
                matches!(
                    name.trim(),
                    "system76_thelio_io" | "system76_io" | "system76"
                )
            })
            .ok_or_else(|| FanCurveError::Config("System76 Thelio IO sensor not found".into()))?;

        // fanN_label / fanN_input pairs are numbered from 1 without gaps
        for fan_number in 1..=10 {
            let Ok(label) = fs::read_to_string(hwmon.join(format!("fan{}_label", fan_number)))
            else {
                break;
            };
            if !hwmon.join(format!("fan{}_input", fan_number)).exists() {
                break;
            }
            let fan_label = label.trim().to_string();
            if fan_label.to_lowercase().contains("cpu") {
                info!(
                    "Found CPU fan: fan{} '{}' at {}",
                    fan_number,
                    fan_label,
                    hwmon.display()
                );
                self.fans.push(FanSensor {
                    fan_number,
                    hwmon_path: hwmon,
                    fan_label,
                });
                return Ok(());
            }
        }

        Err(FanCurveError::Config(
            "CPU Fan not found in System76 Thelio IO".to_string(),
        ))
    }

    /// RPM of each detected fan, as (fan_number, rpm, label)
    pub fn read_all_fan_speeds(&self) -> Result<Vec<(u8, u16, String)>> {
        self.fans
            .iter()
            .map(|fan| {
                let path = fan.hwmon_path.join(format!("fan{}_input", fan.fan_number));
                let rpm = fs::read_to_string(path)?
                    .trim()
                    .parse()
                    .map_err(|_| FanCurveError::Config("Failed to parse fan speed".to_string()))?;
                Ok((fan.fan_number, rpm, fan.fan_label.clone()))
            })
            .collect()
    }

    pub fn is_initialized(&self) -> bool {
        !self.fans.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_automatic_resets_only_system76_fans() {
        let root = std::env::temp_dir().join(format!("fan_restore_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let hwmon = |name: &str, dev: &str, enables: &[u8]| {
            let dir = root.join(dev);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("name"), format!("{}\n", name)).unwrap();
            for n in enables {
                fs::write(dir.join(format!("pwm{}_enable", n)), "1").unwrap();
            }
            dir
        };
        let laptop = hwmon("system76_acpi", "hwmon0", &[1, 2]);
        let thelio = hwmon("system76_thelio_io", "hwmon1", &[]);
        let other = hwmon("nct6775", "hwmon2", &[1]);

        let written = restore_automatic(&root);
        assert_eq!(written.len(), 2);
        for n in [1, 2] {
            let value = fs::read_to_string(laptop.join(format!("pwm{}_enable", n))).unwrap();
            assert_eq!(value, "2");
        }
        // Thelio Io 2 has no enable files; other hardware is untouched
        assert!(!thelio.join("pwm1_enable").exists());
        assert_eq!(fs::read_to_string(other.join("pwm1_enable")).unwrap(), "1");
        let _ = fs::remove_dir_all(&root);
    }
}
