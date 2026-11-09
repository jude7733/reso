//! TUI module facade, terminal initialization, event loop execution, and clean teardown.

pub mod app;
pub mod cover;
pub mod spectrum;
pub mod theme;
pub mod ui;
pub mod visualizer;

pub use app::{ActiveTab, App};
pub use cover::CoverArtManager;
pub use spectrum::{AudioCapture, SpectrumAnalyzer};
pub use theme::Theme;
pub use visualizer::{VisualizerState, VisualizerWidget};

use crate::config::ConfigManager;
use crate::error::Result;
use crate::events::AppAction;
use crate::integration::clipboard::copy_to_clipboard;
use crate::integration::mpris::start_mpris_server;
use crate::mpv::record::StreamRecorder;
use crossterm::event::{Event, EventStream};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures_util::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::stdout;
use std::time::Duration;
use tokio::sync::mpsc;

/// Runs the interactive TUI application.
pub async fn run_tui(config_manager: ConfigManager) -> Result<()> {
    let _session_lock = crate::session::SessionLock::acquire(&config_manager.config.socket_path)?;
    // 1. Establish the owned playback process and IPC session.
    let mut playback_session =
        crate::session::PlaybackSession::connect(&config_manager.config).await?;
    let mpv_client = playback_session.client();
    let mut mpv_rx = playback_session.subscribe();
    let initial_snapshot = playback_session.snapshot().await;

    // 3. Initialize Recorder & MPRIS
    let recorder =
        StreamRecorder::new(mpv_client.clone(), config_manager.config.record_dir.clone());
    let (action_tx, mut action_rx) = mpsc::channel::<AppAction>(64);
    let mpris_state = match start_mpris_server(action_tx.clone()).await {
        Ok(state) => Some(state),
        Err(error) => {
            eprintln!("MPRIS unavailable: {error}");
            None
        }
    };

    // 4. Setup Terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(
        config_manager,
        mpv_client.clone(),
        recorder.clone(),
        action_tx.clone(),
        mpris_state,
    );
    app.current_station = initial_snapshot.current_station;
    app.playback_state = initial_snapshot.playback_state;
    app.track_metadata = initial_snapshot.metadata;
    app.stream_params = initial_snapshot.audio_params;
    app.is_recording = initial_snapshot.is_recording;
    app.active_recording_path = initial_snapshot.recording_path;
    app.visualizer_state.is_playing = app.playback_state == crate::events::PlaybackState::Playing;
    app.update_mpris();

    let mut event_stream = EventStream::new();
    let mut tick_interval = tokio::time::interval(Duration::from_millis(50));

    // 5. Main TUI Event Loop
    while !app.should_quit {
        // Render Frame
        terminal.draw(|f| ui::render(f, &mut app))?;

        tokio::select! {
            _ = tick_interval.tick() => {
                app.on_tick().await;
            }

            Some(Ok(event)) = event_stream.next() => {
                if let Event::Key(key) = event {
                    app.handle_key_event(key).await?;
                }
            }

            Ok(mpv_ev) = mpv_rx.recv() => {
                app.handle_mpv_event(mpv_ev).await;
            }

            Some(action) = action_rx.recv() => {
                match action {
                    AppAction::Play(station) => {
                        let _ = playback_session.play_station(&station).await;
                        let name = station.name.clone();
                        app.current_station = Some(station);
                        app.track_started_at = Some(std::time::Instant::now());
                        app.show_toast(format!("▶ Tuning in: {}", name), crate::tui::app::ToastLevel::Info);
                        app.update_mpris();
                    }
                    AppAction::Pause => {
                        let _ = playback_session.pause().await;
                    }
                    AppAction::Resume => {
                        let _ = playback_session.resume().await;
                    }
                    AppAction::TogglePause => {
                        let _ = playback_session.toggle_pause().await;
                    }
                    AppAction::Stop => {
                        let _ = playback_session.stop().await;
                        app.current_station = None;
                        app.track_started_at = None;
                        app.visualizer_state.stop_capture();
                        app.update_mpris();
                    }
                    AppAction::NextStation => {
                        let total = app.catalog.stations.len();
                        if total > 0 {
                            app.station_list_index = (app.station_list_index + 1) % total;
                            let st = app.catalog.stations[app.station_list_index].clone();
                            let _ = playback_session.play_station(&st).await;
                            app.track_started_at = Some(std::time::Instant::now());
                            app.show_toast(format!("▶ {}", st.name), crate::tui::app::ToastLevel::Info);
                            app.current_station = Some(st);
                            app.update_mpris();
                        }
                    }
                    AppAction::PrevStation => {
                        let total = app.catalog.stations.len();
                        if total > 0 {
                            if app.station_list_index == 0 {
                                app.station_list_index = total - 1;
                            } else {
                                app.station_list_index -= 1;
                            }
                            let st = app.catalog.stations[app.station_list_index].clone();
                            let _ = playback_session.play_station(&st).await;
                            app.track_started_at = Some(std::time::Instant::now());
                            app.show_toast(format!("▶ {}", st.name), crate::tui::app::ToastLevel::Info);
                            app.current_station = Some(st);
                            app.update_mpris();
                        }
                    }
                    AppAction::ToggleRecord => {
                        let st_name = app.current_station.as_ref().map(|s| s.name.as_str()).unwrap_or("Stream");
                        let track_str = app.track_metadata.display_name();
                        if let Ok(is_rec) = app.recorder.toggle(st_name, Some(&track_str)).await {
                            if is_rec.is_some() {
                                app.show_toast("🔴 Lossless recording started", crate::tui::app::ToastLevel::Warning);
                            } else {
                                app.show_toast("■ Recording saved to ~/Music/Radio/", crate::tui::app::ToastLevel::Success);
                            }
                        }
                    }
                    AppAction::StartRecord(p) => {
                        let st_name = app.current_station.as_ref().map(|s| s.name.as_str()).unwrap_or("Stream");
                        let track_str = app.track_metadata.display_name();
                        let _ = app.recorder.start_recording(st_name, Some(&track_str), p).await;
                        app.show_toast("🔴 Recording started", crate::tui::app::ToastLevel::Warning);
                    }
                    AppAction::StopRecord => {
                        let _ = app.recorder.stop_recording().await;
                        app.show_toast("■ Recording saved to ~/Music/Radio/", crate::tui::app::ToastLevel::Success);
                    }
                    AppAction::SetSleepTimer(dur) => {
                        app.sleep_timer_secs_remaining = dur.map(|d| d.as_secs());
                    }
                    AppAction::VolumeUp => {
                        app.adjust_hardware_volume(0.05);
                    }
                    AppAction::VolumeDown => {
                        app.adjust_hardware_volume(-0.05);
                    }
                    AppAction::SetVolume(vol) => {
                        let v = (vol / 100.0).clamp(0.0, 1.5);
                        let _ = std::process::Command::new("wpctl")
                            .arg("set-volume")
                            .arg("@DEFAULT_AUDIO_SINK@")
                            .arg(format!("{:.2}", v))
                            .output();
                        app.refresh_hardware_volume();
                        app.update_mpris();
                    }
                    AppAction::ToggleMute => {
                        app.toggle_hardware_mute();
                    }
                    AppAction::BookmarkTrack => {
                        let st_name = app.current_station.as_ref().map(|s| s.name.clone()).unwrap_or_else(|| "Reso".to_string());
                        let artist = app.track_metadata.artist.clone().unwrap_or_else(|| "Unknown Artist".to_string());
                        let title = app.track_metadata.title.clone().unwrap_or_else(|| {
                            app.track_metadata.raw_title.clone().unwrap_or_else(|| "Unknown Title".to_string())
                        });
                        let album = app.track_metadata.album.clone();
                        let codec = if app.stream_params.codec.is_empty() { None } else { Some(app.stream_params.codec.clone()) };
                        let rate = if app.stream_params.sample_rate == 0 { None } else { Some(app.stream_params.sample_rate) };
                        let bit_depth = app.stream_params.bit_depth;

                        match app.config_manager.bookmark_track(&st_name, &artist, &title, album.as_deref(), codec.as_deref(), rate, bit_depth) {
                            Ok(_) => {
                                app.load_liked_history();
                                if !app.config_manager.config.lastfm.enabled {
                                    app.show_toast(format!("★ Liked locally: {} - {}", artist, title), crate::tui::app::ToastLevel::Success);
                                } else { match (app.track_metadata.artist.clone(), app.track_metadata.title.clone()) {
                                    (Some(lastfm_artist), Some(lastfm_title)) => {
                                        let scrobbler = app.scrobbler.clone();
                                        let result_tx = app.action_tx.clone();
                                        tokio::spawn(async move {
                                            let result = scrobbler.love_track(&lastfm_artist, &lastfm_title).await.map_err(|e| e.to_string());
                                            let _ = result_tx.send(AppAction::LastFmLoveComplete {
                                                artist: lastfm_artist,
                                                title: lastfm_title,
                                                result,
                                            }).await;
                                        });
                                        app.show_toast(format!("★ Saved locally; sending love to Last.fm: {} - {}", artist, title), crate::tui::app::ToastLevel::Info);
                                    }
                                    _ => app.show_toast(format!("★ Liked locally: {} - {} (track metadata unavailable for Last.fm)", artist, title), crate::tui::app::ToastLevel::Info),
                                }}
                            }
                            Err(e) => {
                                app.show_toast(format!("✗ Like failed: {}", e), crate::tui::app::ToastLevel::Error);
                            }
                        }
                    }
                    AppAction::LastFmLoveComplete { artist, title, result } => match result {
                        Ok(()) => app.show_toast(format!("★ Loved on Last.fm: {} - {}", artist, title), crate::tui::app::ToastLevel::Success),
                        Err(e) => app.show_toast(format!("★ Saved locally; Last.fm love failed: {}", e), crate::tui::app::ToastLevel::Error),
                    },
                    AppAction::CopyTrackToClipboard => {
                        let title_str = app.track_metadata.display_name();
                        match copy_to_clipboard(&title_str) {
                            Ok(_) => app.show_toast(format!("✓ Copied: {}", title_str), crate::tui::app::ToastLevel::Success),
                            Err(e) => app.show_toast(format!("✗ Copy failed: {}", e), crate::tui::app::ToastLevel::Error),
                        }
                    }
                    AppAction::Search(query) => {
                        app.search_request_id = app.search_request_id.wrapping_add(1);
                        let request_id = app.search_request_id;
                        app.search_in_flight = true;
                        let api_client = app.api_client.clone();
                        let action_tx = app.action_tx.clone();
                        tokio::spawn(async move {
                            let result = api_client
                                .search_radio_browser(&query, Some("FLAC"), None, 30)
                                .await
                                .map_err(|error| error.to_string());
                            let _ = action_tx
                                .send(AppAction::SearchResultsReady { request_id, result })
                                .await;
                        });
                    }
                    AppAction::SearchResultsReady { request_id, result } => {
                        if request_id == app.search_request_id {
                            app.search_in_flight = false;
                            match result {
                                Ok(results) => {
                                    let count = results.len();
                                    app.search_results = results;
                                    app.search_results_index = 0;
                                    app.show_toast(
                                        if count == 0 {
                                            "No matching stations found".to_string()
                                        } else {
                                            format!("Found {count} stations")
                                        },
                                        crate::tui::app::ToastLevel::Info,
                                    );
                                }
                                Err(error) => app.show_toast(
                                    format!("Station search failed: {error}"),
                                    crate::tui::app::ToastLevel::Error,
                                ),
                            }
                        }
                    }
                    AppAction::ProbeStream(url) => {
                        let _ = playback_session.load_url(&url).await;
                        let tx = app.action_tx.clone();
                        tokio::spawn(async move {
                            let res = crate::integration::prober::StreamProber::probe_endpoint(&url).await;
                            let _ = tx.send(AppAction::ProbeComplete(res)).await;
                        });
                    }
                    AppAction::ProbeComplete(res) => {
                        app.handle_probe_result(res);
                    }
                    AppAction::PlayPreview(url) => {
                        let _ = playback_session.load_url(&url).await;
                    }
                    AppAction::RestorePreviousPlayback => {
                        if let Some(ref st) = app.current_station {
                            let _ = playback_session.play_station(st).await;
                        } else {
                            let _ = playback_session.stop().await;
                        }
                    }
                    AppAction::UpdateStation(st) => {
                        let _ = app.config_manager.add_station(st);
                        app.catalog = app.config_manager.catalog.clone();
                        app.invalidate_station_cache();
                    }
                    AppAction::DiagnosticsReady(status) => {
                        app.pipeline_status = *status;
                        app.diagnostics_in_flight = false;
                    }
                    AppAction::CoverArtReady { url, bytes } => {
                        app.cover_art_manager.load_image(&url, &bytes);
                        if let Some(art_uri) = crate::tui::cover::cache_artwork(&bytes) {
                            app.track_metadata.cover_url = Some(art_uri);
                        }
                        app.update_mpris();
                    }
                    AppAction::RpMetadataReady { metadata, cover_bytes } => {
                        app.rp_poll_in_flight = false;
                        if let (Some(url), Some(bytes)) = (&metadata.cover_url, cover_bytes) {
                            app.cover_art_manager.load_image(url, &bytes);
                            app.track_metadata = *metadata;
                            if let Some(art_uri) = crate::tui::cover::cache_artwork(&bytes) {
                                app.track_metadata.cover_url = Some(art_uri);
                            }
                        } else {
                            app.track_metadata = *metadata;
                        }
                        app.update_mpris();
                    }
                    AppAction::RpPollFinished => {
                        app.rp_poll_in_flight = false;
                    }
                    AppAction::NotificationFailed(message) => {
                        app.show_toast(message, crate::tui::app::ToastLevel::Warning);
                    }
                    AppAction::Quit => {
                        app.should_quit = true;
                    }
                }
            }
        }
    }

    // 6. Clean Terminal Teardown
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
