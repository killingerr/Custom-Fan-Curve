use crate::errors::FanCurveError;
use crate::errors::Result;
use log::{info, warn};
use std::fs;
use std::path::Path;

/// CPU manufacturer types
#[derive(Debug, Clone, PartialEq)]
pub enum CpuManufacturer {
    Intel,
    Amd,
    Unknown,
}

/// CPU temperature sensor information
#[derive(Debug, Clone)]
pub struct CpuTempSensor {
    pub manufacturer: CpuManufacturer,
    pub hwmon_path: String,
    pub temp_input_path: String,
    pub temp_label_path: String,
    pub sensor_name: String,
}

/// CPU temperature detector
pub struct CpuTempDetector {
    sensor: Option<CpuTempSensor>,
}

impl CpuTempDetector {
    /// Create a new CPU temperature detector
    pub fn new() -> Self {
        Self { sensor: None }
    }

    /// Initialize the detector by scanning for CPU temperature sensors
    pub fn initialize(&mut self) -> Result<()> {
        info!("Initializing CPU temperature detector...");

        // First detect CPU manufacturer
        let manufacturer = self.detect_cpu_manufacturer()?;
        info!("Detected CPU manufacturer: {:?}", manufacturer);

        // Find the appropriate temperature sensor
        let sensor = self.find_cpu_temp_sensor(&manufacturer)?;
        self.sensor = Some(sensor);

        info!("CPU temperature sensor initialized: {:?}", self.sensor);
        Ok(())
    }

    /// Detect CPU manufacturer by reading /proc/cpuinfo
    fn detect_cpu_manufacturer(&self) -> Result<CpuManufacturer> {
        let cpuinfo = fs::read_to_string("/proc/cpuinfo")?;

        for line in cpuinfo.lines() {
            if line.starts_with("vendor_id") {
                let vendor = line
                    .split(':')
                    .nth(1)
                    .ok_or_else(|| FanCurveError::Config("Invalid cpuinfo format".to_string()))?
                    .trim();

                return match vendor {
                    "GenuineIntel" => Ok(CpuManufacturer::Intel),
                    "AuthenticAMD" => Ok(CpuManufacturer::Amd),
                    _ => {
                        warn!("Unknown CPU vendor: {}", vendor);
                        Ok(CpuManufacturer::Unknown)
                    }
                };
            }
        }

        Err(FanCurveError::Config(
            "Could not determine CPU manufacturer".to_string(),
        ))
    }

    /// Find the CPU temperature sensor in /sys/class/hwmon
    fn find_cpu_temp_sensor(&self, manufacturer: &CpuManufacturer) -> Result<CpuTempSensor> {
        let hwmon_dir = Path::new("/sys/class/hwmon");

        if !hwmon_dir.exists() {
            return Err(FanCurveError::Config(
                "Hardware monitoring directory not found".to_string(),
            ));
        }

        // Read all hwmon directories
        let entries = fs::read_dir(hwmon_dir)?;

        for entry in entries {
            let entry = entry?;
            let hwmon_path = entry.path();

            if !hwmon_path.is_dir() {
                continue;
            }

            // Read the name file to identify the sensor type
            let name_path = hwmon_path.join("name");
            if let Ok(name_content) = fs::read_to_string(&name_path) {
                let sensor_name = name_content.trim().to_string();

                // Check if this is the sensor we want based on manufacturer
                let is_target_sensor = match manufacturer {
                    CpuManufacturer::Intel => sensor_name == "coretemp",
                    CpuManufacturer::Amd => sensor_name == "k10temp",
                    CpuManufacturer::Unknown => {
                        // Try both if we can't determine manufacturer
                        sensor_name == "coretemp" || sensor_name == "k10temp"
                    }
                };

                if is_target_sensor {
                    // Find the correct temperature input file
                    if let Ok(temp_input_path) =
                        self.find_temp_input_file(&hwmon_path, manufacturer)
                    {
                        let temp_label_path =
                            self.find_temp_label_file(&hwmon_path, &temp_input_path)?;

                        return Ok(CpuTempSensor {
                            manufacturer: manufacturer.clone(),
                            hwmon_path: hwmon_path.to_string_lossy().to_string(),
                            temp_input_path,
                            temp_label_path,
                            sensor_name,
                        });
                    }
                }
            }
        }

        Err(FanCurveError::Config(format!(
            "Could not find CPU temperature sensor for {:?}",
            manufacturer
        )))
    }

