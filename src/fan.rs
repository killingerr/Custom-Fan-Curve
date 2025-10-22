use crate::errors::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use zvariant::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FanPoint {
    pub temp: i16,
    pub duty: u16,
}

impl FanPoint {
    pub fn new(temp: i16, duty: u16) -> Self {
        Self { temp, duty }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FanCurve {
    name: String,
    points: Vec<FanPoint>,
}

impl FanCurve {
    pub fn new(name: String) -> Self {
        Self {
            name,
            points: Vec::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    pub fn points(&self) -> &[FanPoint] {
        &self.points
    }

    pub fn points_mut(&mut self) -> &mut Vec<FanPoint> {
        &mut self.points
    }

    pub fn add_point(&mut self, temp: i16, duty: u16) {
        self.points.push(FanPoint::new(temp, duty));
        self.points.sort_by_key(|p| p.temp);
    }

    pub fn remove_last_point(&mut self) -> Option<FanPoint> {
        self.points.pop()
    }

    pub fn remove_point(&mut self, index: usize) -> Option<FanPoint> {
        if index < self.points.len() {
            Some(self.points.remove(index))
        } else {
            None
        }
    }

    pub fn get_point(&self, index: usize) -> Option<&FanPoint> {
        self.points.get(index)
    }

    /// Calculate fan duty for a given temperature using linear interpolation
    /// Returns duty in ten-thousandths (0-10000) to match system76-power standard
    /// Temperature is in thousandths of Celsius (e.g., 35000 = 35.0°C)
    pub fn calculate_duty_for_temperature(&self, temp_thousandths: u32) -> u16 {
        if self.points.is_empty() {
            return 0;
        }

        // Convert thousandths to tenths for comparison with curve points
        // 30000 thousandths = 30.0°C = 30 tenths (if curve points are in tenths)
        let temp_tenths = (temp_thousandths / 1000) as i16;

        // If temperature is below the lowest point, return the duty of the lowest point
        if temp_tenths <= self.points[0].temp {
            return self.points[0].duty;
        }

        // If temperature is above the highest point, return the duty of the highest point
        if temp_tenths >= self.points.last().unwrap().temp {
            return self.points.last().unwrap().duty;
        }

        // Find the two points to interpolate between
        for i in 0..self.points.len() - 1 {
            let point1 = &self.points[i];
            let point2 = &self.points[i + 1];

            if temp_tenths >= point1.temp && temp_tenths <= point2.temp {
                // Linear interpolation between the two points
                let temp1 = point1.temp as f32;
                let temp2 = point2.temp as f32;
                let duty1 = point1.duty as f32;
                let duty2 = point2.duty as f32;
                let temp_current = temp_tenths as f32;

                // Calculate the interpolation factor
                let factor = (temp_current - temp1) / (temp2 - temp1);

                // Interpolate the duty
                let interpolated_duty = duty1 + factor * (duty2 - duty1);

                return interpolated_duty.round() as u16;
            }
        }

        // Fallback (should not reach here)
        0
    }

    /// Calculate fan duty percentage for a given temperature using linear interpolation
    /// This is a convenience method that maintains backward compatibility
    pub fn calculate_duty_for_temperature_celsius(&self, temperature: f32) -> u16 {
        // Convert Celsius to thousandths of Celsius
        let temp_thousandths = (temperature * 1000.0) as u32;
        self.calculate_duty_for_temperature(temp_thousandths)
    }

    pub fn standard() -> Self {
        let mut curve = Self::new("Standard".to_string());
        curve.add_point(0, 0);
        curve.add_point(30, 2000); // 20% = 2000/10000
        curve.add_point(40, 3000); // 30% = 3000/10000
        curve.add_point(50, 4000); // 40% = 4000/10000
        curve.add_point(60, 5000); // 50% = 5000/10000
        curve.add_point(70, 6000); // 60% = 6000/10000
        curve.add_point(80, 7000); // 70% = 7000/10000
        curve.add_point(90, 8000); // 80% = 8000/10000
        curve.add_point(100, 10000); // 100% = 10000/10000
        curve
    }

    pub fn threadripper2() -> Self {
        let mut curve = Self::new("Threadripper 2".to_string());
        curve.add_point(0, 0);
        curve.add_point(25, 1000); // 10% = 1000/10000
        curve.add_point(35, 2000); // 20% = 2000/10000
        curve.add_point(45, 3000); // 30% = 3000/10000
        curve.add_point(55, 4000); // 40% = 4000/10000
        curve.add_point(65, 5000); // 50% = 5000/10000
        curve.add_point(75, 6000); // 60% = 6000/10000
        curve.add_point(85, 7000); // 70% = 7000/10000
        curve.add_point(95, 8000); // 80% = 8000/10000
        curve.add_point(100, 10000); // 100% = 10000/10000
        curve
    }

    pub fn hedt() -> Self {
        let mut curve = Self::new("HEDT".to_string());
        curve.add_point(0, 0);
        curve.add_point(20, 1500); // 15% = 1500/10000
        curve.add_point(30, 2500); // 25% = 2500/10000
        curve.add_point(40, 3500); // 35% = 3500/10000
        curve.add_point(50, 4500); // 45% = 4500/10000
        curve.add_point(60, 5500); // 55% = 5500/10000
        curve.add_point(70, 6500); // 65% = 6500/10000
        curve.add_point(80, 7500); // 75% = 7500/10000
        curve.add_point(90, 8500); // 85% = 8500/10000
        curve.add_point(100, 10000); // 100% = 10000/10000
        curve
    }

    pub fn xeon() -> Self {
        let mut curve = Self::new("Xeon".to_string());
        curve.add_point(0, 0);
        curve.add_point(15, 500); // 5% = 500/10000
        curve.add_point(25, 1500); // 15% = 1500/10000
        curve.add_point(35, 2500); // 25% = 2500/10000
        curve.add_point(45, 3500); // 35% = 3500/10000
        curve.add_point(55, 4500); // 45% = 4500/10000
        curve.add_point(65, 5500); // 55% = 5500/10000
        curve.add_point(75, 6500); // 65% = 6500/10000
        curve.add_point(85, 7500); // 75% = 7500/10000
        curve.add_point(95, 8500); // 85% = 8500/10000
        curve.add_point(100, 10000); // 100% = 10000/10000
        curve
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn load_from_file(path: &Path) -> Result<Self> {
        let json = fs::read_to_string(path)?;
        let curve: FanCurve = serde_json::from_str(&json)?;
        Ok(curve)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FanCurveConfig {
    pub curves: Vec<FanCurve>,
    pub default_curve_index: Option<usize>,
}

impl FanCurveConfig {
    pub fn new() -> Self {
        Self {
            curves: vec![
                FanCurve::standard(),
                FanCurve::threadripper2(),
                FanCurve::hedt(),
                FanCurve::xeon(),
            ],
            default_curve_index: Some(0),
        }
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn load_from_file(path: &Path) -> Result<Self> {
        let json = fs::read_to_string(path)?;
        let config: FanCurveConfig = serde_json::from_str(&json)?;
        Ok(config)
    }

    pub fn get_config_path() -> std::path::PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        std::path::PathBuf::from(home)
            .join(".fan_curve_app")
            .join("config.json")
    }
}

impl Default for FanCurveConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fan_curve_interpolation() {
        let curve = FanCurve::standard();

        // Test exact points (using thousandths of Celsius)
        assert_eq!(curve.calculate_duty_for_temperature(0), 0);
        assert_eq!(curve.calculate_duty_for_temperature(30000), 2000); // 30°C = 20%
        assert_eq!(curve.calculate_duty_for_temperature(70000), 6000); // 70°C = 60%
        assert_eq!(curve.calculate_duty_for_temperature(100000), 10000); // 100°C = 100%

        // Test interpolation between points
        assert_eq!(curve.calculate_duty_for_temperature(35000), 2500); // Between 30°C(20%) and 40°C(30%)
        assert_eq!(curve.calculate_duty_for_temperature(65000), 5500); // Between 60°C(50%) and 70°C(60%)

        // Test edge cases
        assert_eq!(curve.calculate_duty_for_temperature(0), 0); // Below minimum (0°C)
        assert_eq!(curve.calculate_duty_for_temperature(150000), 10000); // Above maximum (150°C)

        // Test backward compatibility with Celsius
        assert_eq!(curve.calculate_duty_for_temperature_celsius(0.0), 0);
        assert_eq!(curve.calculate_duty_for_temperature_celsius(30.0), 2000);
        assert_eq!(curve.calculate_duty_for_temperature_celsius(70.0), 6000);
        assert_eq!(curve.calculate_duty_for_temperature_celsius(100.0), 10000);
    }
}
