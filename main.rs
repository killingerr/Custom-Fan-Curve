// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

#![deny(clippy::all)]

use clap::Parser;
use log::LevelFilter;
use std::process;
use system76_power::{args::Args, client, daemon, logging};

mod fan;
mod fan_curve_gui;
use fan_curve_gui::FanCurveApp;

fn main() {
    let args = Args::parse();

    let res = match args {
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
                daemon::daemon()
            } else {
                Err(anyhow::anyhow!("must be run as root"))
            }
        }
        Args::Gui => {
            let options = eframe::NativeOptions::default();
            eframe::run_native(
                "Fan Curve Control",
                options,
                Box::new(|cc| Box::new(FanCurveApp::new(cc)))
            );
            Ok(())
        }
        _ => client::client(&args),
    };

    if let Err(err) = res {
        eprintln!("{:?}", err);
        process::exit(1);
    }
}
