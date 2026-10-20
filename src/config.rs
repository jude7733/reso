//! Configuration, station catalog, and liked tracks persistence for `reso`.
//!
//! Manages runtime state stored in `~/.config/reso/`:
//! - `config.toml`: Application preferences and credentials.
//! - `stations.toml`: Mutable radio playlist & channels catalog.
//! - `liked_tracks.toml`: Structured catalog of liked / bookmarked tracks.

use crate::error::{ResoError, Result};
use crate::util::{config_dir, default_record_dir, default_socket_path};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Application settings loaded from `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// MPV Unix domain socket path.
    #[serde(default = "default_socket_path")]
    pub socket_path: PathBuf,

    /// Directory where lossless stream recordings are stored.
    #[serde(default = "default_record_dir")]
    pub record_dir: PathBuf,

    /// UI preferences.
    #[serde(default)]
    pub ui: UiConfig,

    /// Last.fm scrobbling configuration.
    #[serde(default)]
    pub lastfm: LastFmConfig,

    /// ListenBrainz scrobbling configuration.
    #[serde(default)]
    pub listenbrainz: ListenBrainzConfig,

    /// Default station ID to play on launch if none specified.
    pub default_station_id: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            socket_path: default_socket_path(),
            record_dir: default_record_dir(),
            ui: UiConfig::default(),
            lastfm: LastFmConfig::default(),
            listenbrainz: ListenBrainzConfig::default(),
            default_station_id: Some("rp-main".to_string()),
        }
    }
}

/// UI display preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    /// Whether to show the real-time spectrum visualizer bar.
    #[serde(default = "default_true")]
    pub show_visualizer: bool,

    /// Album cover rendering mode: "auto", "kitty", "sixel", "halfblocks", "off".
    #[serde(default = "default_cover_mode")]
    pub cover_mode: String,

    /// TUI color theme ("audiophile_dark", "cyberpunk", "monochrome", "wal").
    #[serde(default = "default_theme")]
    pub theme: String,

    /// Optional explicit path to color scheme file (e.g. `~/.cache/wal/colors.json` or `~/.cache/wal/reso.toml`).
    #[serde(default)]
    pub theme_path: Option<String>,

    /// Whether to watch the theme file and reload dynamically on wallpaper change.
    #[serde(default = "default_true")]
    pub theme_auto_reload: bool,

    /// Interval in seconds for checking theme file modification timestamp. Default: 5 seconds.
    #[serde(default = "default_theme_reload_interval")]
    pub theme_reload_interval_secs: u64,

    /// Default station list sorting mode ("favorites", "default", "quality", "name_asc", "name_desc", "codec").
    #[serde(default = "default_sort_mode")]
    pub default_sort_mode: String,
}

fn default_true() -> bool {
    true
}

fn default_cover_mode() -> String {
    "auto".to_string()
}

fn default_theme() -> String {
    "audiophile_dark".to_string()
}

fn default_theme_reload_interval() -> u64 {
    5
}

fn default_sort_mode() -> String {
    "favorites".to_string()
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            show_visualizer: true,
            cover_mode: default_cover_mode(),
            theme: default_theme(),
            theme_path: None,
            theme_auto_reload: true,
            theme_reload_interval_secs: default_theme_reload_interval(),
            default_sort_mode: default_sort_mode(),
        }
    }
}

/// Last.fm credentials and preferences.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LastFmConfig {
    pub enabled: bool,
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    pub session_key: Option<String>,
    pub username: Option<String>,
}

/// ListenBrainz credentials and preferences.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListenBrainzConfig {
    pub enabled: bool,
    pub user_token: Option<String>,
}

/// A radio station / channel representation in `stations.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Station {
    /// Unique slug or identifier (e.g., `rp-main`, `czech-d-dur`).
    pub id: String,

    /// Human-readable name.
    pub name: String,

    /// Audio stream URL.
    pub url: String,

    /// Audio codec (e.g., `FLAC`, `AAC`, `MP3`).
    pub codec: String,

    /// Nominal sample rate in Hz (e.g., 44100, 48000, 96000).
    #[serde(default)]
    pub sample_rate: u32,

    /// Nominal bit depth (e.g., 16, 24).
    #[serde(default)]
    pub bit_depth: u8,

    /// Optional homepage URL.
    pub homepage: Option<String>,

    /// Station tags or genre categories.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Marked as favorite.
    #[serde(default)]
    pub favorite: bool,

    /// Radio Paradise channel ID (0: Main, 1: Mellow, 2: Rock, 3: World/Eclectic).
    pub rp_channel: Option<u32>,
}

