//! Main entry point for the fan curve application

use clap::Parser;
use eframe::egui;
use fan_curve_app::{
    args::Args, client::FanCurveClient, daemon::FanCurveDaemon, fan_curve_gui::FanCurveApp, logging,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Print version and build metadata for binary identity verification
    let pkg_version = env!("CARGO_PKG_VERSION");
    let git_hash = option_env!("GIT_HASH").unwrap_or("unknown");
    let git_desc = option_env!("GIT_DESC").unwrap_or("unknown");
    let build_time = option_env!("BUILD_TIME").unwrap_or("unknown");
    eprintln!(
        "fan-curve-app v{} (git {} / {}) built {}",
        pkg_version, git_hash, git_desc, build_time
    );
    // Parse command line arguments
    let args = Args::parse();

    // Setup logging
    logging::setup(args.verbose).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

    // Handle GUI mode
    if args.gui {
        return run_gui().map_err(|e| Box::new(e) as Box<dyn std::error::Error>);
    }

    // Handle daemon mode
    if let Some(fan_curve_app::args::Commands::Daemon) = args.command {
        let daemon =
            FanCurveDaemon::new().map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        daemon
            .run()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        return Ok(());
    }

    // Handle client mode
    let client = FanCurveClient::new()
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    client
        .handle_args(args)
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

    Ok(())
}

/// Run the GUI application
fn run_gui() -> Result<(), eframe::Error> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0])
            .with_resizable(true)
            .with_decorations(true)
            .with_title("Fan Curve Control"),
        ..Default::default()
    };

    eframe::run_native(
        "Fan Curve Control",
        native_options,
        Box::new(|cc| Ok(Box::new(FanCurveApp::new(cc)))),
    )
}
