//! Reso: Lossless Audiophile Internet Radio Player & PipeWire Inspector for Arch Linux.

pub mod cli;
pub mod config;
pub mod error;
pub mod events;
pub mod inspector;
pub mod integration;
pub mod mpv;
pub mod tui;
pub mod util;

pub use config::{AppConfig, ConfigManager, Station, StationsCatalog};
pub use error::{ResoError, Result};
pub use events::{AppAction, AudioStreamParams, MpvEvent, PlaybackState, TrackMetadata};
pub use inspector::status::{BitPerfectVerdict, PipelineStatus};
