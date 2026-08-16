//! Configuration and station catalog management for `reso`.
//!
//! Manages `~/.config/reso/config.toml`, `~/.config/reso/stations.toml`, and `~/.config/reso/liked_tracks.md`.

use crate::error::{ResoError, Result};
use crate::util::{config_dir, default_record_dir, default_socket_path};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
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

    /// TUI color theme ("audiophile_dark", "cyberpunk", "monochrome").
    #[serde(default = "default_theme")]
    pub theme: String,
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

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            show_visualizer: true,
            cover_mode: default_cover_mode(),
            theme: default_theme(),
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

/// A radio station representation in `stations.toml`.
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

/// Stations catalog wrapper.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationsCatalog {
    pub stations: Vec<Station>,
}

impl StationsCatalog {
    /// Returns default audiophile lossless stations.
    pub fn default_lossless_stations() -> Self {
        Self {
            stations: vec![
                Station {
                    id: "rp-main".to_string(),
                    name: "Radio Paradise (Main Mix)".to_string(),
                    url: "http://stream.radioparadise.com/flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://radioparadise.com".to_string()),
                    tags: vec!["eclectic".to_string(), "lossless".to_string(), "audiophile".to_string()],
                    favorite: true,
                    rp_channel: Some(0),
                },
                Station {
                    id: "rp-mellow".to_string(),
                    name: "Radio Paradise (Mellow Mix)".to_string(),
                    url: "http://stream.radioparadise.com/mellow-flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://radioparadise.com".to_string()),
                    tags: vec!["mellow".to_string(), "acoustic".to_string(), "lossless".to_string()],
                    favorite: false,
                    rp_channel: Some(1),
                },
                Station {
                    id: "rp-rock".to_string(),
                    name: "Radio Paradise (Rock Mix)".to_string(),
                    url: "http://stream.radioparadise.com/rock-flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://radioparadise.com".to_string()),
                    tags: vec!["rock".to_string(), "lossless".to_string()],
                    favorite: false,
                    rp_channel: Some(2),
                },
                Station {
                    id: "rp-world".to_string(),
                    name: "Radio Paradise (World / Eclectic Mix)".to_string(),
                    url: "http://stream.radioparadise.com/world-flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://radioparadise.com".to_string()),
                    tags: vec!["world".to_string(), "eclectic".to_string(), "lossless".to_string()],
                    favorite: false,
                    rp_channel: Some(3),
                },
                Station {
                    id: "czech-d-dur".to_string(),
                    name: "Czech Radio D-Dur (Hi-Res Classical)".to_string(),
                    url: "http://radio.d-dur.cz/d-dur.flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 48000,
                    bit_depth: 24,
                    homepage: Some("https://d-dur.rozhlas.cz".to_string()),
                    tags: vec!["classical".to_string(), "hi-res".to_string(), "24bit".to_string()],
                    favorite: true,
                    rp_channel: None,
                },
                Station {
                    id: "jb-radio2".to_string(),
                    name: "JB Radio-2 (Hi-Res FLAC 96kHz/24b)".to_string(),
                    url: "http://199.189.87.9:10999/flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 96000,
                    bit_depth: 24,
                    homepage: Some("https://jbradio2.ca".to_string()),
                    tags: vec!["audiophile".to_string(), "hi-res".to_string(), "96khz".to_string(), "rock".to_string()],
                    favorite: true,
                    rp_channel: None,
                },
                Station {
                    id: "mother-earth".to_string(),
                    name: "Mother Earth Radio (Hi-Res 96kHz)".to_string(),
                    url: "https://motherearth.streamserver24.com/listen/motherearth/motherearth.flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 96000,
                    bit_depth: 24,
                    homepage: Some("https://motherearthradio.de".to_string()),
                    tags: vec!["audiophile".to_string(), "hi-res".to_string(), "vinyl".to_string()],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "sector-space".to_string(),
                    name: "Sector Radio (Space FLAC)".to_string(),
                    url: "http://sectorradio.ru:8000/space-flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://sectorradio.ru".to_string()),
                    tags: vec!["ambient".to_string(), "space".to_string(), "lossless".to_string()],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "fip-paris".to_string(),
                    name: "FIP Radio Paris".to_string(),
                    url: "https://icecast.radiofrance.fr/fip-midfi.mp3".to_string(),
                    codec: "MP3".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://www.radiofrance.fr/fip".to_string()),
                    tags: vec!["jazz".to_string(), "eclectic".to_string(), "france".to_string()],
                    favorite: false,
                    rp_channel: None,
                },
            ],
        }
    }
}

/// Central configuration manager for loading and saving settings and stations.
#[derive(Debug, Clone)]
pub struct ConfigManager {
    config_dir: PathBuf,
    pub config: AppConfig,
    pub catalog: StationsCatalog,
}

