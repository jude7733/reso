//! General utilities for audio formatting, paths, time conversions, and string sanitization.

use directories::{ProjectDirs, UserDirs};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Returns the configuration directory for `reso` (`~/.config/reso`).
pub fn config_dir() -> PathBuf {
    if let Some(proj_dirs) = ProjectDirs::from("com", "reso", "reso") {
        proj_dirs.config_dir().to_path_buf()
    } else {
        PathBuf::from("/home/jude/.config/reso")
    }
}

/// Returns the default recording directory (`~/Music/Radio`).
pub fn default_record_dir() -> PathBuf {
    if let Some(user_dirs) = UserDirs::new() {
        if let Some(audio_dir) = user_dirs.audio_dir() {
            return audio_dir.join("Radio");
        }
    }
    PathBuf::from("/home/jude/Music/Radio")
}

/// Returns the default Unix domain socket path for MPV IPC.
///
/// Prefers `$XDG_RUNTIME_DIR/reso-mpv.sock` and falls back to `/tmp/reso-mpv.sock`.
pub fn default_socket_path() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        Path::new(&runtime_dir).join("reso-mpv.sock")
    } else {
        std::env::temp_dir().join("reso-mpv.sock")
    }
}

/// Expands a leading `~` or `~/` to the current user's home directory.
pub fn expand_home_dir(path: &str) -> PathBuf {
    if path == "~" {
        return UserDirs::new()
            .map(|u| u.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(user_dirs) = UserDirs::new() {
            return user_dirs.home_dir().join(rest);
        }
    }
    PathBuf::from(path)
}

/// Returns the default pywal colors path (`~/.cache/wal/colors.json`).
pub fn default_wal_cache_path() -> PathBuf {
    expand_home_dir("~/.cache/wal/colors.json")
}

/// Formats a sample rate in Hertz to a human-readable string (e.g., `44.1 kHz`, `96.0 kHz`).
pub fn format_sample_rate(hz: u32) -> String {
    if hz == 0 {
        return "Unknown".to_string();
    }
    if hz % 1000 == 0 {
        format!("{}.0 kHz", hz / 1000)
    } else {
        format!("{:.1} kHz", hz as f64 / 1000.0)
    }
}

/// Formats a bitrate in bps or kbps to a human-readable string (e.g., `920 kbps`, `1.4 Mbps`).
pub fn format_bitrate(kbps: u32) -> String {
    if kbps == 0 {
        return "Lossless / VBR".to_string();
    }
    if kbps >= 1000 {
        format!("{:.1} Mbps", kbps as f64 / 1000.0)
    } else {
        format!("{} kbps", kbps)
    }
}

/// Formats seconds into `MM:SS` or `HH:MM:SS`.
pub fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, secs)
    } else {
        format!("{:02}:{:02}", minutes, secs)
    }
}

/// Parses human duration strings like `45m`, `1h30m`, `30s`, or raw seconds `120`.
pub fn parse_duration_str(s: &str) -> Option<Duration> {
    let trimmed = s.trim();
    if let Ok(secs) = trimmed.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    humantime::parse_duration(trimmed).ok()
}

/// Sanitizes a string for use in safe file system paths.
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Truncates a string to max_len characters with ellipsis if needed.
pub fn truncate_str(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
        format!("{}…", truncated)
    }
}

/// Normalizes a radio stream URL for duplicate checking and identification
/// by trimming protocol (http/https), trailing slashes, and whitespace.
pub fn normalize_stream_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    if let Some(stripped) = trimmed.strip_prefix("https://") {
        stripped.to_string()
    } else if let Some(stripped) = trimmed.strip_prefix("http://") {
        stripped.to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_stream_url() {
        assert_eq!(
            normalize_stream_url("https://stream.radioparadise.com/flac/"),
            "stream.radioparadise.com/flac"
        );
        assert_eq!(
            normalize_stream_url("http://stream.radioparadise.com/flac"),
            "stream.radioparadise.com/flac"
        );
        assert_eq!(
            normalize_stream_url("stream.radioparadise.com/flac"),
            "stream.radioparadise.com/flac"
        );
    }

    #[test]
    fn test_format_sample_rate() {
        assert_eq!(format_sample_rate(44100), "44.1 kHz");
        assert_eq!(format_sample_rate(48000), "48.0 kHz");
        assert_eq!(format_sample_rate(88200), "88.2 kHz");
        assert_eq!(format_sample_rate(96000), "96.0 kHz");
        assert_eq!(format_sample_rate(192000), "192.0 kHz");
        assert_eq!(format_sample_rate(0), "Unknown");
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(
            parse_duration_str("45m"),
            Some(Duration::from_secs(45 * 60))
        );
        assert_eq!(parse_duration_str("1h"), Some(Duration::from_secs(3600)));
        assert_eq!(parse_duration_str("30s"), Some(Duration::from_secs(30)));
        assert_eq!(parse_duration_str("120"), Some(Duration::from_secs(120)));
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(
            sanitize_filename("Radio Paradise / Main Mix: FLAC (Hi-Res)"),
            "Radio Paradise _ Main Mix_ FLAC _Hi-Res_"
        );
    }
}
