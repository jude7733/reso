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
    /// Returns default audiophile lossless stations and curated high-quality streams.
    pub fn default_lossless_stations() -> Self {
        Self {
            stations: vec![
                // --- Radio Paradise Mixes (Lossless FLAC) ---
                Station {
                    id: "rp-main".to_string(),
                    name: "Radio Paradise (Main Mix)".to_string(),
                    url: "http://stream.radioparadise.com/flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://radioparadise.com".to_string()),
                    tags: vec![
                        "eclectic".to_string(),
                        "lossless".to_string(),
                        "audiophile".to_string(),
                    ],
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
                    tags: vec![
                        "mellow".to_string(),
                        "acoustic".to_string(),
                        "lossless".to_string(),
                    ],
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
                    tags: vec![
                        "world".to_string(),
                        "eclectic".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: Some(3),
                },
                // --- Czech Radio / Český rozhlas (Lossless FLAC) ---
                Station {
                    id: "czech-d-dur".to_string(),
                    name: "Czech Radio D-Dur (Hi-Res Classical)".to_string(),
                    url: "http://amp.cesnet.cz:8000/cro-d-dur.flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 48000,
                    bit_depth: 24,
                    homepage: Some("https://d-dur.rozhlas.cz".to_string()),
                    tags: vec![
                        "classical".to_string(),
                        "hi-res".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: true,
                    rp_channel: None,
                },
                Station {
                    id: "czech-jazz".to_string(),
                    name: "Czech Radio Jazz (FLAC)".to_string(),
                    url: "http://amp.cesnet.cz:8000/cro-jazz.flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 48000,
                    bit_depth: 24,
                    homepage: Some("https://jazz.rozhlas.cz".to_string()),
                    tags: vec![
                        "jazz".to_string(),
                        "hi-res".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: true,
                    rp_channel: None,
                },
                Station {
                    id: "czech-vltava".to_string(),
                    name: "Czech Radio Vltava (Culture/Classical FLAC)".to_string(),
                    url: "http://amp.cesnet.cz:8000/cro3.flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 48000,
                    bit_depth: 24,
                    homepage: Some("https://vltava.rozhlas.cz".to_string()),
                    tags: vec![
                        "classical".to_string(),
                        "culture".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "czech-wave".to_string(),
                    name: "Czech Radio Wave (Alternative/Indie FLAC)".to_string(),
                    url: "http://amp.cesnet.cz:8000/cro-radio-wave.flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 48000,
                    bit_depth: 24,
                    homepage: Some("https://wave.rozhlas.cz".to_string()),
                    tags: vec![
                        "indie".to_string(),
                        "alternative".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                // --- Sector Radio (Lossless FLAC) ---
                Station {
                    id: "sector-space".to_string(),
                    name: "Sector Radio (Space FLAC)".to_string(),
                    url: "http://89.223.45.5:8000/space-flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://sectorradio.ru".to_string()),
                    tags: vec![
                        "ambient".to_string(),
                        "space".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: true,
                    rp_channel: None,
                },
                Station {
                    id: "sector-nota".to_string(),
                    name: "Sector Radio (Classical Nota FLAC)".to_string(),
                    url: "http://89.223.45.5:8000/nota-flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://sectorradio.ru".to_string()),
                    tags: vec!["classical".to_string(), "lossless".to_string()],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "sector-80s".to_string(),
                    name: "Sector Radio (80s Geny FLAC)".to_string(),
                    url: "http://89.223.45.5:8000/geny-flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://sectorradio.ru".to_string()),
                    tags: vec![
                        "80s".to_string(),
                        "retro".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "sector-10s".to_string(),
                    name: "Sector Radio (10s Zed FLAC)".to_string(),
                    url: "http://89.223.45.5:8000/zed-flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://sectorradio.ru".to_string()),
                    tags: vec![
                        "electronic".to_string(),
                        "pop".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "sector-jazz".to_string(),
                    name: "Sector Radio (Hi-Res Jazz FLAC 96kHz)".to_string(),
                    url: "http://89.223.45.5:8000/jazz-flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 96000,
                    bit_depth: 24,
                    homepage: Some("https://sectorradio.ru".to_string()),
                    tags: vec![
                        "jazz".to_string(),
                        "hi-res".to_string(),
                        "96khz".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: true,
                    rp_channel: None,
                },
                // --- Audiophile Lossless & Hi-Res Favorites ---
                Station {
                    id: "radio-calico".to_string(),
                    name: "Radio Calico (Hi-Res 24-bit/48kHz)".to_string(),
                    url: "https://stream.radio-calico.com/calico".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 48000,
                    bit_depth: 24,
                    homepage: Some("https://radio-calico.com".to_string()),
                    tags: vec![
                        "audiophile".to_string(),
                        "hi-res".to_string(),
                        "rock".to_string(),
                        "pop".to_string(),
                    ],
                    favorite: true,
                    rp_channel: None,
                },
                Station {
                    id: "le-bon-mix".to_string(),
                    name: "Le Bon Mix (HiFi FLAC)".to_string(),
                    url: "https://stream10.xdevel.com/audio17s976748-2218/stream/icecast.audio"
                        .to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://lebonmix.radio".to_string()),
                    tags: vec![
                        "eclectic".to_string(),
                        "audiophile".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: true,
                    rp_channel: None,
                },
                Station {
                    id: "blues-flac".to_string(),
                    name: "Radio BluesFlac (Lossless)".to_string(),
                    url: "https://streams.radiomast.io/radioblues-flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("http://bluesflac.com".to_string()),
                    tags: vec![
                        "blues".to_string(),
                        "lossless".to_string(),
                        "audiophile".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "naim-radio".to_string(),
                    name: "Naim Radio (FLAC)".to_string(),
                    url: "http://mscp3.live-streams.nl:8360/flac.flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://naimaudio.com".to_string()),
                    tags: vec![
                        "audiophile".to_string(),
                        "eclectic".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "naim-jazz".to_string(),
                    name: "Naim Jazz (FLAC)".to_string(),
                    url: "http://mscp3.live-streams.nl:8340/jazz-flac.flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://naimaudio.com".to_string()),
                    tags: vec![
                        "jazz".to_string(),
                        "audiophile".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "naim-classical".to_string(),
                    name: "Naim Classical (FLAC)".to_string(),
                    url: "http://mscp3.live-streams.nl:8250/class-flac.flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://naimaudio.com".to_string()),
                    tags: vec![
                        "classical".to_string(),
                        "audiophile".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "sveriges-p2".to_string(),
                    name: "Sveriges Radio P2 (Classical FLAC)".to_string(),
                    url: "https://live1.sr.se/p2-flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 48000,
                    bit_depth: 24,
                    homepage: Some("https://sverigesradio.se".to_string()),
                    tags: vec![
                        "classical".to_string(),
                        "sweden".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "rondo-klasu-pro".to_string(),
                    name: "Rondo Klasu Pro (Classical FLAC)".to_string(),
                    url: "https://iradio.fi/klasupro.flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://rondo.fi".to_string()),
                    tags: vec![
                        "classical".to_string(),
                        "finland".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "smoothjazz-pl".to_string(),
                    name: "SmoothJazz.com.pl (FLAC)".to_string(),
                    url: "https://bcast.vigormultimedia.com:48888/sjcomplflac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://smoothjazz.com.pl".to_string()),
                    tags: vec![
                        "smoothjazz".to_string(),
                        "jazz".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "hionline-pop".to_string(),
                    name: "Hi On Line Radio (FLAC)".to_string(),
                    url: "http://mscp2.live-streams.nl:8100/flac.flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://hionline.eu".to_string()),
                    tags: vec![
                        "pop".to_string(),
                        "audiophile".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "dance-wave-flac".to_string(),
                    name: "Dance Wave (FLAC)".to_string(),
                    url: "http://dancewave.online/dance.flac.ogg".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://dancewave.online".to_string()),
                    tags: vec![
                        "dance".to_string(),
                        "electronic".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "dance-wave-retro".to_string(),
                    name: "Dance Wave Retro (FLAC)".to_string(),
                    url: "http://retro.dancewave.online/retrodance.flac.ogg".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://dancewave.online".to_string()),
                    tags: vec![
                        "eurodance".to_string(),
                        "90s".to_string(),
                        "retro".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "radio-bias".to_string(),
                    name: "Radio Bias (80s / Italo FLAC)".to_string(),
                    url: "https://admin.biasradio.com/radio/8000/flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 48000,
                    bit_depth: 24,
                    homepage: Some("https://biasradio.com".to_string()),
                    tags: vec![
                        "80s".to_string(),
                        "newwave".to_string(),
                        "italo".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "radio-sputnik".to_string(),
                    name: "Radio Sputnik (Underground House FLAC)".to_string(),
                    url: "https://radiosputnik.nl:8443/flac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("http://radiosputnik.nl".to_string()),
                    tags: vec![
                        "house".to_string(),
                        "electronic".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "sanctuary-electro".to_string(),
                    name: "Sanctuary Radio (Dark Electro FLAC)".to_string(),
                    url: "http://66.94.113.88:8000/ultra_hq".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://sanctuaryradio.com".to_string()),
                    tags: vec![
                        "electro".to_string(),
                        "industrial".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "sanctuary-80s".to_string(),
                    name: "Sanctuary Radio (80s Alternative FLAC)".to_string(),
                    url: "http://66.94.113.88:8000/retro_ultra_hq".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://sanctuaryradio.com".to_string()),
                    tags: vec![
                        "80s".to_string(),
                        "alternative".to_string(),
                        "newwave".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "pure-lounge".to_string(),
                    name: "Pure Lounge Radio (FLAC)".to_string(),
                    url: "https://mscp4.live-streams.nl:8142/lounge.ogg".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://pureloungeradio.com".to_string()),
                    tags: vec![
                        "lounge".to_string(),
                        "chillout".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "pure-classix".to_string(),
                    name: "PureClassix Radio (FLAC)".to_string(),
                    url: "https://mscp4.live-streams.nl:8142/flac.ogg".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://pureclassix.com".to_string()),
                    tags: vec![
                        "classic".to_string(),
                        "oldies".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "sing-sing".to_string(),
                    name: "Sing Sing (FLAC)".to_string(),
                    url: "http://stream.sing-sing-bis.org:8000/singsingFlac".to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("http://sing-sing-bis.org".to_string()),
                    tags: vec!["eclectic".to_string(), "lossless".to_string()],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "djam-radio".to_string(),
                    name: "Djam Radio (FLAC)".to_string(),
                    url: "https://stream10.xdevel.com/audio15s976748-2280/stream/icecast.audio"
                        .to_string(),
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://djam.radio".to_string()),
                    tags: vec![
                        "eclectic".to_string(),
                        "pop".to_string(),
                        "lossless".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                // --- Radio France / FIP (High-Bitrate AAC) ---
                Station {
                    id: "fip-paris".to_string(),
                    name: "FIP Radio Paris".to_string(),
                    url: "https://icecast.radiofrance.fr/fip-hifi.aac".to_string(),
                    codec: "AAC".to_string(),
                    sample_rate: 48000,
                    bit_depth: 16,
                    homepage: Some("https://www.radiofrance.fr/fip".to_string()),
                    tags: vec![
                        "jazz".to_string(),
                        "eclectic".to_string(),
                        "france".to_string(),
                    ],
                    favorite: true,
                    rp_channel: None,
                },
                Station {
                    id: "fip-jazz".to_string(),
                    name: "FIP Jazz".to_string(),
                    url: "https://icecast.radiofrance.fr/fipjazz-hifi.aac".to_string(),
                    codec: "AAC".to_string(),
                    sample_rate: 48000,
                    bit_depth: 16,
                    homepage: Some("https://www.radiofrance.fr/fip".to_string()),
                    tags: vec!["jazz".to_string(), "france".to_string()],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "fip-groove".to_string(),
                    name: "FIP Groove".to_string(),
                    url: "https://icecast.radiofrance.fr/fipgroove-hifi.aac".to_string(),
                    codec: "AAC".to_string(),
                    sample_rate: 48000,
                    bit_depth: 16,
                    homepage: Some("https://www.radiofrance.fr/fip".to_string()),
                    tags: vec!["funk".to_string(), "soul".to_string(), "groove".to_string()],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "fip-electro".to_string(),
                    name: "FIP Electro".to_string(),
                    url: "https://icecast.radiofrance.fr/fipelectro-hifi.aac".to_string(),
                    codec: "AAC".to_string(),
                    sample_rate: 48000,
                    bit_depth: 16,
                    homepage: Some("https://www.radiofrance.fr/fip".to_string()),
                    tags: vec!["electronic".to_string(), "downtempo".to_string()],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "fip-rock".to_string(),
                    name: "FIP Rock".to_string(),
                    url: "https://icecast.radiofrance.fr/fiprock-hifi.aac".to_string(),
                    codec: "AAC".to_string(),
                    sample_rate: 48000,
                    bit_depth: 16,
                    homepage: Some("https://www.radiofrance.fr/fip".to_string()),
                    tags: vec!["rock".to_string(), "indie".to_string()],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "france-musique".to_string(),
                    name: "France Musique (Classical/Jazz)".to_string(),
                    url: "https://icecast.radiofrance.fr/francemusique-hifi.aac".to_string(),
                    codec: "AAC".to_string(),
                    sample_rate: 48000,
                    bit_depth: 16,
                    homepage: Some("https://www.radiofrance.fr/francemusique".to_string()),
                    tags: vec![
                        "classical".to_string(),
                        "jazz".to_string(),
                        "france".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                // --- SomaFM Networks (High Bitrate 256k) ---
                Station {
                    id: "somafm-groovesalad".to_string(),
                    name: "SomaFM Groove Salad".to_string(),
                    url: "https://ice1.somafm.com/groovesalad-256-mp3".to_string(),
                    codec: "MP3".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://somafm.com/groovesalad".to_string()),
                    tags: vec![
                        "ambient".to_string(),
                        "downtempo".to_string(),
                        "chillout".to_string(),
                    ],
                    favorite: true,
                    rp_channel: None,
                },
                Station {
                    id: "somafm-dronezone".to_string(),
                    name: "SomaFM Drone Zone".to_string(),
                    url: "https://ice1.somafm.com/dronezone-256-mp3".to_string(),
                    codec: "MP3".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://somafm.com/dronezone".to_string()),
                    tags: vec![
                        "ambient".to_string(),
                        "drone".to_string(),
                        "space".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "somafm-defcon".to_string(),
                    name: "SomaFM DEF CON Radio".to_string(),
                    url: "https://ice1.somafm.com/defcon-256-mp3".to_string(),
                    codec: "MP3".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://somafm.com/defcon".to_string()),
                    tags: vec![
                        "electronic".to_string(),
                        "hacker".to_string(),
                        "cyberpunk".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "somafm-synphaera".to_string(),
                    name: "SomaFM Synphaera Radio".to_string(),
                    url: "https://ice1.somafm.com/synphaera-256-mp3".to_string(),
                    codec: "MP3".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://somafm.com/synphaera".to_string()),
                    tags: vec![
                        "space".to_string(),
                        "ambient".to_string(),
                        "electronic".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                // --- Iconic Cultural & Community Broadcasters ---
                Station {
                    id: "kexp-seattle".to_string(),
                    name: "KEXP 90.3 Seattle".to_string(),
                    url: "https://kexp.streamguys1.com/kexp160.aac".to_string(),
                    codec: "AAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://kexp.org".to_string()),
                    tags: vec![
                        "indie".to_string(),
                        "alternative".to_string(),
                        "seattle".to_string(),
                    ],
                    favorite: true,
                    rp_channel: None,
                },
                Station {
                    id: "nts-1".to_string(),
                    name: "NTS Radio (Channel 1)".to_string(),
                    url: "https://stream-relay-geo.ntslive.net/stream".to_string(),
                    codec: "MP3".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://nts.live".to_string()),
                    tags: vec![
                        "underground".to_string(),
                        "eclectic".to_string(),
                        "london".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "nts-2".to_string(),
                    name: "NTS Radio (Channel 2)".to_string(),
                    url: "https://stream-relay-geo.ntslive.net/stream2".to_string(),
                    codec: "MP3".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://nts.live".to_string()),
                    tags: vec![
                        "underground".to_string(),
                        "experimental".to_string(),
                        "london".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "kcrw-e24".to_string(),
                    name: "KCRW Eclectic24".to_string(),
                    url: "https://streams.kcrw.com/e24_mp3".to_string(),
                    codec: "MP3".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://kcrw.com".to_string()),
                    tags: vec![
                        "eclectic".to_string(),
                        "indie".to_string(),
                        "la".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "wfmu-main".to_string(),
                    name: "WFMU 91.1 Freeform".to_string(),
                    url: "https://stream0.wfmu.org/freeform-128k.mp3".to_string(),
                    codec: "MP3".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://wfmu.org".to_string()),
                    tags: vec![
                        "freeform".to_string(),
                        "indie".to_string(),
                        "jersey-city".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
                Station {
                    id: "dublab-la".to_string(),
                    name: "dublab Los Angeles".to_string(),
                    url: "https://dublab.out.airtime.pro/dublab_a".to_string(),
                    codec: "MP3".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: Some("https://dublab.com".to_string()),
                    tags: vec![
                        "future-roots".to_string(),
                        "eclectic".to_string(),
                        "la".to_string(),
                    ],
                    favorite: false,
                    rp_channel: None,
                },
            ],
        }
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
                // Mother Earth Radio ceased broadcasting; replace with Radio Calico (24-bit/48kHz)
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

        // 2. Append newly added default stations if not already present in the catalog
        for def_st in defaults.stations {
            if !self.stations.iter().any(|s| s.id == def_st.id) {
                self.stations.push(def_st);
                modified = true;
            }
        }

        modified
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
            let content = toml::to_string_pretty(&default_cfg).map_err(ResoError::TomlSer)?;
            fs::write(&config_file, content)?;
            default_cfg
        };

        let stations_file = dir.join("stations.toml");
        let (catalog, needs_save) = if stations_file.exists() {
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

        if needs_save {
            let serialized = toml::to_string_pretty(&catalog).map_err(ResoError::TomlSer)?;
            fs::write(&stations_file, serialized)?;
        }

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
        let album_str = album
            .map(|a| format!(" | Album: *{}*", a))
            .unwrap_or_default();
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

    /// Adds a new station to the catalog and persists it.
    pub fn add_station(&mut self, station: Station) -> Result<()> {
        if let Some(pos) = self
            .catalog
            .stations
            .iter()
            .position(|s| s.id == station.id)
        {
            self.catalog.stations[pos] = station;
        } else {
            self.catalog.stations.push(station);
        }
        self.save_stations()
    }
}
