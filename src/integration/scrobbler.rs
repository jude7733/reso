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
                if is_scrobbleable(artist, title) {
                    let _ = self
                        .update_now_playing(artist, title, track.album.as_deref())
                        .await;
                }
            }
        }
    }

    /// Periodic tick to check if the 30-second / 50% threshold has been reached.
    pub async fn tick(&mut self) {
        if self.scrobbled {
            return;
        }

        if let (Some(start), Some(track)) = (self.start_time, &self.current_track) {
            let elapsed = start.elapsed().as_secs();

            if let (Some(artist), Some(title)) = (&track.artist, &track.title) {
                // 1. Filter out radio advertisements, station IDs, jingles, and DJ chatter
                if !is_scrobbleable(artist, title) {
                    return;
                }

                // 2. Reject tracks known to be shorter than 30 seconds per Last.fm rules
                if let Some(dur) = track.duration_remaining_secs {
                    if dur > 0 && dur < 30 {
                        return;
                    }
                }

                // 3. Compute 30-second / 50% threshold (capped at 4 minutes)
                let threshold = calculate_scrobble_threshold(track.duration_remaining_secs);

                if elapsed >= threshold {
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
pub fn calculate_lastfm_signature(params: &BTreeMap<&str, &str>, secret: &str) -> String {
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

/// Requests a temporary unauthorized token from Last.fm to start web authentication.
pub async fn request_lastfm_token(api_key: &str, secret: &str) -> Result<String> {
    let mut params = BTreeMap::new();
    params.insert("method", "auth.getToken");
    params.insert("api_key", api_key);
    let sig = calculate_lastfm_signature(&params, secret);
    params.insert("api_sig", &sig);
    params.insert("format", "json");

    let client = reqwest::Client::new();
    let resp = client
        .get("https://ws.audioscrobbler.com/2.0/")
        .query(&params)
        .send()
        .await
        .map_err(|e| crate::error::ResoError::Scrobbler(e.to_string()))?;

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| crate::error::ResoError::Scrobbler(e.to_string()))?;

    if let Some(token) = json.get("token").and_then(|t| t.as_str()) {
        Ok(token.to_string())
    } else {
        let msg = json
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown Last.fm error");
        Err(crate::error::ResoError::Scrobbler(msg.to_string()))
    }
}

/// Exchanges an authorized Last.fm token for a permanent session key and username.
pub async fn fetch_lastfm_session(
    api_key: &str,
    secret: &str,
    token: &str,
) -> Result<(String, String)> {
    let mut params = BTreeMap::new();
    params.insert("method", "auth.getSession");
    params.insert("api_key", api_key);
    params.insert("token", token);
    let sig = calculate_lastfm_signature(&params, secret);
    params.insert("api_sig", &sig);
    params.insert("format", "json");

    let client = reqwest::Client::new();
    let resp = client
        .get("https://ws.audioscrobbler.com/2.0/")
        .query(&params)
        .send()
        .await
        .map_err(|e| crate::error::ResoError::Scrobbler(e.to_string()))?;

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| crate::error::ResoError::Scrobbler(e.to_string()))?;

    if let Some(session) = json.get("session") {
        let username = session
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .to_string();
        let key = session
            .get("key")
            .and_then(|k| k.as_str())
            .unwrap_or("")
            .to_string();
        if !key.is_empty() {
            return Ok((username, key));
        }
    }

    let msg = json
        .get("message")
        .and_then(|m| m.as_str())
        .unwrap_or("Failed to obtain session key");
    Err(crate::error::ResoError::Scrobbler(msg.to_string()))
}

/// Determines if a track has valid, scrobbleable music metadata,
/// filtering out radio commercials, station IDs, DJ chatter, and placeholder text.
pub fn is_scrobbleable(artist: &str, title: &str) -> bool {
    let a = artist.trim();
    let t = title.trim();

    // Must have at least 2 characters for both artist and title
    if a.len() < 2 || t.len() < 2 {
        return false;
    }

    let a_lower = a.to_lowercase();
    let t_lower = t.to_lowercase();

    // Discard if artist and title are identical (common for station identifier loops)
    if a_lower == t_lower {
        return false;
    }

    // Exact matches for single-word commercials or station IDs
    const EXACT_GARBAGE: &[&str] = &[
        "ad", "ads", "advert", "adverts", "commercial", "commercials",
        "promo", "promos", "sponsor", "sponsors", "jingle", "jingles",
        "station id", "radio", "reso", "unknown", "offline",
    ];

    for exact in EXACT_GARBAGE {
        if a_lower == *exact || t_lower == *exact {
            return false;
        }
    }

    // Known garbage keywords/phrases to filter out (case-insensitive substring)
    const GARBAGE_PATTERNS: &[&str] = &[
        "commercial break",
        "advertisement",
        "advertising",
        "station id",
        "station promo",
        "station jingle",
        "dj talk",
        "dj speaking",
        "news break",
        "traffic report",
        "weather report",
        "news & weather",
        "news update",
        "breaking news",
        "sponsor break",
        "sponsored by",
        "intermission",
        "stream offline",
        "unknown artist",
        "unknown track",
        "unknown title",
        "various artists",
        "test stream",
        "live stream",
        "stream starting",
        "connecting...",
    ];

    for pattern in GARBAGE_PATTERNS {
        if a_lower.contains(pattern) || t_lower.contains(pattern) {
            return false;
        }
    }

    true
}

/// Calculates the required playback duration threshold (in seconds)
/// before a scrobble can be submitted, strictly adhering to Last.fm guidelines:
/// - Minimum 30 seconds of playback for any track.
/// - If track duration is known: at least 50% of the track length or 4 minutes (240s), whichever is less.
/// - If track duration is unknown (typical for continuous ICY radio streams): 30 seconds.
pub fn calculate_scrobble_threshold(duration_secs: Option<u64>) -> u64 {
    match duration_secs {
        Some(total) if total > 0 => {
            let half = total / 2;
            half.clamp(30, 240)
        }
        _ => 30, // Default 30 seconds for live radio
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_scrobbleable_valid_tracks() {
        assert!(is_scrobbleable("Radiohead", "Karma Police"));
        assert!(is_scrobbleable("Miles Davis", "So What"));
        assert!(is_scrobbleable("Boards of Canada", "Dayvan Cowboy"));
        assert!(is_scrobbleable("Adele", "Hello"));
        assert!(is_scrobbleable("Sad Lovers & Giants", "Imagination"));
    }

    #[test]
    fn test_is_scrobbleable_filters_garbage() {
        // Ads & Commercials
        assert!(!is_scrobbleable("Station", "Commercial Break"));
        assert!(!is_scrobbleable("Radio", "Advertisement"));
        assert!(!is_scrobbleable("Ad", "Promo"));
        assert!(!is_scrobbleable("Sponsor", "Sponsored by Reso"));
        
        // Station Identifiers & Jingle
        assert!(!is_scrobbleable("Radio Paradise", "Station ID"));
        assert!(!is_scrobbleable("BBC Radio", "Station Jingle"));
        assert!(!is_scrobbleable("Radio 1", "DJ Talk"));
        assert!(!is_scrobbleable("News Update", "Headlines"));
        assert!(!is_scrobbleable("Traffic Report", "Morning Traffic"));

        // Unknowns / Placeholders
        assert!(!is_scrobbleable("Unknown Artist", "Unknown Track"));
        assert!(!is_scrobbleable("Various Artists", "Test Stream"));
        assert!(!is_scrobbleable("Stream Offline", "Please wait"));
        assert!(!is_scrobbleable("Connecting...", "Buffering"));

        // Identical artist and title
        assert!(!is_scrobbleable("FIP", "FIP"));

        // Too short
        assert!(!is_scrobbleable("A", "B"));
        assert!(!is_scrobbleable("", "Track"));
    }

    #[test]
    fn test_calculate_scrobble_threshold() {
        // Unknown duration (typical radio ICY stream): 30 seconds
        assert_eq!(calculate_scrobble_threshold(None), 30);
        assert_eq!(calculate_scrobble_threshold(Some(0)), 30);

        // 3-minute track (180s) -> 50% = 90s
        assert_eq!(calculate_scrobble_threshold(Some(180)), 90);

        // 10-minute track (600s) -> 50% = 300s, capped at 240s (4 mins)
        assert_eq!(calculate_scrobble_threshold(Some(600)), 240);

        // Short track (40s) -> 50% is 20s, clamped to minimum 30s
        assert_eq!(calculate_scrobble_threshold(Some(40)), 30);

        // 1-minute track (60s) -> 50% = 30s
        assert_eq!(calculate_scrobble_threshold(Some(60)), 30);
    }
}


