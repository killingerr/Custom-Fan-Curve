//! Fan monitoring and data logging for testing fan curves

use crate::errors::Result;
use log::{info, warn};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Fan data point for logging and monitoring
#[derive(Debug, Clone)]
pub struct FanDataPoint {
    pub timestamp: chrono::DateTime<chrono::Local>,
    pub temperature: f32,
    pub fan_speed: u16,
    pub fan_duty: u16,
    pub cpu_usage: f32,
}

/// Fan monitoring system
pub struct FanMonitor {
    log_file: Option<std::path::PathBuf>,
    is_monitoring: bool,
    last_log_time: Instant,
    current_fan_curve: Option<crate::fan::FanCurve>,
}

impl FanMonitor {
    /// Create a new fan monitor
    pub fn new() -> Self {
        Self {
            log_file: None,
            is_monitoring: false,
            last_log_time: Instant::now(),
            current_fan_curve: None,
        }
    }

    /// Set the current fan curve for duty calculation
    pub fn set_fan_curve(&mut self, curve: crate::fan::FanCurve) {
        self.current_fan_curve = Some(curve);
    }

    /// Update the current fan curve for duty calculation
    pub fn update_fan_curve(&mut self, curve: crate::fan::FanCurve) {
        self.current_fan_curve = Some(curve);
    }

    /// Start monitoring with logging to file
    pub fn start_monitoring(&mut self, log_file: Option<&Path>) -> Result<()> {
        self.log_file = log_file.map(|p| p.to_path_buf());
        self.is_monitoring = true;
        self.last_log_time = Instant::now();
        
        if let Some(ref path) = self.log_file {
            info!("Starting fan monitoring with logging to: {}", path.display());
            // Create log file with header
            fs::write(path, "timestamp,temperature,fan_speed,fan_duty,cpu_usage\n")
                .map_err(|e| crate::errors::FanCurveError::Io(e))?;
        } else {
            info!("Starting fan monitoring without file logging");
        }
        
        Ok(())
    }

    /// Stop monitoring
    pub fn stop_monitoring(&mut self) {
        self.is_monitoring = false;
        info!("Stopped fan monitoring");
    }

    /// Check if currently monitoring
    pub fn is_monitoring(&self) -> bool {
        self.is_monitoring
    }

    /// Get current fan data (simulated for now)
    pub fn get_current_fan_data_sync(&self) -> Result<FanDataPoint> {
        // In a real implementation, this would read from:
        // - /sys/class/thermal/thermal_zone*/temp for temperature
        // - /sys/class/hwmon/hwmon*/fan*_input for fan speed
        // - /proc/stat for CPU usage
        
        // For now, we'll simulate realistic data
        let temperature = self.simulate_temperature();
        let fan_speed = self.simulate_fan_speed(temperature);
        let fan_duty = self.calculate_fan_duty_from_curve(temperature);
        let cpu_usage = self.simulate_cpu_usage();

        Ok(FanDataPoint {
            timestamp: chrono::Local::now(),
            temperature,
            fan_speed,
            fan_duty,
            cpu_usage,
        })
    }

    /// Get current fan data (simulated for now) - async version
    pub async fn get_current_fan_data(&self) -> Result<FanDataPoint> {
        // In a real implementation, this would read from:
        // - /sys/class/thermal/thermal_zone*/temp for temperature
        // - /sys/class/hwmon/hwmon*/fan*_input for fan speed
        // - /proc/stat for CPU usage
        
        // For now, we'll simulate realistic data
        let temperature = self.simulate_temperature();
        let fan_speed = self.simulate_fan_speed(temperature);
        let fan_duty = self.calculate_fan_duty_from_curve(temperature);
        let cpu_usage = self.simulate_cpu_usage();

        Ok(FanDataPoint {
            timestamp: chrono::Local::now(),
            temperature,
            fan_speed,
            fan_duty,
            cpu_usage,
        })
    }

