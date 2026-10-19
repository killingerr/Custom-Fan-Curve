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
}
