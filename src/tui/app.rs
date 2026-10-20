//! Application state machine, keybindings, and async event loop for the TUI.

use crate::config::{ConfigManager, LikedTrack, Station, StationsCatalog};
use crate::error::Result;
use crate::events::{AppAction, AudioStreamParams, MpvEvent, PlaybackState, TrackMetadata};
use crate::inspector::status::PipelineStatus;
use crate::integration::api::ApiClient;
use crate::integration::clipboard::copy_to_clipboard;
use crate::integration::mpris::MprisState;
use crate::integration::prober::StreamProbeResult;
use crate::integration::scrobbler::ScrobbleTracker;
use crate::mpv::client::MpvClient;
use crate::mpv::record::StreamRecorder;
use crate::tui::cover::CoverArtManager;
use crate::tui::theme::Theme;
use crate::tui::visualizer::VisualizerState;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::widgets::ListState;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

/// Active dashboard tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Stations,
    Search,
    Inspector,
    Liked,
    Settings,
}

/// Notification level for transient status messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// A transient toast notification shown in the footer or banner.
#[derive(Debug, Clone, PartialEq)]
pub struct ToastNotification {
    pub message: String,
    pub created_at: std::time::Instant,
    pub duration: std::time::Duration,
    pub level: ToastLevel,
}

/// Settings panel navigation and form state.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsState {
    pub selected_section: usize, // 0: UI & Themes, 1: Playback & General, 2: Scrobbling, 3: System & Save
    pub selected_field: usize,
    pub is_editing_text: bool,
    pub text_buffer: String,

    // UI Preferences
    pub theme: String,
    pub show_visualizer: bool,
    pub default_sort_mode: String,
    pub cover_mode: String,

    // General & Playback
    pub default_station_id: String,
    pub record_dir: String,
    pub socket_path: String,

    // Scrobbling
    pub lastfm_enabled: bool,
    pub lastfm_username: String,
    pub lastfm_api_key: String,
    pub lastfm_api_secret: String,
    pub lastfm_session_key: String,
    pub lastfm_pending_token: Option<String>,
    pub listenbrainz_enabled: bool,
    pub listenbrainz_token: String,
}

impl SettingsState {
    pub fn from_config(config: &crate::config::AppConfig) -> Self {
        Self {
            selected_section: 0,
            selected_field: 0,
            is_editing_text: false,
            text_buffer: String::new(),

            theme: config.ui.theme.clone(),
            show_visualizer: config.ui.show_visualizer,
            default_sort_mode: config.ui.default_sort_mode.clone(),
            cover_mode: config.ui.cover_mode.clone(),

            default_station_id: config
                .default_station_id
                .clone()
                .unwrap_or_else(|| "rp-main".to_string()),
            record_dir: config.record_dir.to_string_lossy().to_string(),
            socket_path: config.socket_path.to_string_lossy().to_string(),

            lastfm_enabled: config.lastfm.enabled,
            lastfm_username: config.lastfm.username.clone().unwrap_or_default(),
            lastfm_api_key: config.lastfm.api_key.clone().unwrap_or_default(),
            lastfm_api_secret: config.lastfm.api_secret.clone().unwrap_or_default(),
            lastfm_session_key: config.lastfm.session_key.clone().unwrap_or_default(),
            lastfm_pending_token: None,
            listenbrainz_enabled: config.listenbrainz.enabled,
            listenbrainz_token: config.listenbrainz.user_token.clone().unwrap_or_default(),
        }
    }

    pub fn fields_count(&self) -> usize {
        match self.selected_section {
            0 => 4, // Theme, Visualizer, Sort Mode, Cover Mode
            1 => 3, // Default Station, Record Dir, Socket Path
            2 => 6, // LastFM Enabled, LastFM Key, LastFM Secret, LastFM Auth, LB Enabled, LB Token
            3 => 1, // Save Settings Button
            _ => 1,
        }
    }
}

/// Modal dialog state for help, adding station, editing, or deletion confirmations.
#[derive(Debug, Clone, PartialEq)]
pub enum ModalState {
    None,
    Help,
    AddStation(AddStationState),
    EditStation(EditStationState),
    ConfirmDeleteStation {
        station_id: String,
        station_name: String,
    },
    ConfirmDeleteLiked {
        track_id: String,
        track_title: String,
    },
}

/// Available sorting modes for the stations catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StationSortMode {
    #[default]
    Default,
    QualityDesc,
    FavoritesFirst,
    NameAsc,
    NameDesc,
    Codec,
}

impl StationSortMode {
    pub fn display_label(&self) -> &'static str {
        match self {
            StationSortMode::Default => "Default",
            StationSortMode::QualityDesc => "Quality (Hi-Res ↓)",
            StationSortMode::FavoritesFirst => "Favorites ★",
            StationSortMode::NameAsc => "Name (A-Z)",
            StationSortMode::NameDesc => "Name (Z-A)",
            StationSortMode::Codec => "Codec",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            StationSortMode::Default => StationSortMode::QualityDesc,
            StationSortMode::QualityDesc => StationSortMode::FavoritesFirst,
            StationSortMode::FavoritesFirst => StationSortMode::NameAsc,
            StationSortMode::NameAsc => StationSortMode::NameDesc,
            StationSortMode::NameDesc => StationSortMode::Codec,
            StationSortMode::Codec => StationSortMode::Default,
        }
    }

    pub fn from_str_key(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "favorites" | "favoritesfirst" | "fav" => StationSortMode::FavoritesFirst,
            "quality" | "qualitydesc" | "hires" => StationSortMode::QualityDesc,
            "name_asc" | "name" | "az" => StationSortMode::NameAsc,
            "name_desc" | "za" => StationSortMode::NameDesc,
            "codec" => StationSortMode::Codec,
            _ => StationSortMode::Default,
        }
    }

    pub fn to_str_key(&self) -> &'static str {
        match self {
            StationSortMode::Default => "default",
            StationSortMode::QualityDesc => "quality",
            StationSortMode::FavoritesFirst => "favorites",
            StationSortMode::NameAsc => "name_asc",
            StationSortMode::NameDesc => "name_desc",
            StationSortMode::Codec => "codec",
        }
    }
}

/// State container for the interactive Add Station modal.
#[derive(Debug, Clone, PartialEq)]
pub struct AddStationState {
    pub step: AddStationStep,
    pub url_input: String,
    pub name_input: String,
    pub tags_input: String,
    pub active_field: usize, // 0: URL, 1: Name, 2: Tags
    pub probe_result: Option<StreamProbeResult>,
    pub probe_error: Option<String>,
    pub duplicate_warning: Option<String>,
    pub previous_station: Option<Station>,
    pub previous_playback_state: Option<PlaybackState>,
}

impl Default for AddStationState {
    fn default() -> Self {
        Self::new()
    }
}

impl AddStationState {
    pub fn new() -> Self {
        Self {
            step: AddStationStep::Input,
            url_input: String::new(),
            name_input: String::new(),
            tags_input: String::new(),
            active_field: 0,
            probe_result: None,
            probe_error: None,
            duplicate_warning: None,
            previous_station: None,
            previous_playback_state: None,
        }
    }

    /// Checks if the currently entered URL is already in the stations catalog.
    pub fn check_duplicate(&mut self, catalog: &StationsCatalog) {
        let clean = self.url_input.trim();
        if clean.is_empty() {
            self.duplicate_warning = None;
            return;
        }

        let norm_input = crate::util::normalize_stream_url(clean);
        if let Some(existing) = catalog
            .stations
            .iter()
            .find(|s| crate::util::normalize_stream_url(&s.url) == norm_input)
        {
            self.duplicate_warning = Some(existing.name.clone());
        } else {
            self.duplicate_warning = None;
        }
    }
}

/// State container for the interactive Edit Station modal.
#[derive(Debug, Clone, PartialEq)]
pub struct EditStationState {
    pub original_id: String,
    pub name_input: String,
    pub url_input: String,
    pub codec_input: String,
    pub sample_rate_input: String,
    pub bit_depth_input: String,
    pub tags_input: String,
    pub active_field: usize, // 0: Name, 1: URL, 2: Codec, 3: Sample Rate, 4: Bit Depth, 5: Tags
    pub is_probing: bool,
    pub probe_message: Option<String>,
}

impl EditStationState {
    pub fn from_station(station: &Station) -> Self {
        Self {
            original_id: station.id.clone(),
            name_input: station.name.clone(),
            url_input: station.url.clone(),
            codec_input: station.codec.clone(),
            sample_rate_input: if station.sample_rate == 0 {
                "44100".to_string()
            } else {
                station.sample_rate.to_string()
            },
            bit_depth_input: if station.bit_depth == 0 {
                "16".to_string()
            } else {
                station.bit_depth.to_string()
            },
            tags_input: station.tags.join(", "),
            active_field: 0,
            is_probing: false,
            probe_message: None,
        }
    }
}

/// Steps in the Add Station workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddStationStep {
    /// User inputs stream URL.
    Input,
    /// Probing stream connection, ICY metadata, and codec headers.
    Probing,
    /// Format verified; user can edit name/tags and confirm permanent addition.
    Confirm,
    /// Probe failed; error displayed with options to retry or force add.
    Error,
}

