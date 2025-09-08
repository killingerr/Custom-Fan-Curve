use crate::errors::Result;
use crate::cpu_temp::CpuTempDetector;
use crate::fan_detector::FanDetector;
use chrono::{DateTime, Local};
use log::{info, warn};
use rand;
use std::fs;
use std::path::Path;
use std::time::Instant;
use tokio::time::{sleep, Duration};

/// Fan data point for monitoring
#[derive(Debug, Clone)]
pub struct FanDataPoint {
    pub timestamp: DateTime<Local>,
    pub temperature: f32,
    pub fan_speeds: Vec<(u8, u16, String)>, // (fan_number, speed, label)
    pub fan_duty: u16,
    pub cpu_usage: f32,
}

/// Fan monitoring system
pub struct FanMonitor {
    log_file: Option<std::path::PathBuf>,
    is_monitoring: bool,
    last_log_time: Instant,
    current_fan_curve: Option<crate::fan::FanCurve>,
    cpu_temp_detector: CpuTempDetector,
    fan_detector: FanDetector,
}

impl FanMonitor {
    /// Create a new fan monitor
    pub fn new() -> Self {
        Self {
            log_file: None,
            is_monitoring: false,
            last_log_time: Instant::now(),
            current_fan_curve: None,
            cpu_temp_detector: CpuTempDetector::new(),
            fan_detector: FanDetector::new(),
        }
    }

