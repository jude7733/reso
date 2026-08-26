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
    pub visualizer_state: VisualizerState,
    pub cover_art_manager: CoverArtManager,
    pub tick_count: u64,
    pub should_quit: bool,

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

        let mut app = Self {
            active_tab: ActiveTab::Stations,
            config_manager,
            catalog,
            station_sort_mode: StationSortMode::Default,
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
            visualizer_state: VisualizerState::default(),
            cover_art_manager: CoverArtManager::new(),
            tick_count: 0,
            should_quit: false,
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

    /// Loads and refreshes liked tracks from `config_manager`.
    pub fn load_liked_history(&mut self) {
        self.liked_tracks = self.config_manager.liked_catalog.tracks.clone();
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

    /// Returns the filtered and sorted list of stations matching `filter_query` and `station_sort_mode`.
    pub fn filtered_stations(&self) -> Vec<&Station> {
        let q = self.filter_query.trim().to_lowercase();
        let mut list: Vec<&Station> = if q.is_empty() {
            self.catalog.stations.iter().collect()
        } else {
            self.catalog
                .stations
                .iter()
                .filter(|s| {
                    s.name.to_lowercase().contains(&q)
                        || s.codec.to_lowercase().contains(&q)
                        || s.tags.iter().any(|t| t.to_lowercase().contains(&q))
                })
                .collect()
        };

        match self.station_sort_mode {
            StationSortMode::Default => {}
            StationSortMode::NameAsc => {
                list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            }
            StationSortMode::NameDesc => {
                list.sort_by(|a, b| b.name.to_lowercase().cmp(&a.name.to_lowercase()));
            }
            StationSortMode::QualityDesc => {
                list.sort_by(|a, b| {
                    b.sample_rate
                        .cmp(&a.sample_rate)
                        .then_with(|| b.bit_depth.cmp(&a.bit_depth))
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });
            }
            StationSortMode::FavoritesFirst => {
                list.sort_by(|a, b| {
                    b.favorite
                        .cmp(&a.favorite)
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });
            }
            StationSortMode::Codec => {
                list.sort_by(|a, b| {
                    let score = |c: &str| match c.to_uppercase().as_str() {
                        "FLAC" | "ALAC" => 1,
                        "OGG" | "OPUS" => 2,
                        "AAC" => 3,
                        "MP3" => 4,
                        _ => 5,
                    };
                    score(&a.codec)
                        .cmp(&score(&b.codec))
                        .then_with(|| b.sample_rate.cmp(&a.sample_rate))
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });
            }
        }

        list
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

        // 3. Global Navigation & Actions
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Char('?') => {
                self.modal_state = ModalState::Help;
            }
            KeyCode::Char('1') => self.active_tab = ActiveTab::Stations,
            KeyCode::Char('2') => {
                self.active_tab = ActiveTab::Search;
                self.search_input_active = true;
            }
            KeyCode::Char('3') => {
                self.active_tab = ActiveTab::Inspector;
                self.refresh_pipeline_diagnostics();
            }
            KeyCode::Char('4') => {
                self.active_tab = ActiveTab::Liked;
                self.load_liked_history();
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

            // Edit Station ('e' or 'c' on Stations tab) / Copy track info on Liked tab
            KeyCode::Char('e') | KeyCode::Char('c') => match self.active_tab {
                ActiveTab::Stations => {
                    let filtered = self.filtered_stations();
                    if let Some(st) = filtered.get(self.station_list_index) {
                        self.modal_state =
                            ModalState::EditStation(EditStationState::from_station(st));
                    }
                }
                ActiveTab::Liked => {
                    if let Some(track) = self.liked_tracks.get(self.liked_list_index) {
                        let _ = copy_to_clipboard(&format!("{} - {}", track.artist, track.title));
                    }
                }
                _ => {
                    let _ = self.action_tx.send(AppAction::CopyTrackToClipboard).await;
                }
            },

            // Cycle Sort Mode ('S' / Shift+S)
            KeyCode::Char('S') => {
                self.station_sort_mode = self.station_sort_mode.next();
                self.station_list_index = 0;
            }

            // Tab-specific Actions: Add Station ('a')
            KeyCode::Char('a') => match self.active_tab {
                ActiveTab::Stations => {
                    self.modal_state = ModalState::AddStation(AddStationState::new());
                }
                ActiveTab::Search => {
                    if let Some(st) = self.search_results.get(self.search_results_index) {
                        let _ = self.config_manager.add_station(st.clone());
                        self.catalog = self.config_manager.catalog.clone();
                    }
                }
                _ => {}
            },

            // Delete Station or Liked Track (vim 'd' or 'x')
            KeyCode::Char('d') | KeyCode::Char('x') => match self.active_tab {
                ActiveTab::Stations => {
                    let filtered = self.filtered_stations();
                    if let Some(st) = filtered.get(self.station_list_index) {
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
                    let filtered = self.filtered_stations();
                    if let Some(st) = filtered.get(self.station_list_index) {
                        let _ = copy_to_clipboard(&st.url);
                    }
                }
                ActiveTab::Liked => {
                    if let Some(track) = self.liked_tracks.get(self.liked_list_index) {
                        let _ = copy_to_clipboard(&format!("{} - {}", track.artist, track.title));
                    }
                }
                _ => {}
            },

            // Favorite
            KeyCode::Char('f') => {
                let filtered = self.filtered_stations();
                if let Some(st) = filtered.get(self.station_list_index) {
                    let id = st.id.clone();
                    self.config_manager.toggle_favorite(&id);
                    self.catalog = self.config_manager.catalog.clone();
                }
            }

            // Play selected item ('Enter' or 'o')
            KeyCode::Enter | KeyCode::Char('o') => match self.active_tab {
                ActiveTab::Stations => {
                    let filtered = self.filtered_stations();
                    if let Some(st) = filtered.get(self.station_list_index) {
                        let station_clone = (*st).clone();
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
                        if let Some(st) = self.config_manager.find_station(&track.station) {
                            let station_clone = st.clone();
                            let _ = self.action_tx.send(AppAction::Play(station_clone)).await;
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
                    let len = self.filtered_stations().len();
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
                    let len = self.filtered_stations().len();
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
                    let len = self.filtered_stations().len();
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
                    let len = self.filtered_stations().len();
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
            ModalState::Help => {
                if key.code == KeyCode::Esc
                    || key.code == KeyCode::Char('?')
                    || key.code == KeyCode::Char('q')
                {
                    self.modal_state = ModalState::None;
                }
            }

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

    /// Handles MPV IPC events pushed asynchronously.
    pub async fn handle_mpv_event(&mut self, event: MpvEvent) {
        match event {
            MpvEvent::State(s) => {
                self.playback_state = s;
                self.visualizer_state.is_playing = s == PlaybackState::Playing;
                if s != PlaybackState::Playing {
                    self.visualizer_state.stop_capture();
                }
                self.update_mpris();
            }
            MpvEvent::Metadata(m) => {
                self.track_metadata = m.clone();
                self.scrobbler.on_track_change(m).await;
                self.update_mpris();
            }
            MpvEvent::AudioParams(p) => {
                self.stream_params = p;
                self.refresh_pipeline_diagnostics();
            }
            MpvEvent::Volume(_vol) => {
                self.refresh_pipeline_diagnostics();
            }
            MpvEvent::RecordingState { is_recording, path } => {
                self.is_recording = is_recording;
                self.active_recording_path = path;
            }
            MpvEvent::Error(_err) => {}
        }
    }

    /// Re-evaluates the 3-stage audiophile pipeline diagnostics.
    pub fn refresh_pipeline_diagnostics(&mut self) {
        self.pipeline_status = PipelineStatus::evaluate(&self.stream_params, 100.0);
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

        // Advance visualizer state
        self.visualizer_state.tick();

        // Check scrobble threshold
        self.scrobbler.tick().await;

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
                self.refresh_pipeline_diagnostics();
            }

            // If playing a Radio Paradise station, poll rich metadata
            if let Some(st) = &self.current_station {
                if let Some(chan) = st.rp_channel {
                    if let Ok(Some(rp_meta)) =
                        self.api_client.fetch_radioparadise_now_playing(chan).await
                    {
                        if self.track_metadata.title != rp_meta.title {
                            if let Some(cover_url) = &rp_meta.cover_url {
                                if let Ok(bytes) =
                                    self.api_client.download_image_bytes(cover_url).await
                                {
                                    self.cover_art_manager.load_image(cover_url, &bytes);
                                }
                            }
                            self.track_metadata = rp_meta;
                            self.update_mpris();
                        }
                    }
                }
            }
        }
    }
}
