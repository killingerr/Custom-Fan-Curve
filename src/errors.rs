//! Error types for the fan curve application

use thiserror::Error;

/// Result type alias for the fan curve application
pub type Result<T> = std::result::Result<T, FanCurveError>;

/// Main error type for the fan curve application
#[derive(Error, Debug)]
pub enum FanCurveError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("DBus error: {0}")]
    DBus(#[from] zbus::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Fan curve not found: {name}")]
    FanCurveNotFound { name: String },

    #[error("Invalid fan curve point: temperature {temp}°C, duty {duty}%")]
    InvalidFanPoint { temp: i16, duty: u16 },

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Daemon not running")]
    DaemonNotRunning,

    #[error("Unknown error: {0}")]
    Unknown(String),
}

/// Helper function to convert display errors to zbus errors
pub fn zbus_error_from_display(err: impl std::fmt::Display) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(format!("{}", err))
}