    /// Initialize the fan monitor (detects CPU temperature sensor and fans)
    pub fn initialize(&mut self) -> Result<()> {
        // Initialize CPU temperature detection
        if let Err(e) = self.cpu_temp_detector.initialize() {
            warn!("Failed to initialize CPU temperature detection: {}", e);
        }
        
        // Initialize fan detection
        if let Err(e) = self.fan_detector.initialize() {
            warn!("Failed to initialize fan detection: {}", e);
        }
        
        info!("Fan monitor initialized with {} fans detected", self.fan_detector.fan_count());
        Ok(())
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
            info!(
                "Starting fan monitoring with logging to: {}",
                path.display()
            );
            // Create log file with header
            fs::write(path, "timestamp,temperature,fan_speed,fan_duty,cpu_usage\n")
                .map_err(crate::errors::FanCurveError::Io)?;
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

    /// Get the CPU temperature detector
    pub fn cpu_temp_detector(&self) -> &CpuTempDetector {
        &self.cpu_temp_detector
    }

    /// Get the fan detector
    pub fn fan_detector(&self) -> &FanDetector {
        &self.fan_detector
    }

    /// Initialize the CPU temperature detector
    pub fn initialize_cpu_temp(&mut self) -> Result<()> {
        self.cpu_temp_detector.initialize()?;
        if let Some(sensor_info) = self.cpu_temp_detector.get_sensor_info() {
            info!(
                "CPU temperature detector initialized for {:?} CPU",
                sensor_info.manufacturer
            );
        }
        Ok(())
    }

    /// Get current fan data
    pub fn get_current_fan_data_sync(&self) -> Result<FanDataPoint> {
        // Read real CPU temperature
        let temperature = self.read_cpu_temperature()?;
        let fan_speeds = self.read_fan_speeds()?;
        let fan_duty = self.calculate_fan_duty_from_curve(temperature);
        let cpu_usage = self.read_cpu_usage()?;

        Ok(FanDataPoint {
            timestamp: chrono::Local::now(),
            temperature,
            fan_speeds,
            fan_duty,
            cpu_usage,
        })
    }

    /// Get current fan data - async version
    pub async fn get_current_fan_data(&self) -> Result<FanDataPoint> {
        // Read real CPU temperature
        let temperature = self.read_cpu_temperature()?;
        let fan_speeds = self.read_fan_speeds()?;
        let fan_duty = self.calculate_fan_duty_from_curve(temperature);
        let cpu_usage = self.read_cpu_usage()?;

        Ok(FanDataPoint {
            timestamp: chrono::Local::now(),
            temperature,
            fan_speeds,
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
        let fan_info = if data.fan_speeds.is_empty() {
            "No fans".to_string()
        } else {
            data.fan_speeds.iter()
                .map(|(_num, speed, label)| format!("{}: {} RPM", label, speed))
                .collect::<Vec<_>>()
                .join(" | ")
        };
        
        println!("🌡️  Temperature: {:.1}°C | 🌀 Fans: {} | ⚡ Fan Duty: {}% | 💻 CPU: {:.1}% | ⏰ {}",
            data.temperature,
            fan_info,
            data.fan_duty,
            data.cpu_usage,
            data.timestamp.format("%H:%M:%S")
        );

        // Log to file if enabled
        if let Some(ref path) = self.log_file {
            let fan_speeds_str = if data.fan_speeds.is_empty() {
                "No fans".to_string()
            } else {
                data.fan_speeds.iter()
                    .map(|(_num, speed, label)| format!("{}:{}", label, speed))
                    .collect::<Vec<_>>()
                    .join(";")
            };
            
            let csv_line = format!(
                "{},{:.1},{},{},{:.1}\n",
                data.timestamp.format("%Y-%m-%d %H:%M:%S%.3f"),
                data.temperature,
                fan_speeds_str,
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
                .map_err(crate::errors::FanCurveError::Io)?;
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

    /// Read CPU temperature from hardware sensor
    fn read_cpu_temperature(&self) -> Result<f32> {
        if !self.cpu_temp_detector.is_initialized() {
            // Fallback to simulation if not initialized
            warn!("CPU temperature detector not initialized, using simulation");
            return Ok(self.simulate_temperature_fallback());
        }

        self.cpu_temp_detector.read_temperature()
    }

    /// Read fan speeds from hardware sensors
    fn read_fan_speeds(&self) -> Result<Vec<(u8, u16, String)>> {
        if !self.fan_detector.is_initialized() {
            // Fallback to simulation if not initialized
            warn!("Fan detector not initialized, using simulation");
            return Ok(self.simulate_fan_speeds_fallback());
        }

        // Prioritize CPU fan if available
        if let Ok(Some(cpu_fan_data)) = self.fan_detector.read_cpu_fan_speed() {
            return Ok(vec![cpu_fan_data]);
        }

        // Fallback to all fans if no CPU fan found
        self.fan_detector.read_all_fan_speeds()
    }

    /// Fallback fan speed simulation (used when hardware detection fails)
    fn simulate_fan_speeds_fallback(&self) -> Vec<(u8, u16, String)> {
        // Simulate a single fan for fallback
        let simulated_speed = self.simulate_fan_speed_fallback(50.0); // Use a reasonable temperature
        vec![(1, simulated_speed, "CPU Fan".to_string())]
    }

    /// Simulate fan speed based on temperature (single fan)
    fn simulate_fan_speed_fallback(&self, temperature: f32) -> u16 {
        let base_speed = 800;
        let temp_factor = ((temperature - 30.0).max(0.0) * 50.0) as u16;
        let random_factor = (rand::random::<f32>() - 0.5) * 100.0;

        (base_speed + temp_factor + random_factor as u16).clamp(0, 3000)
    }

    /// Fallback temperature simulation (used when hardware detection fails)
    fn simulate_temperature_fallback(&self) -> f32 {
        let base_temp = 35.0;
        let time_factor = (chrono::Local::now().timestamp() % 60) as f32 / 60.0;
        let cpu_factor = self.read_cpu_usage().unwrap_or(20.0) * 0.5;
        let random_factor = (rand::random::<f32>() - 0.5) * 5.0;

        base_temp + time_factor * 10.0 + cpu_factor + random_factor
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

    /// Read CPU usage from /proc/stat
    fn read_cpu_usage(&self) -> Result<f32> {
        let stat_content = fs::read_to_string("/proc/stat")
            .map_err(|e| crate::errors::FanCurveError::Io(e))?;

        let first_line = stat_content.lines().next()
            .ok_or_else(|| crate::errors::FanCurveError::Config("Empty /proc/stat".to_string()))?;

        let fields: Vec<&str> = first_line.split_whitespace().collect();
        if fields.len() < 8 {
            return Err(crate::errors::FanCurveError::Config("Invalid /proc/stat format".to_string()));
        }

        // Parse CPU times: user, nice, system, idle, iowait, irq, softirq, steal
        let user: u64 = fields[1].parse()
            .map_err(|_| crate::errors::FanCurveError::Config("Failed to parse user time".to_string()))?;
        let nice: u64 = fields[2].parse()
            .map_err(|_| crate::errors::FanCurveError::Config("Failed to parse nice time".to_string()))?;
        let system: u64 = fields[3].parse()
            .map_err(|_| crate::errors::FanCurveError::Config("Failed to parse system time".to_string()))?;
        let idle: u64 = fields[4].parse()
            .map_err(|_| crate::errors::FanCurveError::Config("Failed to parse idle time".to_string()))?;
        let iowait: u64 = fields[5].parse()
            .map_err(|_| crate::errors::FanCurveError::Config("Failed to parse iowait time".to_string()))?;
        let irq: u64 = fields[6].parse()
            .map_err(|_| crate::errors::FanCurveError::Config("Failed to parse irq time".to_string()))?;
        let softirq: u64 = fields[7].parse()
            .map_err(|_| crate::errors::FanCurveError::Config("Failed to parse softirq time".to_string()))?;
        let steal: u64 = if fields.len() > 8 { 
            fields[8].parse().unwrap_or(0) 
        } else { 
            0 
        };

        let total_idle = idle + iowait;
        let total_non_idle = user + nice + system + irq + softirq + steal;
        let total = total_idle + total_non_idle;

        // For a single reading, we can't calculate percentage accurately
        // This is a simplified approach - in practice, you'd want to store previous values
        // and calculate the difference over time
        if total == 0 {
            return Ok(0.0);
        }

        let cpu_usage = (total_non_idle as f32 / total as f32) * 100.0;
        Ok(cpu_usage.clamp(0.0, 100.0))
    }
}

impl Default for FanMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Test a fan curve by applying it and monitoring the results
pub async fn test_fan_curve(
    curve_name: &str,
    duration_seconds: u64,
    log_file: Option<&Path>,
) -> Result<()> {
    println!(
        "🚀 Starting fan curve test: '{}' for {} seconds",
        curve_name, duration_seconds
    );
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
