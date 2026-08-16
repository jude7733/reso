//! Application state machine, keybindings, and async event loop for the TUI.

use crate::config::{ConfigManager, Station, StationsCatalog};
use crate::error::Result;
use crate::events::{AppAction, AudioStreamParams, MpvEvent, PlaybackState, TrackMetadata};
use crate::inspector::status::PipelineStatus;
use crate::integration::api::ApiClient;
use crate::integration::mpris::MprisState;
use crate::integration::scrobbler::ScrobbleTracker;
use crate::mpv::client::MpvClient;
use crate::mpv::record::StreamRecorder;
use crate::tui::cover::CoverArtManager;
use crate::tui::visualizer::VisualizerState;
use crossterm::event::{KeyCode, KeyEvent};
use std::fs;
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

/// Central state machine for the TUI.
pub struct App {
    pub active_tab: ActiveTab,
    pub config_manager: ConfigManager,
    pub catalog: StationsCatalog,
    pub current_station: Option<Station>,
    pub station_list_index: usize,
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
    pub search_input_active: bool,
    pub liked_tracks_history: Vec<String>,
    pub show_help_modal: bool,
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
        let api_client = ApiClient::new();
        let scrobbler = ScrobbleTracker::new(
            config_manager.config.lastfm.clone(),
            config_manager.config.listenbrainz.clone(),
        );

        let mut app = Self {
            active_tab: ActiveTab::Stations,
            config_manager,
            catalog,
            current_station: None,
            station_list_index: 0,
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
            search_input_active: false,
            liked_tracks_history: Vec::new(),
            show_help_modal: false,
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
        app.load_liked_history();
        app
    }

    /// Loads the liked tracks history from `~/.config/reso/liked_tracks.md`.
    pub fn load_liked_history(&mut self) {
        let path = crate::util::config_dir().join("liked_tracks.md");
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
                self.liked_tracks_history = content
                    .lines()
                    .rev()
                    .take(100)
                    .map(|s| s.to_string())
                    .collect();
            }
        }
    }

    /// Queries the active hardware volume and mute status from PipeWire via `wpctl`.
    pub fn refresh_hardware_volume(&mut self) {
        if let Ok(output) = Command::new("wpctl").arg("get-volume").arg("@DEFAULT_AUDIO_SINK@").output() {
            if output.status.success() {
                let s = String::from_utf8_lossy(&output.stdout);
                // Example: "Volume: 0.25 [MUTED]" or "Volume: 0.50"
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

    /// Returns the filtered list of stations matching `filter_query`.
    pub fn filtered_stations(&self) -> Vec<&Station> {
        let q = self.filter_query.trim().to_lowercase();
        if q.is_empty() {
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
        }
    }

    /// Handles keyboard input from crossterm.
    pub async fn handle_key_event(&mut self, key: KeyEvent) -> Result<()> {
        // Help modal toggle
        if self.show_help_modal {
            if key.code == KeyCode::Esc || key.code == KeyCode::Char('?') || key.code == KeyCode::Char('q') {
                self.show_help_modal = false;
            }
            return Ok(());
        }

        // Search Input mode
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

        // Global Navigation & Actions
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Char('?') => {
                self.show_help_modal = true;
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

            // Station Actions
            KeyCode::Enter => match self.active_tab {
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
                _ => {}
            },
            KeyCode::Char('f') => {
                let filtered = self.filtered_stations();
                if let Some(st) = filtered.get(self.station_list_index) {
                    let id = st.id.clone();
                    self.config_manager.toggle_favorite(&id);
                    self.catalog = self.config_manager.catalog.clone();
                }
            }
            KeyCode::Char('r') => {
                let _ = self.action_tx.send(AppAction::ToggleRecord).await;
            }
            KeyCode::Char('l') => {
                let _ = self.action_tx.send(AppAction::BookmarkTrack).await;
            }
            KeyCode::Char('c') => {
                let _ = self.action_tx.send(AppAction::CopyTrackToClipboard).await;
            }
            KeyCode::Char('a') if self.active_tab == ActiveTab::Search => {
                if let Some(st) = self.search_results.get(self.search_results_index) {
                    let _ = self.config_manager.add_station(st.clone());
                    self.catalog = self.config_manager.catalog.clone();
                }
            }
            KeyCode::Char('/') => {
                if self.active_tab == ActiveTab::Search {
                    self.search_input_active = true;
                } else {
                    self.active_tab = ActiveTab::Search;
                    self.search_input_active = true;
                }
            }

            // Up / Down navigation
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
                _ => {}
            },
            _ => {}
        }

        Ok(())
    }

    /// Handles MPV IPC events pushed asynchronously.
    pub async fn handle_mpv_event(&mut self, event: MpvEvent) {
        match event {
            MpvEvent::State(s) => {
                self.playback_state = s;
                self.visualizer_state.is_playing = s == PlaybackState::Playing;
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
            MpvEvent::Error(_err) => {
                // Log or surface error gracefully
            }
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
                    if let Ok(Some(rp_meta)) = self.api_client.fetch_radioparadise_now_playing(chan).await {
                        if self.track_metadata.title != rp_meta.title {
                            if let Some(cover_url) = &rp_meta.cover_url {
                                if let Ok(bytes) = self.api_client.download_image_bytes(cover_url).await {
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
