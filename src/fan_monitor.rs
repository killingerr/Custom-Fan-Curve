use crate::cpu_temp::CpuTempDetector;
use crate::errors::Result;
use crate::fan_detector::FanDetector;
use crate::system76_power_client::System76PowerClient;
use chrono::{DateTime, Local};
use futures_util::stream::StreamExt;
use log::{info, warn};
use rand;
use std::fs;
use std::time::Instant;
use tokio::time::{sleep, Duration};
use zbus::{Connection, MatchRule, MessageStream};

/// Fan data point for monitoring
#[derive(Debug, Clone)]
pub struct FanDataPoint {
    pub timestamp: DateTime<Local>,
    pub temperature: f32,
    pub cpu_fan_speeds: Vec<(u8, u16, String)>, // (fan_number, speed, label)
    pub intake_fan_speeds: Vec<(u8, u16, String)>, // (fan_number, speed, label)
    pub gpu_fan_speeds: Vec<(u8, u16, String)>, // (fan_number, speed, label)
    pub fan_duty: u16,
    pub cpu_usage: f32,
}

/// Fan monitoring system
pub struct FanMonitor {
    is_monitoring: bool,
    last_log_time: Instant,
    current_fan_curve: Option<crate::fan::FanCurve>,
    cpu_temp_detector: CpuTempDetector,
    fan_detector: FanDetector,
    system76_power_client: Option<System76PowerClient>,
    dbus_connection: Option<Connection>,
}