/// Stations catalog wrapper loaded from `~/.config/reso/stations.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct StationsCatalog {
    #[serde(default)]
    pub stations: Vec<Station>,
}

impl StationsCatalog {
    /// Returns the bundled default audiophile lossless stations catalog loaded from template TOML.
    pub fn default_lossless_stations() -> Self {
        const DEFAULT_TOML: &str = include_str!("default_stations.toml");
        toml::from_str::<StationsCatalog>(DEFAULT_TOML).unwrap_or_default()
    }

    /// Migrates legacy/defunct station URLs and merges newly introduced default stations.
    pub fn migrate_and_merge_defaults(&mut self) -> bool {
        let mut modified = false;
        let defaults = Self::default_lossless_stations();

        // 1. Fix known outdated or broken URLs in existing station configurations
        for station in self.stations.iter_mut() {
            if station.id == "czech-d-dur"
                && (station.url.contains("radio.d-dur.cz") || station.url.contains("d-dur.cz"))
            {
                station.url = "http://amp.cesnet.cz:8000/cro-d-dur.flac".to_string();
                station.sample_rate = 48000;
                station.bit_depth = 24;
                station.codec = "FLAC".to_string();
                modified = true;
            } else if station.id == "sector-space"
                && (station.url.contains("sectorradio.ru:8000")
                    || station.url.contains("sectorradio.ru"))
            {
                station.url = "http://89.223.45.5:8000/space-flac".to_string();
                station.sample_rate = 44100;
                station.bit_depth = 16;
                station.codec = "FLAC".to_string();
                modified = true;
            } else if station.id == "fip-paris" && station.url.contains("fip-midfi.mp3") {
                station.url = "https://icecast.radiofrance.fr/fip-hifi.aac".to_string();
                station.codec = "AAC".to_string();
                station.sample_rate = 48000;
                modified = true;
            } else if station.id == "mother-earth" {
                station.id = "radio-calico".to_string();
                station.name = "Radio Calico (Hi-Res 24-bit/48kHz)".to_string();
                station.url = "https://stream.radio-calico.com/calico".to_string();
                station.codec = "FLAC".to_string();
                station.sample_rate = 48000;
                station.bit_depth = 24;
                station.homepage = Some("https://radio-calico.com".to_string());
                station.tags = vec![
                    "audiophile".to_string(),
                    "hi-res".to_string(),
                    "rock".to_string(),
                    "pop".to_string(),
                ];
                modified = true;
            }
        }

        // 2. Append newly added default stations if not already present in catalog
        for def_st in defaults.stations {
            if !self.stations.iter().any(|s| s.id == def_st.id) {
                self.stations.push(def_st);
                modified = true;
            }
        }

        modified
    }

    /// Adds or updates a station in the catalog.
    pub fn add_station(&mut self, station: Station) {
        if let Some(pos) = self.stations.iter().position(|s| s.id == station.id) {
            self.stations[pos] = station;
        } else {
            self.stations.push(station);
        }
    }

    /// Removes a station from the catalog by ID.
    pub fn remove_station(&mut self, station_id: &str) -> bool {
        let initial_len = self.stations.len();
        self.stations.retain(|s| s.id != station_id);
        self.stations.len() < initial_len
    }

    /// Finds a station by ID, name, or URL.
    pub fn find_station(&self, query: &str) -> Option<&Station> {
        let q = query.trim().to_lowercase();
        let norm_q = crate::util::normalize_stream_url(&q);
        self.stations.iter().find(|s| {
            s.id.to_lowercase() == q
                || s.name.to_lowercase() == q
                || s.name.to_lowercase().contains(&q)
                || crate::util::normalize_stream_url(&s.url.to_lowercase()) == norm_q
        })
    }

