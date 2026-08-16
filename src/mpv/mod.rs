//! MPV subsystem facade, including process supervision, IPC client, and stream recording.

pub mod client;
pub mod process;
pub mod record;

pub use client::{parse_artist_title, MpvClient};
pub use process::MpvProcessManager;
pub use record::StreamRecorder;
