//! Asynchronous Unix domain socket JSON-IPC client for MPV.

use crate::error::{ResoError, Result};
use crate::events::{AudioStreamParams, MpvEvent, PlaybackState, TrackMetadata};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::UnixStream;
use tokio::sync::{broadcast, mpsc};
use tokio_util::codec::{Framed, LinesCodec};

/// Observer IDs mapped to MPV properties.
const OBS_PAUSE: u64 = 1;
const OBS_IDLE: u64 = 2;
const OBS_AUDIO_PARAMS: u64 = 3;
const OBS_MEDIA_TITLE: u64 = 4;
const OBS_ICY_TITLE: u64 = 5;
const OBS_AUDIO_BITRATE: u64 = 6;
const OBS_AUDIO_CODEC: u64 = 7;
const OBS_VOLUME: u64 = 8;
const OBS_STREAM_RECORD: u64 = 9;

/// Command payload sent over MPV JSON-IPC.
#[derive(Debug, Serialize)]
struct IpcCommand {
    command: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<u64>,
}

/// Generic message received from MPV JSON-IPC.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct IpcResponse {
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    request_id: Option<u64>,
}

/// High-level async MPV IPC client.
#[derive(Clone)]
#[allow(dead_code)]
pub struct MpvClient {
    pub socket_path: PathBuf,
    cmd_tx: mpsc::Sender<String>,
    request_counter: Arc<AtomicU64>,
    event_bus: broadcast::Sender<MpvEvent>,
}