    /// Edits an existing station by original ID.
    pub fn edit_station(&mut self, original_id: &str, station: Station) -> bool {
        if let Some(pos) = self.stations.iter().position(|s| s.id == original_id) {
            self.stations[pos] = station;
            true
        } else {
            false
        }
    }
}

/// A structured liked / bookmarked track entry stored in `liked_tracks.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LikedTrack {
    /// Unique track identifier.
    pub id: String,

    /// Formatted timestamp when the track was liked (e.g. `2026-08-26 15:40:00`).
    pub timestamp: String,

    /// Radio station where the track was playing.
    pub station: String,

    /// Track artist.
    pub artist: String,

    /// Track title.
    pub title: String,

    /// Album title if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,

    /// Audio codec when bookmarked (e.g. `FLAC`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,

    /// Sample rate in Hz when bookmarked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,

    /// Bit depth when bookmarked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bit_depth: Option<u8>,
}

impl LikedTrack {
    /// Formats the track as a human-readable display line.
    pub fn display_summary(&self) -> String {
        let album_part = self
            .album
            .as_deref()
            .map(|a| format!(" | Album: {}", a))
            .unwrap_or_default();
        let codec_part = self.codec.as_deref().unwrap_or("FLAC");
        let rate_part = self
            .sample_rate
            .map(|r| format!(" ({})", crate::util::format_sample_rate(r)))
            .unwrap_or_default();

        format!(
            "[{}] {} - {} [Station: {} | Codec: {}{}{}]",
            self.timestamp,
            self.artist,
            self.title,
            self.station,
            codec_part,
            rate_part,
            album_part
        )
    }
}

/// Catalog of liked tracks stored in `~/.config/reso/liked_tracks.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct LikedCatalog {
    #[serde(default)]
    pub tracks: Vec<LikedTrack>,
}

impl LikedCatalog {
    /// Adds a liked track entry to the front of the catalog.
    pub fn add_track(&mut self, track: LikedTrack) {
        // Avoid duplicate consecutive bookmark of the same artist and title
        if let Some(first) = self.tracks.first() {
            if first.artist.to_lowercase() == track.artist.to_lowercase()
                && first.title.to_lowercase() == track.title.to_lowercase()
            {
                return;
            }
        }
        self.tracks.insert(0, track);
    }

    /// Removes a liked track by ID.
    pub fn remove_track(&mut self, track_id: &str) -> bool {
        let initial_len = self.tracks.len();
        self.tracks.retain(|t| t.id != track_id);
        self.tracks.len() < initial_len
    }

    /// Migrates legacy lines from `liked_tracks.md` if available.
    pub fn migrate_from_markdown(content: &str) -> Self {
        let mut tracks = Vec::new();
        for (idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || !trimmed.starts_with('-') {
                continue;
            }
            // Line format: - `[2026-08-26 12:00:00]` **Artist** - *Title* [Station: Name | Codec: FLAC]
            let ts = if let Some(start) = trimmed.find("`[") {
                if let Some(end) = trimmed[start..].find("]`") {
                    trimmed[start + 2..start + end].to_string()
                } else {
                    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
                }
            } else {
                Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
            };

            let clean_line = trimmed.trim_start_matches('-').trim();
            let mut artist = "Unknown Artist".to_string();
            let mut title = "Liked Track".to_string();
            let mut station = "Reso".to_string();
            let mut codec = Some("FLAC".to_string());

            let mut remaining = clean_line;
            if let Some(pos) = clean_line.rfind("[Station: ") {
                let station_block = &clean_line[pos + 10..];
                if let Some(end_bracket) = station_block.find(']') {
                    let parts: Vec<&str> = station_block[..end_bracket].split('|').collect();
                    if let Some(st) = parts.first() {
                        station = st.trim().to_string();
                    }
                    for part in parts.iter().skip(1) {
                        if let Some(c) = part.trim().strip_prefix("Codec:") {
                            let clean_c = c.trim().split_whitespace().next().unwrap_or("FLAC");
                            codec = Some(clean_c.to_string());
                        }
                    }
                }
                remaining = clean_line[..pos].trim();
            }

            if let Some(ts_end) = remaining.find("]`") {
                remaining = remaining[ts_end + 2..].trim();
            }

            if let Some(hyphen_pos) = remaining.find(" - ") {
                let art_raw = remaining[..hyphen_pos].trim();
                let tit_raw = remaining[hyphen_pos + 3..].trim();
                artist = art_raw.trim_matches('*').trim().to_string();
                title = tit_raw.trim_matches('*').trim().to_string();
            } else if !remaining.is_empty() {
                artist = remaining.trim_matches('*').trim().to_string();
            }

            tracks.push(LikedTrack {
                id: format!("migrated-{}", idx),
                timestamp: ts,
                station,
                artist,
                title,
                album: None,
                codec,
                sample_rate: Some(44100),
                bit_depth: Some(16),
            });
        }
        Self { tracks }
    }
}