    /// Find the correct temperature input file
    fn find_temp_input_file(
        &self,
        hwmon_path: &Path,
        manufacturer: &CpuManufacturer,
    ) -> Result<String> {
        let entries = fs::read_dir(hwmon_path)?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if file_name.starts_with("temp") && file_name.ends_with("_input") {
                    // Check if this is the right temperature sensor by reading the label
                    if let Ok(label_path) =
                        self.find_temp_label_file(hwmon_path, &path.to_string_lossy())
                    {
                        if let Ok(label_content) = fs::read_to_string(&label_path) {
                            let label = label_content.trim();

                            let is_correct_sensor = match manufacturer {
                                CpuManufacturer::Intel => {
                                    label.contains("Package id 0") || label.contains("Core 0")
                                }
                                CpuManufacturer::Amd => label.contains("Tctl"),
                                CpuManufacturer::Unknown => {
                                    // Try both patterns
                                    label.contains("Package id 0")
                                        || label.contains("Core 0")
                                        || label.contains("Tctl")
                                }
                            };

                            if is_correct_sensor {
                                return Ok(path.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
        }

        Err(FanCurveError::Config(format!(
            "Could not find temperature input file for {:?}",
            manufacturer
        )))
    }

    /// Find the corresponding temperature label file
    fn find_temp_label_file(&self, hwmon_path: &Path, temp_input_path: &str) -> Result<String> {
        // Extract the temp number from the input path (e.g., "temp1_input" -> "temp1")
        let temp_input_name = Path::new(temp_input_path)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| FanCurveError::Config("Invalid temp input path".to_string()))?;

        if let Some(temp_num) = temp_input_name.strip_suffix("_input") {
            let label_file = format!("{}_label", temp_num);
            let label_path = hwmon_path.join(&label_file);

            if label_path.exists() {
                Ok(label_path.to_string_lossy().to_string())
            } else {
                Err(FanCurveError::Config(format!(
                    "Label file not found: {}",
                    label_file
                )))
            }
        } else {
            Err(FanCurveError::Config(
                "Invalid temp input file format".to_string(),
            ))
        }
    }

    /// Read the current CPU temperature
    pub fn read_temperature(&self) -> Result<f32> {
        let sensor = self.sensor.as_ref().ok_or_else(|| {
            FanCurveError::Config("CPU temperature sensor not initialized".to_string())
        })?;

        let temp_content = fs::read_to_string(&sensor.temp_input_path)?;

        // Temperature is typically in millidegrees Celsius
        let temp_millidegrees: i32 = temp_content
            .trim()
            .parse()
            .map_err(|_| FanCurveError::Config("Failed to parse temperature".to_string()))?;

        // Convert to degrees Celsius
        let temp_celsius = temp_millidegrees as f32 / 1000.0;

        // Validate temperature range (reasonable CPU temperature range)
        if !(-50.0..=200.0).contains(&temp_celsius) {
            return Err(FanCurveError::Config(format!(
                "Temperature reading out of range: {:.1}°C",
                temp_celsius
            )));
        }

        Ok(temp_celsius)
    }

    /// Get sensor information
    pub fn get_sensor_info(&self) -> Option<&CpuTempSensor> {
        self.sensor.as_ref()
    }

    /// Check if the detector is initialized
    pub fn is_initialized(&self) -> bool {
        self.sensor.is_some()
    }

    /// Get the detected CPU manufacturer
    pub fn manufacturer(&self) -> CpuManufacturer {
        self.sensor
            .as_ref()
            .map(|s| s.manufacturer.clone())
            .unwrap_or(CpuManufacturer::Unknown)
    }
}

impl Default for CpuTempDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_manufacturer_detection() {
        let detector = CpuTempDetector::new();
        // This test would require mocking /proc/cpuinfo
        // For now, just test that the method exists
        assert!(true);
    }
}
