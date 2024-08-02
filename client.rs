// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

use crate::args::{Args, FanCurveArgs, GraphicsArgs};
use anyhow::Context;
use system76_power_zbus::PowerDaemonProxy;

async fn profile(client: &mut PowerDaemonProxy<'_>) -> anyhow::Result<()> {
    let profile = client.get_profile().await?;
    println!("Power Profile: {}", profile);
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
pub async fn client(args: &Args) -> anyhow::Result<()> {
    let connection =
        zbus::Connection::system().await.context("failed to create zbus system connection")?;

    let mut client = PowerDaemonProxy::new(&connection)
        .await
        .context("failed to connect to system76-power daemon")?;

    match args {
        Args::Profile { profile: name } => match name.as_deref() {
            Some("balanced") => client.balanced().await.map_err(zbus_error),
            Some("battery") => {
                if client.get_desktop().await.map_err(zbus_error)? {
                    anyhow::bail!("Battery power profile is not supported on desktop computers.");
                }
                client.battery().await.map_err(zbus_error)
            }
            Some("performance") => client.performance().await.map_err(zbus_error),
            _ => profile(&mut client).await,
        },
        Args::Graphics { cmd } => {
            if !client.get_switchable().await? {
                anyhow::bail!("Graphics switching is not supported on this device.");
            }
            match cmd {
                Some(GraphicsArgs::Integrated) => client.set_graphics("integrated").await.map_err(zbus_error),
                Some(GraphicsArgs::Nvidia) => client.set_graphics("nvidia").await.map_err(zbus_error),
                Some(GraphicsArgs::Compute) => client.set_graphics("compute").await.map_err(zbus_error),
                Some(GraphicsArgs::Hybrid) => client.set_graphics("hybrid").await.map_err(zbus_error),
                Some(GraphicsArgs::Switchable) => {
                    println!("{}", if client.get_switchable().await? { "switchable" } else { "not switchable" });
                    Ok(())
                },
                Some(GraphicsArgs::Power { state }) => match state.as_deref() {
                    Some("auto") => client.auto_graphics_power().await.map_err(zbus_error),
                    Some("off") => client.set_graphics_power(false).await.map_err(zbus_error),
                    Some("on") => client.set_graphics_power(true).await.map_err(zbus_error),
                    _ => {
                        println!("{}", if client.get_graphics_power().await? { "on (discrete)" } else { "off (discrete)" });
                        Ok(())
                    }
                },
                None => {
                    println!("{}", client.get_graphics().await?);
                    Ok(())
                }
            }
        },
        Args::ChargeThresholds { profile, list_profiles, thresholds } => {
            if client.get_desktop().await.map_err(zbus_error)? {
                anyhow::bail!("Charge thresholds are not supported on desktop computers.");
            }

            let profiles = client.get_charge_profiles().await.map_err(zbus_error)?;

            if !thresholds.is_empty() {
                let start = thresholds[0];
                let end = thresholds[1];
                client.set_charge_thresholds(&(start, end)).await.map_err(zbus_error)?;
            } else if let Some(name) = profile {
                if let Some(profile) = profiles.iter().find(|p| &p.id == name) {
                    client.set_charge_thresholds(&(profile.start, profile.end)).await.map_err(zbus_error)?;
                } else {
                    anyhow::bail!("No such profile '{}'", name);
                }
            } else if *list_profiles {
                for profile in &profiles {
                    println!("{}", profile.id);
                    println!("  Title: {}", profile.title);
                    println!("  Description: {}", profile.description);
                    println!("  Start: {}", profile.start);
                    println!("  End: {}", profile.end);
                }
                return Ok(());
            }

            let (start, end) = client.get_charge_thresholds().await.map_err(zbus_error)?;
            if let Some(profile) = profiles.iter().find(|p| p.start == start && p.end == end) {
                println!("Profile: {} ({})", profile.title, profile.id);
            } else {
                println!("Profile: Custom");
            }
            println!("Start: {}", start);
            println!("End: {}", end);

            Ok(())
        },
        Args::FanCurve { cmd } => {
            match cmd {
                Some(FanCurveArgs::Get) => {
                    let curve = client.get_fan_curve().await.map_err(zbus_error)?;
                    println!("Current Fan Curve:");
                    for (temp, speed) in curve {
                        println!("Temperature: {}°C, Speed: {}%", temp, speed);
                    }
                },
                Some(FanCurveArgs::Set { points }) => {
                    let curve: Vec<(u8, u8)> = points.chunks(2).map(|chunk| (chunk[0], chunk[1])).collect();
                    client.set_fan_curve(&curve).await.map_err(zbus_error)?;
                    println!("Fan curve set successfully");
                },
                None => {
                    println!("No fan curve command specified. Use 'get' to view the current curve or 'set' to set a new one.");
                }
            }
            Ok(())
        },
        Args::Gui => {
            println!("Launching fan curve control GUI...");
            // The actual GUI launching is handled in main.rs
            Ok(())
        },
        Args::Daemon { .. } => unreachable!(),
    }
}

fn zbus_error(why: zbus::Error) -> anyhow::Error { anyhow::anyhow!("{}", why) }
