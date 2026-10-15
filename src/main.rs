//! Main entry point for the fan curve application

use clap::Parser;
use eframe::egui;
use fan_curve_app::{
    args::Args, client::FanCurveClient, daemon::FanCurveDaemon, fan_curve_gui::FanCurveApp, logging,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    logging::setup(args.verbose).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

    // GUI uses zbus's blocking API, which cannot run inside a Tokio runtime.
    // Keep this path fully synchronous.
    if args.gui {
        return run_gui().map_err(|e| Box::new(e) as Box<dyn std::error::Error>);
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async_main(args))
}

async fn async_main(args: Args) -> Result<(), Box<dyn std::error::Error>> {
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

    if let Some(fan_curve_app::args::Commands::Notify) = args.command {
        fan_curve_app::notifier::run()
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
            .with_inner_size(fan_curve_app::fan_curve_gui::initial_window_size())
            .with_min_inner_size(fan_curve_app::fan_curve_gui::MIN_WINDOW_SIZE)
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
