//! Daemon implementation for the fan curve application

use crate::{
    cpu_temp::CpuTempDetector,
    errors::{zbus_error_from_display, FanCurveError, Result},
    fan::{FanCurve, FanCurveConfig},
    fan_detector::FanDetector,
    thelio_io::ThelioIoClient,
    DBUS_OBJECT_PATH, DBUS_SERVICE_NAME,
};
use log::{debug, error, info, warn};
use std::sync::{Arc, Mutex};
use tokio::time::{sleep, Duration};
use zbus::{dbus_interface, ConnectionBuilder, SignalContext};

/// Live telemetry published by the control loop for DBus clients
#[derive(Clone, Debug, Default)]
pub struct DaemonStatus {
    pub temperature: f64,
    /// Duty in ten-thousandths (0-10000)
    pub duty: u16,
    /// PWM value written to hwmon (0-255)
    pub pwm: u8,
    pub fan_speeds: Vec<(u8, u16, String)>,
}

/// Main daemon structure
pub struct FanCurveDaemon {
    config: Arc<Mutex<FanCurveConfig>>,
    current_curve_index: Arc<Mutex<usize>>,
    status: Arc<Mutex<DaemonStatus>>,
    #[allow(dead_code)]
    thelio: Option<ThelioIoClient>,
}

impl FanCurveDaemon {
    /// Create a new daemon instance
    pub fn new() -> Result<Self> {
        let loaded = Self::load_config()?;
        // Always start on the persisted default so behavior survives reboots
        let initial_index = loaded
            .default_curve_index
            .unwrap_or(0)
            .min(loaded.curves.len().saturating_sub(1));
        let default_name = loaded
            .curves
            .get(initial_index)
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "(none)".to_string());
        info!(
            "Loaded config from {}; default curve: '{}' (index {})",
            FanCurveConfig::get_config_path().display(),
            default_name,
            initial_index
        );

        let config = Arc::new(Mutex::new(loaded));
        let current_curve_index = Arc::new(Mutex::new(initial_index));

        // Thelio client is optional and non-fatal if unavailable
        let thelio = match ThelioIoClient::new() {
            Ok(client) => {
                if client.available() {
                    Some(client)
                } else {
                    None
                }
            }
            Err(_) => None,
        };

        Ok(Self {
            config,
            current_curve_index,
            status: Arc::new(Mutex::new(DaemonStatus::default())),
            thelio,
        })
    }

    /// Load configuration from file or create default
    fn load_config() -> Result<FanCurveConfig> {
        let config_path = FanCurveConfig::get_config_path();
        if config_path.exists() {
            FanCurveConfig::load_from_file(&config_path)
                .map_err(|e| FanCurveError::Config(format!("Failed to load config: {}", e)))
        } else {
            let config = FanCurveConfig::new();
            config.save_to_file(&config_path).map_err(|e| {
                FanCurveError::Config(format!("Failed to save default config: {}", e))
            })?;
            Ok(config)
        }
    }

    /// Save configuration to file with proper error handling
    fn save_config_internal(&self) -> Result<()> {
        let config = self.config.lock().unwrap();
        let config_path = FanCurveConfig::get_config_path();

        // Ensure the directory exists
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                error!("Failed to create config directory: {}", e);
                FanCurveError::Io(e)
            })?;
        }

        // Create a temporary file first, then rename for atomic operation
        let temp_path = config_path.with_extension("tmp");

        // Save to temporary file
        config.save_to_file(&temp_path).map_err(|e| {
            error!("Failed to save config to temp file: {}", e);
            FanCurveError::Config(format!("Failed to save config: {}", e))
        })?;

        // Atomically rename temp file to final location
        std::fs::rename(&temp_path, &config_path).map_err(|e| {
            error!("Failed to rename temp config file: {}", e);
            // Try to clean up temp file
            let _ = std::fs::remove_file(&temp_path);
            FanCurveError::Io(e)
        })?;

        info!(
            "Configuration saved successfully to: {}",
            config_path.display()
        );
        Ok(())
    }

    /// Ensure configuration is saved with retry logic
    fn ensure_config_saved(&self) -> Result<()> {
        let mut retries = 3;
        while retries > 0 {
            match self.save_config_internal() {
                Ok(()) => {
                    // Validate persistence after successful save
                    if let Err(e) = self.validate_persistence() {
                        error!("Persistence validation failed: {}", e);
                        // Don't fail the save operation, but log the issue
                    }
                    return Ok(());
                }
                Err(e) => {
                    retries -= 1;
                    if retries > 0 {
                        error!(
                            "Failed to save config, retrying... ({} attempts left): {}",
                            retries, e
                        );
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    } else {
                        error!("Failed to save config after all retries: {}", e);
                        return Err(e);
                    }
                }
            }
        }
        Ok(())
    }

    /// Validate that configuration persists correctly
    fn validate_persistence(&self) -> Result<()> {
        let config = self.config.lock().unwrap();
        config.validate_persistence()
    }

    /// Run the daemon: serve the DBus interface and drive the fan control loop
    pub async fn run(self) -> Result<()> {
        info!("Starting fan curve daemon");
        info!(
            "Note: if system76-power's fan loop is also running, both will write PWM \
             every second — stop or disable that fan path to avoid fighting."
        );

        let config = Arc::clone(&self.config);
        let current_curve_index = Arc::clone(&self.current_curve_index);
        let status = Arc::clone(&self.status);

        let _connection = ConnectionBuilder::system()?
            .name(DBUS_SERVICE_NAME)?
            .serve_at(DBUS_OBJECT_PATH, self)?
            .build()
            .await?;

        info!("Daemon started, listening on DBus");

        run_control_loop(config, current_curve_index, status).await;
        Ok(())
    }
}

