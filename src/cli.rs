//! Clap command line interface definitions and argument schemas for `reso`.

use clap::{Args, Parser, Subcommand};

/// Lossless Audiophile Internet Radio Player & PipeWire Inspector for Arch Linux.
#[derive(Parser, Debug)]
#[command(name = "reso", author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Available CLI subcommands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Launch the interactive full-screen TUI dashboard (default).
    Tui,

    /// List all configured radio stations.
    List(ListArgs),

    /// Play a station by ID, name, or URL.
    Play(PlayArgs),

    /// Pause active playback.
    Pause,

    /// Resume paused playback.
    Resume,

    /// Toggle pause / play.
    Toggle,

    /// Stop playback.
    Stop,

    /// Skip to the next station in catalog.
    Next,

    /// Skip to the previous station in catalog.
    Prev,

    /// Display currently playing station, track metadata, and audio specs.
    Current(CurrentArgs),

    /// Search `radio-browser.info` for lossless online radio streams.
    Search(SearchArgs),

    /// Run the 3-stage Audiophile Pipeline Inspector (Source -> PipeWire -> Hardware DAC).
    Inspect(InspectArgs),

    /// Manage lossless stream recording (~/Music/Radio/).
    Record(RecordArgs),

    /// Bookmark the currently playing track to ~/.config/reso/liked_tracks.md.
    Like,

    /// Copy current track `Artist - Title` to clipboard (Wayland / X11).
    Copy,

    /// Query or adjust PipeWire hardware volume (`wpctl`).
    Volume(VolumeArgs),

    /// Run in background headless daemon mode (MPV IPC + MPRIS2 D-Bus).
    Daemon,
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Show only favorite stations.
    #[arg(short, long)]
    pub favorites: bool,

    /// Output list as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct PlayArgs {
    /// Station ID (e.g. `rp-main`), name substring, or stream URL.
    pub station: Option<String>,

    /// Auto-stop sleep timer (e.g. `45m`, `1h30m`, `300s`).
    #[arg(short, long)]
    pub sleep: Option<String>,

    /// Keep process in foreground until interrupted.
    #[arg(short, long)]
    pub foreground: bool,
}

#[derive(Args, Debug)]
pub struct CurrentArgs {
    /// Output metadata as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Search query term (e.g. `jazz`, `classical`, `chill`).
    pub query: String,

    /// Filter by codec (default: `FLAC`).
    #[arg(short, long, default_value = "FLAC")]
    pub codec: Option<String>,

    /// Filter by minimum bitrate in kbps (e.g. `1000`).
    #[arg(short, long)]
    pub bitrate: Option<u32>,

    /// Maximum results to return.
    #[arg(short, long, default_value = "20")]
    pub limit: usize,

    /// Automatically save search results to stations.toml.
    #[arg(short, long)]
    pub add: bool,
}

#[derive(Args, Debug)]
pub struct InspectArgs {
    /// Output inspection report as raw JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct RecordArgs {
    /// Recording action: `start`, `stop`, `status`, or `toggle` (default).
    #[arg(default_value = "toggle")]
    pub action: String,

    /// Custom output file path.
    #[arg(short, long)]
    pub output: Option<std::path::PathBuf>,
}

#[derive(Args, Debug)]
pub struct VolumeArgs {
    /// Volume action: `get`, `up`, `down`, `mute`, or a percentage value like `50%` or `0.5`.
    #[arg(default_value = "get")]
    pub action: String,
}
