//! Command line argument parsing for the fan curve application

use clap::{Parser, Subcommand};

/// Fan Curve Control Application
///
/// A System76 Power-compatible fan curve management application with GUI and DBus interfaces.
#[derive(Parser)]
#[command(name = "fan-curve-app")]
#[command(about = "Fan curve control application")]
#[command(version)]
pub struct Args {
    /// Increase verbosity (can be used multiple times)
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Run in GUI mode
    #[arg(long)]
    pub gui: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run the daemon
    Daemon,
    /// Fan curve management
    FanCurve {
        #[command(subcommand)]
        command: FanCurveCommands,
    },
}

#[derive(Subcommand)]
pub enum FanCurveCommands {
    /// List available fan curves
    List,
    /// Get current fan curve
    Get,
    /// Set fan curve by name
    Set {
        /// Name of the fan curve to set
        name: String,
    },
    /// Set default fan curve
    SetDefault {
        /// Name of the fan curve to set as default
        name: String,
    },
    /// Add a new fan curve point
    AddPoint {
        /// Temperature in Celsius
        temp: i16,
        /// Fan duty percentage (0-100)
        duty: u16,
    },
    /// Remove the last fan curve point
    RemovePoint,
    /// Save current configuration
    Save,
    /// Load configuration from file
    Load,
    /// Test fan curve with monitoring
    Test {
        /// Duration of test in seconds
        duration: u64,
        /// Log file path (optional)
        log_file: Option<String>,
    },
}
