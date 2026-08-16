//! Stream recorder manager using MPV lossless stream dumping.

use crate::error::Result;
use crate::mpv::client::MpvClient;
use crate::util::sanitize_filename;
use chrono::Local;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Manages recording lossless streams to local FLAC files.
#[derive(Clone)]
pub struct StreamRecorder {
    client: MpvClient,
    record_dir: PathBuf,
    current_recording: Arc<Mutex<Option<PathBuf>>>,
}

impl StreamRecorder {
    /// Creates a new `StreamRecorder`.
    pub fn new(client: MpvClient, record_dir: PathBuf) -> Self {
        Self {
            client,
            record_dir,
            current_recording: Arc::new(Mutex::new(None)),
        }
    }

    /// Checks if currently recording.
    pub async fn is_recording(&self) -> bool {
        self.current_recording.lock().await.is_some()
    }

    /// Returns the active recording file path if active.
    pub async fn active_path(&self) -> Option<PathBuf> {
        self.current_recording.lock().await.clone()
    }

    /// Starts recording the current stream.
    ///
    /// Constructs a filename: `~/Music/Radio/<Station>_<YYYYMMDD_HHMMSS>_<Track>.flac`.
    pub async fn start_recording(
        &self,
        station_name: &str,
        track_info: Option<&str>,
        custom_path: Option<PathBuf>,
    ) -> Result<PathBuf> {
        let path = match custom_path {
            Some(p) => p,
            None => {
                let _ = std::fs::create_dir_all(&self.record_dir);
                let timestamp = Local::now().format("%Y%m%d_%H%M%S");
                let station_sanitized = sanitize_filename(station_name);
                let track_sanitized = track_info
                    .map(sanitize_filename)
                    .unwrap_or_else(|| "Stream".to_string());

                let filename = format!("{}_{}_{}.flac", station_sanitized, timestamp, track_sanitized);
                self.record_dir.join(filename)
            }
        };

        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let path_str = path.to_string_lossy().to_string();
        self.client.set_stream_record(Some(&path_str)).await?;

        let mut lock = self.current_recording.lock().await;
        *lock = Some(path.clone());

        Ok(path)
    }

    /// Stops the active stream recording.
    pub async fn stop_recording(&self) -> Result<Option<PathBuf>> {
        self.client.set_stream_record(None).await?;
        let mut lock = self.current_recording.lock().await;
        let prev = lock.take();
        Ok(prev)
    }

    /// Toggles the recording state.
    pub async fn toggle(
        &self,
        station_name: &str,
        track_info: Option<&str>,
    ) -> Result<Option<PathBuf>> {
        if self.is_recording().await {
            self.stop_recording().await
        } else {
            let p = self.start_recording(station_name, track_info, None).await?;
            Ok(Some(p))
        }
    }
}