    /// Log fan data if monitoring is enabled
    pub async fn log_fan_data(&mut self) -> Result<()> {
        if !self.is_monitoring {
            return Ok(());
        }

        // Log every 1 second for real-time updates
        if self.last_log_time.elapsed() < Duration::from_secs(1) {
            return Ok(());
        }

        let data = self.get_current_fan_data().await?;
        self.last_log_time = Instant::now();

        // Real-time console output with formatting
        println!("🌡️  Temperature: {:.1}°C | 🌀 Fan Speed: {} RPM | ⚡ Fan Duty: {}% | 💻 CPU: {:.1}% | ⏰ {}", 
            data.temperature, 
            data.fan_speed, 
            data.fan_duty, 
            data.cpu_usage,
            data.timestamp.format("%H:%M:%S")
        );

        // Log to file if enabled
        if let Some(ref path) = self.log_file {
            let csv_line = format!("{},{:.1},{},{},{:.1}\n",
                data.timestamp.format("%Y-%m-%d %H:%M:%S%.3f"),
                data.temperature,
                data.fan_speed,
                data.fan_duty,
                data.cpu_usage
            );
            
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut file| {
                    use std::io::Write;
                    file.write_all(csv_line.as_bytes())
                })
                .map_err(|e| crate::errors::FanCurveError::Io(e))?;
        }

        Ok(())
    }

    /// Run monitoring loop
    pub async fn run_monitoring_loop(&mut self) -> Result<()> {
        info!("Starting fan monitoring loop");
        
        while self.is_monitoring {
            if let Err(e) = self.log_fan_data().await {
                warn!("Failed to log fan data: {}", e);
            }
            
            sleep(Duration::from_secs(1)).await;
        }
        
        Ok(())
    }

    /// Simulate temperature based on CPU usage and time
    fn simulate_temperature(&self) -> f32 {
        let base_temp = 35.0;
        let time_factor = (chrono::Local::now().timestamp() % 60) as f32 / 60.0;
        let cpu_factor = self.simulate_cpu_usage() * 0.5;
        let random_factor = (rand::random::<f32>() - 0.5) * 5.0;
        
        base_temp + time_factor * 10.0 + cpu_factor + random_factor
    }

    /// Simulate fan speed based on temperature
    fn simulate_fan_speed(&self, temperature: f32) -> u16 {
        let base_speed = 800;
        let temp_factor = ((temperature - 30.0).max(0.0) * 50.0) as u16;
        let random_factor = (rand::random::<f32>() - 0.5) * 100.0;
        
        (base_speed + temp_factor + random_factor as u16).max(0).min(3000)
    }

    /// Calculate fan duty based on the current fan curve
    fn calculate_fan_duty_from_curve(&self, temperature: f32) -> u16 {
        if let Some(ref curve) = self.current_fan_curve {
            curve.calculate_duty_for_temperature(temperature)
        } else {
            // Fallback to simple simulation if no curve is set
            let duty = ((temperature - 30.0).max(0.0) * 2.0) as u16;
            duty.min(100)
        }
    }

    /// Simulate CPU usage
    fn simulate_cpu_usage(&self) -> f32 {
        let base_usage = 20.0;
        let time_factor = (chrono::Local::now().timestamp() % 30) as f32 / 30.0;
        let random_factor = (rand::random::<f32>() - 0.5) * 30.0;
        
        (base_usage + time_factor * 40.0 + random_factor).max(0.0).min(100.0)
    }
}

/// Test a fan curve by applying it and monitoring the results
pub async fn test_fan_curve(
    curve_name: &str,
    duration_seconds: u64,
    log_file: Option<&Path>
) -> Result<()> {
    println!("🚀 Starting fan curve test: '{}' for {} seconds", curve_name, duration_seconds);
    if let Some(path) = log_file {
        println!("📁 Logging data to: {}", path.display());
    }
    println!("⏱️  Real-time monitoring will begin in 3 seconds...\n");
    
    // Countdown
    for i in (1..=3).rev() {
        println!("⏳ Starting in {}...", i);
        sleep(Duration::from_secs(1)).await;
    }
    
    println!("🎯 Test started! Press Ctrl+C to stop early.\n");
    
    let mut monitor = FanMonitor::new();
    monitor.start_monitoring(log_file)?;
    
    // Start monitoring in background
    let monitor_handle = {
        let mut monitor = monitor;
        tokio::spawn(async move {
            if let Err(e) = monitor.run_monitoring_loop().await {
                warn!("Fan monitoring error: {}", e);
            }
        })
    };
    
    // Show countdown during test
    for remaining in (1..=duration_seconds).rev() {
        if remaining % 10 == 0 || remaining <= 10 {
            println!("⏰ Time remaining: {} seconds", remaining);
        }
        sleep(Duration::from_secs(1)).await;
    }
    
    // Stop monitoring
    monitor_handle.abort();
    
    println!("\n✅ Fan curve test completed!");
    if let Some(path) = log_file {
        println!("📊 Data saved to: {}", path.display());
    }
    
    Ok(())
}
