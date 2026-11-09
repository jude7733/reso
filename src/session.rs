use crate::config::AppConfig;
use crate::config::Station;
use crate::error::{ResoError, Result};
use crate::events::{AudioStreamParams, MpvEvent, PlaybackState, TrackMetadata};
use crate::mpv::client::MpvClient;
use crate::mpv::process::MpvProcessManager;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use tokio::sync::{broadcast, RwLock};

/// Owns the MPV process and IPC client for one Reso playback session.
///
/// Interactive entry points keep this value alive for their whole lifetime. Short-lived
/// CLI commands can disown the child process after connecting so playback continues.
#[derive(Debug, Clone)]
pub struct PlaybackSnapshot {
    pub current_station: Option<Station>,
    pub playback_state: PlaybackState,
    pub metadata: TrackMetadata,
    pub audio_params: AudioStreamParams,
    pub volume: f64,
    pub is_recording: bool,
    pub recording_path: Option<std::path::PathBuf>,
    pub last_error: Option<String>,
}

impl Default for PlaybackSnapshot {
    fn default() -> Self {
        Self {
            current_station: None,
            playback_state: PlaybackState::Stopped,
            metadata: TrackMetadata::default(),
            audio_params: AudioStreamParams::default(),
            volume: 100.0,
            is_recording: false,
            recording_path: None,
            last_error: None,
        }
    }
}

pub struct PlaybackSession {
    process: MpvProcessManager,
    client: MpvClient,
    initial_events: Option<broadcast::Receiver<MpvEvent>>,
    event_tx: broadcast::Sender<MpvEvent>,
    snapshot: std::sync::Arc<RwLock<PlaybackSnapshot>>,
}

impl PlaybackSession {
    pub async fn connect(config: &AppConfig) -> Result<Self> {
        let mut process = MpvProcessManager::new(config.socket_path.clone())
            .with_reconnect_policy(config.reconnect_streams, config.reconnect_max_delay_secs);
        process.ensure_running().await?;
        let (client, mut mpv_events) = MpvClient::connect(config.socket_path.clone()).await?;
        let (event_tx, _) = broadcast::channel(128);
        let initial_events = event_tx.subscribe();
        let snapshot = std::sync::Arc::new(RwLock::new(PlaybackSnapshot::default()));
        let event_snapshot = snapshot.clone();
        let event_tx_clone = event_tx.clone();
        tokio::spawn(async move {
            loop {
                let event = match mpv_events.recv().await {
                    Ok(event) => event,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                {
                    let mut state = event_snapshot.write().await;
                    match &event {
                        MpvEvent::State(playback_state) => state.playback_state = *playback_state,
                        MpvEvent::Metadata(metadata) => state.metadata = metadata.clone(),
                        MpvEvent::AudioParams(params) => state.audio_params = params.clone(),
                        MpvEvent::Volume(volume) => state.volume = *volume,
                        MpvEvent::RecordingState { is_recording, path } => {
                            state.is_recording = *is_recording;
                            state.recording_path = path.clone();
                        }
                        MpvEvent::Error(error) => state.last_error = Some(error.clone()),
                    }
                }
                let _ = event_tx_clone.send(event);
            }
        });
        Ok(Self {
            process,
            client,
            initial_events: Some(initial_events),
            event_tx,
            snapshot,
        })
    }

    pub fn client(&self) -> MpvClient {
        self.client.clone()
    }

    pub fn subscribe(&mut self) -> broadcast::Receiver<MpvEvent> {
        self.initial_events
            .take()
            .unwrap_or_else(|| self.event_tx.subscribe())
    }

    pub async fn snapshot(&self) -> PlaybackSnapshot {
        self.snapshot.read().await.clone()
    }

    pub async fn play_station(&self, station: &Station) -> Result<()> {
        self.client.load_file(&station.url).await?;
        {
            let mut snapshot = self.snapshot.write().await;
            snapshot.current_station = Some(station.clone());
            snapshot.metadata = TrackMetadata::default();
            snapshot.last_error = None;
        }
        self.client.resume().await?;
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        self.client.stop().await?;
        let mut snapshot = self.snapshot.write().await;
        snapshot.current_station = None;
        snapshot.metadata = TrackMetadata::default();
        snapshot.playback_state = PlaybackState::Stopped;
        Ok(())
    }

    pub async fn pause(&self) -> Result<()> {
        self.client.pause().await
    }

    pub async fn resume(&self) -> Result<()> {
        self.client.resume().await
    }

    pub async fn toggle_pause(&self) -> Result<()> {
        self.client.toggle_pause().await
    }

    pub async fn load_url(&self, url: &str) -> Result<()> {
        self.client.load_file(url).await
    }

    /// Lets MPV continue running after this session wrapper is dropped.
    pub fn disown_process(&mut self) {
        self.process.disown();
    }
}

pub struct SessionLock {
    _file: File,
}

impl SessionLock {
    pub fn acquire(socket_path: &Path) -> Result<Self> {
        let path = socket_path.with_extension("lock");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)?;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result != 0 {
            return Err(ResoError::Config(format!(
                "Another Reso TUI or daemon already owns the playback session ({})",
                path.display()
            )));
        }
        file.set_len(0)?;
        writeln!(file, "{}", std::process::id())?;
        file.flush()?;
        Ok(Self { _file: file })
    }
}
