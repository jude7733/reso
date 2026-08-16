//! Ecosystem integrations facade: MPRIS2, Radio-Browser/Radio Paradise APIs, Scrobbler, and Clipboard.

pub mod api;
pub mod clipboard;
pub mod mpris;
pub mod scrobbler;

pub use api::ApiClient;
pub use clipboard::copy_to_clipboard;
pub use mpris::{start_mpris_server, MprisState};
pub use scrobbler::ScrobbleTracker;