/// Central state machine for the TUI.
pub struct App {
    pub active_tab: ActiveTab,
    pub config_manager: ConfigManager,
    pub catalog: StationsCatalog,
    pub station_sort_mode: StationSortMode,
    pub current_station: Option<Station>,
    pub station_list_index: usize,
    pub station_list_state: ListState,
    pub playback_state: PlaybackState,
    pub track_metadata: TrackMetadata,
    pub stream_params: AudioStreamParams,
    pub pipeline_status: PipelineStatus,
    pub hardware_volume: f64,
    pub is_muted: bool,
    pub is_recording: bool,
    pub active_recording_path: Option<PathBuf>,
    pub sleep_timer_secs_remaining: Option<u64>,
    pub filter_query: String,
    pub search_query: String,
    pub search_results: Vec<Station>,
    pub search_results_index: usize,
    pub search_list_state: ListState,
    pub search_input_active: bool,
    pub liked_tracks: Vec<LikedTrack>,
    pub liked_list_index: usize,
    pub liked_list_state: ListState,
    pub modal_state: ModalState,
    pub help_scroll: usize,
    pub visualizer_state: VisualizerState,
    pub cover_art_manager: CoverArtManager,
    pub tick_count: u64,
    pub should_quit: bool,

    // Performance: cached filtered station indices to avoid per-frame cloning
    cached_filtered_indices: Vec<usize>,
    filtered_cache_dirty: bool,
    pub diagnostics_in_flight: bool,
    pub rp_poll_in_flight: bool,

    pub toast: Option<ToastNotification>,
    pub track_started_at: Option<std::time::Instant>,
    pub settings_state: SettingsState,
    pub theme: Theme,
    pub theme_last_mtime: Option<std::time::SystemTime>,

    // Subsystems
    pub mpv_client: MpvClient,
    pub api_client: ApiClient,
    pub recorder: StreamRecorder,
    pub scrobbler: ScrobbleTracker,
    pub mpris_state: Option<Arc<Mutex<MprisState>>>,
    pub action_tx: mpsc::Sender<AppAction>,
}

impl App {
    /// Initializes a new `App` state container.
    pub fn new(
        config_manager: ConfigManager,
        mpv_client: MpvClient,
        recorder: StreamRecorder,
        action_tx: mpsc::Sender<AppAction>,
        mpris_state: Option<Arc<Mutex<MprisState>>>,
    ) -> Self {
        let catalog = config_manager.catalog.clone();
        let liked_tracks = config_manager.liked_catalog.tracks.clone();
        let api_client = ApiClient::new();
        let scrobbler = ScrobbleTracker::new(
            config_manager.config.lastfm.clone(),
            config_manager.config.listenbrainz.clone(),
        );
        let show_visualizer = config_manager.config.ui.show_visualizer;
        let default_sort =
            StationSortMode::from_str_key(&config_manager.config.ui.default_sort_mode);
        let settings_state = SettingsState::from_config(&config_manager.config);

        let theme = Theme::load(
            &config_manager.config.ui.theme,
            config_manager.config.ui.theme_path.as_deref(),
        );
        let theme_file_path = if config_manager.config.ui.theme == "wal" {
            Some(
                config_manager
                    .config
                    .ui
                    .theme_path
                    .as_deref()
                    .map(crate::util::expand_home_dir)
                    .unwrap_or_else(crate::util::default_wal_cache_path),
            )
        } else {
            config_manager
                .config
                .ui
                .theme_path
                .as_deref()
                .map(crate::util::expand_home_dir)
        };
        let theme_last_mtime = theme_file_path
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok());

        let mut app = Self {
            active_tab: ActiveTab::Stations,
            config_manager,
            catalog,
            station_sort_mode: default_sort,
            current_station: None,
            station_list_index: 0,
            station_list_state: ListState::default(),
            playback_state: PlaybackState::Stopped,
            track_metadata: TrackMetadata::default(),
            stream_params: AudioStreamParams::default(),
            pipeline_status: PipelineStatus::default(),
            hardware_volume: 0.5,
            is_muted: false,
            is_recording: false,
            active_recording_path: None,
            sleep_timer_secs_remaining: None,
            filter_query: String::new(),
            search_query: String::new(),
            search_results: Vec::new(),
            search_results_index: 0,
            search_list_state: ListState::default(),
            search_input_active: false,
            liked_tracks,
            liked_list_index: 0,
            liked_list_state: ListState::default(),
            modal_state: ModalState::None,
            help_scroll: 0,
            visualizer_state: {
                let mut vs = VisualizerState::default();
                vs.show_visualizer = show_visualizer;
                vs
            },
            cover_art_manager: CoverArtManager::new(),
            tick_count: 0,
            should_quit: false,
            cached_filtered_indices: Vec::new(),
            filtered_cache_dirty: true,
            diagnostics_in_flight: false,
            rp_poll_in_flight: false,
            toast: None,
            track_started_at: None,
            settings_state,
            theme,
            theme_last_mtime,
            mpv_client,
            api_client,
            recorder,
            scrobbler,
            mpris_state,
            action_tx,
        };

