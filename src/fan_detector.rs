use crate::errors::Result;
use log::{info, warn};
use std::fs;
use std::path::Path;

/// Fan sensor information
#[derive(Debug, Clone)]
pub struct FanSensor {
    pub fan_number: u8,
    pub hwmon_path: String,
    pub fan_input_path: String,
    pub fan_label_path: String,
    pub fan_label: String,
}

/// Fan detector for System76 Thelio IO
pub struct FanDetector {
    fans: Vec<FanSensor>,
    hwmon_path: Option<String>,
}

impl FanDetector {
    /// Create a new fan detector
    pub fn new() -> Self {
        Self {
            fans: Vec::new(),
            hwmon_path: None,
        }
    }

    /// Initialize the detector by finding System76 Thelio IO sensors
    pub fn initialize(&mut self) -> Result<()> {
        info!("Initializing fan detector...");
        
        // Find the System76 Thelio IO hwmon directory
        self.find_thelio_io_sensor()?;
        
        // Find all fan sensors in that directory
        self.find_fan_sensors()?;
        
        info!("Fan detector initialized with {} fans found", self.fans.len());
        
        // Debug: List all found fans
        for (i, fan) in self.fans.iter().enumerate() {
            info!("Fan {}: number={}, label='{}', input_path='{}'", 
                  i, fan.fan_number, fan.fan_label, fan.fan_input_path);
        }
        
        Ok(())
    }

    /// Find the System76 Thelio IO sensor directory
    fn find_thelio_io_sensor(&mut self) -> Result<()> {
        let hwmon_dir = Path::new("/sys/class/hwmon");
        
        if !hwmon_dir.exists() {
            return Err(crate::errors::FanCurveError::Config(
                "Hardware monitoring directory not found".to_string()
            ));
        }

        let entries = fs::read_dir(hwmon_dir)?;
        
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_dir() {
                let name_file = path.join("name");
                if name_file.exists() {
                    if let Ok(name_content) = fs::read_to_string(&name_file) {
                        let name = name_content.trim();
                        info!("Checking hwmon device: {} -> '{}'", path.display(), name);
                        
                        if name == "system76-thelio-io" || name == "pch_cannonlake" {
                            self.hwmon_path = Some(path.to_string_lossy().to_string());
                            info!("Found fan sensor device '{}' at: {}", name, path.display());
                            return Ok(());
                        }
                    }
                }
            }
        }