/// Central configuration manager for loading and saving settings, stations, and liked tracks.
#[derive(Debug, Clone)]
pub struct ConfigManager {
    config_dir: PathBuf,
    pub config: AppConfig,
    pub catalog: StationsCatalog,
    pub liked_catalog: LikedCatalog,
}

impl ConfigManager {
    /// Loads configuration, stations playlist, and liked tracks from `~/.config/reso/` or creates defaults.
    pub fn load_or_init() -> Result<Self> {
        Self::load_from_dir(config_dir())
    }

    /// Loads configuration, stations playlist, and liked tracks from a specific directory.
    pub fn load_from_dir(dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&dir)?;

        // 1. App Configuration (`config.toml`)
        let config_file = dir.join("config.toml");
        let config = if config_file.exists() {
            let content = fs::read_to_string(&config_file)?;
            toml::from_str(&content).unwrap_or_default()
        } else {
            let default_cfg = AppConfig::default();
            let content = toml::to_string_pretty(&default_cfg).map_err(ResoError::TomlSer)?;
            fs::write(&config_file, content)?;
            default_cfg
        };

        // 2. Stations Playlist Catalog (`stations.toml`)
        let stations_file = dir.join("stations.toml");
        let (catalog, stations_need_save) = if stations_file.exists() {
            let content = fs::read_to_string(&stations_file)?;
            match toml::from_str::<StationsCatalog>(&content) {
                Ok(mut cat) => {
                    let migrated = cat.migrate_and_merge_defaults();
                    (cat, migrated)
                }
                Err(_) => {
                    let default_cat = StationsCatalog::default_lossless_stations();
                    (default_cat, true)
                }
            }
        } else {
            let default_cat = StationsCatalog::default_lossless_stations();
            (default_cat, true)
        };

        if stations_need_save {
            let serialized = toml::to_string_pretty(&catalog).map_err(ResoError::TomlSer)?;
            fs::write(&stations_file, serialized)?;
        }

        // 3. Liked Tracks Catalog (`liked_tracks.toml` with migration from legacy `liked_tracks.md`)
        let liked_file = dir.join("liked_tracks.toml");
        let legacy_md_file = dir.join("liked_tracks.md");
        let (liked_catalog, liked_need_save) = if liked_file.exists() {
            let content = fs::read_to_string(&liked_file)?;
            (
                toml::from_str::<LikedCatalog>(&content).unwrap_or_default(),
                false,
            )
        } else if legacy_md_file.exists() {
            let md_content = fs::read_to_string(&legacy_md_file).unwrap_or_default();
            let cat = LikedCatalog::migrate_from_markdown(&md_content);
            (cat, true)
        } else {
            (LikedCatalog::default(), false)
        };

        if liked_need_save {
            let serialized = toml::to_string_pretty(&liked_catalog).map_err(ResoError::TomlSer)?;
            fs::write(&liked_file, serialized)?;
        }

        // Ensure recording directory exists
        let _ = fs::create_dir_all(&config.record_dir);

