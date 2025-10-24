//! System76 Power DBus client for fan control integration

use crate::errors::Result;
use log::{debug, info, warn};
use zbus::Connection;

/// System76 Power DBus client
#[derive(Clone)]
pub struct System76PowerClient {
    connection: Connection,
}

impl System76PowerClient {
    /// Create a new System76 Power client (synchronous)
    pub fn new_sync() -> Result<Self> {
        log::debug!("System76PowerClient::new_sync() called");
        
        // Try to use existing runtime first
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            log::debug!("Found existing Tokio runtime, using it");
            return handle.block_on(async {
                log::debug!("About to call Connection::system()");
                let connection = Connection::system()
                    .await
                    .map_err(crate::errors::FanCurveError::DBus)?;

                log::debug!("Connection::system() succeeded");
                info!("Connected to System76 Power DBus service");
                Ok(Self { connection })
            });
        }
        
        // No existing runtime, create one in a separate thread to avoid GUI conflicts
        log::debug!("No existing Tokio runtime found, creating in separate thread");
        
        let (tx, rx) = std::sync::mpsc::channel();
        
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => {
                    log::debug!("Successfully created Tokio runtime in separate thread");
                    rt
                }
                Err(e) => {
                    log::error!("Failed to create Tokio runtime in separate thread: {}", e);
                    let _ = tx.send(Err(crate::errors::FanCurveError::Unknown(format!("Failed to create Tokio runtime: {}", e))));
                    return;
                }
            };
            
            let result = rt.block_on(async {
                log::debug!("About to call Connection::system()");
                let connection = Connection::system()
                    .await
                    .map_err(crate::errors::FanCurveError::DBus)?;

                log::debug!("Connection::system() succeeded");
                info!("Connected to System76 Power DBus service");
                Ok(Self { connection })
            });
            
            let _ = tx.send(result);
        });
        
        let result = rx.recv().map_err(|_| crate::errors::FanCurveError::Unknown("Failed to receive result from D-Bus initialization thread".to_string()))?;
        
        log::debug!("System76PowerClient::new_sync() completed with result: {:?}", result.is_ok());
        result
    }

    /// Create a new System76 Power client
    pub async fn new() -> Result<Self> {
        let connection = Connection::system()
            .await
            .map_err(crate::errors::FanCurveError::DBus)?;

        info!("Connected to System76 Power DBus service");
        Ok(Self { connection })
    }

    /// Check if System76 Power service is available
    pub async fn is_available(&self) -> bool {
        // Check if the service is available by trying to get a proxy
        match zbus::Proxy::new(
            &self.connection,
            "com.system76.PowerDaemon",
            "/com/system76/PowerDaemon",
            "com.system76.PowerDaemon",
        )
        .await
        {
            Ok(_) => {
                debug!("System76 Power service is available");
                true
            }
            Err(e) => {
                warn!("System76 Power service not available: {}", e);
                false
            }
        }
    }

    /// Get current temperature from System76 Power daemon
    /// Returns temperature in thousandths of Celsius (e.g., 35000 = 35.0°C)
    pub async fn get_current_temperature_from_daemon(&self) -> Result<u32> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            "com.system76.PowerDaemon",
            "/com/system76/PowerDaemon/Fan",
            "com.system76.PowerDaemon.Fan",
        )
        .await
        .map_err(crate::errors::FanCurveError::DBus)?;

        let response = proxy
            .call_method("GetCurrentTemperature", &())
            .await
            .map_err(crate::errors::FanCurveError::DBus)?;

        let temp: u32 = response.body::<u32>()?;
        Ok(temp)
    }

    /// Get current fan duty from System76 Power daemon
    /// Returns duty as PWM value (0-255)
    pub async fn get_current_duty_from_daemon(&self) -> Result<u8> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            "com.system76.PowerDaemon",
            "/com/system76/PowerDaemon/Fan",
            "com.system76.PowerDaemon.Fan",
        )
        .await
        .map_err(crate::errors::FanCurveError::DBus)?;



        let response = proxy
            .call_method("GetCurrentDuty", &())
            .await
            .map_err(crate::errors::FanCurveError::DBus)?;

        let duty: u8 = response.body::<u8>()?;
        Ok(duty)
    }

    /// Get fan speeds from System76 Power daemon
    /// Returns fan speeds in RPM as Vec<u32>
    pub async fn get_fan_speeds_from_daemon(&self) -> Result<Vec<u32>> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            "com.system76.PowerDaemon",
            "/com/system76/PowerDaemon/Fan",
            "com.system76.PowerDaemon.Fan",
        )
        .await
        .map_err(crate::errors::FanCurveError::DBus)?;

       

        let response = proxy
            .call_method("GetFanSpeeds", &())
            .await
            .map_err(crate::errors::FanCurveError::DBus)?;

        let speeds: Vec<u32> = response.body::<Vec<u32>>()?;
        Ok(speeds)
    }

    /// Get fan curve from System76 Power daemon
    /// Returns fan curve points as Vec<(i16, u16)> (temp, duty pairs)
    pub async fn get_fan_curve_from_daemon(&self) -> Result<Vec<(i16, u16)>> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            "com.system76.PowerDaemon",
            "/com/system76/PowerDaemon/Fan",
            "com.system76.PowerDaemon.Fan",
        )
        .await
        .map_err(crate::errors::FanCurveError::DBus)?;

 
        
        let response = proxy
            .call_method("GetFanCurve", &())
            .await
            .map_err(crate::errors::FanCurveError::DBus)?;
        
        let curve_points: Vec<(i16, u16)> = response.body::<Vec<(i16, u16)>>()?;
        Ok(curve_points)
    }

    /// Set fan curve to System76 Power daemon
    /// Takes fan curve points as Vec<(i16, u16)> (temp, duty pairs)
    pub async fn set_fan_curve_to_daemon(&self, points: Vec<(i16, u16)>) -> Result<()> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            "com.system76.PowerDaemon",
            "/com/system76/PowerDaemon/Fan",
            "com.system76.PowerDaemon.Fan",
        )
        .await
        .map_err(crate::errors::FanCurveError::DBus)?;

        proxy
            .call_method("SetFanCurve", &(points,))
            .await
            .map_err(crate::errors::FanCurveError::DBus)?;

        Ok(())
    }

   
    /// Apply fan curve to hardware via System76 Power daemon
    /// This triggers the daemon to apply the current fan curve based on current temperature
    pub async fn apply_fan_curve(&self, temperature: f32, duty_percentage: u16) -> Result<()> {
        if !self.is_available().await {
            return Err(crate::errors::FanCurveError::Config(
                "System76 Power service not available".to_string(),
            ));
        }

        info!(
            "Applying fan curve via System76 Power: {:.1}°C -> {}%",
            temperature, duty_percentage
        );

        // Use the new D-Bus method to apply the fan curve
        let proxy = zbus::Proxy::new(
            &self.connection,
            "com.system76.PowerDaemon",
            "/com/system76/PowerDaemon/Fan",
            "com.system76.PowerDaemon.Fan",
        )
        .await
        .map_err(crate::errors::FanCurveError::DBus)?;

        proxy
            .call_method("ApplyFanCurve", &())
            .await
            .map_err(crate::errors::FanCurveError::DBus)?;

        info!("Fan curve applied successfully via daemon");
        Ok(())
    }

    /// Set power profile via System76 Power
    async fn set_power_profile(&self, profile: &str) -> Result<()> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            "com.system76.PowerDaemon",
            "/com/system76/PowerDaemon",
            "com.system76.PowerDaemon",
        )
        .await
        .map_err(crate::errors::FanCurveError::DBus)?;

        match profile {
            "Battery" => {
                proxy
                    .call_method("Battery", &())
                    .await
                    .map_err(crate::errors::FanCurveError::DBus)?;
            }
            "Balanced" => {
                proxy
                    .call_method("Balanced", &())
                    .await
                    .map_err(crate::errors::FanCurveError::DBus)?;
            }
            "Performance" => {
                proxy
                    .call_method("Performance", &())
                    .await
                    .map_err(crate::errors::FanCurveError::DBus)?;
            }
            _ => {
                return Err(crate::errors::FanCurveError::Config(format!(
                    "Unknown power profile: {}",
                    profile
                )));
            }
        }

        info!("Set System76 Power profile to: {}", profile);
        Ok(())
    }

    /// Get current fan speeds from System76 Power
    pub async fn get_fan_speeds(&self) -> Result<Vec<(u8, u16, String)>> {
        if !self.is_available().await {
            return Err(crate::errors::FanCurveError::Config(
                "System76 Power service not available".to_string(),
            ));
        }

        // TODO: Implement fan speed reading from System76 Power
        // This would involve calling the appropriate DBus method
        // to get current fan RPM values

        warn!("System76 Power fan speed reading not yet implemented");
        Ok(vec![])
    }

    /// Set fan duty directly (0-255 PWM value)
    pub async fn set_fan_duty(&self, duty: u8) -> Result<()> {
        log::debug!("System76PowerClient::set_fan_duty called with duty={}", duty);
        
        let proxy = zbus::Proxy::new(
            &self.connection,
            "com.system76.PowerDaemon",
            "/com/system76/PowerDaemon/Fan",
            "com.system76.PowerDaemon.Fan",
        ).await.map_err(crate::errors::FanCurveError::DBus)?;

        proxy.call_method("SetDuty", &(duty,))
            .await
            .map_err(crate::errors::FanCurveError::DBus)?;

        log::debug!("System76PowerClient::set_fan_duty completed successfully");
        Ok(())
    }
}

impl Default for System76PowerClient {
    fn default() -> Self {
        // This will panic if called, but provides a default implementation
        // In practice, use System76PowerClient::new() instead
        panic!("System76PowerClient::default() should not be called. Use System76PowerClient::new() instead.");
    }
}
