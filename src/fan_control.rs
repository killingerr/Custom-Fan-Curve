use std::fs;
use std::path::Path;
use crate::errors::FanCurveError;
use crate::errors::Result;
use log::{info, debug};

/// Fan control information
#[derive(Debug, Clone)]
pub struct FanControlInfo {
    pub hwmon_path: String,
    pub pwm_path: String,
    pub fan_input_path: String,
    pub fan_label_path: String,
    pub device_name: String,
}

/// Fan controller for PWM control
pub struct FanController {
    control_info: Option<FanControlInfo>,
}

impl FanController {
    /// Create a new fan controller
    pub fn new() -> Self {
        Self { control_info: None }
    }

    /// Initialize the fan controller by detecting Thelio IO board
    pub fn initialize(&mut self) -> Result<()> {
        info!("Initializing fan controller...");
        
        // Look for Thelio IO board in hwmon
        let control_info = self.find_thelio_io_board()?;
        self.control_info = Some(control_info);
        
        info!("Fan controller initialized: {:?}", self.control_info);
        Ok(())
    }

    /// Find Thelio IO board in /sys/class/hwmon
    fn find_thelio_io_board(&self) -> Result<FanControlInfo> {
        let hwmon_dir = Path::new("/sys/class/hwmon");
        
        if !hwmon_dir.exists() {
            return Err(FanCurveError::Config("Hardware monitoring directory not found".to_string()));
        }

        let entries = fs::read_dir(hwmon_dir)?;

        for entry in entries {
            let entry = entry?;
            let hwmon_path = entry.path();
            
            if !hwmon_path.is_dir() {
                continue;
            }

            // Read the name file to identify the device
            let name_path = hwmon_path.join("name");
            if let Ok(name_content) = fs::read_to_string(&name_path) {
                let device_name = name_content.trim().to_string();
                
                // Look for Thelio IO board (this might be "thelio-io" or similar)
                // We'll also check for common fan control devices
                let is_fan_control_device = device_name.contains("thelio") || 
                                          device_name.contains("io") ||
                                          device_name.contains("pwm") ||
                                          self.has_pwm_files(&hwmon_path);

                if is_fan_control_device {
                    // Check if this device has PWM control files
                    if let Ok(pwm_path) = self.find_pwm_file(&hwmon_path) {
                        let fan_input_path = self.find_fan_input_file(&hwmon_path)?;
                        let fan_label_path = self.find_fan_label_file(&hwmon_path)?;
                        
                        return Ok(FanControlInfo {
                            hwmon_path: hwmon_path.to_string_lossy().to_string(),
                            pwm_path,
                            fan_input_path,
                            fan_label_path,
                            device_name,
                        });
                    }
                }
            }
        }

        Err(FanCurveError::Config("Could not find Thelio IO board or compatible fan control device".to_string()))
    }

    /// Check if a hwmon device has PWM files
    fn has_pwm_files(&self, hwmon_path: &Path) -> bool {
        // Look for pwm1, pwm2, etc.
        for i in 1..=4 {
            let pwm_file = hwmon_path.join(format!("pwm{}", i));
            if pwm_file.exists() {
                return true;
            }
        }
        false
    }

    /// Find the first available PWM file
    fn find_pwm_file(&self, hwmon_path: &Path) -> Result<String> {
        for i in 1..=4 {
            let pwm_file = hwmon_path.join(format!("pwm{}", i));
            if pwm_file.exists() {
                return Ok(pwm_file.to_string_lossy().to_string());
            }
        }
        Err(FanCurveError::Config("No PWM files found".to_string()))
    }

    /// Find fan input file (fan1_input, fan2_input, etc.)
    fn find_fan_input_file(&self, hwmon_path: &Path) -> Result<String> {
        for i in 1..=4 {
            let fan_file = hwmon_path.join(format!("fan{}_input", i));
            if fan_file.exists() {
                return Ok(fan_file.to_string_lossy().to_string());
            }
        }
        Err(FanCurveError::Config("No fan input files found".to_string()))
    }

    /// Find fan label file
    fn find_fan_label_file(&self, hwmon_path: &Path) -> Result<String> {
        for i in 1..=4 {
            let fan_file = hwmon_path.join(format!("fan{}_label", i));
            if fan_file.exists() {
                return Ok(fan_file.to_string_lossy().to_string());
            }
        }
        Err(FanCurveError::Config("No fan label files found".to_string()))
    }

    /// Set fan PWM value (0-255)
    pub fn set_fan_pwm(&self, pwm_value: u8) -> Result<()> {
        let control_info = self.control_info.as_ref()
            .ok_or_else(|| FanCurveError::Config("Fan controller not initialized".to_string()))?;

        // PWM value is already validated by the u8 type (0-255)

        // Write PWM value to the control file
        fs::write(&control_info.pwm_path, pwm_value.to_string())
            .map_err(|e| FanCurveError::Io(e))?;

        debug!("Set fan PWM to {} ({}%)", pwm_value, (pwm_value as f32 / 255.0 * 100.0) as u8);
        Ok(())
    }

    /// Read current fan speed in RPM
    pub fn read_fan_speed(&self) -> Result<u16> {
        let control_info = self.control_info.as_ref()
            .ok_or_else(|| FanCurveError::Config("Fan controller not initialized".to_string()))?;

        let speed_content = fs::read_to_string(&control_info.fan_input_path)?;
        let speed: u16 = speed_content.trim().parse()
            .map_err(|_| FanCurveError::Config("Failed to parse fan speed".to_string()))?;

        Ok(speed)
    }

    /// Read current PWM value
    pub fn read_fan_pwm(&self) -> Result<u8> {
        let control_info = self.control_info.as_ref()
            .ok_or_else(|| FanCurveError::Config("Fan controller not initialized".to_string()))?;

        let pwm_content = fs::read_to_string(&control_info.pwm_path)?;
        let pwm: u8 = pwm_content.trim().parse()
            .map_err(|_| FanCurveError::Config("Failed to parse PWM value".to_string()))?;

        Ok(pwm)
    }

    /// Set fan duty cycle as percentage (0-100)
    pub fn set_fan_duty(&self, duty_percent: u8) -> Result<()> {
        if duty_percent > 100 {
            return Err(FanCurveError::Config("Duty cycle must be between 0 and 100".to_string()));
        }

        // Convert percentage to PWM value (0-255)
        let pwm_value = ((duty_percent as f32 / 100.0) * 255.0) as u8;
        self.set_fan_pwm(pwm_value)
    }

    /// Get control information
    pub fn get_control_info(&self) -> Option<&FanControlInfo> {
        self.control_info.as_ref()
    }

    /// Check if the controller is initialized
    pub fn is_initialized(&self) -> bool {
        self.control_info.is_some()
    }
}

impl Default for FanController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pwm_conversion() {
        let controller = FanController::new();
        // Test PWM conversion logic
        assert_eq!(0, ((0.0 / 100.0) * 255.0) as u8);
        assert_eq!(255, ((100.0 / 100.0) * 255.0) as u8);
        assert_eq!(127, ((50.0 / 100.0) * 255.0) as u8);
    }
}
