//! CLI client: sends `fan-curve` subcommands to the daemon over DBus

use crate::{
    args::FanCurveCommands,
    errors::{FanCurveError, Result},
    fan::FanCurve,
    DBUS_INTERFACE_NAME, DBUS_OBJECT_PATH, DBUS_SERVICE_NAME,
};
use zbus::{Connection, Proxy};

/// Map a DBus call error to something actionable for the user
fn daemon_error(e: zbus::Error) -> FanCurveError {
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

/// Run a `fan-curve` subcommand against the daemon
pub async fn run(command: FanCurveCommands) -> Result<()> {
    let connection = Connection::system().await?;
    let proxy = Proxy::new(
        &connection,
        DBUS_SERVICE_NAME,
        DBUS_OBJECT_PATH,
        DBUS_INTERFACE_NAME,
    )
    .await?;

    match command {
        FanCurveCommands::List => {
            let curves: Vec<FanCurve> = proxy
                .call("GetFanCurves", &())
                .await
                .map_err(daemon_error)?;
            let current: FanCurve = proxy
                .call("GetCurrentFanCurve", &())
                .await
                .map_err(daemon_error)?;
            let default: FanCurve = proxy
                .call("GetDefaultFanCurve", &())
                .await
                .map_err(daemon_error)?;

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
        }
        FanCurveCommands::Get => {
            let curve: FanCurve = proxy
                .call("GetCurrentFanCurve", &())
                .await
                .map_err(daemon_error)?;
            println!("Current fan curve: {}", curve.name());
            for point in curve.points() {
                println!("  {:>3}°C -> {:>3}%", point.temp, point.duty_percent());
            }
        }
        FanCurveCommands::Set { name } => {
            proxy
                .call::<_, _, ()>("SetFanCurveByName", &(name.as_str(),))
                .await
                .map_err(daemon_error)?;
            println!("Fan curve set to: {}", name);
        }
        FanCurveCommands::SetDefault { name } => {
            proxy
                .call::<_, _, ()>("SetDefaultFanCurve", &(name.as_str(),))
                .await
                .map_err(daemon_error)?;
            println!(
                "Default fan curve set to: {} (persists across reboots)",
                name
            );
        }
        FanCurveCommands::AddPoint { temp, duty } => {
            proxy
                .call::<_, _, ()>("AddFanCurvePoint", &(temp, duty))
                .await
                .map_err(daemon_error)?;
            println!("Added point to current curve: {}°C -> {}%", temp, duty);
        }
        FanCurveCommands::RemovePoint => {
            proxy
                .call::<_, _, ()>("RemoveFanCurvePoint", &())
                .await
                .map_err(daemon_error)?;
            println!("Removed last point from current curve");
        }
        FanCurveCommands::Save => {
            proxy
                .call::<_, _, ()>("SaveConfig", &())
                .await
                .map_err(daemon_error)?;
            println!("Configuration saved");
        }
        FanCurveCommands::Load => {
            proxy
                .call::<_, _, ()>("ReloadConfig", &())
                .await
                .map_err(daemon_error)?;
            println!("Configuration reloaded from disk");
        }
        FanCurveCommands::Status => {
            let (temp, duty, pwm, speeds): (f64, u16, u8, Vec<(u8, u16, String)>) =
                proxy.call("GetStatus", &()).await.map_err(daemon_error)?;
            let curve: FanCurve = proxy
                .call("GetCurrentFanCurve", &())
                .await
                .map_err(daemon_error)?;
            let default: FanCurve = proxy
                .call("GetDefaultFanCurve", &())
                .await
                .map_err(daemon_error)?;

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
        }
    }

    Ok(())
}
