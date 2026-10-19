//! Main entry point for the fan curve application

use clap::Parser;
use eframe::egui;
use fan_curve_app::{
    args::{Args, Commands},
    client,
    daemon::FanCurveDaemon,
    fan_curve_gui::{self, FanCurveApp},
    fan_detector, logging, notifier,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    logging::setup(args.verbose)?;

    // GUI uses zbus's blocking API, which cannot run inside a Tokio runtime.
    // Keep this path fully synchronous.
    if args.gui {
        return Ok(run_gui()?);
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        match args.command {
            Some(Commands::Daemon) => FanCurveDaemon::new()?.run().await?,
            Some(Commands::Notify) => notifier::run().await?,
            Some(Commands::RestoreFans { hwmon_root }) => {
                let written = fan_detector::restore_automatic(&hwmon_root);
                for path in &written {
                    println!("Automatic fan control: {}", path.display());
                }
                if written.is_empty() {
                    println!("No System76 fan controls to restore");
                }
            }
            Some(Commands::FanCurve { command }) => client::run(command).await?,
            None => return Err("No command specified".into()),
        }
        Ok(())
    })
}

fn run_gui() -> Result<(), eframe::Error> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(fan_curve_gui::initial_window_size())
            .with_min_inner_size(fan_curve_gui::MIN_WINDOW_SIZE)
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
