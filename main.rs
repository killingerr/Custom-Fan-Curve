// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

#![deny(clippy::all)]

use clap::Parser;
use log::LevelFilter;
use std::process;
use system76_power::{args::Args, client, daemon, logging};
use anyhow::Result;

mod fan;
mod fan_curve_gui;
use fan_curve_gui::FanCurveApp;

fn main() -> Result<()> {
    let args = Args::parse();

    match args {
        Args::Daemon { quiet, verbose } => {
            if let Err(why) = logging::setup(if verbose {
                LevelFilter::Debug
            } else if quiet {
                LevelFilter::Off
            } else {
                LevelFilter::Info
            }) {
                eprintln!("failed to set up logging: {}", why);
                process::exit(1);
            }

            if unsafe { libc::geteuid() } == 0 {
                daemon::daemon()?;
            } else {
                return Err(anyhow::anyhow!("Daemon must be run as root"));
            }
        }
        
        Args::Gui => {
            let native_options = eframe::NativeOptions {
                decorated: false,
                transparent: true,
                min_window_size: Some(egui::vec2(320.0, 240.0)),
                resizable: true,
                ..Default::default()
            };
            
            eframe::run_native(
                "Fan Curve Control",
                native_options,
                Box::new(|cc| Box::new(FanCurveApp::new(cc)))
            );
        }
        _ => {
            client::client(&args)?;
        }
    }

    Ok(())
}
