//! Local sensor readings for the GUI when no daemon reports them: CPU
//! temperature, fan speeds and CPU usage.

use crate::cpu_temp::CpuTempDetector;
use crate::errors::{FanCurveError, Result};
use crate::fan_detector::FanDetector;
use chrono::{DateTime, Local};
use log::{info, warn};
use std::fs;
use std::sync::Mutex;

/// Fan data point for monitoring
#[derive(Debug, Clone)]
pub struct FanDataPoint {
    pub timestamp: DateTime<Local>,
    pub temperature: f32,
    pub fan_speeds: Vec<(u8, u16, String)>, // (fan_number, speed, label)
    pub fan_duty: u16,
    pub cpu_usage: f32,
}

/// Reads the CPU temperature sensor and fans directly
pub struct FanMonitor {
    current_fan_curve: Option<crate::fan::FanCurve>,
    cpu_temp_detector: CpuTempDetector,
    fan_detector: FanDetector,
    /// Previous /proc/stat CPU times (idle, total), so usage can be measured
    /// over the interval between readings rather than since boot
    prev_cpu_times: Mutex<Option<(u64, u64)>>,
}

impl FanMonitor {
    pub fn new() -> Self {
        Self {
            current_fan_curve: None,
            cpu_temp_detector: CpuTempDetector::new(),
            fan_detector: FanDetector::new(),
            prev_cpu_times: Mutex::new(None),
        }
    }

    /// Detect the CPU temperature sensor and fans
    pub fn initialize(&mut self) -> Result<()> {
        if let Err(e) = self.cpu_temp_detector.initialize() {
            warn!("Failed to initialize CPU temperature detection: {}", e);
        }
        if let Err(e) = self.fan_detector.initialize() {
            warn!("Failed to initialize fan detection: {}", e);
        }
        info!(
            "Fan monitor initialized with {} fans detected",
            self.fan_detector.fan_count()
        );
        Ok(())
    }

    /// Curve used to show the duty the fans would run at
    pub fn set_fan_curve(&mut self, curve: crate::fan::FanCurve) {
        self.current_fan_curve = Some(curve);
    }

    pub fn cpu_temp_detector(&self) -> &CpuTempDetector {
        &self.cpu_temp_detector
    }

    /// Current readings, or an error when no CPU temperature sensor was found
    pub fn get_current_fan_data_sync(&self) -> Result<FanDataPoint> {
        if !self.cpu_temp_detector.is_initialized() {
            return Err(FanCurveError::Config(
                "No CPU temperature sensor detected".to_string(),
            ));
        }
        let temperature = self.cpu_temp_detector.read_temperature()?;
        let fan_speeds = if self.fan_detector.is_initialized() {
            self.fan_detector.read_all_fan_speeds()?
        } else {
            Vec::new()
        };
        let fan_duty = self
            .current_fan_curve
            .as_ref()
            .map_or(0, |c| c.calculate_duty_for_temperature_celsius(temperature));

        Ok(FanDataPoint {
            timestamp: chrono::Local::now(),
            temperature,
            fan_speeds,
            fan_duty,
            cpu_usage: self.read_cpu_usage()?,
        })
    }

    /// CPU usage in percent since the previous call (since boot on the first)
    pub fn read_cpu_usage(&self) -> Result<f32> {
        let stat_content = fs::read_to_string("/proc/stat").map_err(FanCurveError::Io)?;
        let current = parse_cpu_times(&stat_content)?;
        let previous = self.prev_cpu_times.lock().unwrap().replace(current);
        Ok(cpu_usage_between(previous.unwrap_or((0, 0)), current))
    }
}

impl Default for FanMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Aggregate CPU times from /proc/stat as (idle, total), in clock ticks.
fn parse_cpu_times(stat_content: &str) -> Result<(u64, u64)> {
    let first_line = stat_content
        .lines()
        .next()
        .ok_or_else(|| crate::errors::FanCurveError::Config("Empty /proc/stat".to_string()))?;

    // cpu user nice system idle iowait irq softirq steal ...
    let fields: Vec<u64> = first_line
        .split_whitespace()
        .skip(1)
        .take(8)
        .map(|f| f.parse())
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| {
            crate::errors::FanCurveError::Config("Invalid /proc/stat format".to_string())
        })?;
    if fields.len() < 7 {
        return Err(crate::errors::FanCurveError::Config(
            "Invalid /proc/stat format".to_string(),
        ));
    }

    let idle = fields[3] + fields[4]; // idle + iowait
    let total = fields.iter().sum();
    Ok((idle, total))
}

/// Percent of time the CPU was busy between two (idle, total) samples.
fn cpu_usage_between(previous: (u64, u64), current: (u64, u64)) -> f32 {
    let idle = current.0.saturating_sub(previous.0);
    let total = current.1.saturating_sub(previous.1);
    if total == 0 {
        return 0.0;
    }
    ((total - idle.min(total)) as f32 / total as f32 * 100.0).clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_aggregate_cpu_line() {
        let stat = "cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 50 0 25 400 25 0 0 0 0 0\n";
        assert_eq!(parse_cpu_times(stat).unwrap(), (850, 1000));
    }

    #[test]
    fn usage_is_measured_between_samples() {
        // 1000 ticks pass, 250 of them idle -> 75% busy
        assert_eq!(cpu_usage_between((850, 1000), (1100, 2000)), 75.0);
        // No time passed
        assert_eq!(cpu_usage_between((850, 1000), (850, 1000)), 0.0);
    }
}
