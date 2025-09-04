//! Daemon implementation for the fan curve application

use crate::{
    errors::{FanCurveError, Result, zbus_error_from_display},
    fan::{FanCurve, FanCurveConfig},
    DBUS_OBJECT_PATH, DBUS_SERVICE_NAME,
};
use log::{debug, error, info};
use std::sync::{Arc, Mutex};
use tokio::time::{sleep, Duration};
use zbus::{dbus_interface, ConnectionBuilder};

/// Main daemon structure
pub struct FanCurveDaemon {
    config: Arc<Mutex<FanCurveConfig>>,
    current_curve_index: Arc<Mutex<usize>>,
}

impl FanCurveDaemon {
    /// Create a new daemon instance
    pub fn new() -> Result<Self> {
        let config = Arc::new(Mutex::new(Self::load_config()?));
        let current_curve_index = Arc::new(Mutex::new(0));

        Ok(Self {
            config,
            current_curve_index,
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
            config.save_to_file(&config_path)
                .map_err(|e| FanCurveError::Config(format!("Failed to save default config: {}", e)))?;
            Ok(config)
        }
    }

    /// Save configuration to file
    fn save_config_internal(&self) -> Result<()> {
        let config = self.config.lock().unwrap();
        let config_path = FanCurveConfig::get_config_path();
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| FanCurveError::Io(e))?;
        }
        config.save_to_file(&config_path)
            .map_err(|e| FanCurveError::Config(format!("Failed to save config: {}", e)))
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
        let mut current_index = self.current_curve_index.lock().unwrap();
        let config = self.config.lock().unwrap();
        
        if index as usize >= config.curves.len() {
            return Err(zbus_error_from_display("Invalid fan curve index"));
        }

        *current_index = index as usize;
        info!("Fan curve set to: {}", config.curves[*current_index].name());
        Ok(())
    }

    /// Set fan curve by name
    async fn set_fan_curve_by_name(&self, name: &str) -> zbus::fdo::Result<()> {
        debug!("Setting fan curve to name: {}", name);
        let config = self.config.lock().unwrap();
        
        if let Some(index) = config.curves.iter().position(|c| c.name() == name) {
            let mut current_index = self.current_curve_index.lock().unwrap();
            *current_index = index;
            info!("Fan curve set to: {}", name);
            Ok(())
        } else {
            Err(zbus_error_from_display(format!("Fan curve not found: {}", name)))
        }
    }

    /// Set default fan curve
    async fn set_default_fan_curve(&self, name: &str) -> zbus::fdo::Result<()> {
        debug!("Setting default fan curve to: {}", name);
        let mut config = self.config.lock().unwrap();
        
        if let Some(index) = config.curves.iter().position(|c| c.name() == name) {
            config.default_curve_index = Some(index);
            drop(config);
            
            if let Err(e) = self.save_config_internal() {
                error!("Failed to save config: {}", e);
                return Err(zbus_error_from_display(format!("Failed to save config: {}", e)));
            }
            
            info!("Default fan curve set to: {}", name);
            Ok(())
        } else {
            Err(zbus_error_from_display(format!("Fan curve not found: {}", name)))
        }
    }

    /// Add a fan curve point
    async fn add_fan_curve_point(&self, temp: i16, duty: u16) -> zbus::fdo::Result<()> {
        debug!("Adding fan curve point: {}°C -> {}%", temp, duty);
        
        if temp < 0 || temp > 100 || duty > 100 {
            return Err(zbus_error_from_display("Invalid fan curve point values"));
        }

        let mut config = self.config.lock().unwrap();
        let current_index = self.current_curve_index.lock().unwrap();
        
        if *current_index < config.curves.len() {
            config.curves[*current_index].add_point(temp, duty);
            drop(config);
            
            if let Err(e) = self.save_config_internal() {
                error!("Failed to save config: {}", e);
                return Err(zbus_error_from_display(format!("Failed to save config: {}", e)));
            }
            
            info!("Added fan curve point: {}°C -> {}%", temp, duty);
            Ok(())
        } else {
            Err(zbus_error_from_display("Invalid current fan curve index"))
        }
    }

    /// Remove last fan curve point
    async fn remove_fan_curve_point(&self) -> zbus::fdo::Result<()> {
        debug!("Removing last fan curve point");
        
        let mut config = self.config.lock().unwrap();
        let current_index = self.current_curve_index.lock().unwrap();
        
        if *current_index < config.curves.len() {
            if let Some(_point) = config.curves[*current_index].remove_last_point() {
                drop(config);
                
                if let Err(e) = self.save_config_internal() {
                    error!("Failed to save config: {}", e);
                    return Err(zbus_error_from_display(format!("Failed to save config: {}", e)));
                }
                
                info!("Removed last fan curve point");
                Ok(())
            } else {
                Err(zbus_error_from_display("No points to remove"))
            }
        } else {
            Err(zbus_error_from_display("Invalid current fan curve index"))
        }
    }

    /// Save configuration
    async fn save_config(&self) -> zbus::fdo::Result<()> {
        debug!("Saving configuration");
        
        if let Err(e) = self.save_config_internal() {
            error!("Failed to save config: {}", e);
            return Err(zbus_error_from_display(format!("Failed to save config: {}", e)));
        }
        
        info!("Configuration saved");
        Ok(())
    }
}
