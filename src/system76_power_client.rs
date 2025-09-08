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
    /// This method should integrate with the System76 Power fan control system
    /// to apply the calculated fan duty percentage.
    pub async fn apply_fan_curve(&self, temperature: f32, duty_percentage: u16) -> Result<()> {
        if !self.is_available().await {
            return Err(crate::errors::FanCurveError::Config(
                "System76 Power service not available".to_string()
            ));
        }

        info!("Applying fan curve via System76 Power: {:.1}°C -> {}%", temperature, duty_percentage);
        
        // TODO: Implement actual System76 Power fan control
        // This would involve:
        // 1. Getting a proxy to the PowerDaemon
        // 2. Calling the appropriate fan control method
        // 3. Passing the duty percentage to the System76 Power system
        
        warn!("System76 Power fan control integration not yet implemented");
        warn!("Would apply {}% duty for {:.1}°C via System76 Power", duty_percentage, temperature);
        
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
