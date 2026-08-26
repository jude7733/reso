//! External API integrations: Radio-Browser discovery and Radio Paradise rich metadata.

use crate::config::Station;
use crate::error::{ResoError, Result};
use crate::events::TrackMetadata;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Radio Paradise now playing response JSON schema.
#[derive(Debug, Clone, Deserialize)]
pub struct RadioParadiseNowPlaying {
    pub time: Option<u64>,
    pub artist: Option<String>,
    pub title: Option<String>,
    pub album: Option<String>,
    pub year: Option<String>,
    pub cover: Option<String>,
    pub cover_med: Option<String>,
    pub cover_small: Option<String>,
}

/// iTunes search API response schema.
#[derive(Debug, Clone, Deserialize)]
pub struct ITunesSearchResponse {
    pub results: Vec<ITunesSearchResult>,
}

/// iTunes individual track search item schema.
#[derive(Debug, Clone, Deserialize)]
pub struct ITunesSearchResult {
    #[serde(rename = "artworkUrl100")]
    pub artwork_url_100: Option<String>,
    #[serde(rename = "artworkUrl600")]
    pub artwork_url_600: Option<String>,
}

/// Radio-Browser station search response JSON schema.
#[derive(Debug, Clone, Deserialize)]
pub struct RadioBrowserStation {
    pub stationuuid: String,
    pub name: String,
    pub url: String,
    pub url_resolved: Option<String>,
    pub homepage: Option<String>,
    pub favicon: Option<String>,
    pub tags: Option<String>,
    pub codec: Option<String>,
    pub bitrate: Option<u32>,
    pub votes: Option<u32>,
}

/// Client for fetching online radio metadata and directory listings.
#[derive(Clone)]
pub struct ApiClient {
    client: reqwest::Client,
    cover_cache: Arc<RwLock<HashMap<String, Option<String>>>>,
}

impl Default for ApiClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ApiClient {
    /// Creates a new `ApiClient` with standard timeout and user agent headers.
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .user_agent("reso-audiophile-player/0.1.0 (Arch Linux)")
            .build()
            .unwrap_or_default();

        Self {
            client,
            cover_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Fetches rich metadata from Radio Paradise for a specific channel (0: Main, 1: Mellow, 2: Rock, 3: World).
    pub async fn fetch_radioparadise_now_playing(
        &self,
        channel: u32,
    ) -> Result<Option<TrackMetadata>> {
        let url = format!(
            "https://api.radioparadise.com/api/now_playing?chan={}",
            channel
        );
        let resp = self.client.get(&url).send().await?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let body = resp.text().await?;
        if body.trim().is_empty() {
            return Ok(None);
        }

        let rp: RadioParadiseNowPlaying = serde_json::from_str(&body)?;
        let cover = rp.cover.or(rp.cover_med).or(rp.cover_small);

        Ok(Some(TrackMetadata {
            artist: rp.artist,
            title: rp.title,
            album: rp.album,
            year: rp.year,
            raw_title: None,
            cover_url: cover,
            duration_remaining_secs: rp.time,
        }))
    }

    /// Searches `radio-browser.info` for stations matching name, codec, and bitrate.
    pub async fn search_radio_browser(
        &self,
        query: &str,
        codec: Option<&str>,
        min_bitrate: Option<u32>,
        limit: usize,
    ) -> Result<Vec<Station>> {
        let servers = [
            "https://de1.api.radio-browser.info",
            "https://nl1.api.radio-browser.info",
            "https://at1.api.radio-browser.info",
        ];

        let mut last_err = None;

        for server in servers {
            let mut url = format!(
                "{}/json/stations/search?name={}&limit={}&order=votes&reverse=true",
                server, query, limit
            );
            if let Some(c) = codec {
                url.push_str(&format!("&codec={}", c));
            }
            if let Some(br) = min_bitrate {
                url.push_str(&format!("&bitrateMin={}", br));
            }

            match self.client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(rb_stations) = resp.json::<Vec<RadioBrowserStation>>().await {
                        let stations = rb_stations
                            .into_iter()
                            .map(|rb| {
                                let stream_url = rb.url_resolved.unwrap_or(rb.url);
                                let tags_vec = rb
                                    .tags
                                    .unwrap_or_default()
                                    .split(',')
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty())
                                    .collect();

                                let codec_str = rb
                                    .codec
                                    .unwrap_or_else(|| "UNKNOWN".to_string())
                                    .to_uppercase();
                                let sample_rate = 44100;
                                let bit_depth = 16;

                                Station {
                                    id: format!(
                                        "rb-{}",
                                        &rb.stationuuid[..8.min(rb.stationuuid.len())]
                                    ),
                                    name: rb.name.trim().to_string(),
                                    url: stream_url,
                                    codec: codec_str,
                                    sample_rate,
                                    bit_depth,
                                    homepage: rb.homepage,
                                    tags: tags_vec,
                                    favorite: false,
                                    rp_channel: None,
                                }
                            })
                            .collect();

                        return Ok(stations);
                    }
                }
                Ok(resp) => {
                    last_err = Some(ResoError::Api(resp.error_for_status().unwrap_err()));
                }
                Err(e) => {
                    last_err = Some(ResoError::Api(e));
                }
            }
        }

        if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(Vec::new())
        }
    }

    /// Downloads cover art image bytes from a URL.
    pub async fn download_image_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self.client.get(url).send().await?;
        let bytes = resp.bytes().await?;
        Ok(bytes.to_vec())
    }

    /// Searches iTunes for album artwork given artist and track title.
    pub async fn search_itunes_cover_art(
        &self,
        artist: &str,
        title: &str,
    ) -> Result<Option<String>> {
        let clean_artist = artist.trim();
        let clean_title = title.trim();
        if clean_artist.is_empty() && clean_title.is_empty() {
            return Ok(None);
        }

        let cache_key = format!("{} - {}", clean_artist, clean_title).to_lowercase();
        if let Ok(cache) = self.cover_cache.read() {
            if let Some(cached_url) = cache.get(&cache_key) {
                return Ok(cached_url.clone());
            }
        }

        let query = format!("{} {}", clean_artist, clean_title);
        let resp = self
            .client
            .get("https://itunes.apple.com/search")
            .query(&[
                ("term", &query),
                ("media", &"music".to_string()),
                ("entity", &"song".to_string()),
                ("limit", &"1".to_string()),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let search_res: ITunesSearchResponse = match resp.json().await {
            Ok(res) => res,
            Err(_) => return Ok(None),
        };

        let cover_url = search_res.results.into_iter().next().and_then(|r| {
            r.artwork_url_600.or_else(|| {
                r.artwork_url_100
                    .map(|u| u.replace("100x100bb", "600x600bb"))
            })
        });

        if let Ok(mut cache) = self.cover_cache.write() {
            cache.insert(cache_key, cover_url.clone());
        }

        Ok(cover_url)
    }
}
