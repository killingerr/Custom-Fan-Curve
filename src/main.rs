//! Main entry point for the fan curve application

use clap::Parser;
use fan_curve_app::{
    args::Args, client::FanCurveClient, daemon::FanCurveDaemon, iced_gui, logging,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
        run_gui()?;
        return Ok(());
    }

    // For non-GUI modes, we need async, so create a Tokio runtime
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async_main(args))?;

    Ok(())
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
fn run_gui() -> Result<(), Box<dyn std::error::Error>> {
    iced_gui::run_iced_gui()?;
    Ok(())
}
