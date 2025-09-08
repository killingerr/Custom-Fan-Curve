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
                        
                        if name == "system76_thelio_io" || name == "system76" {
                            self.hwmon_path = Some(path.to_string_lossy().to_string());
                            info!("Found System76 sensor '{}' at: {}", name, path.display());
                            return Ok(());
                        }
                    }
                }
            }
        }

        Err(crate::errors::FanCurveError::Config(
            "System76 Thelio IO sensor not found".to_string()
        ))
    }

    /// Find the CPU Fan sensor in the System76 Thelio IO directory
    fn find_fan_sensors(&mut self) -> Result<()> {
        let hwmon_path = self.hwmon_path.as_ref()
            .ok_or_else(|| crate::errors::FanCurveError::Config(
                "System76 Thelio IO sensor path not found".to_string()
            ))?;

        let hwmon_dir = Path::new(hwmon_path);
        info!("Searching for CPU Fan in directory: {}", hwmon_dir.display());
        
        // Search through fan1_label, fan2_label, fan3_label, etc. until we find "CPU Fan"
        let mut fan_number = 1;
        loop {
            let label_path = hwmon_dir.join(format!("fan{}_label", fan_number));
            let input_path = hwmon_dir.join(format!("fan{}_input", fan_number));
            
            info!("Checking fan{}_label at: {}", fan_number, label_path.display());
            
            if label_path.exists() && input_path.exists() {
                if let Ok(label_content) = fs::read_to_string(&label_path) {
                    let fan_label = label_content.trim().to_string();
                    info!("Found fan{}_label: '{}'", fan_number, fan_label);
                    
                    if fan_label == "CPU Fan" || fan_label == "CPU fan" || fan_label.to_lowercase().contains("cpu") {
                        info!("Found CPU Fan at fan{}! Using fan{}_input for data", fan_number, fan_number);
                        
                        let fan_sensor = FanSensor {
                            fan_number,
                            hwmon_path: hwmon_path.clone(),
                            fan_input_path: input_path.to_string_lossy().to_string(),
                            fan_label_path: label_path.to_string_lossy().to_string(),
                            fan_label: fan_label.clone(),
                        };
                        
                        self.fans.push(fan_sensor);
                        info!("CPU Fan sensor added: Fan {} - {} -> {}", 
                              fan_number, fan_label, input_path.display());
                        return Ok(());
                    }
                }
            } else {
                // No more fan files found, stop searching
                break;
            }
            
            fan_number += 1;
            
            // Safety limit to prevent infinite loop
            if fan_number > 10 {
                break;
            }
        }

        Err(crate::errors::FanCurveError::Config(
            "CPU Fan not found in System76 Thelio IO".to_string()
        ))
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
        let cpu_fan = self.fans.iter().find(|f| f.fan_label == "CPU Fan" || 
                                   f.fan_label == "CPU fan" || 
                                   f.fan_label.to_lowercase().contains("cpu"));
        if cpu_fan.is_none() {
            warn!("CPU Fan not found. Available fans: {:?}", 
                  self.fans.iter().map(|f| (f.fan_number, &f.fan_label)).collect::<Vec<_>>());
        }
        cpu_fan
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

    /// Set fan PWM duty (0-255, where 255 = 100%)
    pub fn set_fan_pwm(&self, fan_number: u8, duty: u8) -> Result<()> {
        if let Some(fan) = self.fans.iter().find(|f| f.fan_number == fan_number) {
            let pwm_path = Path::new(&fan.hwmon_path).join(format!("pwm{}", fan_number));
            let pwm_enable_path = Path::new(&fan.hwmon_path).join(format!("pwm{}_enable", fan_number));
            
            info!("Setting fan {} PWM to {} (duty: {})", fan_number, duty, duty);
            info!("PWM paths: enable={}, pwm={}", pwm_enable_path.display(), pwm_path.display());
            
            // Check if files exist and are writable
            if !pwm_enable_path.exists() {
                return Err(crate::errors::FanCurveError::Config(
                    format!("PWM enable file not found: {}", pwm_enable_path.display())
                ));
            }
            if !pwm_path.exists() {
                return Err(crate::errors::FanCurveError::Config(
                    format!("PWM file not found: {}", pwm_path.display())
                ));
            }
            
            // Enable PWM control (1 = manual control, 2 = automatic)
            fs::write(&pwm_enable_path, "1").map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    crate::errors::FanCurveError::PermissionDenied(format!(
                        "Failed to enable PWM control for fan {} at {}: {}",
                        fan_number,
                        pwm_enable_path.display(),
                        e
                    ))
                } else {
                    crate::errors::FanCurveError::Io(e)
                }
            })?;
            
            // Set PWM duty (0-255)
            fs::write(&pwm_path, duty.to_string()).map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    crate::errors::FanCurveError::PermissionDenied(format!(
                        "Failed to set PWM duty for fan {} at {}: {}",
                        fan_number,
                        pwm_path.display(),
                        e
                    ))
                } else {
                    crate::errors::FanCurveError::Io(e)
                }
            })?;
            
            info!("Fan {} PWM set to {} at {}", fan_number, duty, pwm_path.display());
            Ok(())
        } else {
            Err(crate::errors::FanCurveError::Config(
                format!("Fan {} not found for PWM control", fan_number)
            ))
        }
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
