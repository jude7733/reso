//! Central event definitions and asynchronous messaging channels for `reso`.

use crate::config::Station;
use std::path::PathBuf;
use std::time::Duration;

/// Playback state of the audio engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Buffering,
    Playing,
    Paused,
}

/// Metadata extracted from ICY stream headers or API polling.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackMetadata {
    pub artist: Option<String>,
    pub title: Option<String>,
    pub album: Option<String>,
    pub year: Option<String>,
    pub raw_title: Option<String>,
    pub cover_url: Option<String>,
    pub duration_remaining_secs: Option<u64>,
}

impl TrackMetadata {
    /// Formats the track as `Artist - Title` or fallback to `raw_title`.
    pub fn display_name(&self) -> String {
        match (&self.artist, &self.title) {
            (Some(a), Some(t)) if !a.is_empty() && !t.is_empty() => format!("{} - {}", a, t),
            (_, Some(t)) if !t.is_empty() => t.clone(),
            _ => self.raw_title.clone().unwrap_or_else(|| "Unknown Track".to_string()),
        }
    }
}

/// Stream parameters reported by the MPV decoder.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudioStreamParams {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u32,
    pub bit_depth: Option<u8>,
    pub bitrate_kbps: Option<u32>,
}

/// Events emitted by the MPV IPC client.
#[derive(Debug, Clone)]
pub enum MpvEvent {
    State(PlaybackState),
    Metadata(TrackMetadata),
    AudioParams(AudioStreamParams),
    Volume(f64),
    RecordingState { is_recording: bool, path: Option<PathBuf> },
    Error(String),
}

/// User or system action dispatched to the audio engine / orchestrator.
#[derive(Debug, Clone)]
pub enum AppAction {
    Play(Station),
    Pause,
    Resume,
    TogglePause,
    Stop,
    NextStation,
    PrevStation,
    ToggleRecord,
    StartRecord(Option<PathBuf>),
    StopRecord,
    SetSleepTimer(Option<Duration>),
    VolumeUp,
    VolumeDown,
    SetVolume(f64),
    ToggleMute,
    BookmarkTrack,
    CopyTrackToClipboard,
    Search(String),
    Quit,
}
