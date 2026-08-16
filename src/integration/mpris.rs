//! MPRIS2 D-Bus service exposing `reso` to desktop media controllers, Waybar, and keybindings.

use crate::error::Result;
use crate::events::{AppAction, PlaybackState, TrackMetadata};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use zbus::interface;
use zbus::zvariant::Value;

/// Internal state shared with the D-Bus interfaces.
pub struct MprisState {
    pub playback_state: PlaybackState,
    pub track_metadata: TrackMetadata,
    pub station_name: String,
    pub volume: f64,
    pub action_tx: mpsc::Sender<AppAction>,
}

/// D-Bus interface for root `org.mpris.MediaPlayer2`.
pub struct ResoMprisRoot {
    state: Arc<Mutex<MprisState>>,
}

impl ResoMprisRoot {
    pub fn new(state: Arc<Mutex<MprisState>>) -> Self {
        Self { state }
    }
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl ResoMprisRoot {
    async fn raise(&self) {}

    async fn quit(&self) {
        let lock = self.state.lock().await;
        let _ = lock.action_tx.send(AppAction::Quit).await;
    }

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn identity(&self) -> &str {
        "Reso Audiophile Radio"
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        vec!["http".to_string(), "https".to_string()]
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        vec![
            "audio/flac".to_string(),
            "audio/x-flac".to_string(),
            "audio/aac".to_string(),
            "audio/mpeg".to_string(),
        ]
    }
}

/// D-Bus interface for player `org.mpris.MediaPlayer2.Player`.
pub struct ResoMprisPlayer {
    state: Arc<Mutex<MprisState>>,
}

impl ResoMprisPlayer {
    pub fn new(state: Arc<Mutex<MprisState>>) -> Self {
        Self { state }
    }
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl ResoMprisPlayer {
    async fn next(&self) {
        let lock = self.state.lock().await;
        let _ = lock.action_tx.send(AppAction::NextStation).await;
    }

    async fn previous(&self) {
        let lock = self.state.lock().await;
        let _ = lock.action_tx.send(AppAction::PrevStation).await;
    }

    async fn pause(&self) {
        let lock = self.state.lock().await;
        let _ = lock.action_tx.send(AppAction::Pause).await;
    }

    async fn play_pause(&self) {
        let lock = self.state.lock().await;
        let _ = lock.action_tx.send(AppAction::TogglePause).await;
    }

    async fn stop(&self) {
        let lock = self.state.lock().await;
        let _ = lock.action_tx.send(AppAction::Stop).await;
    }

    async fn play(&self) {
        let lock = self.state.lock().await;
        let _ = lock.action_tx.send(AppAction::Resume).await;
    }

    #[zbus(property)]
    async fn playback_status(&self) -> String {
        let lock = self.state.lock().await;
        match lock.playback_state {
            PlaybackState::Playing => "Playing".to_string(),
            PlaybackState::Paused => "Paused".to_string(),
            PlaybackState::Stopped | PlaybackState::Buffering => "Stopped".to_string(),
        }
    }

    #[zbus(property)]
    fn loop_status(&self) -> &str {
        "None"
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn shuffle(&self) -> bool {
        false
    }

    #[zbus(property)]
    async fn metadata(&self) -> HashMap<String, Value<'static>> {
        let lock = self.state.lock().await;
        let mut map = HashMap::new();

        map.insert(
            "mpris:trackid".to_string(),
            Value::from("/org/mpris/MediaPlayer2/CurrentTrack"),
        );

        let title = lock
            .track_metadata
            .title
            .clone()
            .unwrap_or_else(|| lock.track_metadata.raw_title.clone().unwrap_or_else(|| lock.station_name.clone()));
        map.insert("xesam:title".to_string(), Value::from(title));

        if let Some(artist) = &lock.track_metadata.artist {
            map.insert("xesam:artist".to_string(), Value::from(vec![artist.clone()]));
        } else {
            map.insert("xesam:artist".to_string(), Value::from(vec![lock.station_name.clone()]));
        }

        if let Some(album) = &lock.track_metadata.album {
            map.insert("xesam:album".to_string(), Value::from(album.clone()));
        } else {
            map.insert("xesam:album".to_string(), Value::from(lock.station_name.clone()));
        }

        if let Some(art_url) = &lock.track_metadata.cover_url {
            map.insert("mpris:artUrl".to_string(), Value::from(art_url.clone()));
        }

        map
    }

    #[zbus(property)]
    async fn volume(&self) -> f64 {
        let lock = self.state.lock().await;
        lock.volume / 100.0
    }

    #[zbus(property)]
    async fn set_volume(&self, vol: f64) {
        let lock = self.state.lock().await;
        let _ = lock.action_tx.send(AppAction::SetVolume(vol * 100.0)).await;
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }
}

/// Spawns the MPRIS2 D-Bus daemon on the user's session bus.
pub async fn start_mpris_server(
    action_tx: mpsc::Sender<AppAction>,
) -> Result<Arc<Mutex<MprisState>>> {
    let state = Arc::new(Mutex::new(MprisState {
        playback_state: PlaybackState::Stopped,
        track_metadata: TrackMetadata::default(),
        station_name: "Reso Radio".to_string(),
        volume: 100.0,
        action_tx,
    }));

    let root_iface = ResoMprisRoot::new(state.clone());
    let player_iface = ResoMprisPlayer::new(state.clone());

    tokio::spawn(async move {
        if let Ok(connection) = zbus::connection::Builder::session() {
            if let Ok(builder) = connection.name("org.mpris.MediaPlayer2.reso") {
                if let Ok(builder) = builder.serve_at("/org/mpris/MediaPlayer2", root_iface) {
                    if let Ok(builder) = builder.serve_at("/org/mpris/MediaPlayer2", player_iface) {
                        if let Ok(conn) = builder.build().await {
                            // Keep connection alive
                            let _ = std::future::pending::<()>().await;
                            drop(conn);
                        }
                    }
                }
            }
        }
    });

    Ok(state)
}