impl ConfigManager {
    /// Loads configuration and stations from `~/.config/reso/` or creates defaults.
    pub fn load_or_init() -> Result<Self> {
        let dir = config_dir();
        fs::create_dir_all(&dir)?;

        let config_file = dir.join("config.toml");
        let config = if config_file.exists() {
            let content = fs::read_to_string(&config_file)?;
            toml::from_str(&content).unwrap_or_default()
        } else {
            let default_cfg = AppConfig::default();
            let content = toml::to_string_pretty(&default_cfg)
                .map_err(ResoError::TomlSer)?;
            fs::write(&config_file, content)?;
            default_cfg
        };

        let stations_file = dir.join("stations.toml");
        let catalog = if stations_file.exists() {
            let content = fs::read_to_string(&stations_file)?;
            match toml::from_str(&content) {
                Ok(cat) => cat,
                Err(_) => {
                    let default_cat = StationsCatalog::default_lossless_stations();
                    let serialized = toml::to_string_pretty(&default_cat)
                        .map_err(ResoError::TomlSer)?;
                    fs::write(&stations_file, serialized)?;
                    default_cat
                }
            }
        } else {
            let default_cat = StationsCatalog::default_lossless_stations();
            let serialized = toml::to_string_pretty(&default_cat)
                .map_err(ResoError::TomlSer)?;
            fs::write(&stations_file, serialized)?;
            default_cat
        };

        // Ensure recording directory exists
        let _ = fs::create_dir_all(&config.record_dir);

        Ok(Self {
            config_dir: dir,
            config,
            catalog,
        })
    }

    /// Saves the current configuration to `config.toml`.
    pub fn save_config(&self) -> Result<()> {
        let config_file = self.config_dir.join("config.toml");
        let content = toml::to_string_pretty(&self.config)
            .map_err(ResoError::TomlSer)?;
        fs::write(config_file, content)?;
        Ok(())
    }

    /// Saves the current station catalog to `stations.toml`.
    pub fn save_stations(&self) -> Result<()> {
        let stations_file = self.config_dir.join("stations.toml");
        let content = toml::to_string_pretty(&self.catalog)
            .map_err(ResoError::TomlSer)?;
        fs::write(stations_file, content)?;
        Ok(())
    }

    /// Appends a bookmark entry for the currently liked track to `liked_tracks.md`.
    pub fn bookmark_track(
        &self,
        station_name: &str,
        artist: &str,
        title: &str,
        album: Option<&str>,
        codec: Option<&str>,
        sample_rate: Option<u32>,
    ) -> Result<PathBuf> {
        let file_path = self.config_dir.join("liked_tracks.md");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;

        let now = Local::now().format("%Y-%m-%d %H:%M:%S");
        let album_str = album.map(|a| format!(" | Album: *{}*", a)).unwrap_or_default();
        let codec_str = codec.unwrap_or("Lossless");
        let rate_str = sample_rate
            .map(|r| format!(" ({})", crate::util::format_sample_rate(r)))
            .unwrap_or_default();

        let line = format!(
            "- `[{}]` **{}** - *{}* [Station: {} | Codec: {}{}{}]\n",
            now, artist, title, station_name, codec_str, rate_str, album_str
        );

        file.write_all(line.as_bytes())?;
        Ok(file_path)
    }

    /// Finds a station by ID or by case-insensitive name match.
    pub fn find_station(&self, query: &str) -> Option<&Station> {
        let q_lower = query.to_lowercase();
        // Exact ID match first
        if let Some(s) = self.catalog.stations.iter().find(|s| s.id == query) {
            return Some(s);
        }
        // Case-insensitive ID match
        if let Some(s) = self.catalog.stations.iter().find(|s| s.id.to_lowercase() == q_lower) {
            return Some(s);
        }
        // Substring / Name match
        self.catalog.stations.iter().find(|s| s.name.to_lowercase().contains(&q_lower))
    }

    /// Toggles the favorite status of a station.
    pub fn toggle_favorite(&mut self, station_id: &str) -> bool {
        if let Some(s) = self.catalog.stations.iter_mut().find(|s| s.id == station_id) {
            s.favorite = !s.favorite;
            let fav = s.favorite;
            let _ = self.save_stations();
            fav
        } else {
            false
        }
    }

    /// Adds a new station to the catalog and persists it.
    pub fn add_station(&mut self, station: Station) -> Result<()> {
        if let Some(pos) = self.catalog.stations.iter().position(|s| s.id == station.id) {
            self.catalog.stations[pos] = station;
        } else {
            self.catalog.stations.push(station);
        }
        self.save_stations()
    }
}