impl MpvClient {
    /// Connects to the MPV IPC socket, initiates property observers, and spawns the background event loop.
    pub async fn connect(socket_path: PathBuf) -> Result<(Self, broadcast::Receiver<MpvEvent>)> {
        let stream = UnixStream::connect(&socket_path)
            .await
            .map_err(|e| ResoError::Ipc(format!("Failed to connect to MPV socket at {}: {}", socket_path.display(), e)))?;

        let framed = Framed::new(stream, LinesCodec::new());
        let (mut writer, mut reader) = framed.split();

        let (cmd_tx, mut cmd_rx) = mpsc::channel::<String>(64);
        let (event_tx, event_rx) = broadcast::channel::<MpvEvent>(128);

        let event_tx_clone = event_tx.clone();

        // Background Writer Task
        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                if writer.send(cmd).await.is_err() {
                    break;
                }
            }
        });

        // Background Reader / Event Loop Task
        tokio::spawn(async move {
            let mut current_state = PlaybackState::Stopped;
            let mut current_metadata = TrackMetadata::default();
            let mut current_params = AudioStreamParams::default();
            let mut is_idle = true;
            let mut is_paused = false;

            while let Some(Ok(line)) = reader.next().await {
                if let Ok(resp) = serde_json::from_str::<IpcResponse>(&line) {
                    if let Some(event_name) = &resp.event {
                        match event_name.as_str() {
                            "property-change" => {
                                let id = resp.id.unwrap_or(0);
                                match id {
                                    OBS_PAUSE => {
                                        if let Some(Value::Bool(p)) = resp.data {
                                            is_paused = p;
                                            let new_state = if is_idle {
                                                PlaybackState::Stopped
                                            } else if is_paused {
                                                PlaybackState::Paused
                                            } else {
                                                PlaybackState::Playing
                                            };
                                            if new_state != current_state {
                                                current_state = new_state;
                                                let _ = event_tx_clone.send(MpvEvent::State(current_state));
                                            }
                                        }
                                    }
                                    OBS_IDLE => {
                                        if let Some(Value::Bool(idle)) = resp.data {
                                            is_idle = idle;
                                            let new_state = if is_idle {
                                                PlaybackState::Stopped
                                            } else if is_paused {
                                                PlaybackState::Paused
                                            } else {
                                                PlaybackState::Playing
                                            };
                                            if new_state != current_state {
                                                current_state = new_state;
                                                let _ = event_tx_clone.send(MpvEvent::State(current_state));
                                            }
                                        }
                                    }
                                    OBS_AUDIO_PARAMS => {
                                        if let Some(Value::Object(map)) = resp.data {
                                            if let Some(Value::Number(sr)) = map.get("samplerate") {
                                                current_params.sample_rate = sr.as_u64().unwrap_or(0) as u32;
                                            }
                                            if let Some(Value::Number(ch)) = map.get("channel-count") {
                                                current_params.channels = ch.as_u64().unwrap_or(2) as u32;
                                            }
                                            if let Some(Value::String(fmt)) = map.get("format") {
                                                if fmt.contains("24") || fmt.contains("s24") {
                                                    current_params.bit_depth = Some(24);
                                                } else if fmt.contains("16") || fmt.contains("s16") {
                                                    current_params.bit_depth = Some(16);
                                                } else if fmt.contains("32") || fmt.contains("f32") {
                                                    current_params.bit_depth = Some(32);
                                                }
                                            }
                                            let _ = event_tx_clone.send(MpvEvent::AudioParams(current_params.clone()));
                                        }
                                    }
                                    OBS_MEDIA_TITLE | OBS_ICY_TITLE => {
                                        if let Some(Value::String(title_str)) = resp.data {
                                            if !title_str.trim().is_empty() {
                                                let (artist, title) = parse_artist_title(&title_str);
                                                current_metadata.artist = artist;
                                                current_metadata.title = title;
                                                current_metadata.raw_title = Some(title_str);
                                                let _ = event_tx_clone.send(MpvEvent::Metadata(current_metadata.clone()));
                                            }
                                        }
                                    }
                                    OBS_AUDIO_BITRATE => {
                                        if let Some(Value::Number(br)) = resp.data {
                                            let bps = br.as_f64().unwrap_or(0.0) as u32;
                                            current_params.bitrate_kbps = Some(bps / 1000);
                                            let _ = event_tx_clone.send(MpvEvent::AudioParams(current_params.clone()));
                                        }
                                    }
                                    OBS_AUDIO_CODEC => {
                                        if let Some(Value::String(codec)) = resp.data {
                                            current_params.codec = codec.to_uppercase();
                                            let _ = event_tx_clone.send(MpvEvent::AudioParams(current_params.clone()));
                                        }
                                    }
                                    OBS_VOLUME => {
                                        if let Some(Value::Number(vol)) = resp.data {
                                            let v = vol.as_f64().unwrap_or(100.0);
                                            let _ = event_tx_clone.send(MpvEvent::Volume(v));
                                        }
                                    }
                                    OBS_STREAM_RECORD => {
                                        match resp.data {
                                            Some(Value::String(p)) if !p.trim().is_empty() => {
                                                let _ = event_tx_clone.send(MpvEvent::RecordingState {
                                                    is_recording: true,
                                                    path: Some(PathBuf::from(p)),
                                                });
                                            }
                                            _ => {
                                                let _ = event_tx_clone.send(MpvEvent::RecordingState {
                                                    is_recording: false,
                                                    path: None,
                                                });
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            "start-file" => {
                                is_idle = false;
                            }
                            "playback-restart" => {
                                is_idle = false;
                                if !is_paused {
                                    current_state = PlaybackState::Playing;
                                    let _ = event_tx_clone.send(MpvEvent::State(current_state));
                                }
                            }
                            "end-file" => {
                                current_state = PlaybackState::Stopped;
                                let _ = event_tx_clone.send(MpvEvent::State(current_state));
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        let client = Self {
            socket_path,
            cmd_tx,
            request_counter: Arc::new(AtomicU64::new(100)),
            event_bus: event_tx,
        };

        // Register Property Observers
        client.observe_property(OBS_PAUSE, "pause").await?;
        client.observe_property(OBS_IDLE, "idle-active").await?;
        client.observe_property(OBS_AUDIO_PARAMS, "audio-params").await?;
        client.observe_property(OBS_MEDIA_TITLE, "media-title").await?;
        client.observe_property(OBS_ICY_TITLE, "icy-title").await?;
        client.observe_property(OBS_AUDIO_BITRATE, "audio-bitrate").await?;
        client.observe_property(OBS_AUDIO_CODEC, "audio-codec-name").await?;
        client.observe_property(OBS_VOLUME, "volume").await?;
        client.observe_property(OBS_STREAM_RECORD, "stream-record").await?;

        Ok((client, event_rx))
    }

    /// Subscribes to the broadcast event receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<MpvEvent> {
        self.event_bus.subscribe()
    }

    /// Sends a raw command array to MPV.
    async fn send_command(&self, args: Vec<Value>) -> Result<u64> {
        let req_id = self.request_counter.fetch_add(1, Ordering::SeqCst);
        let cmd = IpcCommand {
            command: args,
            request_id: Some(req_id),
        };
        let serialized = serde_json::to_string(&cmd)?;
        self.cmd_tx
            .send(serialized)
            .await
            .map_err(|e| ResoError::Ipc(format!("Failed to send IPC command: {}", e)))?;
        Ok(req_id)
    }

    /// Instructs MPV to observe a property and push changes asynchronously.
    pub async fn observe_property(&self, id: u64, name: &str) -> Result<()> {
        self.send_command(vec![
            json!("observe_property"),
            json!(id),
            json!(name),
        ])
        .await?;
        Ok(())
    }

    /// Loads and begins streaming the given URL.
    pub async fn load_file(&self, url: &str) -> Result<()> {
        self.send_command(vec![
            json!("loadfile"),
            json!(url),
            json!("replace"),
        ])
        .await?;
        let _ = self.resume().await;
        Ok(())
    }

    /// Stops playback.
    pub async fn stop(&self) -> Result<()> {
        self.send_command(vec![json!("stop")]).await?;
        Ok(())
    }

    /// Pauses playback.
    pub async fn pause(&self) -> Result<()> {
        self.set_property("pause", json!(true)).await
    }

    /// Resumes playback.
    pub async fn resume(&self) -> Result<()> {
        self.set_property("pause", json!(false)).await
    }

    /// Toggles pause/playback state.
    pub async fn toggle_pause(&self) -> Result<()> {
        self.send_command(vec![
            json!("cycle"),
            json!("pause"),
        ])
        .await?;
        Ok(())
    }

    /// Sets an MPV property by name.
    pub async fn set_property(&self, name: &str, value: Value) -> Result<()> {
        self.send_command(vec![
            json!("set_property"),
            json!(name),
            value,
        ])
        .await?;
        Ok(())
    }

    /// Starts or stops stream dumping via `stream-record`.
    pub async fn set_stream_record(&self, path: Option<&str>) -> Result<()> {
        let val = match path {
            Some(p) => json!(p),
            None => json!(""),
        };
        self.set_property("stream-record", val).await
    }
}

/// Parses "Artist - Title" or similar delimiter variants from ICY strings.
pub fn parse_artist_title(raw: &str) -> (Option<String>, Option<String>) {
    let raw = raw.trim();
    if raw.is_empty() {
        return (None, None);
    }

    // Check for "Artist - Title"
    if let Some((artist, title)) = raw.split_once(" - ") {
        let a = artist.trim();
        let t = title.trim();
        if !a.is_empty() && !t.is_empty() {
            return (Some(a.to_string()), Some(t.to_string()));
        }
    }

    // Check for "Artist / Title"
    if let Some((artist, title)) = raw.split_once(" / ") {
        let a = artist.trim();
        let t = title.trim();
        if !a.is_empty() && !t.is_empty() {
            return (Some(a.to_string()), Some(t.to_string()));
        }
    }

    (None, Some(raw.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_artist_title() {
        let (artist, title) = parse_artist_title("Radiohead - Karma Police");
        assert_eq!(artist, Some("Radiohead".to_string()));
        assert_eq!(title, Some("Karma Police".to_string()));

        let (artist2, title2) = parse_artist_title("Pink Floyd / Time");
        assert_eq!(artist2, Some("Pink Floyd".to_string()));
        assert_eq!(title2, Some("Time".to_string()));

        let (artist3, title3) = parse_artist_title("Single Title Only");
        assert_eq!(artist3, None);
        assert_eq!(title3, Some("Single Title Only".to_string()));
    }
}
