//! Fan Curve Control Application
//!
//! A System76 Power-compatible fan curve management application with GUI and DBus interfaces.

pub mod args;
pub mod client;
pub mod cpu_temp;
pub mod daemon;
pub mod errors;
pub mod fan;
pub mod fan_control;
pub mod fan_curve_gui;
pub mod fan_detector;
pub mod fan_monitor;
pub mod logging;
pub mod system76_power_client;
pub mod thelio_io;

// DBus constants following System76 Power patterns
pub const DBUS_SERVICE_NAME: &str = "com.system76.FanCurveDaemon";
pub const DBUS_OBJECT_PATH: &str = "/com/system76/FanCurveDaemon";
pub const DBUS_INTERFACE_NAME: &str = "com.system76.FanCurveDaemon";

// Re-export commonly used types
pub use errors::{FanCurveError, Result};
pub use fan::{FanCurve, FanCurveConfig, FanPoint};
