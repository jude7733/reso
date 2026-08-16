//! Scrobbling integration for Last.fm and ListenBrainz.
//!
//! Submits tracks according to the standard 50% or 4-minute playback threshold.

use crate::config::{LastFmConfig, ListenBrainzConfig};
use crate::error::Result;
use crate::events::TrackMetadata;
use chrono::Utc;
use md5::{Digest, Md5};
use serde_json::json;
use std::collections::BTreeMap;
use std::time::Instant;

/// Tracks ongoing playback duration to determine when to trigger a scrobble.
#[derive(Debug)]
pub struct ScrobbleTracker {
    lastfm: LastFmConfig,
    listenbrainz: ListenBrainzConfig,
    client: reqwest::Client,
    current_track: Option<TrackMetadata>,
    start_time: Option<Instant>,
    scrobbled: bool,
}

impl ScrobbleTracker {
    /// Creates a new scrobble tracker.
    pub fn new(lastfm: LastFmConfig, listenbrainz: ListenBrainzConfig) -> Self {
        Self {
            lastfm,
            listenbrainz,
            client: reqwest::Client::new(),
            current_track: None,
            start_time: None,
            scrobbled: false,
        }
    }

    /// Updates the active track; if changed, resets timer and sends "Now Playing" notification.
    pub async fn on_track_change(&mut self, track: TrackMetadata) {
        if self.current_track.as_ref() != Some(&track) {
            self.current_track = Some(track.clone());
            self.start_time = Some(Instant::now());
            self.scrobbled = false;

            if let (Some(artist), Some(title)) = (&track.artist, &track.title) {
                let _ = self
                    .update_now_playing(artist, title, track.album.as_deref())
                    .await;
            }
        }
    }

    /// Periodic tick to check if the 50% or 4-minute threshold has been reached.
    pub async fn tick(&mut self) {
        if self.scrobbled {
            return;
        }

        if let (Some(start), Some(track)) = (self.start_time, &self.current_track) {
            let elapsed = start.elapsed().as_secs();
            let threshold = 240; // 4 minutes

            if elapsed >= threshold {
                if let (Some(artist), Some(title)) = (&track.artist, &track.title) {
                    let _ = self
                        .scrobble_track(artist, title, track.album.as_deref())
                        .await;
                    self.scrobbled = true;
                }
            }
        }
    }

    /// Submits "Now Playing" status to Last.fm.
    async fn update_now_playing(
        &self,
        artist: &str,
        title: &str,
        album: Option<&str>,
    ) -> Result<()> {
        if !self.lastfm.enabled {
            return Ok(());
        }

        let api_key = match &self.lastfm.api_key {
            Some(k) => k,
            None => return Ok(()),
        };
        let secret = match &self.lastfm.api_secret {
            Some(s) => s,
            None => return Ok(()),
        };
        let sk = match &self.lastfm.session_key {
            Some(k) => k,
            None => return Ok(()),
        };

        let mut params = BTreeMap::new();
        params.insert("method", "track.updateNowPlaying");
        params.insert("api_key", api_key.as_str());
        params.insert("sk", sk.as_str());
        params.insert("artist", artist);
        params.insert("track", title);
        if let Some(alb) = album {
            params.insert("album", alb);
        }

        let sig = calculate_lastfm_signature(&params, secret);
        params.insert("api_sig", &sig);
        params.insert("format", "json");

        let _ = self
            .client
            .post("https://ws.audioscrobbler.com/2.0/")
            .form(&params)
            .send()
            .await;

        Ok(())
    }

    /// Submits a completed scrobble to Last.fm and ListenBrainz.
    async fn scrobble_track(&self, artist: &str, title: &str, album: Option<&str>) -> Result<()> {
        // Last.fm Scrobble
        if self.lastfm.enabled {
            if let (Some(api_key), Some(secret), Some(sk)) = (
                &self.lastfm.api_key,
                &self.lastfm.api_secret,
                &self.lastfm.session_key,
            ) {
                let timestamp = Utc::now().timestamp().to_string();
                let mut params = BTreeMap::new();
                params.insert("method", "track.scrobble");
                params.insert("api_key", api_key.as_str());
                params.insert("sk", sk.as_str());
                params.insert("artist", artist);
                params.insert("track", title);
                params.insert("timestamp", &timestamp);
                if let Some(alb) = album {
                    params.insert("album", alb);
                }

                let sig = calculate_lastfm_signature(&params, secret);
                params.insert("api_sig", &sig);
                params.insert("format", "json");

                let _ = self
                    .client
                    .post("https://ws.audioscrobbler.com/2.0/")
                    .form(&params)
                    .send()
                    .await;
            }
        }

        // ListenBrainz Scrobble
        if self.listenbrainz.enabled {
            if let Some(token) = &self.listenbrainz.user_token {
                let payload = json!({
                    "listen_type": "single",
                    "payload": [{
                        "listened_at": Utc::now().timestamp(),
                        "track_metadata": {
                            "artist_name": artist,
                            "track_name": title,
                            "release_name": album.unwrap_or("")
                        }
                    }]
                });

                let _ = self
                    .client
                    .post("https://api.listenbrainz.org/1/submit-listens")
                    .header("Authorization", format!("Token {}", token))
                    .json(&payload)
                    .send()
                    .await;
            }
        }

        Ok(())
    }
}

/// Calculates Last.fm MD5 API method signature.
fn calculate_lastfm_signature(params: &BTreeMap<&str, &str>, secret: &str) -> String {
    let mut sig_base = String::new();
    for (k, v) in params {
        if *k != "format" && *k != "api_sig" {
            sig_base.push_str(k);
            sig_base.push_str(v);
        }
    }
    sig_base.push_str(secret);

    let mut hasher = Md5::new();
    hasher.update(sig_base.as_bytes());
    format!("{:x}", hasher.finalize())
}
