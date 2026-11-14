use crate::config::AppConfig;
use crate::config::Station;
use crate::error::{ResoError, Result};
use crate::events::{AudioStreamParams, MpvEvent, PlaybackState, TrackMetadata};
use crate::mpv::client::MpvClient;
use crate::mpv::process::MpvProcessManager;
use crate::mpv::record::StreamRecorder;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Duration;
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
    pub recording_state_known: bool,
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
            recording_state_known: false,
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
    recorder: StreamRecorder,
}

impl PlaybackSession {
    pub async fn connect(config: &AppConfig) -> Result<Self> {
        let mut process = MpvProcessManager::new(config.socket_path.clone())
            .with_reconnect_policy(config.reconnect_streams, config.reconnect_max_delay_secs);
        process.ensure_running().await?;
        let (client, mut mpv_events) = MpvClient::connect(config.socket_path.clone()).await?;
        let recorder = StreamRecorder::new(client.clone(), config.record_dir.clone());
        let (event_tx, _) = broadcast::channel(128);
        let initial_events = event_tx.subscribe();
        let snapshot = std::sync::Arc::new(RwLock::new(PlaybackSnapshot::default()));
        let event_snapshot = snapshot.clone();
        let event_tx_clone = event_tx.clone();
        let event_recorder = recorder.clone();
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
                            state.recording_state_known = true;
                        }
                        MpvEvent::Error(error) => state.last_error = Some(error.clone()),
                    }
                }
                if let MpvEvent::RecordingState { path, .. } = &event {
                    event_recorder.sync_active_path(path.clone()).await;
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
            recorder,
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

    pub async fn recording_status(&self) -> Result<(bool, Option<std::path::PathBuf>)> {
        if !self
            .wait_for_recording_state(Duration::from_millis(500))
            .await
        {
            return Err(crate::error::ResoError::Mpv(
                "Timed out reading MPV recording state".to_string(),
            ));
        }
        let snapshot = self.snapshot().await;
        Ok((snapshot.is_recording, snapshot.recording_path))
    }

    pub async fn start_recording(
        &self,
        custom_path: Option<std::path::PathBuf>,
    ) -> Result<std::path::PathBuf> {
        let _ = self.recording_status().await?;
        let snapshot = self.snapshot().await;
        let station = snapshot
            .current_station
            .as_ref()
            .map(|station| station.name.as_str())
            .unwrap_or("Stream");
        let track = snapshot.metadata.display_name();
        self.recorder
            .start_recording(station, Some(&track), custom_path)
            .await
    }

    pub async fn stop_recording(&self) -> Result<Option<std::path::PathBuf>> {
        let _ = self.recording_status().await?;
        self.recorder.stop_recording().await
    }

    pub async fn toggle_recording(&self) -> Result<Option<std::path::PathBuf>> {
        let (is_recording, _) = self.recording_status().await?;
        if is_recording {
            self.recorder.stop_recording().await
        } else {
            self.start_recording(None).await.map(Some)
        }
    }

    async fn wait_for_recording_state(&self, timeout: Duration) -> bool {
        let mut events = self.event_tx.subscribe();
        if self.snapshot.read().await.recording_state_known {
            return true;
        }
        tokio::time::timeout(timeout, async {
            loop {
                match events.recv().await {
                    Ok(MpvEvent::RecordingState { .. }) => return true,
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return false,
                }
            }
        })
        .await
        .unwrap_or(false)
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