impl FanMonitor {
    /// Create a new fan monitor
    pub fn new() -> Self {
        Self {
            is_monitoring: false,
            last_log_time: Instant::now(),
            current_fan_curve: None,
            cpu_temp_detector: CpuTempDetector::new(),
            fan_detector: FanDetector::new(),
            system76_power_client: None,
            dbus_connection: None,
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

        info!(
            "Fan monitor initialized with {} fans detected",
            self.fan_detector.fan_count()
        );
        Ok(())
    }

    /// Initialize System76 Power client
    pub async fn initialize_system76_power(&mut self) -> Result<()> {
        match System76PowerClient::new().await {
            Ok(client) => {
                if client.is_available().await {
                    self.system76_power_client = Some(client);
                    info!("System76 Power client initialized and available");
                } else {
                    warn!("System76 Power service not available");
                }
                Ok(())
            }
            Err(e) => {
                warn!("Failed to initialize System76 Power client: {}", e);
                Ok(()) // Don't fail initialization if System76 Power is not available
            }
        }
    }

    /// Initialize DBus connection for listening to fan curve changes
    pub async fn initialize_dbus(&mut self) -> Result<()> {
        match Connection::system().await {
            Ok(connection) => {
                self.dbus_connection = Some(connection);
                info!("DBus connection initialized for fan curve change notifications");
                Ok(())
            }
            Err(e) => {
                warn!("Failed to initialize DBus connection: {}", e);
                Ok(()) // Don't fail initialization if DBus is not available
            }
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

    /// Start listening for fan curve change signals from the daemon
    pub async fn start_dbus_listener(&mut self) -> Result<()> {
        if let Some(ref connection) = self.dbus_connection {
            // Create a match rule for fan curve changed signals
            let match_rule = MatchRule::builder()
                .msg_type(zbus::MessageType::Signal)
                .sender("com.system76.FanCurveDaemon")?
                .path("/com/system76/FanCurveDaemon")?
                .member("fan_curve_changed")?
                .build();

            // Subscribe to the signal
            let mut stream = MessageStream::for_match_rule(match_rule, connection, None).await?;

            info!("Started listening for fan curve change signals");

            // Spawn a task to handle incoming signals
            tokio::spawn(async move {
                while let Some(msg) = stream.next().await {
                    if let Ok(_msg) = msg {
                        info!("Received fan curve changed signal, updating curve...");

                        // In a real implementation, we would fetch the current curve from the daemon
                        // For now, we'll just log that we received the signal
                        // TODO: Implement actual curve fetching from daemon
                        info!("Fan curve change signal received - curve update needed");
                    }
                }
            });

            Ok(())
        } else {
            warn!("DBus connection not initialized, cannot listen for signals");
            Ok(())
        }
    }

    /// Start monitoring
    pub fn start_monitoring(&mut self) -> Result<()> {
        self.is_monitoring = true;
        self.last_log_time = Instant::now();
        info!("Starting fan monitoring");
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
        let cpu_fan_speeds = self.read_fan_speeds()?;
        let _intake_fan_speeds = self.read_fan_speeds()?;
        let _gpu_fan_speeds = self.read_fan_speeds()?;
        let fan_duty = self.calculate_fan_duty_from_curve(temperature);
        let cpu_usage = self.read_cpu_usage()?;

        Ok(FanDataPoint {
            timestamp: chrono::Local::now(),
            temperature,
            cpu_fan_speeds,
            intake_fan_speeds: Vec::new(),
            gpu_fan_speeds: Vec::new(),
            fan_duty,
            cpu_usage,
        })
    }

    /// Get current fan data - async version
    pub async fn get_current_fan_data(&self) -> Result<FanDataPoint> {
        // Read real CPU temperature
        let temperature = self.read_cpu_temperature()?;
        let cpu_fan_speeds = self.read_fan_speeds()?;
        let fan_duty = self.calculate_fan_duty_from_curve(temperature);
        let cpu_usage = self.read_cpu_usage()?;

        Ok(FanDataPoint {
            timestamp: chrono::Local::now(),
            temperature,
            cpu_fan_speeds,
            intake_fan_speeds: Vec::new(),
            gpu_fan_speeds: Vec::new(),
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

        // Apply fan curve to hardware
        if let Err(e) = self.apply_fan_curve(data.temperature).await {
            warn!("Failed to apply fan curve: {}", e);
        }

        self.last_log_time = Instant::now();

        // Real-time console output with formatting
        let fan_info = if data.cpu_fan_speeds.is_empty() {
            "No fans".to_string()
        } else {
            data.cpu_fan_speeds
                .iter()
                .map(|(_num, speed, label)| format!("{}: {} RPM", label, speed))
                .collect::<Vec<_>>()
                .join(" | ")
        };

        // Convert duty from ten-thousandths to percentage for display
        let duty_percentage = data.fan_duty / 100;

        println!(
            "🌡️  Temperature: {:.1}°C | 🌀 Fans: {} | ⚡ Fan Duty: {}% | 💻 CPU: {:.1}% | ⏰ {}",
            data.temperature,
            fan_info,
            duty_percentage,
            data.cpu_usage,
            data.timestamp.format("%H:%M:%S")
        );

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

        info!("Fan detector initialized, reading from hardware sensors");

        // Prioritize CPU fan if available
        if let Ok(Some(cpu_fan_data)) = self.fan_detector.read_cpu_fan_speed() {
            info!(
                "Found CPU fan: Fan {} at {} RPM",
                cpu_fan_data.0, cpu_fan_data.1
            );
            return Ok(vec![cpu_fan_data]);
        }

        // Fallback to all fans if no CPU fan found
        info!("No CPU fan found, reading all fans");
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
    /// Returns duty in ten-thousandths (0-10000) to match system76-power standard
    fn calculate_fan_duty_from_curve(&self, temperature: f32) -> u16 {
        if let Some(ref curve) = self.current_fan_curve {
            // Convert Celsius to thousandths of Celsius
            let temp_thousandths = (temperature * 1000.0) as u32;
            curve.calculate_duty_for_temperature(temp_thousandths)
        } else {
            // Fallback to simple simulation if no curve is set
            let duty_percent = ((temperature - 30.0).max(0.0) * 2.0) as u16;
            let duty_percent = duty_percent.min(100);
            // Convert percentage to ten-thousandths
            duty_percent * 100
        }
    }

    /// Calculate PWM value from duty (0-10000) to PWM (0-255)
    /// Matches system76-power conversion: (duty * 255) / 10000
    fn duty_to_pwm(&self, duty: u16) -> u8 {
        ((u32::from(duty) * 255) / 10000) as u8
    }

    /// Apply fan curve to hardware via System76 Power DBus interface and direct PWM control
    pub async fn apply_fan_curve(&self, temperature: f32) -> Result<()> {
        if !self.fan_detector.is_initialized() {
            warn!("Fan detector not initialized, cannot apply fan curve");
            return Ok(());
        }

        let duty = self.calculate_fan_duty_from_curve(temperature);
        let duty_percentage = duty / 100; // Convert ten-thousandths to percentage for display

        info!(
            "Applying fan curve: {:.1}°C -> {}% duty ({} ten-thousandths)",
            temperature, duty_percentage, duty
        );

        // Try to use System76 Power client if available (for power profiles)
        if let Some(ref client) = self.system76_power_client {
            if let Err(e) = client.apply_fan_curve(temperature, duty_percentage).await {
                warn!("Failed to apply fan curve via System76 Power: {}", e);
            }
        } else {
            warn!("System76 Power client not initialized");
        }

        // Convert duty (0-10000) to PWM value (0-255) using system76-power formula
        let pwm_value = self.duty_to_pwm(duty);

        // Apply to all fans using the new set_duty method (matches system76-power approach)
        if let Err(e) = self.fan_detector.set_duty(Some(pwm_value)) {
            warn!("Failed to set fan PWM via set_duty: {}", e);

            // Fallback to individual CPU fan control
            if let Some(cpu_fan) = self.fan_detector.get_cpu_fan() {
                info!(
                    "Fallback: Applying direct PWM control to CPU fan {} -> PWM {}",
                    cpu_fan.fan_number, pwm_value
                );
                if let Err(e) = self.fan_detector.set_fan_pwm(cpu_fan.fan_number, pwm_value) {
                    warn!("Failed to set CPU fan PWM directly: {}", e);
                }
            } else {
                warn!("No CPU fan found for direct PWM control");
            }
        } else {
            info!(
                "Applied PWM control to all fans: {} (duty: {})",
                pwm_value, duty
            );
        }

        Ok(())
    }

    /// Read CPU usage from /proc/stat
    fn read_cpu_usage(&self) -> Result<f32> {
        let stat_content =
            fs::read_to_string("/proc/stat").map_err(crate::errors::FanCurveError::Io)?;

        let first_line = stat_content
            .lines()
            .next()
            .ok_or_else(|| crate::errors::FanCurveError::Config("Empty /proc/stat".to_string()))?;

        let fields: Vec<&str> = first_line.split_whitespace().collect();
        if fields.len() < 8 {
            return Err(crate::errors::FanCurveError::Config(
                "Invalid /proc/stat format".to_string(),
            ));
        }

        // Parse CPU times: user, nice, system, idle, iowait, irq, softirq, steal
        let user: u64 = fields[1].parse().map_err(|_| {
            crate::errors::FanCurveError::Config("Failed to parse user time".to_string())
        })?;
        let nice: u64 = fields[2].parse().map_err(|_| {
            crate::errors::FanCurveError::Config("Failed to parse nice time".to_string())
        })?;
        let system: u64 = fields[3].parse().map_err(|_| {
            crate::errors::FanCurveError::Config("Failed to parse system time".to_string())
        })?;
        let idle: u64 = fields[4].parse().map_err(|_| {
            crate::errors::FanCurveError::Config("Failed to parse idle time".to_string())
        })?;
        let iowait: u64 = fields[5].parse().map_err(|_| {
            crate::errors::FanCurveError::Config("Failed to parse iowait time".to_string())
        })?;
        let irq: u64 = fields[6].parse().map_err(|_| {
            crate::errors::FanCurveError::Config("Failed to parse irq time".to_string())
        })?;
        let softirq: u64 = fields[7].parse().map_err(|_| {
            crate::errors::FanCurveError::Config("Failed to parse softirq time".to_string())
        })?;
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
pub async fn test_fan_curve(curve_name: &str, duration_seconds: u64) -> Result<()> {
    println!(
        "🚀 Starting fan curve test: '{}' for {} seconds",
        curve_name, duration_seconds
    );
    println!("⏱️  Real-time monitoring will begin in 3 seconds...\n");

    // Countdown
    for i in (1..=3).rev() {
        println!("⏳ Starting in {}...", i);
        sleep(Duration::from_secs(1)).await;
    }

    println!("🎯 Test started! Press Ctrl+C to stop early.\n");

    let mut monitor = FanMonitor::new();
    monitor.initialize()?;

    // Initialize System76 Power client
    if let Err(e) = monitor.initialize_system76_power().await {
        warn!("Failed to initialize System76 Power client: {}", e);
    }

    // Initialize DBus connection for fan curve change notifications
    if let Err(e) = monitor.initialize_dbus().await {
        warn!("Failed to initialize DBus connection: {}", e);
    }

    // Start listening for fan curve changes
    if let Err(e) = monitor.start_dbus_listener().await {
        warn!("Failed to start DBus listener: {}", e);
    }

    monitor.start_monitoring()?;

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

    Ok(())
}
