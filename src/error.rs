//! Error handling definitions for the `reso` application.

use thiserror::Error;

/// Domain errors encountered across the `reso` subsystems.
#[derive(Error, Debug)]
pub enum ResoError {
    /// Configuration reading or writing error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// File I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// TOML parsing or serialization error.
    #[error("TOML serialization error: {0}")]
    Toml(#[from] toml::de::Error),

    /// TOML write error.
    #[error("TOML write error: {0}")]
    TomlSer(#[from] toml::ser::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// MPV process or IPC error.
    #[error("MPV error: {0}")]
    Mpv(String),

    /// IPC communication socket error.
    #[error("MPV IPC error: {0}")]
    Ipc(String),

    /// Audio pipeline inspection error.
    #[error("Audio inspector error: {0}")]
    Inspector(String),

    /// PipeWire command or dump error.
    #[error("PipeWire error: {0}")]
    PipeWire(String),

    /// ALSA /proc/asound error.
    #[error("ALSA error: {0}")]
    Alsa(String),

    /// HTTP request or network API error.
    #[error("Network API error: {0}")]
    Api(#[from] reqwest::Error),

    /// Clipboard access error.
    #[error("Clipboard error: {0}")]
    Clipboard(String),

    /// MPRIS D-Bus error.
    #[error("MPRIS D-Bus error: {0}")]
    Mpris(String),

    /// Scrobbler error.
    #[error("Scrobbler error: {0}")]
    Scrobbler(String),

    /// Generic application error.
    #[error("{0}")]
    Custom(String),
}

/// Specialized `Result` type for `reso`.
pub type Result<T> = std::result::Result<T, ResoError>;
