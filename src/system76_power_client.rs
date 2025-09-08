//! System76 Power DBus client for fan control integration

use crate::errors::Result;
use log::{debug, info, warn};
use zbus::Connection;

/// System76 Power DBus client
pub struct System76PowerClient {
    connection: Connection,
}

impl System76PowerClient {
    /// Create a new System76 Power client
    pub async fn new() -> Result<Self> {
        let connection = Connection::system().await
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
        ).await {
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

    /// Apply fan curve through System76 Power
    /// 
    /// Since System76 Power doesn't expose direct fan control via DBus,
    /// we'll use power profiles to influence fan behavior and fall back
    /// to direct PWM control for custom fan curves.
    pub async fn apply_fan_curve(&self, temperature: f32, duty_percentage: u16) -> Result<()> {
        if !self.is_available().await {
            return Err(crate::errors::FanCurveError::Config(
                "System76 Power service not available".to_string()
            ));
        }

        info!("Applying fan curve via System76 Power: {:.1}°C -> {}%", temperature, duty_percentage);
        
        // System76 Power doesn't expose direct fan control via DBus
        // The fan control is handled internally by power profiles
        // For custom fan curves, we need to use direct PWM control
        
        // Check if we should use Performance mode for high fan speeds
        if duty_percentage > 80 {
            if let Err(e) = self.set_power_profile("Performance").await {
                warn!("Failed to set Performance profile: {}", e);
            }
        } else if duty_percentage < 20 {
            if let Err(e) = self.set_power_profile("Battery").await {
                warn!("Failed to set Battery profile: {}", e);
            }
        } else {
            if let Err(e) = self.set_power_profile("Balanced").await {
                warn!("Failed to set Balanced profile: {}", e);
            }
        }
        
        // Note: For precise fan control, direct PWM manipulation is still needed
        // as System76 Power doesn't expose granular fan control via DBus
        warn!("System76 Power profiles set, but direct PWM control still needed for precise fan curves");
        
        Ok(())
    }

    /// Set power profile via System76 Power
    async fn set_power_profile(&self, profile: &str) -> Result<()> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            "com.system76.PowerDaemon",
            "/com/system76/PowerDaemon",
            "com.system76.PowerDaemon",
        ).await.map_err(crate::errors::FanCurveError::DBus)?;

        match profile {
            "Battery" => {
                proxy.call_method("Battery", &()).await
                    .map_err(crate::errors::FanCurveError::DBus)?;
            }
            "Balanced" => {
                proxy.call_method("Balanced", &()).await
                    .map_err(crate::errors::FanCurveError::DBus)?;
            }
            "Performance" => {
                proxy.call_method("Performance", &()).await
                    .map_err(crate::errors::FanCurveError::DBus)?;
            }
            _ => {
                return Err(crate::errors::FanCurveError::Config(
                    format!("Unknown power profile: {}", profile)
                ));
            }
        }

        info!("Set System76 Power profile to: {}", profile);
        Ok(())
    }

    /// Get current fan speeds from System76 Power
    pub async fn get_fan_speeds(&self) -> Result<Vec<(u8, u16, String)>> {
        if !self.is_available().await {
            return Err(crate::errors::FanCurveError::Config(
                "System76 Power service not available".to_string()
            ));
        }

        // TODO: Implement fan speed reading from System76 Power
        // This would involve calling the appropriate DBus method
        // to get current fan RPM values
        
        warn!("System76 Power fan speed reading not yet implemented");
        Ok(vec![])
    }
}

impl Default for System76PowerClient {
    fn default() -> Self {
        // This will panic if called, but provides a default implementation
        // In practice, use System76PowerClient::new() instead
        panic!("System76PowerClient::default() should not be called. Use System76PowerClient::new() instead.");
    }
}