        Err(crate::errors::FanCurveError::Config(
            "Fan sensor device not found (looking for 'system76-thelio-io' or 'pch_cannonlake')".to_string()
        ))
    }

    /// Find all fan sensors in the detected device directory
    fn find_fan_sensors(&mut self) -> Result<()> {
        let hwmon_path = self.hwmon_path.as_ref()
            .ok_or_else(|| crate::errors::FanCurveError::Config(
                "Fan sensor device path not found".to_string()
            ))?;

        let hwmon_dir = Path::new(hwmon_path);
        let entries = fs::read_dir(hwmon_dir)?;
        
        let mut fan_files = Vec::new();
        
        // Collect all fan input files
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if file_name.starts_with("fan") && file_name.ends_with("_input") {
                    // Extract fan number
                    if let Some(fan_num_str) = file_name
                        .strip_prefix("fan")
                        .and_then(|s| s.strip_suffix("_input"))
                    {
                        if let Ok(fan_number) = fan_num_str.parse::<u8>() {
                            fan_files.push((fan_number, path.to_string_lossy().to_string()));
                        }
                    }
                }
            }
        }

        // Sort by fan number
        fan_files.sort_by_key(|(num, _)| *num);

        // Create fan sensors and prioritize CPU Fan
        let mut cpu_fan_found = false;
        
        for (fan_number, input_path) in fan_files {
            let label_path = hwmon_dir.join(format!("fan{}_label", fan_number));
            
            if label_path.exists() {
                let fan_label = fs::read_to_string(&label_path)
                    .unwrap_or_else(|_| format!("Fan {}", fan_number))
                    .trim()
                    .to_string();
                
                let fan_sensor = FanSensor {
                    fan_number,
                    hwmon_path: hwmon_path.clone(),
                    fan_input_path: input_path,
                    fan_label_path: label_path.to_string_lossy().to_string(),
                    fan_label,
                };
                
                // Prioritize CPU Fan by adding it first
                if fan_sensor.fan_label == "CPU Fan" {
                    info!("Found CPU Fan sensor: Fan {} - {}", fan_number, fan_sensor.fan_label);
                    self.fans.insert(0, fan_sensor);
                    cpu_fan_found = true;
                } else {
                    info!("Found fan sensor: Fan {} - {}", fan_number, fan_sensor.fan_label);
                    self.fans.push(fan_sensor);
                }
            } else {
                warn!("Fan {} input found but no corresponding label file", fan_number);
            }
        }

        if self.fans.is_empty() {
            return Err(crate::errors::FanCurveError::Config(
                "No fan sensors found in detected device".to_string()
            ));
        }

        if !cpu_fan_found {
            warn!("No CPU Fan found, but {} other fans detected", self.fans.len());
        }

        Ok(())
    }

    /// Read fan speed for a specific fan
    pub fn read_fan_speed(&self, fan_number: u8) -> Result<u16> {
        if let Some(fan) = self.fans.iter().find(|f| f.fan_number == fan_number) {
            info!("Reading fan {} from path: {}", fan_number, fan.fan_input_path);
            let speed_content = fs::read_to_string(&fan.fan_input_path)?;
            let raw_speed: u16 = speed_content.trim().parse()
                .map_err(|_| crate::errors::FanCurveError::Config(
                    "Failed to parse fan speed".to_string()
                ))?;
            
            info!("Fan {} raw reading: {} RPM from {}", fan_number, raw_speed, fan.fan_input_path);
            
            // Use raw sensor reading directly as RPM
            Ok(raw_speed)
        } else {
            warn!("Fan {} not found in detected fans: {:?}", fan_number, 
                  self.fans.iter().map(|f| f.fan_number).collect::<Vec<_>>());
            Err(crate::errors::FanCurveError::Config(
                format!("Fan {} not found", fan_number)
            ))
        }
    }

    /// Read all fan speeds
    pub fn read_all_fan_speeds(&self) -> Result<Vec<(u8, u16, String)>> {
        let mut speeds = Vec::new();
        
        // Since fans are already prioritized with CPU Fan first, just read them in order
        for fan in &self.fans {
            let speed = self.read_fan_speed(fan.fan_number)?;
            speeds.push((fan.fan_number, speed, fan.fan_label.clone()));
        }
        
        Ok(speeds)
    }

    /// Get all detected fans
    pub fn get_fans(&self) -> &[FanSensor] {
        &self.fans
    }

    /// Get fan by number
    pub fn get_fan(&self, fan_number: u8) -> Option<&FanSensor> {
        self.fans.iter().find(|f| f.fan_number == fan_number)
    }

    /// Get the CPU fan specifically
    pub fn get_cpu_fan(&self) -> Option<&FanSensor> {
        self.fans.iter().find(|f| f.fan_label == "CPU Fan")
    }

    /// Read CPU fan speed specifically
    pub fn read_cpu_fan_speed(&self) -> Result<Option<(u8, u16, String)>> {
        if let Some(cpu_fan) = self.get_cpu_fan() {
            let speed = self.read_fan_speed(cpu_fan.fan_number)?;
            Ok(Some((cpu_fan.fan_number, speed, cpu_fan.fan_label.clone())))
        } else {
            Ok(None)
        }
    }

    /// Check if the detector is initialized
    pub fn is_initialized(&self) -> bool {
        !self.fans.is_empty()
    }

    /// Get the number of detected fans
    pub fn fan_count(&self) -> usize {
        self.fans.len()
    }
}

impl Default for FanDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fan_detector_creation() {
        let detector = FanDetector::new();
        assert!(!detector.is_initialized());
        assert_eq!(detector.fan_count(), 0);
    }
}
