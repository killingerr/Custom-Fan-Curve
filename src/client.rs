//! Client implementation for the fan curve application

use crate::{
    args::{Args, Commands, FanCurveCommands},
    errors::{FanCurveError, Result},
    fan::FanCurve,
    fan_monitor, DBUS_INTERFACE_NAME, DBUS_OBJECT_PATH, DBUS_SERVICE_NAME,
};
use log::{debug, error, info};
use zbus::Connection;

/// Client for communicating with the fan curve daemon
pub struct FanCurveClient {
    connection: Connection,
}

impl FanCurveClient {
    /// Create a new client
    pub async fn new() -> Result<Self> {
        let connection = Connection::system().await.map_err(FanCurveError::DBus)?;

        Ok(Self { connection })
    }

    /// Create a proxy to the fan curve daemon
    async fn proxy(&self) -> Result<zbus::Proxy<'_>> {
        zbus::Proxy::new(
            &self.connection,
            DBUS_SERVICE_NAME,
            DBUS_OBJECT_PATH,
            DBUS_INTERFACE_NAME,
        )
        .await
        .map_err(FanCurveError::DBus)
    }

    /// Map a DBus call error to something actionable for the user
    fn map_call_error(e: zbus::Error) -> FanCurveError {
        let msg = e.to_string();
        if msg.contains("ServiceUnknown") || msg.contains("NameHasNoOwner") {
            FanCurveError::Config(
                "Fan curve daemon is not running. Start it with: sudo fan-curve daemon \
                 (or: sudo systemctl start fan-curve-daemon)"
                    .to_string(),
            )
        } else {
            FanCurveError::DBus(e)
        }
    }

    /// Handle CLI commands
    pub async fn handle_args(&self, args: Args) -> Result<()> {
        match args.command {
            Some(Commands::Daemon) => {
                error!("Daemon command should not be handled by client");
                Err(FanCurveError::Unknown(
                    "Invalid command for client".to_string(),
                ))
            }
            Some(Commands::FanCurve { command }) => self.handle_fan_curve_command(command).await,
            None => {
                error!("No command specified");
                Err(FanCurveError::Unknown("No command specified".to_string()))
            }
        }
    }

    /// Handle fan curve commands
    async fn handle_fan_curve_command(&self, command: FanCurveCommands) -> Result<()> {
        match command {
            FanCurveCommands::List => self.list_fan_curves().await,
            FanCurveCommands::Get => self.get_current_fan_curve().await,
            FanCurveCommands::Set { name } => self.set_fan_curve_by_name(&name).await,
            FanCurveCommands::SetDefault { name } => self.set_default_fan_curve(&name).await,
            FanCurveCommands::AddPoint { temp, duty } => self.add_fan_curve_point(temp, duty).await,
            FanCurveCommands::RemovePoint => self.remove_fan_curve_point().await,
            FanCurveCommands::Save => self.save_config().await,
            FanCurveCommands::Load => self.load_config().await,
            FanCurveCommands::Status => self.show_status().await,
            FanCurveCommands::Test { duration } => self.test_fan_curve(duration).await,
        }
    }

    /// List all fan curves
    async fn list_fan_curves(&self) -> Result<()> {
        debug!("Listing fan curves");

        let proxy = self.proxy().await?;
        let curves: Vec<FanCurve> = proxy
            .call("GetFanCurves", &())
            .await
            .map_err(Self::map_call_error)?;
        let current: FanCurve = proxy
            .call("GetCurrentFanCurve", &())
            .await
            .map_err(Self::map_call_error)?;
        let default: FanCurve = proxy
            .call("GetDefaultFanCurve", &())
            .await
            .map_err(Self::map_call_error)?;

        println!("Available fan curves:");
        for curve in &curves {
            let mut markers = Vec::new();
            if curve.name() == current.name() {
                markers.push("current");
            }
            if curve.name() == default.name() {
                markers.push("default");
            }
            if markers.is_empty() {
                println!("  - {}", curve.name());
            } else {
                println!("  - {} ({})", curve.name(), markers.join(", "));
            }
        }

        Ok(())
    }

    /// Get current fan curve
    async fn get_current_fan_curve(&self) -> Result<()> {
        debug!("Getting current fan curve");

        let proxy = self.proxy().await?;
        let curve: FanCurve = proxy
            .call("GetCurrentFanCurve", &())
            .await
            .map_err(Self::map_call_error)?;

        println!("Current fan curve: {}", curve.name());
        for point in curve.points() {
            println!("  {:>3}°C -> {:>3}%", point.temp, point.duty_percent());
        }

        Ok(())
    }

    /// Set fan curve by name
    async fn set_fan_curve_by_name(&self, name: &str) -> Result<()> {
        debug!("Setting fan curve to: {}", name);

        let proxy = self.proxy().await?;
        proxy
            .call::<_, _, ()>("SetFanCurveByName", &(name,))
            .await
            .map_err(Self::map_call_error)?;

        println!("Fan curve set to: {}", name);
        Ok(())
    }

    /// Set default fan curve
    async fn set_default_fan_curve(&self, name: &str) -> Result<()> {
        debug!("Setting default fan curve to: {}", name);

        let proxy = self.proxy().await?;
        proxy
            .call::<_, _, ()>("SetDefaultFanCurve", &(name,))
            .await
            .map_err(Self::map_call_error)?;

        println!(
            "Default fan curve set to: {} (persists across reboots)",
            name
        );
        Ok(())
    }

    /// Add fan curve point (duty is a percentage, 0-100)
    async fn add_fan_curve_point(&self, temp: i16, duty: u16) -> Result<()> {
        debug!("Adding fan curve point: {}°C -> {}%", temp, duty);

        let proxy = self.proxy().await?;
        proxy
            .call::<_, _, ()>("AddFanCurvePoint", &(temp, duty))
            .await
            .map_err(Self::map_call_error)?;

        println!("Added point to current curve: {}°C -> {}%", temp, duty);
        Ok(())
    }

    /// Remove fan curve point
    async fn remove_fan_curve_point(&self) -> Result<()> {
        debug!("Removing last fan curve point");

        let proxy = self.proxy().await?;
        proxy
            .call::<_, _, ()>("RemoveFanCurvePoint", &())
            .await
            .map_err(Self::map_call_error)?;

        println!("Removed last point from current curve");
        Ok(())
    }

    /// Save configuration
    async fn save_config(&self) -> Result<()> {
        debug!("Saving configuration");

        let proxy = self.proxy().await?;
        proxy
            .call::<_, _, ()>("SaveConfig", &())
            .await
            .map_err(Self::map_call_error)?;

        println!("Configuration saved");
        Ok(())
    }

    /// Reload configuration from disk
    async fn load_config(&self) -> Result<()> {
        debug!("Reloading configuration");

        let proxy = self.proxy().await?;
        proxy
            .call::<_, _, ()>("ReloadConfig", &())
            .await
            .map_err(Self::map_call_error)?;

        println!("Configuration reloaded from disk");
        Ok(())
    }

    /// Show live daemon status
    async fn show_status(&self) -> Result<()> {
        let proxy = self.proxy().await?;
        let (temp, duty, pwm, speeds): (f64, u16, u8, Vec<(u8, u16, String)>) = proxy
            .call("GetStatus", &())
            .await
            .map_err(Self::map_call_error)?;

        let curve: FanCurve = proxy
            .call("GetCurrentFanCurve", &())
            .await
            .map_err(Self::map_call_error)?;
        let default: FanCurve = proxy
            .call("GetDefaultFanCurve", &())
            .await
            .map_err(Self::map_call_error)?;

        println!("Daemon status:");
        println!("  Current:     {}", curve.name());
        println!("  Default:     {} (used after reboot)", default.name());
        println!("  Temperature: {:.1}°C", temp);
        println!("  Duty:        {}%", duty / 100);
        println!("  PWM:         {}", pwm);
        if speeds.is_empty() {
            println!("  Fans:        (none reported yet)");
        } else {
            for (num, rpm, label) in speeds {
                println!("  Fan {}:     {} — {} RPM", num, label, rpm);
            }
        }

        Ok(())
    }

    /// Test fan curve with monitoring
    async fn test_fan_curve(&self, duration: u64) -> Result<()> {
        debug!("Testing fan curve for {} seconds", duration);

        info!("Starting fan curve test for {} seconds", duration);

        // Run the fan curve test
        fan_monitor::test_fan_curve("current", duration).await?;

        info!("Fan curve test completed");
        Ok(())
    }
}
