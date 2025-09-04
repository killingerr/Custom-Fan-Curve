//! Main entry point for the fan curve application

use fan_curve_app::{args::Args, client::FanCurveClient, daemon::FanCurveDaemon, fan_curve_gui::FanCurveApp, logging};
use eframe::egui;
use clap::Parser;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
        let daemon = FanCurveDaemon::new().map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        daemon.run().await.map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        return Ok(());
    }

    // Handle client mode
    let client = FanCurveClient::new().await.map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    client.handle_args(args).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

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
        Box::new(|cc| Ok(Box::new(FanCurveApp::new(cc))))
    )
}