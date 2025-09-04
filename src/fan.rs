use serde::{Serialize, Deserialize};
use std::fs;
use std::path::Path;
use crate::errors::{FanCurveError, Result};
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

    /// Calculate fan duty percentage for a given temperature using linear interpolation
    pub fn calculate_duty_for_temperature(&self, temperature: f32) -> u16 {
        if self.points.is_empty() {
            return 0;
        }

        // If temperature is below the lowest point, return the duty of the lowest point
        if temperature <= self.points[0].temp as f32 {
            return self.points[0].duty;
        }

        // If temperature is above the highest point, return the duty of the highest point
        if temperature >= self.points.last().unwrap().temp as f32 {
            return self.points.last().unwrap().duty;
        }

        // Find the two points to interpolate between
        for i in 0..self.points.len() - 1 {
            let point1 = &self.points[i];
            let point2 = &self.points[i + 1];
            
            if temperature >= point1.temp as f32 && temperature <= point2.temp as f32 {
                // Linear interpolation between the two points
                let temp1 = point1.temp as f32;
                let temp2 = point2.temp as f32;
                let duty1 = point1.duty as f32;
                let duty2 = point2.duty as f32;
                
                // Calculate the interpolation factor
                let factor = (temperature - temp1) / (temp2 - temp1);
                
                // Interpolate the duty
                let interpolated_duty = duty1 + factor * (duty2 - duty1);
                
                return interpolated_duty.round() as u16;
            }
        }

        // Fallback (should not reach here)
        0
    }

    pub fn standard() -> Self {
        let mut curve = Self::new("Standard".to_string());
        curve.add_point(0, 0);
        curve.add_point(30, 20);
        curve.add_point(40, 30);
        curve.add_point(50, 40);
        curve.add_point(60, 50);
        curve.add_point(70, 60);
        curve.add_point(80, 70);
        curve.add_point(90, 80);
        curve.add_point(100, 100);
        curve
    }

    pub fn threadripper2() -> Self {
        let mut curve = Self::new("Threadripper 2".to_string());
        curve.add_point(0, 0);
        curve.add_point(25, 10);
        curve.add_point(35, 20);
        curve.add_point(45, 30);
        curve.add_point(55, 40);
        curve.add_point(65, 50);
        curve.add_point(75, 60);
        curve.add_point(85, 70);
        curve.add_point(95, 80);
        curve.add_point(100, 100);
        curve
    }

    pub fn hedt() -> Self {
        let mut curve = Self::new("HEDT".to_string());
        curve.add_point(0, 0);
        curve.add_point(20, 15);
        curve.add_point(30, 25);
        curve.add_point(40, 35);
        curve.add_point(50, 45);
        curve.add_point(60, 55);
        curve.add_point(70, 65);
        curve.add_point(80, 75);
        curve.add_point(90, 85);
        curve.add_point(100, 100);
        curve
    }

    pub fn xeon() -> Self {
        let mut curve = Self::new("Xeon".to_string());
        curve.add_point(0, 0);
        curve.add_point(15, 5);
        curve.add_point(25, 15);
        curve.add_point(35, 25);
        curve.add_point(45, 35);
        curve.add_point(55, 45);
        curve.add_point(65, 55);
        curve.add_point(75, 65);
        curve.add_point(85, 75);
        curve.add_point(95, 85);
        curve.add_point(100, 100);
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
        std::path::PathBuf::from(home).join(".fan_curve_app").join("config.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fan_curve_interpolation() {
        let curve = FanCurve::standard();
        
        // Test exact points
        assert_eq!(curve.calculate_duty_for_temperature(0.0), 0);
        assert_eq!(curve.calculate_duty_for_temperature(30.0), 20);
        assert_eq!(curve.calculate_duty_for_temperature(70.0), 60);
        assert_eq!(curve.calculate_duty_for_temperature(100.0), 100);
        
        // Test interpolation between points
        assert_eq!(curve.calculate_duty_for_temperature(35.0), 25); // Between 30°C(20%) and 40°C(30%)
        assert_eq!(curve.calculate_duty_for_temperature(65.0), 55); // Between 60°C(50%) and 70°C(60%)
        
        // Test edge cases
        assert_eq!(curve.calculate_duty_for_temperature(-10.0), 0); // Below minimum
        assert_eq!(curve.calculate_duty_for_temperature(150.0), 100); // Above maximum
    }
}