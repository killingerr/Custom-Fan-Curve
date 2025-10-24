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
    pub cpu_model: String,
}

/// Fan monitoring system
#[derive(Clone)]
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

    /// Get current fan data with automatic D-Bus initialization
    pub fn get_current_fan_data_with_dbus(&mut self) -> Result<FanDataPoint> {
        // Initialize D-Bus if not already initialized
        if self.system76_power_client.is_none() {
            if let Err(e) = self.initialize_system76_power_sync() {
                warn!("Failed to initialize D-Bus client: {}", e);
            }
        }
        
        // Get data using the synchronous method
        self.get_current_fan_data_sync()
    }

    /// Get current fan data using direct file reading (for display)
    pub fn get_current_fan_data_direct(&self) -> Result<FanDataPoint> {
        log::debug!("FanMonitor::get_current_fan_data_direct called");
        
        // Use existing detectors for direct file reading (no D-Bus needed)
        let temperature = if self.cpu_temp_detector.is_initialized() {
            self.cpu_temp_detector.read_temperature()?
        } else {
            // Initialize CPU temp detector if not already initialized
            let mut temp_detector = self.cpu_temp_detector.clone();
            temp_detector.initialize()?;
            temp_detector.read_temperature()?
        };
        
        let cpu_fan_speeds = if self.fan_detector.is_initialized() {
            self.fan_detector.read_all_fan_speeds()?
        } else {
            // Initialize fan detector if not already initialized
            let mut fan_detector = self.fan_detector.clone();
            fan_detector.initialize()?;
            fan_detector.read_all_fan_speeds()?
        };
        
        // Read current fan duty from PWM files
        let fan_duty = self.read_current_fan_duty_from_pwm()?;
        let cpu_usage = self.read_cpu_usage_direct().unwrap_or(0.0);
        let cpu_model = self.get_cpu_model();
        
        // Create empty vectors for other fan types (we can add these later if needed)
        let intake_fan_speeds = Vec::new();
        let gpu_fan_speeds = Vec::new();
        
        let data_point = FanDataPoint {
            temperature,
            fan_duty,
            cpu_fan_speeds: cpu_fan_speeds.clone(),
            intake_fan_speeds,
            gpu_fan_speeds,
            cpu_usage,
            cpu_model,
            timestamp: chrono::Local::now(),
        };
        
        log::debug!("Direct file reading - Temperature: {:.1}°C, Fan Duty: {:.1}%, Fan RPMs: {:?}", 
            temperature, fan_duty as f32 / 100.0, cpu_fan_speeds);
        
        Ok(data_point)
    }

    /// Read current fan duty from PWM files using existing fan detector
    fn read_current_fan_duty_from_pwm(&self) -> Result<u16> {
        if self.fan_detector.is_initialized() {
            // Use existing fan detector to find PWM files
            if let Some(cpu_fan) = self.fan_detector.get_cpu_fan() {
                let pwm_path = std::path::Path::new(&cpu_fan.hwmon_path).join(format!("pwm{}", cpu_fan.fan_number));
                if let Ok(content) = std::fs::read_to_string(&pwm_path) {
                    if let Ok(pwm_value) = content.trim().parse::<u16>() {
                        // Convert PWM (0-255) to duty percentage (0-10000)
                        let duty_percentage = (pwm_value as f32 / 255.0 * 10000.0) as u16;
                        log::debug!("Read fan duty from {:?}: PWM={}, Duty={:.1}%", 
                            pwm_path, pwm_value, duty_percentage as f32 / 100.0);
                        return Ok(duty_percentage);
                    }
                }
            }
        }
        
        // Fallback: try to initialize fan detector and read PWM
        let mut fan_detector = self.fan_detector.clone();
        if fan_detector.initialize().is_ok() {
            if let Some(cpu_fan) = fan_detector.get_cpu_fan() {
                let pwm_path = std::path::Path::new(&cpu_fan.hwmon_path).join(format!("pwm{}", cpu_fan.fan_number));
                if let Ok(content) = std::fs::read_to_string(&pwm_path) {
                    if let Ok(pwm_value) = content.trim().parse::<u16>() {
                        // Convert PWM (0-255) to duty percentage (0-10000)
                        let duty_percentage = (pwm_value as f32 / 255.0 * 10000.0) as u16;
                        log::debug!("Read fan duty from {:?}: PWM={}, Duty={:.1}%", 
                            pwm_path, pwm_value, duty_percentage as f32 / 100.0);
                        return Ok(duty_percentage);
                    }
                }
            }
        }
        
        Err(crate::errors::FanCurveError::Config(
            "Could not read fan duty from PWM files".to_string()
        ))
    }

    /// Get current fan data using D-Bus (for control operations)
    pub fn get_current_fan_data_sync(&self) -> Result<FanDataPoint> {
        log::debug!("FanMonitor::get_current_fan_data_sync called");
        log::debug!("D-Bus client initialized: {}", self.system76_power_client.is_some());
        
        // Try to initialize D-Bus if not already initialized
        if self.system76_power_client.is_none() {
            log::info!("D-Bus client not initialized, attempting to initialize...");
            // We can't modify self here, but we can log the issue
            log::warn!("D-Bus client not initialized, will use simulation data");
            return Err(crate::errors::FanCurveError::Config(
                "System76 Power D-Bus client not initialized. Please ensure the daemon is running.".to_string()
            ));
        }
        
        // Create a new Tokio runtime for this operation
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => {
                log::debug!("Successfully created Tokio runtime");
                rt
            }
            Err(e) => {
                log::error!("Failed to create Tokio runtime: {}", e);
                return Err(crate::errors::FanCurveError::Unknown(format!("Failed to create Tokio runtime: {}", e)));
            }
        };
        
        log::debug!("About to call get_current_fan_data()");
        let result = rt.block_on(self.get_current_fan_data());
        log::debug!("get_current_fan_data() completed with result: {:?}", result.is_ok());
        result
    }
    /// Synchronous wrapper for apply_fan_curve
    pub fn apply_fan_curve_sync(&self, temperature: f32) -> Result<()> {
        log::debug!("FanMonitor::apply_fan_curve_sync called with temperature={}", temperature);
        
        // Use the same separate thread approach to avoid GUI conflicts
        let (tx, rx) = std::sync::mpsc::channel();
        let self_clone = self.clone();
        
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new();
            match rt {
                Ok(runtime) => {
                    let result = runtime.block_on(self_clone.apply_fan_curve(temperature));
                    let _ = tx.send(result);
                }
                Err(e) => {
                    let _ = tx.send(Err(crate::errors::FanCurveError::Unknown(format!("Failed to create Tokio runtime: {}", e))));
                }
            }
        });
        
        // Wait for the result
        rx.recv().map_err(|e| crate::errors::FanCurveError::Unknown(format!("Failed to receive result: {}", e)))?
    }

    /// Apply fan curve using daemon D-Bus interface (for GUI integration)
    pub fn apply_fan_curve_from_gui(&mut self, curve: &crate::fan::FanCurve, temperature: f32) -> Result<()> {
        log::info!("=== FAN CURVE APPLICATION START ===");
        log::info!("Applying fan curve '{}' at temperature {:.1}°C", curve.name(), temperature);
        
        // Set the fan curve in the monitor
        self.current_fan_curve = Some(curve.clone());
        log::info!("Fan curve set in monitor: {} points", curve.points().len());
        
        // Use daemon D-Bus interface instead of direct PWM control
        log::info!("Attempting to use daemon D-Bus interface...");
        
        // Initialize D-Bus client if not already initialized
        if self.system76_power_client.is_none() {
            log::info!("D-Bus client not initialized, attempting to initialize...");
            if let Err(e) = self.initialize_system76_power_sync() {
                log::warn!("Failed to initialize D-Bus client: {}", e);
                log::info!("Falling back to direct PWM control...");
                return self.apply_fan_curve_direct_pwm(curve, temperature);
            }
        }
        
        // Use the synchronous wrapper to avoid runtime conflicts
        log::info!("Using D-Bus interface to set fan curve in daemon...");
        match self.apply_fan_curve_sync(temperature) {
            Ok(_) => {
                log::info!("✅ Successfully applied fan curve via daemon D-Bus");
                log::info!("=== FAN CURVE APPLICATION SUCCESS (DAEMON) ===");
                Ok(())
            }
            Err(e) => {
                log::warn!("Failed to apply fan curve via daemon: {}", e);
                log::info!("Falling back to direct PWM control...");
                self.apply_fan_curve_direct_pwm(curve, temperature)
            }
        }
    }
    
    /// Fallback method for direct PWM control when daemon is unavailable
    fn apply_fan_curve_direct_pwm(&mut self, curve: &crate::fan::FanCurve, temperature: f32) -> Result<()> {
        log::info!("=== FALLBACK: DIRECT PWM CONTROL ===");
        
        // Use direct PWM control for GUI (avoids D-Bus runtime conflicts)
        log::info!("Checking fan detector initialization status...");
        if !self.fan_detector.is_initialized() {
            log::warn!("Fan detector not initialized, attempting to initialize");
            log::info!("Calling fan_detector.initialize()...");
            match self.fan_detector.initialize() {
                Ok(_) => {
                    log::info!("✅ Fan detector initialized successfully");
                    log::info!("Fan detector now has {} fans", self.fan_detector.get_fans().len());
                }
                Err(e) => {
                    log::error!("❌ Failed to initialize fan detector: {}", e);
                    log::error!("Fan detector initialization error details: {:?}", e);
                    return Err(e);
                }
            }
        } else {
            log::info!("Fan detector already initialized with {} fans", self.fan_detector.get_fans().len());
        }

        log::info!("Calculating fan duty from curve...");
        let duty = self.calculate_fan_duty_from_curve(temperature);
        let duty_percentage = duty / 100; // Convert ten-thousandths to percentage for display
        let pwm_value = self.duty_to_pwm(duty);

        log::info!(
            "Fan curve calculation: {:.1}°C -> {}% duty ({} ten-thousandths) -> PWM {}",
            temperature, duty_percentage, duty, pwm_value
        );

        log::info!("Attempting to apply PWM control to fans...");
        // Apply to all fans using the set_duty method
        match self.fan_detector.set_duty(Some(pwm_value)) {
            Ok(_) => {
                log::info!(
                    "✅ Successfully applied PWM control to all fans: {} (duty: {})",
                    pwm_value, duty
                );
                log::info!("=== FAN CURVE APPLICATION SUCCESS (DIRECT PWM) ===");
                return Ok(());
            }
            Err(e) => {
                log::warn!("Failed to set fan PWM via set_duty: {}", e);
                log::info!("Attempting fallback to individual CPU fan control...");
            }
        }

        // Fallback to individual CPU fan control
        log::info!("Getting CPU fan information...");
        if let Some(cpu_fan) = self.fan_detector.get_cpu_fan() {
            log::info!(
                "Found CPU fan: number={}, applying PWM control -> PWM {}",
                cpu_fan.fan_number, pwm_value
            );
            match self.fan_detector.set_fan_pwm(cpu_fan.fan_number, pwm_value) {
                Ok(_) => {
                    log::info!("✅ Successfully applied PWM control to CPU fan {}", cpu_fan.fan_number);
                    log::info!("=== FAN CURVE APPLICATION SUCCESS (FALLBACK) ===");
                    return Ok(());
                }
                Err(e) => {
                    log::error!("Failed to set CPU fan PWM directly: {}", e);
                    return Err(e);
                }
            }
        } else {
            log::error!("No CPU fan found for direct PWM control");
            log::error!("=== FAN CURVE APPLICATION FAILED ===");
            return Err(crate::errors::FanCurveError::Unknown("No CPU fan found".to_string()));
        }
    }

    /// Set fan duty directly from GUI (0-255 PWM value)
    pub fn set_fan_duty_from_gui(&mut self, duty: u8) -> Result<()> {
        log::debug!("FanMonitor::set_fan_duty_from_gui called with duty={}", duty);
        
        // Create a new Tokio runtime for this synchronous call
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| crate::errors::FanCurveError::Unknown(format!("Failed to create Tokio runtime: {}", e)))?;
        
        rt.block_on(async {
            if let Some(ref client) = self.system76_power_client {
                log::debug!("Using existing D-Bus client to set fan duty");
                client.set_fan_duty(duty).await
            } else {
                log::debug!("No D-Bus client available, creating new one");
                let client = System76PowerClient::new().await?;
                client.set_fan_duty(duty).await
            }
        })
    }

    /// Get current fan data - async version
    pub async fn get_current_fan_data(&self) -> Result<FanDataPoint> {
        log::debug!("FanMonitor::get_current_fan_data() called");
        log::debug!("D-Bus client initialized: {}", self.system76_power_client.is_some());
        
        // Try to initialize D-Bus client if not already initialized
        if self.system76_power_client.is_none() {
            // We can't modify self here, but we can log the issue
            log::warn!("D-Bus client not initialized, will use simulation data");
            return Err(crate::errors::FanCurveError::Config(
                "System76 Power D-Bus client not initialized. Please ensure the daemon is running.".to_string()
            ));
        }
        
        // Read real CPU temperature using async method
        let temperature = self.read_cpu_temperature_async().await?;
        let cpu_fan_speeds = self.read_fan_speeds_async().await?;
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
            cpu_model: self.get_cpu_model(),
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

    /// Read CPU temperature from System76 Power daemon (synchronous)
    fn read_cpu_temperature(&self) -> Result<f32> {
        // Use System76 Power daemon if available
        if let Some(ref client) = self.system76_power_client {
            // Use tokio::runtime::Handle to run async code in sync context
            let handle = tokio::runtime::Handle::current();
            let temp_thousandths = handle.block_on(client.get_current_temperature_from_daemon())?;
            
            // Convert to Celsius
            let temp_celsius = temp_thousandths as f32 / 1000.0;
            
            info!("Temperature from daemon: {:.1}°C ({} thousandths)", temp_celsius, temp_thousandths);
            return Ok(temp_celsius);
        }
        
        // Fallback to direct sysfs if daemon not available
        if !self.cpu_temp_detector.is_initialized() {
            warn!("CPU temperature detector not initialized, using simulation");
            return Ok(self.simulate_temperature_fallback());
        }

        self.cpu_temp_detector.read_temperature()
    }

    /// Read CPU temperature from System76 Power daemon (asynchronous)
    async fn read_cpu_temperature_async(&self) -> Result<f32> {
        // Use System76 Power daemon if available
        if let Some(ref client) = self.system76_power_client {
            let temp_thousandths = client.get_current_temperature_from_daemon().await?;
            
            // Convert to Celsius
            let temp_celsius = temp_thousandths as f32 / 1000.0;
            
            info!("Temperature from daemon: {:.1}°C ({} thousandths)", temp_celsius, temp_thousandths);
            return Ok(temp_celsius);
        }
        
        // Force D-Bus usage - no simulation fallback
        Err(crate::errors::FanCurveError::Config(
            "System76 Power D-Bus client not initialized. Please ensure the daemon is running.".to_string()
        ))
    }

    /// Read fan speeds from System76 Power daemon (synchronous)
    fn read_fan_speeds(&self) -> Result<Vec<(u8, u16, String)>> {
        // Use System76 Power daemon if available
        if let Some(ref client) = self.system76_power_client {
            // Use tokio::runtime::Handle to run async code in sync context
            let handle = tokio::runtime::Handle::current();
            let speeds_rpm = handle.block_on(client.get_fan_speeds_from_daemon())?;
            
            // Convert Vec<u32> (RPM) to Vec<(u8, u16, String)> (fan_number, speed, label)
            let mut fan_speeds = Vec::new();
            for (i, speed) in speeds_rpm.iter().enumerate() {
                let fan_number = (i + 1) as u8; // Fan numbers start from 1
                let speed_u16 = *speed as u16; // Convert u32 to u16
                let label = format!("Fan {}", fan_number);
                fan_speeds.push((fan_number, speed_u16, label));
            }
            
            info!("Fan speeds from daemon: {:?}", fan_speeds);
            return Ok(fan_speeds);
        }
        
        // Fallback to direct sysfs if daemon not available
        if !self.fan_detector.is_initialized() {
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

    /// Read fan speeds from System76 Power daemon (asynchronous)
    async fn read_fan_speeds_async(&self) -> Result<Vec<(u8, u16, String)>> {
        // Use System76 Power daemon if available
        if let Some(ref client) = self.system76_power_client {
            let speeds_rpm = client.get_fan_speeds_from_daemon().await?;
            
            // Convert Vec<u32> (RPM) to Vec<(u8, u16, String)> (fan_number, speed, label)
            let mut fan_speeds = Vec::new();
            for (i, speed) in speeds_rpm.iter().enumerate() {
                let fan_number = (i + 1) as u8; // Fan numbers start from 1
                let speed_u16 = *speed as u16; // Convert u32 to u16
                let label = format!("Fan {}", fan_number);
                fan_speeds.push((fan_number, speed_u16, label));
            }
            
            info!("Fan speeds from daemon: {:?}", fan_speeds);
            return Ok(fan_speeds);
        }
        
        // Force D-Bus usage - no simulation fallback
        Err(crate::errors::FanCurveError::Config(
            "System76 Power D-Bus client not initialized. Please ensure the daemon is running.".to_string()
        ))
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
        log::debug!("Calculating fan duty for temperature: {:.1}°C", temperature);
        
        if let Some(ref curve) = self.current_fan_curve {
            log::debug!("Using fan curve '{}' with {} points", curve.name(), curve.points().len());
            
            // Log all curve points
            for (i, point) in curve.points().iter().enumerate() {
                log::debug!("  Point {}: {}°C -> {:.1}%", i + 1, point.temp, point.duty as f32 / 100.0);
            }
            
            // Convert Celsius to thousandths of Celsius
            let temp_thousandths = (temperature * 1000.0) as u32;
            log::debug!("Temperature in thousandths: {}", temp_thousandths);
            
            let duty = curve.calculate_duty_for_temperature(temp_thousandths);
            log::debug!("Calculated duty from curve: {} (ten-thousandths)", duty);
            duty
        } else {
            log::warn!("No fan curve set, using fallback calculation");
            // Fallback to simple simulation if no curve is set
            let duty_percent = ((temperature - 30.0).max(0.0) * 2.0) as u16;
            let duty_percent = duty_percent.min(100);
            // Convert percentage to ten-thousandths
            let duty = duty_percent * 100;
            log::debug!("Fallback calculation: {}°C -> {}% -> {} ten-thousandths", temperature, duty_percent, duty);
            duty
        }
    }

    /// Calculate PWM value from duty (0-10000) to PWM (0-255)
    /// Matches system76-power conversion: (duty * 255) / 10000
    fn duty_to_pwm(&self, duty: u16) -> u8 {
        ((u32::from(duty) * 255) / 10000) as u8
    }

    /// Apply fan curve to hardware via System76 Power daemon
    pub async fn apply_fan_curve(&self, temperature: f32) -> Result<()> {
        // Use System76 Power daemon if available
        if let Some(ref client) = self.system76_power_client {
            // Get current fan curve from daemon
            let current_curve = client.get_fan_curve_from_daemon().await?;
            
            // Convert app's FanCurve to daemon format
            if let Some(ref curve) = self.current_fan_curve {
                let daemon_points = curve.to_daemon_points();
                
                // Check if curve has changed
                if current_curve != daemon_points {
                    info!("Fan curve changed, updating daemon");
                    client.set_fan_curve_to_daemon(daemon_points).await?;
                    info!("Fan curve updated in daemon successfully");
                } else {
                    info!("Fan curve unchanged, daemon already has current curve");
                }
                
                // Apply the fan curve to hardware
                info!("Applying fan curve to hardware via daemon");
                let duty = self.calculate_fan_duty_from_curve(temperature);
                let duty_percentage = duty / 100; // Convert ten-thousandths to percentage for display
                client.apply_fan_curve(temperature, duty_percentage).await?;
                info!("Fan curve applied to hardware successfully");
            } else {
                warn!("No fan curve set in app, cannot apply to daemon");
            }
            
            return Ok(());
        }

        // Fallback to direct PWM control - requires fan detector to be initialized
        if !self.fan_detector.is_initialized() {
            warn!("Fan detector not initialized, cannot apply fan curve");
            return Ok(());
        }

        let duty = self.calculate_fan_duty_from_curve(temperature);
        let duty_percentage = duty / 100; // Convert ten-thousandths to percentage for display
        let pwm_value = self.duty_to_pwm(duty);

        info!(
            "Applying fan curve: {:.1}°C -> {}% duty ({} ten-thousandths)",
            temperature, duty_percentage, duty
        );

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

    /// Get CPU model information
    fn get_cpu_model(&self) -> String {
        // Try to read CPU model from /proc/cpuinfo
        if let Ok(content) = std::fs::read_to_string("/proc/cpuinfo") {
            for line in content.lines() {
                if line.starts_with("model name") {
                    if let Some(model) = line.split(':').nth(1) {
                        return model.trim().to_string();
                    }
                }
            }
        }
        "Unknown CPU".to_string()
    }

    /// Read CPU temperature directly from thermal zone files
    fn read_cpu_temperature_direct(&self) -> Result<f32> {
        // Try different thermal zone paths
        let thermal_paths = [
            "/sys/class/thermal/thermal_zone0/temp",
            "/sys/class/thermal/thermal_zone1/temp",
            "/sys/devices/virtual/thermal/thermal_zone0/temp",
            "/sys/devices/virtual/thermal/thermal_zone1/temp",
        ];

        for path in &thermal_paths {
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Ok(temp_millicelsius) = content.trim().parse::<f32>() {
                    let temp_celsius = temp_millicelsius / 1000.0;
                    log::debug!("Read temperature from {}: {:.1}°C", path, temp_celsius);
                    return Ok(temp_celsius);
                }
            }
        }

        Err(crate::errors::FanCurveError::Config(
            "Could not read CPU temperature from thermal zone files".to_string()
        ))
    }

    /// Read fan speeds directly from hwmon files
    fn read_fan_speeds_direct(&self) -> Result<Vec<(u8, u16, String)>> {
        let mut fan_speeds = Vec::new();
        
        // Look for hwmon directories
        if let Ok(entries) = std::fs::read_dir("/sys/class/hwmon") {
            for entry in entries.flatten() {
                let hwmon_path = entry.path();
                if let Some(_hwmon_name) = hwmon_path.file_name() {
                    let _hwmon_name = _hwmon_name.to_string_lossy();
                    
                    // Look for fan input files
                    if let Ok(fan_entries) = std::fs::read_dir(&hwmon_path) {
                        for fan_entry in fan_entries.flatten() {
                            let fan_name = fan_entry.file_name().to_string_lossy().to_string();
                            if fan_name.starts_with("fan") && fan_name.ends_with("_input") {
                                let fan_num_str = fan_name.replace("fan", "").replace("_input", "");
                                if let Ok(fan_num) = fan_num_str.parse::<u8>() {
                                    let fan_path = fan_entry.path();
                                    if let Ok(content) = std::fs::read_to_string(&fan_path) {
                                        if let Ok(rpm) = content.trim().parse::<u16>() {
                                            let label = format!("Fan {}", fan_num);
                                            fan_speeds.push((fan_num, rpm, label));
                                            log::debug!("Read fan speed from {:?}: {} RPM", fan_path, rpm);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if fan_speeds.is_empty() {
            Err(crate::errors::FanCurveError::Config(
                "Could not read fan speeds from hwmon files".to_string()
            ))
        } else {
            Ok(fan_speeds)
        }
    }

    /// Read current fan duty directly from hwmon PWM files
    fn read_current_fan_duty_direct(&self) -> Result<u16> {
        // Look for hwmon directories
        if let Ok(entries) = std::fs::read_dir("/sys/class/hwmon") {
            for entry in entries.flatten() {
                let hwmon_path = entry.path();
                
                // Look for PWM files
                if let Ok(pwm_entries) = std::fs::read_dir(&hwmon_path) {
                    for pwm_entry in pwm_entries.flatten() {
                        let pwm_name = pwm_entry.file_name().to_string_lossy().to_string();
                        if pwm_name.starts_with("pwm") && !pwm_name.contains("_") {
                            let pwm_path = pwm_entry.path();
                            if let Ok(content) = std::fs::read_to_string(&pwm_path) {
                                if let Ok(pwm_value) = content.trim().parse::<u16>() {
                                    // Convert PWM (0-255) to duty percentage (0-10000)
                                    let duty_percentage = (pwm_value as f32 / 255.0 * 10000.0) as u16;
                                    log::debug!("Read fan duty from {:?}: PWM={}, Duty={:.1}%", 
                                        pwm_path, pwm_value, duty_percentage as f32 / 100.0);
                                    return Ok(duty_percentage);
                                }
                            }
                        }
                    }
                }
            }
        }

        Err(crate::errors::FanCurveError::Config(
            "Could not read fan duty from hwmon PWM files".to_string()
        ))
    }

    /// Read CPU usage from /proc/stat
    fn read_cpu_usage_direct(&self) -> Result<f32> {
        if let Ok(content) = std::fs::read_to_string("/proc/stat") {
            if let Some(first_line) = content.lines().next() {
                let parts: Vec<&str> = first_line.split_whitespace().collect();
                if parts.len() >= 8 {
                    // Parse CPU times: user, nice, system, idle, iowait, irq, softirq, steal
                    let user: u64 = parts[1].parse().unwrap_or(0);
                    let nice: u64 = parts[2].parse().unwrap_or(0);
                    let system: u64 = parts[3].parse().unwrap_or(0);
                    let idle: u64 = parts[4].parse().unwrap_or(0);
                    let iowait: u64 = parts[5].parse().unwrap_or(0);
                    let irq: u64 = parts[6].parse().unwrap_or(0);
                    let softirq: u64 = parts[7].parse().unwrap_or(0);
                    let steal: u64 = parts.get(8).and_then(|s| s.parse().ok()).unwrap_or(0);

                    let total_idle = idle + iowait;
                    let total_non_idle = user + nice + system + irq + softirq + steal;
                    let total = total_idle + total_non_idle;

                    // For simplicity, return a basic calculation
                    // In a real implementation, you'd need to track previous values
                    let usage_percent = if total > 0 {
                        (total_non_idle as f32 / total as f32) * 100.0
                    } else {
                        0.0
                    };

                    log::debug!("Read CPU usage: {:.1}%", usage_percent);
                    return Ok(usage_percent);
                }
            }
        }

        Err(crate::errors::FanCurveError::Config(
            "Could not read CPU usage from /proc/stat".to_string()
        ))
    }

    /// Check if System76 Power D-Bus client is initialized
    pub fn is_system76_power_initialized(&self) -> bool {
        self.system76_power_client.is_some()
    }

    /// Synchronous method to initialize System76 Power client
    pub fn initialize_system76_power_sync(&mut self) -> Result<()> {
        log::debug!("FanMonitor::initialize_system76_power_sync called");
        
        match System76PowerClient::new_sync() {
            Ok(client) => {
                log::debug!("System76PowerClient::new_sync() succeeded");
                
                // Check if service is available by testing a simple call
                // Use the same separate thread approach to avoid GUI conflicts
                let (tx, rx) = std::sync::mpsc::channel();
                let client_clone = client.clone();
                
                std::thread::spawn(move || {
                    let rt = match tokio::runtime::Runtime::new() {
                        Ok(rt) => {
                            log::debug!("Created Tokio runtime for availability check in separate thread");
                            rt
                        }
                        Err(e) => {
                            log::error!("Failed to create Tokio runtime for availability check: {}", e);
                            let _ = tx.send(Err(crate::errors::FanCurveError::Unknown(format!("Failed to create Tokio runtime: {}", e))));
                            return;
                        }
                    };
                    
                    let is_available = rt.block_on(async {
                        log::debug!("Checking if System76 Power service is available");
                        client_clone.is_available().await
                    });
                    
                    log::debug!("System76 Power service available: {}", is_available);
                    let _ = tx.send(Ok(is_available));
                });
                
                let is_available = rx.recv().map_err(|_| crate::errors::FanCurveError::Unknown("Failed to receive availability check result".to_string()))??;
                
                if is_available {
                    self.system76_power_client = Some(client);
                    info!("System76 Power client initialized and available");
                } else {
                    warn!("System76 Power service not available");
                }
                Ok(())
            }
            Err(e) => {
                log::error!("System76PowerClient::new_sync() failed: {}", e);
                warn!("Failed to initialize System76 Power client: {}", e);
                Ok(()) // Don't fail initialization if System76 Power is not available
            }
        }
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

