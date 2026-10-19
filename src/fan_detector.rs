use crate::errors::{FanCurveError, Result};
use log::{debug, info, warn};
use std::fs;
use std::path::{Path, PathBuf};

/// hwmon devices whose fans this app may have switched to manual control:
/// System76 laptops (system76, system76_acpi) and Thelio Io boards.
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

/// CPU fan control for System76 hwmon devices without a Thelio Io board's
/// per-channel support (e.g. laptops)
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

    /// Set every fan to `duty` (PWM 0-255), or return them to automatic
    /// control (pwmN_enable = 2) when `duty` is None, like system76-power
    pub fn set_duty(&self, duty: Option<u8>) -> Result<()> {
        for fan in &self.fans {
            let pwm_path = fan.hwmon_path.join(format!("pwm{}", fan.fan_number));
            let enable_path = fan.hwmon_path.join(format!("pwm{}_enable", fan.fan_number));

            let Some(duty) = duty else {
                match fs::write(&enable_path, "2") {
                    Ok(()) => info!("Fan {} set to automatic mode", fan.fan_number),
                    Err(e) => warn!(
                        "Failed to enable automatic mode for fan {} at {}: {}",
                        fan.fan_number,
                        enable_path.display(),
                        e
                    ),
                }
                continue;
            };

            if !pwm_path.exists() {
                warn!("PWM file does not exist: {}", pwm_path.display());
                continue;
            }
            // Some boards have no enable file; the PWM write still takes effect.
            let _ = fs::write(&enable_path, "1");
            fs::write(&pwm_path, duty.to_string())?;
            debug!("Fan {} PWM set to {}", fan.fan_number, duty);
        }
        Ok(())
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