/// Periodically read the CPU temperature, look up the duty on the current
/// curve, and apply it to the fans. Returns fans to automatic (firmware)
/// control on shutdown, mirroring system76-power's behavior.
async fn run_control_loop(
    config: Arc<Mutex<FanCurveConfig>>,
    current_curve_index: Arc<Mutex<usize>>,
    status: Arc<Mutex<DaemonStatus>>,
) {
    let mut cpu_temp = CpuTempDetector::new();
    if let Err(e) = cpu_temp.initialize() {
        warn!("Control loop: CPU temperature detection unavailable: {}", e);
    }

    let mut fan_detector = FanDetector::new();
    if let Err(e) = fan_detector.initialize() {
        warn!("Control loop: fan detection unavailable: {}", e);
    }

    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);
    let mut sigterm =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();

    let mut last_pwm: Option<u8> = None;
    let mut tick: u32 = 0;

    loop {
        tokio::select! {
            _ = sleep(Duration::from_secs(1)) => {
                tick = tick.wrapping_add(1);
                control_step(
                    &config,
                    &current_curve_index,
                    &status,
                    &cpu_temp,
                    &fan_detector,
                    &mut last_pwm,
                    tick,
                );
            }
            _ = &mut ctrl_c => {
                info!("Received SIGINT, shutting down");
                break;
            }
            _ = async {
                match sigterm.as_mut() {
                    Some(s) => { s.recv().await; }
                    None => std::future::pending::<()>().await,
                }
            } => {
                info!("Received SIGTERM, shutting down");
                break;
            }
        }
    }

    if fan_detector.is_initialized() {
        info!("Returning fans to automatic control");
        if let Err(e) = fan_detector.set_duty(None) {
            warn!("Failed to return fans to automatic control: {}", e);
        }
    }
}

/// One iteration of the control loop
fn control_step(
    config: &Arc<Mutex<FanCurveConfig>>,
    current_curve_index: &Arc<Mutex<usize>>,
    status: &Arc<Mutex<DaemonStatus>>,
    cpu_temp: &CpuTempDetector,
    fan_detector: &FanDetector,
    last_pwm: &mut Option<u8>,
    tick: u32,
) {
    if !cpu_temp.is_initialized() || !fan_detector.is_initialized() {
        return;
    }

    let temperature = match cpu_temp.read_temperature() {
        Ok(t) => t,
        Err(e) => {
            warn!("Control loop: failed to read temperature: {}", e);
            return;
        }
    };

    let fan_speeds = fan_detector.read_all_fan_speeds().unwrap_or_default();

    let curve = {
        let cfg = config.lock().unwrap();
        if cfg.curves.is_empty() {
            return;
        }
        let idx = (*current_curve_index.lock().unwrap()).min(cfg.curves.len() - 1);
        cfg.curves[idx].clone()
    };

    let duty = curve.calculate_duty_for_temperature_celsius(temperature);
    let pwm = ((u32::from(duty) * 255) / 10_000) as u8;

    {
        let mut st = status.lock().unwrap();
        st.temperature = temperature as f64;
        st.duty = duty;
        st.pwm = pwm;
        st.fan_speeds = fan_speeds;
    }

    // Skip redundant writes, but rewrite every 10s in case something
    // else (e.g. system76-power) changed the PWM values behind our back.
    if *last_pwm == Some(pwm) && tick % 10 != 0 {
        return;
    }

    debug!(
        "Control loop: {:.1}°C -> duty {} ({}%) -> PWM {}",
        temperature,
        duty,
        duty / 100,
        pwm
    );

    match fan_detector.set_duty(Some(pwm)) {
        Ok(()) => *last_pwm = Some(pwm),
        Err(e) => warn!("Control loop: failed to set fan duty: {}", e),
    }
}

