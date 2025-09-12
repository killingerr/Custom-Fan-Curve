//! Daemon implementation for the fan curve application

use crate::{
    errors::{zbus_error_from_display, FanCurveError, Result},
    fan::{FanCurve, FanCurveConfig},
    thelio_io::ThelioIoClient,
    DBUS_OBJECT_PATH, DBUS_SERVICE_NAME,
};
use log::{debug, error, info};
use std::sync::{Arc, Mutex};
use tokio::time::{sleep, Duration};
use zbus::{dbus_interface, ConnectionBuilder, SignalContext};

/// Main daemon structure
pub struct FanCurveDaemon {
    config: Arc<Mutex<FanCurveConfig>>,
    current_curve_index: Arc<Mutex<usize>>,
    #[allow(dead_code)]
    thelio: Option<ThelioIoClient>,
}

impl FanCurveDaemon {
    /// Create a new daemon instance
    pub fn new() -> Result<Self> {
        let config = Arc::new(Mutex::new(Self::load_config()?));
        let current_curve_index = Arc::new(Mutex::new(0));

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
        
        info!("Configuration saved successfully to: {}", config_path.display());
        Ok(())
    }

    /// Send a fan curve changed signal
    async fn send_fan_curve_changed_signal(&self) {
        // For now, just log that we would send a signal
        // TODO: Implement proper signal sending when signal context is available
        info!("Fan curve changed - signal would be sent to fan monitor");
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
                        error!("Failed to save config, retrying... ({} attempts left): {}", retries, e);
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

    /// Run the daemon
    pub async fn run(self) -> Result<()> {
        info!("Starting fan curve daemon");

        let _connection = ConnectionBuilder::system()?
            .name(DBUS_SERVICE_NAME)?
            .serve_at(DBUS_OBJECT_PATH, self)?
            .build()
            .await?;

        info!("Daemon started, listening on DBus");

        // Keep the daemon running
        loop {
            sleep(Duration::from_secs(1)).await;
        }
    }
}

#[dbus_interface(name = "com.system76.FanCurveDaemon")]
impl FanCurveDaemon {
    /// Signal emitted when fan curve changes
    #[dbus_interface(signal)]
    async fn fan_curve_changed(&self, signal_ctx: &SignalContext<'_>) -> zbus::Result<()> {
        info!("Emitting fan curve changed signal");
        Ok(())
    }
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
        let current_index = self.current_curve_index.lock().unwrap();
        Ok(config.curves[*current_index].clone())
    }

    /// Set current fan curve by index
    async fn set_fan_curve(&self, index: u32) -> zbus::fdo::Result<()> {
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
        
        // Emit signal to notify fan monitor of the change
        self.send_fan_curve_changed_signal().await;
        
        Ok(())
    }

    /// Set fan curve by name
    async fn set_fan_curve_by_name(&self, name: &str) -> zbus::fdo::Result<()> {
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
            
            // Emit signal to notify fan monitor of the change
            self.send_fan_curve_changed_signal().await;
            
            Ok(())
        } else {
            Err(zbus_error_from_display(format!(
                "Fan curve not found: {}",
                name
            )))
        }
    }

    /// Set default fan curve
    async fn set_default_fan_curve(&self, name: &str) -> zbus::fdo::Result<()> {
        debug!("Setting default fan curve to: {}", name);
        let mut config = self.config.lock().unwrap();

        if let Some(index) = config.curves.iter().position(|c| c.name() == name) {
            config.default_curve_index = Some(index);
            drop(config);

            // Ensure config is saved immediately
            if let Err(e) = self.ensure_config_saved() {
                error!("Failed to save config after setting default: {}", e);
                return Err(zbus_error_from_display(format!(
                    "Failed to save config: {}",
                    e
                )));
            }

            info!("Default fan curve set to: {}", name);
            Ok(())
        } else {
            Err(zbus_error_from_display(format!(
                "Fan curve not found: {}",
                name
            )))
        }
    }

    /// Add a fan curve point
    async fn add_fan_curve_point(&self, temp: i16, duty: u16) -> zbus::fdo::Result<()> {
        debug!("Adding fan curve point: {}°C -> {}%", temp, duty);

        if !(0..=100).contains(&temp) || duty > 100 {
            return Err(zbus_error_from_display("Invalid fan curve point values"));
        }

        let valid_index = {
            let mut config = self.config.lock().unwrap();
            let current_index = self.current_curve_index.lock().unwrap();

            if *current_index < config.curves.len() {
                config.curves[*current_index].add_point(temp, duty);
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
            
            // Emit signal to notify fan monitor of the change
            self.send_fan_curve_changed_signal().await;
            
            Ok(())
        } else {
            Err(zbus_error_from_display("Invalid current fan curve index"))
        }
    }

    /// Remove last fan curve point
    async fn remove_fan_curve_point(&self) -> zbus::fdo::Result<()> {
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
            
            // Emit signal to notify fan monitor of the change
            self.send_fan_curve_changed_signal().await;
            
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
}