        app.refresh_hardware_volume();
        app
    }

    /// Reloads the active theme from disk or preset.
    pub fn reload_theme(&mut self) {
        let is_wal = self.config_manager.config.ui.theme == "wal";
        let resolved_path = if is_wal {
            Some(
                self.config_manager
                    .config
                    .ui
                    .theme_path
                    .as_deref()
                    .map(crate::util::expand_home_dir)
                    .unwrap_or_else(crate::util::default_wal_cache_path),
            )
        } else {
            self.config_manager
                .config
                .ui
                .theme_path
                .as_deref()
                .map(crate::util::expand_home_dir)
        };

        if let Some(ref path) = resolved_path {
            if let Ok(meta) = std::fs::metadata(path) {
                self.theme_last_mtime = meta.modified().ok();
            }
        }

        self.theme = Theme::load(
            &self.config_manager.config.ui.theme,
            self.config_manager.config.ui.theme_path.as_deref(),
        );
    }

    /// Checks if the theme file on disk has been updated (e.g. wallpaper changed via pywal).
    pub fn check_theme_file_update(&mut self) {
        if !self.config_manager.config.ui.theme_auto_reload {
            return;
        }

        let is_wal = self.config_manager.config.ui.theme == "wal";
        let has_custom = self.config_manager.config.ui.theme_path.is_some();
        if !is_wal && !has_custom {
            return;
        }

        let resolved_path = if is_wal {
            Some(
                self.config_manager
                    .config
                    .ui
                    .theme_path
                    .as_deref()
                    .map(crate::util::expand_home_dir)
                    .unwrap_or_else(crate::util::default_wal_cache_path),
            )
        } else {
            self.config_manager
                .config
                .ui
                .theme_path
                .as_deref()
                .map(crate::util::expand_home_dir)
        };

        if let Some(path) = resolved_path {
            if let Ok(meta) = std::fs::metadata(&path) {
                if let Ok(mtime) = meta.modified() {
                    if let Some(last) = self.theme_last_mtime {
                        if mtime > last {
                            self.theme_last_mtime = Some(mtime);
                            let theme_name = self.config_manager.config.ui.theme.clone();
                            self.theme = Theme::load(
                                &theme_name,
                                self.config_manager.config.ui.theme_path.as_deref(),
                            );
                            self.show_toast(
                                format!("🎨 Wallpaper theme reloaded: {}", self.theme.name),
                                ToastLevel::Info,
                            );
                        }
                    } else {
                        self.theme_last_mtime = Some(mtime);
                    }
                }
            }
        }
    }

    /// Shows a transient toast notification in the UI.
    pub fn show_toast(&mut self, message: impl Into<String>, level: ToastLevel) {
        self.toast = Some(ToastNotification {
            message: message.into(),
            created_at: std::time::Instant::now(),
            duration: std::time::Duration::from_secs(3),
            level,
        });
    }

    /// Checks if the active toast has expired.
    pub fn check_toast_expiry(&mut self) {
        if let Some(ref t) = self.toast {
            if t.created_at.elapsed() >= t.duration {
                self.toast = None;
            }
        }
    }

    /// Cycles the station sort mode and persists preference to config.toml.
    pub fn cycle_sort_mode(&mut self) {
        self.station_sort_mode = self.station_sort_mode.next();
        self.station_list_index = 0;
        self.invalidate_station_cache();
        self.config_manager.config.ui.default_sort_mode =
            self.station_sort_mode.to_str_key().to_string();
        self.settings_state.default_sort_mode =
            self.station_sort_mode.to_str_key().to_string();
        let _ = self.config_manager.save_config();
        self.show_toast(
            format!("Station sort: {}", self.station_sort_mode.display_label()),
            ToastLevel::Info,
        );
    }

    /// Saves pending settings form to config.toml and applies them immediately.
    pub fn save_settings(&mut self) -> Result<()> {
        let s = self.settings_state.clone();
        self.config_manager.config.ui.theme = s.theme.clone();
        self.reload_theme();
        self.config_manager.config.ui.show_visualizer = s.show_visualizer;
        self.config_manager.config.ui.default_sort_mode = s.default_sort_mode.clone();
        self.config_manager.config.ui.cover_mode = s.cover_mode.clone();

        self.config_manager.config.default_station_id =
            if s.default_station_id.trim().is_empty() {
                None
            } else {
                Some(s.default_station_id.trim().to_string())
            };
        self.config_manager.config.record_dir = PathBuf::from(&s.record_dir);
        self.config_manager.config.socket_path = PathBuf::from(&s.socket_path);

        self.config_manager.config.lastfm.enabled = s.lastfm_enabled;
        self.config_manager.config.lastfm.username = if s.lastfm_username.trim().is_empty() {
            None
        } else {
            Some(s.lastfm_username.trim().to_string())
        };
        self.config_manager.config.lastfm.api_key = if s.lastfm_api_key.trim().is_empty() {
            None
        } else {
            Some(s.lastfm_api_key.trim().to_string())
        };
        self.config_manager.config.lastfm.api_secret = if s.lastfm_api_secret.trim().is_empty() {
            None
        } else {
            Some(s.lastfm_api_secret.trim().to_string())
        };
        self.config_manager.config.lastfm.session_key = if s.lastfm_session_key.trim().is_empty() {
            None
        } else {
            Some(s.lastfm_session_key.trim().to_string())
        };

        self.config_manager.config.listenbrainz.enabled = s.listenbrainz_enabled;
        self.config_manager.config.listenbrainz.user_token =
            if s.listenbrainz_token.trim().is_empty() {
                None
            } else {
                Some(s.listenbrainz_token.trim().to_string())
            };

        // Reload live scrobbler tracker with updated credentials
        self.scrobbler = ScrobbleTracker::new(
            self.config_manager.config.lastfm.clone(),
            self.config_manager.config.listenbrainz.clone(),
        );

        self.visualizer_state.show_visualizer = s.show_visualizer;
        self.station_sort_mode = StationSortMode::from_str_key(&s.default_sort_mode);
        self.invalidate_station_cache();

        self.config_manager.save_config()?;
        self.show_toast(
            "✓ Settings saved to ~/.config/reso/config.toml",
            ToastLevel::Success,
        );
        Ok(())
    }

    /// Loads and refreshes liked tracks from `config_manager`, sanitizing any legacy imported tracks.
    pub fn load_liked_history(&mut self) {
        let mut tracks = self.config_manager.liked_catalog.tracks.clone();
        for track in &mut tracks {
            Self::sanitize_legacy_track(track);
        }
        self.liked_tracks = tracks;
    }

    /// Cleans and sanitizes legacy imported liked tracks that may contain raw markdown,
    /// duplicated timestamps, or embedded metadata in the artist field.
    pub fn sanitize_legacy_track(track: &mut crate::config::LikedTrack) {
        if track.artist.contains("**") || track.artist.contains("[Station:") || track.title == "Liked Track" {
            let raw = track.artist.clone();
            // 1. Strip leading timestamp, e.g. `[2026-08-16 22:08:59]` or [2026-08-16 22:08:59]
            let without_ts = if let Some(idx) = raw.find("]` ") {
                &raw[idx + 3..]
            } else if let Some(idx) = raw.find("] `") {
                &raw[idx + 3..]
            } else if let Some(idx) = raw.find("] ") {
                &raw[idx + 2..]
            } else {
                &raw
            };

            // 2. Separate song title/artist from trailing metadata [Station: ...]
            let (song_part, meta_part) = if let Some(m_idx) = without_ts.find(" [Station:") {
                (&without_ts[..m_idx], Some(&without_ts[m_idx + 1..]))
            } else {
                (without_ts, None)
            };

            // 3. Parse Artist - Title
            if let Some(dash_idx) = song_part.find(" - ") {
                let art = song_part[..dash_idx].replace("**", "").replace('*', "").replace('`', "").trim().to_string();
                let tit = song_part[dash_idx + 3..].replace("**", "").replace('*', "").replace('`', "").trim().to_string();
                if !art.is_empty() {
                    track.artist = art;
                }
                if !tit.is_empty() {
                    track.title = tit;
                }
            } else {
                let clean = song_part.replace("**", "").replace('*', "").replace('`', "").trim().to_string();
                if !clean.is_empty() {
                    track.artist = clean;
                }
            }

            // 4. Parse [Station: ... | Codec: ... | Album: *...*]
            if let Some(meta) = meta_part {
                let clean_meta = meta.trim_start_matches('[').trim_end_matches(']');
                for sec in clean_meta.split('|') {
                    let part = sec.trim();
                    if let Some(st) = part.strip_prefix("Station:") {
                        let st_name = st.trim().to_string();
                        if !st_name.is_empty() {
                            track.station = st_name;
                        }
                    } else if let Some(alb) = part.strip_prefix("Album:") {
                        let alb_name = alb.replace('*', "").trim().to_string();
                        if !alb_name.is_empty() {
                            track.album = Some(alb_name);
                        }
                    } else if let Some(cod) = part.strip_prefix("Codec:") {
                        let cod_str = cod.trim();
                        if let Some(paren) = cod_str.find('(') {
                            track.codec = Some(cod_str[..paren].trim().to_string());
                        } else {
                            track.codec = Some(cod_str.to_string());
                        }
                    }
                }
            }
        }
    }

    /// Queries the active hardware volume and mute status from PipeWire via `wpctl`.
    pub fn refresh_hardware_volume(&mut self) {
        if let Ok(output) = Command::new("wpctl")
            .arg("get-volume")
            .arg("@DEFAULT_AUDIO_SINK@")
            .output()
        {
            if output.status.success() {
                let s = String::from_utf8_lossy(&output.stdout);
                self.is_muted = s.contains("[MUTED]");
                if let Some(vol_part) = s.split_whitespace().nth(1) {
                    if let Ok(v) = vol_part.parse::<f64>() {
                        self.hardware_volume = v.clamp(0.0, 1.5);
                    }
                }
            }
        }
    }

    /// Adjusts hardware volume up or down via `wpctl`.
    pub fn adjust_hardware_volume(&mut self, delta: f64) {
        let arg = if delta > 0.0 {
            format!("{:.0}%+", delta.abs() * 100.0)
        } else {
            format!("{:.0}%-", delta.abs() * 100.0)
        };
        let _ = Command::new("wpctl")
            .arg("set-volume")
            .arg("@DEFAULT_AUDIO_SINK@")
            .arg(&arg)
            .output();
        self.refresh_hardware_volume();
    }

    /// Toggles hardware mute via `wpctl`.
    pub fn toggle_hardware_mute(&mut self) {
        let _ = Command::new("wpctl")
            .arg("set-mute")
            .arg("@DEFAULT_AUDIO_SINK@")
            .arg("toggle")
            .output();
        self.refresh_hardware_volume();
    }

    /// Marks the cached filtered station indices as stale, forcing recomputation on next access.
    pub fn invalidate_station_cache(&mut self) {
        self.filtered_cache_dirty = true;
    }

    /// Returns cached filtered and sorted station indices. Recomputes only when dirty.
    pub fn filtered_station_indices(&mut self) -> &[usize] {
        if self.filtered_cache_dirty {
            self.recompute_filtered_indices();
            self.filtered_cache_dirty = false;
        }
        &self.cached_filtered_indices
    }

    /// Returns the number of filtered stations (uses cache).
    pub fn filtered_station_count(&mut self) -> usize {
        self.filtered_station_indices().len()
    }

    /// Returns a station reference by filtered index (uses cache).
    pub fn filtered_station_at(&mut self, filtered_idx: usize) -> Option<&Station> {
        let indices = if self.filtered_cache_dirty {
            self.recompute_filtered_indices();
            self.filtered_cache_dirty = false;
            &self.cached_filtered_indices
        } else {
            &self.cached_filtered_indices
        };
        indices.get(filtered_idx).map(|&i| &self.catalog.stations[i])
    }

    /// Internal: recomputes filtered + sorted station indices from catalog.
    fn recompute_filtered_indices(&mut self) {
        let q = self.filter_query.trim().to_lowercase();
        let mut indices: Vec<usize> = if q.is_empty() {
            (0..self.catalog.stations.len()).collect()
        } else {
            self.catalog
                .stations
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    s.name.to_lowercase().contains(&q)
                        || s.codec.to_lowercase().contains(&q)
                        || s.tags.iter().any(|t| t.to_lowercase().contains(&q))
                })
                .map(|(i, _)| i)
                .collect()
        };

        let stations = &self.catalog.stations;
        match self.station_sort_mode {
            StationSortMode::Default => {}
            StationSortMode::NameAsc => {
                indices.sort_by(|&a, &b| {
                    stations[a]
                        .name
                        .to_lowercase()
                        .cmp(&stations[b].name.to_lowercase())
                });
            }
            StationSortMode::NameDesc => {
                indices.sort_by(|&a, &b| {
                    stations[b]
                        .name
                        .to_lowercase()
                        .cmp(&stations[a].name.to_lowercase())
                });
            }
            StationSortMode::QualityDesc => {
                indices.sort_by(|&a, &b| {
                    stations[b]
                        .sample_rate
                        .cmp(&stations[a].sample_rate)
                        .then_with(|| stations[b].bit_depth.cmp(&stations[a].bit_depth))
                        .then_with(|| {
                            stations[a]
                                .name
                                .to_lowercase()
                                .cmp(&stations[b].name.to_lowercase())
                        })
                });
            }
            StationSortMode::FavoritesFirst => {
                indices.sort_by(|&a, &b| {
                    stations[b]
                        .favorite
                        .cmp(&stations[a].favorite)
                        .then_with(|| {
                            stations[a]
                                .name
                                .to_lowercase()
                                .cmp(&stations[b].name.to_lowercase())
                        })
                });
            }
            StationSortMode::Codec => {
                indices.sort_by(|&a, &b| {
                    let score = |c: &str| match c.to_uppercase().as_str() {
                        "FLAC" | "ALAC" => 1,
                        "OGG" | "OPUS" => 2,
                        "AAC" => 3,
                        "MP3" => 4,
                        _ => 5,
                    };
                    score(&stations[a].codec)
                        .cmp(&score(&stations[b].codec))
                        .then_with(|| stations[b].sample_rate.cmp(&stations[a].sample_rate))
                        .then_with(|| {
                            stations[a]
                                .name
                                .to_lowercase()
                                .cmp(&stations[b].name.to_lowercase())
                        })
                });
            }
        }

        self.cached_filtered_indices = indices;
    }

    /// Handles prober completion event for both Add and Edit modals.
    pub fn handle_probe_result(&mut self, res: std::result::Result<StreamProbeResult, String>) {
        match &mut self.modal_state {
            ModalState::AddStation(ref mut state) => match res {
                Ok(probe) => {
                    if state.name_input.trim().is_empty() {
                        state.name_input = probe.suggested_name.clone();
                    }
                    if state.tags_input.trim().is_empty() {
                        state.tags_input = probe.tags.join(", ");
                    }
                    state.probe_result = Some(probe);
                    state.probe_error = None;
                    state.step = AddStationStep::Confirm;
                }
                Err(err) => {
                    state.probe_error = Some(err);
                    state.step = AddStationStep::Error;
                }
            },
            ModalState::EditStation(ref mut state) => {
                state.is_probing = false;
                match res {
                    Ok(probe) => {
                        state.codec_input = probe.codec;
                        state.sample_rate_input = probe.sample_rate.to_string();
                        state.bit_depth_input = probe.bit_depth.to_string();
                        if !probe.tags.is_empty() && state.tags_input.trim().is_empty() {
                            state.tags_input = probe.tags.join(", ");
                        }
                        state.probe_message = Some("✓ Stream format auto-detected!".to_string());
                    }
                    Err(err) => {
                        state.probe_message = Some(format!("✗ Probe failed: {}", err));
                    }
                }
            }
            _ => {}
        }
    }

    /// Handles keyboard input from crossterm.
    pub async fn handle_key_event(&mut self, key: KeyEvent) -> Result<()> {
        if key.kind != KeyEventKind::Press {
            return Ok(());
        }

        // 1. Handle Active Modals
        if self.modal_state != ModalState::None {
            self.handle_modal_key_event(key).await?;
            return Ok(());
        }

        // 2. Search Input mode
        if self.search_input_active {
            match key.code {
                KeyCode::Esc => {
                    self.search_input_active = false;
                }
                KeyCode::Enter => {
                    self.search_input_active = false;
                    let query = self.search_query.clone();
                    if !query.trim().is_empty() {
                        let _ = self.action_tx.send(AppAction::Search(query)).await;
                    }
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                }
                _ => {}
            }
            return Ok(());
        }

        // 3. Settings Tab specific keyboard routing
        if self.active_tab == ActiveTab::Settings {
            return self.handle_settings_key_event(key).await;
        }

        // 4. Global Navigation & Actions
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Char('?') => {
                self.help_scroll = 0;
                self.modal_state = ModalState::Help;
            }
            KeyCode::Char('1') => self.active_tab = ActiveTab::Stations,
            KeyCode::Char('2') => {
                self.active_tab = ActiveTab::Search;
                self.search_input_active = true;
            }
            KeyCode::Char('3') => {
                self.active_tab = ActiveTab::Inspector;
                self.trigger_pipeline_diagnostics();
            }
            KeyCode::Char('4') => {
                self.active_tab = ActiveTab::Liked;
                self.load_liked_history();
            }
            KeyCode::Char('5') => {
                self.active_tab = ActiveTab::Settings;
            }
            KeyCode::Tab => {
                self.active_tab = match self.active_tab {
                    ActiveTab::Stations => ActiveTab::Search,
                    ActiveTab::Search => ActiveTab::Inspector,
                    ActiveTab::Inspector => ActiveTab::Liked,
                    ActiveTab::Liked => ActiveTab::Settings,
                    ActiveTab::Settings => ActiveTab::Stations,
                };
                if self.active_tab == ActiveTab::Search {
                    self.search_input_active = true;
                } else if self.active_tab == ActiveTab::Liked {
                    self.load_liked_history();
                } else if self.active_tab == ActiveTab::Inspector {
                    self.trigger_pipeline_diagnostics();
                }
            }
            KeyCode::BackTab => {
                self.active_tab = match self.active_tab {
                    ActiveTab::Stations => ActiveTab::Settings,
                    ActiveTab::Search => ActiveTab::Stations,
                    ActiveTab::Inspector => ActiveTab::Search,
                    ActiveTab::Liked => ActiveTab::Inspector,
                    ActiveTab::Settings => ActiveTab::Liked,
                };
                if self.active_tab == ActiveTab::Search {
                    self.search_input_active = true;
                } else if self.active_tab == ActiveTab::Liked {
                    self.load_liked_history();
                } else if self.active_tab == ActiveTab::Inspector {
                    self.trigger_pipeline_diagnostics();
                }
            }

            // Playback controls
            KeyCode::Char(' ') => {
                let _ = self.action_tx.send(AppAction::TogglePause).await;
            }
            KeyCode::Char('s') => {
                let _ = self.action_tx.send(AppAction::Stop).await;
            }
            KeyCode::Char('n') => {
                let _ = self.action_tx.send(AppAction::NextStation).await;
            }
            KeyCode::Char('p') => {
                let _ = self.action_tx.send(AppAction::PrevStation).await;
            }

            // Volume
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.adjust_hardware_volume(0.05);
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.adjust_hardware_volume(-0.05);
            }
            KeyCode::Char('m') => {
                self.toggle_hardware_mute();
            }

            // Recording & Liking
            KeyCode::Char('r') => {
                let _ = self.action_tx.send(AppAction::ToggleRecord).await;
            }
            KeyCode::Char('l') => {
                let _ = self.action_tx.send(AppAction::BookmarkTrack).await;
            }

            // Edit Station ('e') on Stations tab
            KeyCode::Char('e') => {
                if self.active_tab == ActiveTab::Stations {
                    if let Some(st) = self.filtered_station_at(self.station_list_index) {
                        let st_clone = st.clone();
                        self.modal_state =
                            ModalState::EditStation(EditStationState::from_station(&st_clone));
                    }
                }
            }

            // Copy ('c') - Copy track info across tabs with toast feedback
            KeyCode::Char('c') => match self.active_tab {
                ActiveTab::Liked => {
                    if let Some(track) = self.liked_tracks.get(self.liked_list_index) {
                        let text = format!("{} - {}", track.artist, track.title);
                        match copy_to_clipboard(&text) {
                            Ok(_) => self.show_toast(
                                format!("✓ Copied: {}", text),
                                ToastLevel::Success,
                            ),
                            Err(e) => self.show_toast(
                                format!("✗ Copy failed: {}", e),
                                ToastLevel::Error,
                            ),
                        }
                    }
                }
                _ => {
                    let title_str = self.track_metadata.display_name();
                    match copy_to_clipboard(&title_str) {
                        Ok(_) => self.show_toast(
                            format!("✓ Copied: {}", title_str),
                            ToastLevel::Success,
                        ),
                        Err(e) => self.show_toast(
                            format!("✗ Copy failed: {}", e),
                            ToastLevel::Error,
                        ),
                    }
                }
            },

            // Cycle Sort Mode ('S' / Shift+S)
            KeyCode::Char('S') => {
                self.cycle_sort_mode();
            }

            // Tab-specific Actions: Add Station ('a')
            KeyCode::Char('a') => match self.active_tab {
                ActiveTab::Stations => {
                    self.modal_state = ModalState::AddStation(AddStationState::new());
                }
                ActiveTab::Search => {
                    if let Some(st) = self.search_results.get(self.search_results_index).cloned() {
                        let _ = self.config_manager.add_station(st.clone());
                        self.catalog = self.config_manager.catalog.clone();
                        self.invalidate_station_cache();
                        self.show_toast(
                            format!("✓ Added \"{}\" to catalog", st.name),
                            ToastLevel::Success,
                        );
                    }
                }
                _ => {}
            },

            // Delete Station or Liked Track (vim 'd' or 'x')
            KeyCode::Char('d') | KeyCode::Char('x') => match self.active_tab {
                ActiveTab::Stations => {
                    if let Some(st) = self.filtered_station_at(self.station_list_index) {
                        self.modal_state = ModalState::ConfirmDeleteStation {
                            station_id: st.id.clone(),
                            station_name: st.name.clone(),
                        };
                    }
                }
                ActiveTab::Liked => {
                    if let Some(track) = self.liked_tracks.get(self.liked_list_index) {
                        self.modal_state = ModalState::ConfirmDeleteLiked {
                            track_id: track.id.clone(),
                            track_title: format!("{} - {}", track.artist, track.title),
                        };
                    }
                }
                _ => {}
            },

            // Yank / Copy (vim 'y')
            KeyCode::Char('y') => match self.active_tab {
                ActiveTab::Stations => {
                    if let Some(st) = self.filtered_station_at(self.station_list_index) {
                        let url = st.url.clone();
                        match copy_to_clipboard(&url) {
                            Ok(_) => self.show_toast(
                                format!("✓ Copied URL: {}", url),
                                ToastLevel::Success,
                            ),
                            Err(e) => self.show_toast(
                                format!("✗ Copy failed: {}", e),
                                ToastLevel::Error,
                            ),
                        }
                    }
                }
                ActiveTab::Liked => {
                    if let Some(track) = self.liked_tracks.get(self.liked_list_index) {
                        let text = format!("{} - {}", track.artist, track.title);
                        match copy_to_clipboard(&text) {
                            Ok(_) => self.show_toast(
                                format!("✓ Copied: {}", text),
                                ToastLevel::Success,
                            ),
                            Err(e) => self.show_toast(
                                format!("✗ Copy failed: {}", e),
                                ToastLevel::Error,
                            ),
                        }
                    }
                }
                _ => {}
            },

            // Favorite
            KeyCode::Char('f') => {
                if let Some(st) = self.filtered_station_at(self.station_list_index) {
                    let id = st.id.clone();
                    let was_fav = st.favorite;
                    let name = st.name.clone();
                    self.config_manager.toggle_favorite(&id);
                    self.catalog = self.config_manager.catalog.clone();
                    self.invalidate_station_cache();
                    if was_fav {
                        self.show_toast(
                            format!("Removed \"{}\" from favorites", name),
                            ToastLevel::Info,
                        );
                    } else {
                        self.show_toast(
                            format!("★ Added \"{}\" to favorites", name),
                            ToastLevel::Success,
                        );
                    }
                }
            }

            // Play selected item ('Enter' or 'o')
            KeyCode::Enter | KeyCode::Char('o') => match self.active_tab {
                ActiveTab::Stations => {
                    if let Some(st) = self.filtered_station_at(self.station_list_index) {
                        let station_clone = st.clone();
                        let _ = self.action_tx.send(AppAction::Play(station_clone)).await;
                    }
                }
                ActiveTab::Search => {
                    if let Some(st) = self.search_results.get(self.search_results_index) {
                        let station_clone = st.clone();
                        let _ = self.action_tx.send(AppAction::Play(station_clone)).await;
                    }
                }
                ActiveTab::Liked => {
                    if let Some(track) = self.liked_tracks.get(self.liked_list_index) {
                        let st_opt = self.config_manager.find_station(&track.station).cloned().or_else(|| {
                            let query_lower = track.station.to_lowercase();
                            self.catalog.stations.iter().find(|s| {
                                s.name.to_lowercase().contains(&query_lower) || query_lower.contains(&s.name.to_lowercase())
                            }).cloned()
                        });
                        if let Some(st) = st_opt {
                            let _ = self.action_tx.send(AppAction::Play(st)).await;
                        } else {
                            self.show_toast(
                                format!("Station \"{}\" not found in catalog", track.station),
                                ToastLevel::Warning,
                            );
                        }
                    }
                }
                _ => {}
            },

            KeyCode::Char('/') => {
                self.active_tab = ActiveTab::Search;
                self.search_input_active = true;
            }

            // Up / Down navigation (j / k / arrows)
            KeyCode::Down | KeyCode::Char('j') => match self.active_tab {
                ActiveTab::Stations => {
                    let len = self.filtered_station_count();
                    if len > 0 {
                        self.station_list_index = (self.station_list_index + 1) % len;
                    }
                }
                ActiveTab::Search => {
                    let len = self.search_results.len();
                    if len > 0 {
                        self.search_results_index = (self.search_results_index + 1) % len;
                    }
                }
                ActiveTab::Liked => {
                    let len = self.liked_tracks.len();
                    if len > 0 {
                        self.liked_list_index = (self.liked_list_index + 1) % len;
                    }
                }
                _ => {}
            },
            KeyCode::Up | KeyCode::Char('k') => match self.active_tab {
                ActiveTab::Stations => {
                    let len = self.filtered_station_count();
                    if len > 0 {
                        if self.station_list_index == 0 {
                            self.station_list_index = len - 1;
                        } else {
                            self.station_list_index -= 1;
                        }
                    }
                }
                ActiveTab::Search => {
                    let len = self.search_results.len();
                    if len > 0 {
                        if self.search_results_index == 0 {
                            self.search_results_index = len - 1;
                        } else {
                            self.search_results_index -= 1;
                        }
                    }
                }
                ActiveTab::Liked => {
                    let len = self.liked_tracks.len();
                    if len > 0 {
                        if self.liked_list_index == 0 {
                            self.liked_list_index = len - 1;
                        } else {
                            self.liked_list_index -= 1;
                        }
                    }
                }
                _ => {}
            },
            KeyCode::PageDown => match self.active_tab {
                ActiveTab::Stations => {
                    let len = self.filtered_station_count();
                    if len > 0 {
                        self.station_list_index = (self.station_list_index + 5).min(len - 1);
                    }
                }
                ActiveTab::Search => {
                    let len = self.search_results.len();
                    if len > 0 {
                        self.search_results_index = (self.search_results_index + 5).min(len - 1);
                    }
                }
                ActiveTab::Liked => {
                    let len = self.liked_tracks.len();
                    if len > 0 {
                        self.liked_list_index = (self.liked_list_index + 5).min(len - 1);
                    }
                }
                _ => {}
            },
            KeyCode::PageUp => match self.active_tab {
                ActiveTab::Stations => {
                    self.station_list_index = self.station_list_index.saturating_sub(5);
                }
                ActiveTab::Search => {
                    self.search_results_index = self.search_results_index.saturating_sub(5);
                }
                ActiveTab::Liked => {
                    self.liked_list_index = self.liked_list_index.saturating_sub(5);
                }
                _ => {}
            },
            KeyCode::Home | KeyCode::Char('g') => match self.active_tab {
                ActiveTab::Stations => {
                    self.station_list_index = 0;
                }
                ActiveTab::Search => {
                    self.search_results_index = 0;
                }
                ActiveTab::Liked => {
                    self.liked_list_index = 0;
                }
                _ => {}
            },
            KeyCode::End | KeyCode::Char('G') => match self.active_tab {
                ActiveTab::Stations => {
                    let len = self.filtered_station_count();
                    if len > 0 {
                        self.station_list_index = len - 1;
                    }
                }
                ActiveTab::Search => {
                    let len = self.search_results.len();
                    if len > 0 {
                        self.search_results_index = len - 1;
                    }
                }
                ActiveTab::Liked => {
                    let len = self.liked_tracks.len();
                    if len > 0 {
                        self.liked_list_index = len - 1;
                    }
                }
                _ => {}
            },
            _ => {}
        }

        Ok(())
    }

    /// Handles key events when a modal dialog is displayed.
    async fn handle_modal_key_event(&mut self, key: KeyEvent) -> Result<()> {
        match &mut self.modal_state {
            ModalState::Help => match key.code {
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
                    self.modal_state = ModalState::None;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    self.help_scroll = self.help_scroll.saturating_add(1).min(60);
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.help_scroll = self.help_scroll.saturating_sub(1);
                }
                KeyCode::Char('d') | KeyCode::PageDown => {
                    self.help_scroll = self.help_scroll.saturating_add(5).min(60);
                }
                KeyCode::Char('u') | KeyCode::PageUp => {
                    self.help_scroll = self.help_scroll.saturating_sub(5);
                }
                KeyCode::Char('g') | KeyCode::Home => {
                    self.help_scroll = 0;
                }
                KeyCode::Char('G') | KeyCode::End => {
                    self.help_scroll = 60;
                }
                _ => {}
            },

            ModalState::ConfirmDeleteStation { station_id, .. } => match key.code {
                KeyCode::Char('y') | KeyCode::Enter | KeyCode::Char('d') | KeyCode::Char('x') => {
                    let id = station_id.clone();
                    let _ = self.config_manager.remove_station(&id);
                    self.catalog = self.config_manager.catalog.clone();
                    self.modal_state = ModalState::None;
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') | KeyCode::Char('c') => {
                    self.modal_state = ModalState::None;
                }
                _ => {}
            },

            ModalState::ConfirmDeleteLiked { track_id, .. } => match key.code {
                KeyCode::Char('y') | KeyCode::Enter | KeyCode::Char('d') | KeyCode::Char('x') => {
                    let id = track_id.clone();
                    let _ = self.config_manager.remove_liked_track(&id);
                    self.load_liked_history();
                    self.modal_state = ModalState::None;
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') | KeyCode::Char('c') => {
                    self.modal_state = ModalState::None;
                }
                _ => {}
            },

            ModalState::AddStation(state) => match state.step {
                AddStationStep::Input => match key.code {
                    KeyCode::Esc => {
                        self.modal_state = ModalState::None;
                    }
                    KeyCode::Tab | KeyCode::Down => {
                        state.active_field = (state.active_field + 1) % 3;
                    }
                    KeyCode::BackTab | KeyCode::Up => {
                        state.active_field = (state.active_field + 2) % 3;
                    }
                    KeyCode::Enter => {
                        let url = state.url_input.trim().to_string();
                        if !url.is_empty() {
                            state.previous_station = self.current_station.clone();
                            state.previous_playback_state = Some(self.playback_state);
                            state.step = AddStationStep::Probing;
                            let _ = self.action_tx.send(AppAction::ProbeStream(url)).await;
                        }
                    }
                    KeyCode::Backspace => match state.active_field {
                        0 => {
                            state.url_input.pop();
                            state.check_duplicate(&self.catalog);
                        }
                        1 => {
                            state.name_input.pop();
                        }
                        _ => {
                            state.tags_input.pop();
                        }
                    },
                    KeyCode::Char('u')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        match state.active_field {
                            0 => {
                                state.url_input.clear();
                                state.duplicate_warning = None;
                            }
                            1 => state.name_input.clear(),
                            _ => state.tags_input.clear(),
                        }
                    }
                    KeyCode::Char(c) => match state.active_field {
                        0 => {
                            state.url_input.push(c);
                            state.check_duplicate(&self.catalog);
                        }
                        1 => state.name_input.push(c),
                        _ => state.tags_input.push(c),
                    },
                    _ => {}
                },

                AddStationStep::Probing => {
                    if key.code == KeyCode::Esc {
                        let _ = self
                            .action_tx
                            .send(AppAction::RestorePreviousPlayback)
                            .await;
                        self.modal_state = ModalState::None;
                    }
                }

                AddStationStep::Confirm => match key.code {
                    KeyCode::Enter | KeyCode::Char('y') => {
                        let probe =
                            state
                                .probe_result
                                .clone()
                                .unwrap_or_else(|| StreamProbeResult {
                                    url: state.url_input.clone(),
                                    suggested_name: state.name_input.clone(),
                                    codec: "FLAC".to_string(),
                                    sample_rate: 44100,
                                    bit_depth: 16,
                                    bitrate_kbps: None,
                                    tags: vec!["custom".to_string()],
                                    is_lossless: true,
                                    content_type: "audio/flac".to_string(),
                                    homepage: None,
                                });

                        let final_name = if !state.name_input.trim().is_empty() {
                            state.name_input.trim().to_string()
                        } else {
                            probe.suggested_name
                        };

                        let slug_id = crate::util::sanitize_filename(&final_name)
                            .to_lowercase()
                            .replace(' ', "-");

                        let mut tags: Vec<String> = if !state.tags_input.trim().is_empty() {
                            state
                                .tags_input
                                .split([';', ',', '/'])
                                .map(|t| t.trim().to_lowercase())
                                .filter(|t| !t.is_empty())
                                .collect()
                        } else {
                            probe.tags
                        };

                        if probe.is_lossless && !tags.contains(&"lossless".to_string()) {
                            tags.push("lossless".to_string());
                        }

                        let new_station = Station {
                            id: slug_id,
                            name: final_name,
                            url: state.url_input.trim().to_string(),
                            codec: probe.codec,
                            sample_rate: probe.sample_rate,
                            bit_depth: probe.bit_depth,
                            homepage: probe.homepage,
                            tags,
                            favorite: false,
                            rp_channel: None,
                        };

                        let _ = self.config_manager.add_station(new_station.clone());
                        self.catalog = self.config_manager.catalog.clone();
                        self.current_station = Some(new_station);
                        self.modal_state = ModalState::None;
                    }

                    KeyCode::Char('t') => {
                        let url = state.url_input.trim().to_string();
                        if !url.is_empty() {
                            state.step = AddStationStep::Probing;
                            let _ = self.action_tx.send(AppAction::ProbeStream(url)).await;
                        }
                    }

                    KeyCode::Tab | KeyCode::Down => {
                        state.active_field = if state.active_field == 1 { 2 } else { 1 };
                    }
                    KeyCode::BackTab | KeyCode::Up => {
                        state.active_field = if state.active_field == 1 { 2 } else { 1 };
                    }
                    KeyCode::Backspace => {
                        if state.active_field == 1 {
                            state.name_input.pop();
                        } else {
                            state.tags_input.pop();
                        }
                    }
                    KeyCode::Esc => {
                        let _ = self
                            .action_tx
                            .send(AppAction::RestorePreviousPlayback)
                            .await;
                        self.modal_state = ModalState::None;
                    }
                    KeyCode::Char('c')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        let _ = self
                            .action_tx
                            .send(AppAction::RestorePreviousPlayback)
                            .await;
                        self.modal_state = ModalState::None;
                    }
                    KeyCode::Char(c) => {
                        if state.active_field == 1 {
                            state.name_input.push(c);
                        } else if state.active_field == 2 {
                            state.tags_input.push(c);
                        }
                    }
                    _ => {}
                },

                AddStationStep::Error => match key.code {
                    KeyCode::Char('e') | KeyCode::Tab => {
                        state.step = AddStationStep::Input;
                    }
                    KeyCode::Char('t') | KeyCode::Enter => {
                        let url = state.url_input.trim().to_string();
                        if !url.is_empty() {
                            state.step = AddStationStep::Probing;
                            let _ = self.action_tx.send(AppAction::ProbeStream(url)).await;
                        }
                    }
                    KeyCode::Char('f') => {
                        let name = if !state.name_input.trim().is_empty() {
                            state.name_input.trim().to_string()
                        } else {
                            crate::integration::prober::infer_name_from_url(&state.url_input)
                        };
                        let slug_id = crate::util::sanitize_filename(&name)
                            .to_lowercase()
                            .replace(' ', "-");
                        let new_station = Station {
                            id: slug_id,
                            name,
                            url: state.url_input.trim().to_string(),
                            codec: "FLAC".to_string(),
                            sample_rate: 44100,
                            bit_depth: 16,
                            homepage: None,
                            tags: vec!["custom".to_string()],
                            favorite: false,
                            rp_channel: None,
                        };
                        let _ = self.config_manager.add_station(new_station.clone());
                        self.catalog = self.config_manager.catalog.clone();
                        self.current_station = Some(new_station);
                        self.modal_state = ModalState::None;
                    }
                    KeyCode::Esc | KeyCode::Char('c') | KeyCode::Char('q') => {
                        let _ = self
                            .action_tx
                            .send(AppAction::RestorePreviousPlayback)
                            .await;
                        self.modal_state = ModalState::None;
                    }
                    _ => {}
                },
            },

            ModalState::EditStation(state) => match key.code {
                KeyCode::Esc => {
                    if state.is_probing {
                        let _ = self
                            .action_tx
                            .send(AppAction::RestorePreviousPlayback)
                            .await;
                    }
                    self.modal_state = ModalState::None;
                }
                KeyCode::Char('c')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    if state.is_probing {
                        let _ = self
                            .action_tx
                            .send(AppAction::RestorePreviousPlayback)
                            .await;
                    }
                    self.modal_state = ModalState::None;
                }
                KeyCode::Tab | KeyCode::Down => {
                    state.active_field = (state.active_field + 1) % 6;
                }
                KeyCode::BackTab | KeyCode::Up => {
                    state.active_field = (state.active_field + 5) % 6;
                }
                KeyCode::Char('t')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL)
                        || (state.active_field != 0
                            && state.active_field != 1
                            && state.active_field != 5) =>
                {
                    let url = state.url_input.trim().to_string();
                    if !url.is_empty() {
                        state.is_probing = true;
                        state.probe_message = Some("Probing endpoint format...".to_string());
                        let _ = self.action_tx.send(AppAction::ProbeStream(url)).await;
                    }
                }
                KeyCode::Enter => {
                    let name = if !state.name_input.trim().is_empty() {
                        state.name_input.trim().to_string()
                    } else {
                        "Custom Station".to_string()
                    };
                    let url = state.url_input.trim().to_string();
                    let codec = if !state.codec_input.trim().is_empty() {
                        state.codec_input.trim().to_uppercase()
                    } else {
                        "FLAC".to_string()
                    };
                    let sample_rate = state
                        .sample_rate_input
                        .trim()
                        .parse::<u32>()
                        .unwrap_or(44100);
                    let bit_depth = state.bit_depth_input.trim().parse::<u8>().unwrap_or(16);
                    let tags: Vec<String> = state
                        .tags_input
                        .split([';', ',', '/'])
                        .map(|t| t.trim().to_lowercase())
                        .filter(|t| !t.is_empty())
                        .collect();

                    let updated = Station {
                        id: state.original_id.clone(),
                        name,
                        url,
                        codec,
                        sample_rate: if sample_rate == 0 { 44100 } else { sample_rate },
                        bit_depth: if bit_depth == 0 { 16 } else { bit_depth },
                        homepage: None,
                        tags,
                        favorite: false,
                        rp_channel: None,
                    };

                    let _ = self
                        .config_manager
                        .edit_station(&state.original_id, updated.clone());
                    self.catalog = self.config_manager.catalog.clone();
                    if let Some(ref cur) = self.current_station {
                        if cur.id == state.original_id {
                            self.current_station = Some(updated);
                        }
                    }
                    self.modal_state = ModalState::None;
                }
                KeyCode::Backspace => match state.active_field {
                    0 => {
                        state.name_input.pop();
                    }
                    1 => {
                        state.url_input.pop();
                    }
                    2 => {
                        state.codec_input.pop();
                    }
                    3 => {
                        state.sample_rate_input.pop();
                    }
                    4 => {
                        state.bit_depth_input.pop();
                    }
                    _ => {
                        state.tags_input.pop();
                    }
                },
                KeyCode::Char('u')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    match state.active_field {
                        0 => state.name_input.clear(),
                        1 => state.url_input.clear(),
                        2 => state.codec_input.clear(),
                        3 => state.sample_rate_input.clear(),
                        4 => state.bit_depth_input.clear(),
                        _ => state.tags_input.clear(),
                    }
                }
                KeyCode::Char(c) => match state.active_field {
                    0 => state.name_input.push(c),
                    1 => state.url_input.push(c),
                    2 => state.codec_input.push(c),
                    3 => {
                        if c.is_ascii_digit() {
                            state.sample_rate_input.push(c);
                        }
                    }
                    4 => {
                        if c.is_ascii_digit() {
                            state.bit_depth_input.push(c);
                        }
                    }
                    _ => state.tags_input.push(c),
                },
                _ => {}
            },

            ModalState::None => {}
        }

        Ok(())
    }

    /// Handles keyboard events specific to ActiveTab::Settings.
    pub async fn handle_settings_key_event(&mut self, key: KeyEvent) -> Result<()> {
        if self.settings_state.is_editing_text {
            match key.code {
                KeyCode::Esc => {
                    self.settings_state.is_editing_text = false;
                }
                KeyCode::Enter => {
                    self.settings_state.is_editing_text = false;
                    let val = self.settings_state.text_buffer.trim().to_string();
                    match (
                        self.settings_state.selected_section,
                        self.settings_state.selected_field,
                    ) {
                        (1, 0) => self.settings_state.default_station_id = val,
                        (1, 1) => self.settings_state.record_dir = val,
                        (1, 2) => self.settings_state.socket_path = val,
                        (2, 1) => self.settings_state.lastfm_api_key = val,
                        (2, 2) => self.settings_state.lastfm_api_secret = val,
                        (2, 5) => self.settings_state.listenbrainz_token = val,
                        _ => {}
                    }
                    self.show_toast("Field updated (press Ctrl+S to save)", ToastLevel::Info);
                }
                KeyCode::Backspace => {
                    self.settings_state.text_buffer.pop();
                }
                KeyCode::Char(c) => {
                    self.settings_state.text_buffer.push(c);
                }
                _ => {}
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Char('1') => self.active_tab = ActiveTab::Stations,
            KeyCode::Char('2') => {
                self.active_tab = ActiveTab::Search;
                self.search_input_active = true;
            }
            KeyCode::Char('3') => {
                self.active_tab = ActiveTab::Inspector;
                self.trigger_pipeline_diagnostics();
            }
            KeyCode::Char('4') => {
                self.active_tab = ActiveTab::Liked;
                self.load_liked_history();
            }
            KeyCode::Char('5') => {}

            KeyCode::Tab => {
                self.active_tab = ActiveTab::Stations;
            }
            KeyCode::BackTab => {
                self.active_tab = ActiveTab::Liked;
                self.load_liked_history();
            }

            KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('[') => {
                if self.settings_state.selected_section == 0 {
                    self.settings_state.selected_section = 3;
                } else {
                    self.settings_state.selected_section -= 1;
                }
                self.settings_state.selected_field = 0;
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(']') => {
                self.settings_state.selected_section =
                    (self.settings_state.selected_section + 1) % 4;
                self.settings_state.selected_field = 0;
            }

            KeyCode::Up | KeyCode::Char('k') => {
                let count = self.settings_state.fields_count();
                if self.settings_state.selected_field == 0 {
                    self.settings_state.selected_field = count.saturating_sub(1);
                } else {
                    self.settings_state.selected_field -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let count = self.settings_state.fields_count();
                if count > 0 {
                    self.settings_state.selected_field =
                        (self.settings_state.selected_field + 1) % count;
                }
            }

            KeyCode::Char(' ') | KeyCode::Enter => match self.settings_state.selected_section {
                0 => match self.settings_state.selected_field {
                    0 => {
                        self.settings_state.theme = match self.settings_state.theme.as_str() {
                            "audiophile_dark" => "cyberpunk".to_string(),
                            "cyberpunk" => "monochrome".to_string(),
                            "monochrome" => "wal".to_string(),
                            _ => "audiophile_dark".to_string(),
                        };
                        self.config_manager.config.ui.theme = self.settings_state.theme.clone();
                        self.reload_theme();
                        let _ = self.config_manager.save_config();
                        self.show_toast(
                            format!("Theme switched to: {}", self.settings_state.theme),
                            ToastLevel::Info,
                        );
                    }
                    1 => {
                        self.settings_state.show_visualizer =
                            !self.settings_state.show_visualizer;
                        let state_str = if self.settings_state.show_visualizer {
                            "Enabled"
                        } else {
                            "Disabled"
                        };
                        self.show_toast(
                            format!("Visualizer {}", state_str),
                            ToastLevel::Info,
                        );
                    }
                    2 => {
                        let curr = StationSortMode::from_str_key(
                            &self.settings_state.default_sort_mode,
                        );
                        let nxt = curr.next();
                        self.settings_state.default_sort_mode = nxt.to_str_key().to_string();
                        self.show_toast(
                            format!("Default Sort: {}", nxt.display_label()),
                            ToastLevel::Info,
                        );
                    }
                    3 => {
                        self.settings_state.cover_mode =
                            match self.settings_state.cover_mode.as_str() {
                                "auto" => "kitty".to_string(),
                                "kitty" => "sixel".to_string(),
                                "sixel" => "halfblocks".to_string(),
                                "halfblocks" => "off".to_string(),
                                _ => "auto".to_string(),
                            };
                        self.show_toast(
                            format!("Cover Art Mode: {}", self.settings_state.cover_mode),
                            ToastLevel::Info,
                        );
                    }
                    _ => {}
                },
                1 => match self.settings_state.selected_field {
                    0 => {
                        self.settings_state.text_buffer =
                            self.settings_state.default_station_id.clone();
                        self.settings_state.is_editing_text = true;
                    }
                    1 => {
                        self.settings_state.text_buffer =
                            self.settings_state.record_dir.clone();
                        self.settings_state.is_editing_text = true;
                    }
                    2 => {
                        self.settings_state.text_buffer =
                            self.settings_state.socket_path.clone();
                        self.settings_state.is_editing_text = true;
                    }
                    _ => {}
                },
                2 => match self.settings_state.selected_field {
                    0 => {
                        self.settings_state.lastfm_enabled =
                            !self.settings_state.lastfm_enabled;
                        let s = if self.settings_state.lastfm_enabled {
                            "Enabled"
                        } else {
                            "Disabled"
                        };
                        self.show_toast(format!("Last.fm {}", s), ToastLevel::Info);
                    }
                    1 => {
                        self.settings_state.text_buffer =
                            self.settings_state.lastfm_api_key.clone();
                        self.settings_state.is_editing_text = true;
                    }
                    2 => {
                        self.settings_state.text_buffer =
                            self.settings_state.lastfm_api_secret.clone();
                        self.settings_state.is_editing_text = true;
                    }
                    3 => {
                        // Field 3: Last.fm Authorization Flow
                        let api_key = self.settings_state.lastfm_api_key.trim().to_string();
                        let secret = self.settings_state.lastfm_api_secret.trim().to_string();

                        if api_key.is_empty() || secret.is_empty() {
                            self.show_toast(
                                "Please configure Last.fm API Key and Shared Secret first",
                                ToastLevel::Warning,
                            );
                        } else if let Some(token) = self.settings_state.lastfm_pending_token.clone() {
                            // Try to exchange the pending token for session key
                            match crate::integration::scrobbler::fetch_lastfm_session(&api_key, &secret, &token).await {
                                Ok((username, sk)) => {
                                    self.settings_state.lastfm_username = username.clone();
                                    self.settings_state.lastfm_session_key = sk.clone();
                                    self.settings_state.lastfm_pending_token = None;
                                    self.config_manager.config.lastfm.username = Some(username.clone());
                                    self.config_manager.config.lastfm.session_key = Some(sk);
                                    self.config_manager.config.lastfm.api_key = Some(api_key);
                                    self.config_manager.config.lastfm.api_secret = Some(secret);
                                    let _ = self.config_manager.save_config();
                                    self.scrobbler = ScrobbleTracker::new(
                                        self.config_manager.config.lastfm.clone(),
                                        self.config_manager.config.listenbrainz.clone(),
                                    );
                                    self.show_toast(
                                        format!("✓ Last.fm linked as @{}!", username),
                                        ToastLevel::Success,
                                    );
                                }
                                Err(e) => {
                                    let err_str = e.to_string();
                                    if err_str.contains("Unauthorized") || err_str.contains("authorized") {
                                        self.show_toast(
                                            "Click 'Yes, allow access' in your browser, then press Enter here",
                                            ToastLevel::Warning,
                                        );
                                    } else {
                                        self.settings_state.lastfm_pending_token = None;
                                        self.show_toast(
                                            format!("Last.fm Auth Error: {}", err_str),
                                            ToastLevel::Error,
                                        );
                                    }
                                }
                            }
                        } else {
                            // Request a new token and open the browser
                            match crate::integration::scrobbler::request_lastfm_token(&api_key, &secret).await {
                                Ok(token) => {
                                    let auth_url = format!(
                                        "https://www.last.fm/api/auth/?api_key={}&token={}",
                                        api_key, token
                                    );
                                    let _ = std::process::Command::new("xdg-open")
                                        .arg(&auth_url)
                                        .spawn();
                                    self.settings_state.lastfm_pending_token = Some(token);
                                    self.show_toast(
                                        "Browser opened! Allow access on Last.fm, then press Enter here",
                                        ToastLevel::Info,
                                    );
                                }
                                Err(e) => {
                                    self.show_toast(
                                        format!("Failed to request Last.fm token: {}", e),
                                        ToastLevel::Error,
                                    );
                                }
                            }
                        }
                    }
                    4 => {
                        self.settings_state.listenbrainz_enabled =
                            !self.settings_state.listenbrainz_enabled;
                        let s = if self.settings_state.listenbrainz_enabled {
                            "Enabled"
                        } else {
                            "Disabled"
                        };
                        self.show_toast(format!("ListenBrainz {}", s), ToastLevel::Info);
                    }
                    5 => {
                        self.settings_state.text_buffer =
                            self.settings_state.listenbrainz_token.clone();
                        self.settings_state.is_editing_text = true;
                    }
                    _ => {}
                },
                3 => {
                    self.save_settings()?;
                }
                _ => {}
            },

            KeyCode::Char('s')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                self.save_settings()?;
            }
            KeyCode::Char('S') => {
                self.save_settings()?;
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.should_quit = true;
            }
            _ => {}
        }

        Ok(())
    }

    /// Handles MPV IPC events pushed asynchronously.
    pub async fn handle_mpv_event(&mut self, event: MpvEvent) {
        match event {
            MpvEvent::State(s) => {
                if s == PlaybackState::Playing && self.playback_state != PlaybackState::Playing {
                    self.track_started_at = Some(std::time::Instant::now());
                } else if s == PlaybackState::Stopped {
                    self.track_started_at = None;
                }
                self.playback_state = s;
                self.visualizer_state.is_playing = s == PlaybackState::Playing;
                if s != PlaybackState::Playing {
                    self.visualizer_state.stop_capture();
                }
                if s == PlaybackState::Stopped {
                    self.cover_art_manager.set_active_key(None);
                }
                self.update_mpris();
            }
            MpvEvent::Metadata(m) => {
                if self.track_metadata.title != m.title || self.track_metadata.raw_title != m.raw_title {
                    self.track_started_at = Some(std::time::Instant::now());
                }
                self.track_metadata = m.clone();
                self.scrobbler.on_track_change(m.clone()).await;
                self.update_mpris();

                // Universal Cover Art: For non-Radio Paradise channels or fallback
                let is_rp = self
                    .current_station
                    .as_ref()
                    .and_then(|s| s.rp_channel)
                    .is_some();
                if !is_rp {
                    let artist_opt = m.artist.as_ref().cloned().or_else(|| {
                        m.raw_title
                            .as_ref()
                            .and_then(|r| crate::mpv::client::parse_artist_title(r).0)
                    });
                    let title_opt = m.title.as_ref().cloned().or_else(|| {
                        m.raw_title
                            .as_ref()
                            .and_then(|r| crate::mpv::client::parse_artist_title(r).1)
                    });

                    if let (Some(artist), Some(title)) = (artist_opt, title_opt) {
                        if !artist.trim().is_empty() && !title.trim().is_empty() {
                            let api_client = self.api_client.clone();
                            let action_tx = self.action_tx.clone();
                            tokio::spawn(async move {
                                if let Ok(Some(cover_url)) =
                                    api_client.search_itunes_cover_art(&artist, &title).await
                                {
                                    if let Ok(bytes) =
                                        api_client.download_image_bytes(&cover_url).await
                                    {
                                        let _ = action_tx
                                            .send(AppAction::CoverArtReady {
                                                url: cover_url,
                                                bytes,
                                            })
                                            .await;
                                    }
                                }
                            });
                        }
                    }
                }
            }
            MpvEvent::AudioParams(p) => {
                self.stream_params = p;
                self.trigger_pipeline_diagnostics();
            }
            MpvEvent::Volume(_vol) => {}
            MpvEvent::RecordingState { is_recording, path } => {
                self.is_recording = is_recording;
                self.active_recording_path = path;
            }
            MpvEvent::Error(_err) => {}
        }
    }

    /// Triggers an asynchronous re-evaluation of the 3-stage audiophile pipeline diagnostics off-thread.
    pub fn trigger_pipeline_diagnostics(&mut self) {
        if self.diagnostics_in_flight {
            return;
        }

        self.diagnostics_in_flight = true;
        let stream_params = self.stream_params.clone();
        let media_rate = self
            .current_station
            .as_ref()
            .map(|s| s.sample_rate)
            .filter(|&r| r > 0);
        let action_tx = self.action_tx.clone();

        tokio::spawn(async move {
            let status_res = tokio::task::spawn_blocking(move || {
                PipelineStatus::evaluate(&stream_params, 100.0, media_rate)
            })
            .await;
            if let Ok(status) = status_res {
                let _ = action_tx
                    .send(AppAction::DiagnosticsReady(Box::new(status)))
                    .await;
            }
        });
    }

    /// Re-evaluates the 3-stage audiophile pipeline diagnostics synchronously.
    pub fn refresh_pipeline_diagnostics(&mut self) {
        let media_rate = self
            .current_station
            .as_ref()
            .map(|s| s.sample_rate)
            .filter(|&r| r > 0);
        self.pipeline_status = PipelineStatus::evaluate(&self.stream_params, 100.0, media_rate);
    }

    /// Updates shared MPRIS2 D-Bus state.
    pub fn update_mpris(&self) {
        if let Some(state_arc) = &self.mpris_state {
            let state_clone = state_arc.clone();
            let pb = self.playback_state;
            let meta = self.track_metadata.clone();
            let st_name = self
                .current_station
                .as_ref()
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "Reso Radio".to_string());
            let vol = self.hardware_volume * 100.0;

            tokio::spawn(async move {
                let mut lock = state_clone.lock().await;
                lock.playback_state = pb;
                lock.track_metadata = meta;
                lock.station_name = st_name;
                lock.volume = vol;
            });
        }
    }

    /// Periodic UI tick (invoked every 50ms).
    pub async fn on_tick(&mut self) {
        self.tick_count = self.tick_count.wrapping_add(1);

        // Check active toast notifications
        self.check_toast_expiry();

        // Advance visualizer state
        self.visualizer_state.show_visualizer = self.config_manager.config.ui.show_visualizer;
        self.visualizer_state.tick();

        // Check scrobble threshold
        self.scrobbler.tick().await;

        // Dynamic theme updates: default every 5 seconds (configurable via ui.theme_reload_interval_secs)
        let theme_interval_ticks =
            (self.config_manager.config.ui.theme_reload_interval_secs.max(1)) * 20;
        if self.tick_count % theme_interval_ticks == 0 {
            self.check_theme_file_update();
        }

        // Decrement sleep timer every second (every 20 ticks of 50ms)
        if self.tick_count % 20 == 0 {
            if let Some(secs) = self.sleep_timer_secs_remaining {
                if secs > 1 {
                    self.sleep_timer_secs_remaining = Some(secs - 1);
                } else {
                    self.sleep_timer_secs_remaining = None;
                    let _ = self.action_tx.send(AppAction::Stop).await;
                }
            }

            // Periodic diagnostics refresh
            if self.playback_state == PlaybackState::Playing && self.tick_count % 100 == 0 {
                self.trigger_pipeline_diagnostics();
            }

            // If playing a Radio Paradise station, poll rich metadata off-thread
            if !self.rp_poll_in_flight {
                if let Some(st) = &self.current_station {
                    if let Some(chan) = st.rp_channel {
                        self.rp_poll_in_flight = true;
                        let api_client = self.api_client.clone();
                        let action_tx = self.action_tx.clone();
                        let current_title = self.track_metadata.title.clone();
                        tokio::spawn(async move {
                            if let Ok(Some(rp_meta)) =
                                api_client.fetch_radioparadise_now_playing(chan).await
                            {
                                if current_title.as_deref() != rp_meta.title.as_deref() {
                                    let cover_bytes = if let Some(cover_url) = &rp_meta.cover_url {
                                        api_client.download_image_bytes(cover_url).await.ok()
                                    } else {
                                        None
                                    };
                                    let _ = action_tx
                                        .send(AppAction::RpMetadataReady {
                                            metadata: Box::new(rp_meta),
                                            cover_bytes,
                                        })
                                        .await;
                                    return;
                                }
                            }
                            let _ = action_tx.send(AppAction::RpPollFinished).await;
                        });
                    }
                }
            }
        }
    }
}