#[dbus_interface(name = "com.system76.FanCurveDaemon")]
impl FanCurveDaemon {
    /// Signal emitted when fan curve changes
    #[dbus_interface(signal)]
    async fn fan_curve_changed(signal_ctx: &SignalContext<'_>) -> zbus::Result<()>;

    /// Get all available fan curves
    async fn get_fan_curves(&self) -> zbus::fdo::Result<Vec<FanCurve>> {
        debug!("Getting fan curves");
        let config = self.config.lock().unwrap();
        Ok(config.curves.clone())
    }

    /// Get current fan curve
    async fn get_current_fan_curve(&self) -> zbus::fdo::Result<FanCurve> {
        debug!("Getting current fan curve");
        let config = self.config.lock().unwrap();
        if config.curves.is_empty() {
            return Err(zbus_error_from_display("No fan curves configured"));
        }
        let current_index =
            (*self.current_curve_index.lock().unwrap()).min(config.curves.len() - 1);
        Ok(config.curves[current_index].clone())
    }

    /// Set current fan curve by index
    async fn set_fan_curve(
        &self,
        index: u32,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        debug!("Setting fan curve to index {}", index);
        let curve_name = {
            let mut current_index = self.current_curve_index.lock().unwrap();
            let config = self.config.lock().unwrap();

            if index as usize >= config.curves.len() {
                return Err(zbus_error_from_display("Invalid fan curve index"));
            }

            *current_index = index as usize;
            config.curves[*current_index].name().to_string()
        };

        info!("Fan curve set to: {}", curve_name);

        if let Err(e) = Self::fan_curve_changed(&ctxt).await {
            warn!("Failed to emit FanCurveChanged signal: {}", e);
        }

        Ok(())
    }

    /// Set fan curve by name
    async fn set_fan_curve_by_name(
        &self,
        name: &str,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        debug!("Setting fan curve to name: {}", name);
        let found = {
            let config = self.config.lock().unwrap();
            config.curves.iter().position(|c| c.name() == name)
        };

        if let Some(index) = found {
            {
                let mut current_index = self.current_curve_index.lock().unwrap();
                *current_index = index;
            }
            info!("Fan curve set to: {}", name);

            if let Err(e) = Self::fan_curve_changed(&ctxt).await {
                warn!("Failed to emit FanCurveChanged signal: {}", e);
            }

            Ok(())
        } else {
            Err(zbus_error_from_display(format!(
                "Fan curve not found: {}",
                name
            )))
        }
    }

    /// Set the persistent default fan curve (survives reboots) and activate it now.
    async fn set_default_fan_curve(
        &self,
        name: &str,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        debug!("Setting default fan curve to: {}", name);
        let index = {
            let config = self.config.lock().unwrap();
            config.curves.iter().position(|c| c.name() == name)
        };

        let Some(index) = index else {
            return Err(zbus_error_from_display(format!(
                "Fan curve not found: {}",
                name
            )));
        };

        {
            let mut config = self.config.lock().unwrap();
            let mut current = self.current_curve_index.lock().unwrap();
            config.default_curve_index = Some(index);
            *current = index;
        }

        if let Err(e) = self.ensure_config_saved() {
            error!("Failed to save config after setting default: {}", e);
            return Err(zbus_error_from_display(format!(
                "Failed to save config: {}",
                e
            )));
        }

        info!(
            "Default fan curve set to '{}' and persisted to {}",
            name,
            FanCurveConfig::get_config_path().display()
        );

        if let Err(e) = Self::fan_curve_changed(&ctxt).await {
            warn!("Failed to emit FanCurveChanged signal: {}", e);
        }

        Ok(())
    }

    /// Get the persisted default fan curve (used on daemon startup / reboot)
    async fn get_default_fan_curve(&self) -> zbus::fdo::Result<FanCurve> {
        let config = self.config.lock().unwrap();
        if config.curves.is_empty() {
            return Err(zbus_error_from_display("No fan curves configured"));
        }
        let index = config
            .default_curve_index
            .unwrap_or(0)
            .min(config.curves.len() - 1);
        Ok(config.curves[index].clone())
    }

    /// Add a fan curve point. `duty` is a percentage (0-100) at the DBus edge;
    /// it is stored internally in ten-thousandths (0-10000).
    async fn add_fan_curve_point(
        &self,
        temp: i16,
        duty: u16,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        debug!("Adding fan curve point: {}°C -> {}%", temp, duty);

        if !(0..=100).contains(&temp) || duty > 100 {
            return Err(zbus_error_from_display("Invalid fan curve point values"));
        }

        let valid_index = {
            let mut config = self.config.lock().unwrap();
            let current_index = self.current_curve_index.lock().unwrap();

            if *current_index < config.curves.len() {
                config.curves[*current_index].add_point(temp, duty * 100);
                true
            } else {
                false
            }
        };

        if valid_index {
            // Ensure config is saved immediately
            if let Err(e) = self.ensure_config_saved() {
                error!("Failed to save config after adding point: {}", e);
                return Err(zbus_error_from_display(format!(
                    "Failed to save config: {}",
                    e
                )));
            }

            info!("Added fan curve point: {}°C -> {}%", temp, duty);

            if let Err(e) = Self::fan_curve_changed(&ctxt).await {
                warn!("Failed to emit FanCurveChanged signal: {}", e);
            }

            Ok(())
        } else {
            Err(zbus_error_from_display("Invalid current fan curve index"))
        }
    }

    /// Remove last fan curve point
    async fn remove_fan_curve_point(
        &self,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        debug!("Removing last fan curve point");

        let point_removed = {
            let mut config = self.config.lock().unwrap();
            let current_index = self.current_curve_index.lock().unwrap();

            if *current_index < config.curves.len() {
                config.curves[*current_index].remove_last_point().is_some()
            } else {
                return Err(zbus_error_from_display("Invalid current fan curve index"));
            }
        };

        if point_removed {
            // Ensure config is saved immediately
            if let Err(e) = self.ensure_config_saved() {
                error!("Failed to save config after removing point: {}", e);
                return Err(zbus_error_from_display(format!(
                    "Failed to save config: {}",
                    e
                )));
            }

            info!("Removed last fan curve point");

            if let Err(e) = Self::fan_curve_changed(&ctxt).await {
                warn!("Failed to emit FanCurveChanged signal: {}", e);
            }

            Ok(())
        } else {
            Err(zbus_error_from_display("No points to remove"))
        }
    }

    /// Save configuration
    async fn save_config(&self) -> zbus::fdo::Result<()> {
        debug!("Saving configuration");

        if let Err(e) = self.ensure_config_saved() {
            error!("Failed to save config: {}", e);
            return Err(zbus_error_from_display(format!(
                "Failed to save config: {}",
                e
            )));
        }

        info!("Configuration saved");
        Ok(())
    }

    /// Replace the daemon's in-memory curves with a full config from a client
    /// (e.g. the GUI), persist it, and select `current_name` as the active curve.
    /// `default_index` of -1 means "leave default unchanged / none".
    async fn set_config(
        &self,
        curves: Vec<FanCurve>,
        default_index: i32,
        current_name: &str,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        if curves.is_empty() {
            return Err(zbus_error_from_display("Cannot set an empty curve list"));
        }

        let current_pos = curves
            .iter()
            .position(|c| c.name() == current_name)
            .ok_or_else(|| {
                zbus_error_from_display(format!(
                    "Current curve not found in provided config: {}",
                    current_name
                ))
            })?;

        let default = if default_index < 0 {
            None
        } else if (default_index as usize) < curves.len() {
            Some(default_index as usize)
        } else {
            return Err(zbus_error_from_display("Invalid default curve index"));
        };

        {
            let mut config = self.config.lock().unwrap();
            let mut index = self.current_curve_index.lock().unwrap();
            *config = FanCurveConfig {
                curves,
                default_curve_index: default,
            };
            *index = current_pos;
        }

        if let Err(e) = self.ensure_config_saved() {
            error!("Failed to save config after SetConfig: {}", e);
            return Err(zbus_error_from_display(format!(
                "Failed to save config: {}",
                e
            )));
        }

        info!(
            "Config replaced via DBus; active curve: {} (index {})",
            current_name, current_pos
        );

        if let Err(e) = Self::fan_curve_changed(&ctxt).await {
            warn!("Failed to emit FanCurveChanged signal: {}", e);
        }

        Ok(())
    }

    /// Live status from the control loop: (temperature_c, duty_0_10000, pwm_0_255, fan_speeds)
    async fn get_status(&self) -> zbus::fdo::Result<(f64, u16, u8, Vec<(u8, u16, String)>)> {
        let st = self.status.lock().unwrap();
        Ok((st.temperature, st.duty, st.pwm, st.fan_speeds.clone()))
    }

    /// Reload configuration from disk, replacing the in-memory state
    async fn reload_config(
        &self,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        debug!("Reloading configuration from disk");

        let new_config = Self::load_config().map_err(zbus_error_from_display)?;

        {
            let mut config = self.config.lock().unwrap();
            let mut current_index = self.current_curve_index.lock().unwrap();
            *current_index = new_config
                .default_curve_index
                .unwrap_or(0)
                .min(new_config.curves.len().saturating_sub(1));
            *config = new_config;
        }

        info!("Configuration reloaded from disk");

        if let Err(e) = Self::fan_curve_changed(&ctxt).await {
            warn!("Failed to emit FanCurveChanged signal: {}", e);
        }

        Ok(())
    }
}
