use crate::errors::{FanCurveError, Result};
use log::info;
use std::fs;

/// CPU manufacturer types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CpuManufacturer {
    Intel,
    Amd,
    Unknown,
}

/// CPU temperature sensor information
#[derive(Debug, Clone)]
pub struct CpuTempSensor {
    pub manufacturer: CpuManufacturer,
    pub temp_input_path: String,
    /// hwmon driver name, e.g. "k10temp"
    pub sensor_name: String,
}

/// Average and highest current clock of the CPU cores in MHz, from cpufreq.
pub fn cpu_clock_mhz() -> Option<(u32, u32)> {
    let entries = fs::read_dir("/sys/devices/system/cpu").ok()?;
    let khz: Vec<u64> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.strip_prefix("cpu")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .filter_map(|e| fs::read_to_string(e.path().join("cpufreq/scaling_cur_freq")).ok())
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    clock_summary(&khz)
}

/// (average, highest) in MHz of per-core clocks in kHz.
fn clock_summary(khz: &[u64]) -> Option<(u32, u32)> {
    let max = *khz.iter().max()?;
    let avg = khz.iter().sum::<u64>() / khz.len() as u64;
    Some(((avg / 1000) as u32, (max / 1000) as u32))
}

/// CPU model name from /proc/cpuinfo, e.g. "AMD Ryzen 9 7950X 16-Core Processor"
pub fn cpu_model_name() -> Option<String> {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").ok()?;
    parse_model_name(&cpuinfo)
}

fn parse_model_name(cpuinfo: &str) -> Option<String> {
    cpuinfo
        .lines()
        .find(|line| line.starts_with("model name"))
        .and_then(|line| line.split_once(':'))
        .map(|(_, model)| model.trim().to_string())
        .filter(|model| !model.is_empty())
}

/// CPU temperature detector
#[derive(Default)]
pub struct CpuTempDetector {
    sensor: Option<CpuTempSensor>,
}

impl CpuTempDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Find the CPU package temperature: coretemp "Package id 0" (or
    /// "Core 0") on Intel, k10temp "Tctl" on AMD
    pub fn initialize(&mut self) -> Result<()> {
        let cpuinfo = fs::read_to_string("/proc/cpuinfo")?;
        let vendor = cpuinfo
            .lines()
            .find(|line| line.starts_with("vendor_id"))
            .and_then(|line| line.split_once(':'))
            .map(|(_, v)| v.trim());
        let manufacturer = match vendor {
            Some("GenuineIntel") => CpuManufacturer::Intel,
            Some("AuthenticAMD") => CpuManufacturer::Amd,
            _ => CpuManufacturer::Unknown,
        };

        let (drivers, labels): (&[&str], &[&str]) = match manufacturer {
            CpuManufacturer::Intel => (&["coretemp"], &["Package id 0", "Core 0"]),
            CpuManufacturer::Amd => (&["k10temp"], &["Tctl"]),
            CpuManufacturer::Unknown => (
                &["coretemp", "k10temp"],
                &["Package id 0", "Core 0", "Tctl"],
            ),
        };

        for hwmon in fs::read_dir("/sys/class/hwmon")?
            .flatten()
            .map(|e| e.path())
        {
            let name = fs::read_to_string(hwmon.join("name")).unwrap_or_default();
            let name = name.trim();
            if !drivers.contains(&name) {
                continue;
            }
            for entry in fs::read_dir(&hwmon)?.flatten() {
                let file = entry.file_name().to_string_lossy().into_owned();
                let Some(temp) = file
                    .strip_suffix("_input")
                    .filter(|t| t.starts_with("temp"))
                else {
                    continue;
                };
                let label =
                    fs::read_to_string(hwmon.join(format!("{}_label", temp))).unwrap_or_default();
                if labels.iter().any(|l| label.contains(l)) {
                    info!(
                        "CPU temperature sensor: {} {} ({:?} CPU)",
                        name,
                        label.trim(),
                        manufacturer
                    );
                    self.sensor = Some(CpuTempSensor {
                        manufacturer,
                        temp_input_path: entry.path().to_string_lossy().into_owned(),
                        sensor_name: name.to_string(),
                    });
                    return Ok(());
                }
            }
        }

        Err(FanCurveError::Config(format!(
            "Could not find CPU temperature sensor for {:?}",
            manufacturer
        )))
    }

    /// Read the current CPU temperature in °C
    pub fn read_temperature(&self) -> Result<f32> {
        let sensor = self.sensor.as_ref().ok_or_else(|| {
            FanCurveError::Config("CPU temperature sensor not initialized".to_string())
        })?;

        let millidegrees: i32 = fs::read_to_string(&sensor.temp_input_path)?
            .trim()
            .parse()
            .map_err(|_| FanCurveError::Config("Failed to parse temperature".to_string()))?;
        let celsius = millidegrees as f32 / 1000.0;

        // Reject readings no CPU can produce, so a glitched sensor can't stop the fans
        if !(-50.0..=200.0).contains(&celsius) {
            return Err(FanCurveError::Config(format!(
                "Temperature reading out of range: {:.1}°C",
                celsius
            )));
        }
        Ok(celsius)
    }

    pub fn get_sensor_info(&self) -> Option<&CpuTempSensor> {
        self.sensor.as_ref()
    }

    pub fn is_initialized(&self) -> bool {
        self.sensor.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_summary_averages_cores_and_finds_the_fastest() {
        assert_eq!(
            clock_summary(&[613_954, 4_333_316, 3_000_000, 3_200_000]),
            Some((2786, 4333))
        );
        assert_eq!(clock_summary(&[]), None);
    }

    #[test]
    fn test_parse_model_name() {
        let cpuinfo = "processor\t: 0\nvendor_id\t: AuthenticAMD\n\
                       model name\t: AMD Ryzen 9 7950X 16-Core Processor\n";
        assert_eq!(
            parse_model_name(cpuinfo).as_deref(),
            Some("AMD Ryzen 9 7950X 16-Core Processor")
        );
        assert_eq!(parse_model_name("processor\t: 0\n"), None);
    }
}