        Ok(Self {
            config_dir: dir,
            config,
            catalog,
            liked_catalog,
        })
    }

    /// Saves the current configuration to `config.toml`.
    pub fn save_config(&self) -> Result<()> {
        let config_file = self.config_dir.join("config.toml");
        let content = toml::to_string_pretty(&self.config).map_err(ResoError::TomlSer)?;
        fs::write(config_file, content)?;
        Ok(())
    }

    /// Saves the current station catalog to `stations.toml`.
    pub fn save_stations(&self) -> Result<()> {
        let stations_file = self.config_dir.join("stations.toml");
        let content = toml::to_string_pretty(&self.catalog).map_err(ResoError::TomlSer)?;
        fs::write(stations_file, content)?;
        Ok(())
    }

    /// Saves the liked tracks catalog to `liked_tracks.toml`.
    pub fn save_liked_tracks(&self) -> Result<()> {
        let liked_file = self.config_dir.join("liked_tracks.toml");
        let content = toml::to_string_pretty(&self.liked_catalog).map_err(ResoError::TomlSer)?;
        fs::write(liked_file, content)?;
        Ok(())
    }

    /// Bookmarks a track to `liked_tracks.toml`.
    pub fn bookmark_track(
        &mut self,
        station_name: &str,
        artist: &str,
        title: &str,
        album: Option<&str>,
        codec: Option<&str>,
        sample_rate: Option<u32>,
        bit_depth: Option<u8>,
    ) -> Result<LikedTrack> {
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let track_id = format!(
            "{}-{}",
            Local::now().timestamp_millis(),
            crate::util::sanitize_filename(artist)
                .chars()
                .take(10)
                .collect::<String>()
        );

        let track = LikedTrack {
            id: track_id,
            timestamp: now,
            station: station_name.to_string(),
            artist: artist.to_string(),
            title: title.to_string(),
            album: album.map(|a| a.to_string()),
            codec: codec.map(|c| c.to_string()),
            sample_rate,
            bit_depth,
        };

        self.liked_catalog.add_track(track.clone());
        self.save_liked_tracks()?;
        Ok(track)
    }

    /// Removes a liked track by ID and persists to `liked_tracks.toml`.
    pub fn remove_liked_track(&mut self, track_id: &str) -> Result<bool> {
        let removed = self.liked_catalog.remove_track(track_id);
        if removed {
            self.save_liked_tracks()?;
        }
        Ok(removed)
    }

    /// Finds a station by ID or by case-insensitive name match.
    pub fn find_station(&self, query: &str) -> Option<&Station> {
        let q_lower = query.to_lowercase();
        // Exact ID match first
        if let Some(s) = self.catalog.stations.iter().find(|s| s.id == query) {
            return Some(s);
        }
        // Case-insensitive ID match
        if let Some(s) = self
            .catalog
            .stations
            .iter()
            .find(|s| s.id.to_lowercase() == q_lower)
        {
            return Some(s);
        }
        // Substring / Name match
        self.catalog
            .stations
            .iter()
            .find(|s| s.name.to_lowercase().contains(&q_lower))
    }

    /// Toggles the favorite status of a station.
    pub fn toggle_favorite(&mut self, station_id: &str) -> bool {
        if let Some(s) = self
            .catalog
            .stations
            .iter_mut()
            .find(|s| s.id == station_id)
        {
            s.favorite = !s.favorite;
            let fav = s.favorite;
            let _ = self.save_stations();
            fav
        } else {
            false
        }
    }

    /// Adds or updates a station in the catalog and persists to `stations.toml`.
    pub fn add_station(&mut self, station: Station) -> Result<()> {
        self.catalog.add_station(station);
        self.save_stations()
    }

    /// Edits an existing station (preserving position or replacing ID) and persists to `stations.toml`.
    pub fn edit_station(&mut self, original_id: &str, updated_station: Station) -> Result<()> {
        if let Some(pos) = self
            .catalog
            .stations
            .iter()
            .position(|s| s.id == original_id)
        {
            self.catalog.stations[pos] = updated_station;
            self.save_stations()?;
            Ok(())
        } else {
            self.add_station(updated_station)
        }
    }

    /// Removes a station from the catalog by ID and persists to `stations.toml`.
    pub fn remove_station(&mut self, station_id: &str) -> Result<bool> {
        let removed = self.catalog.remove_station(station_id);
        if removed {
            self.save_stations()?;
        }
        Ok(removed)
    }
}
