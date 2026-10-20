//! TUI Theme engine supporting built-in presets, pywal-16 direct JSON, and custom TOML templates.

use crate::util::{default_wal_cache_path, expand_home_dir};
use ratatui::style::Color;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Fully resolved TUI color theme for Reso.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub name: String,

    // Primary UI accents
    pub primary: Color,
    pub secondary: Color,
    pub background: Option<Color>,
    pub foreground: Color,
    pub text_dim: Color,

    // Borders & Navigation
    pub border: Color,
    pub border_active: Color,

    // Selection & Highlighting
    pub selection_bg: Color,
    pub selection_fg: Color,

    // Status Indicators & Badges
    pub status_lossless: Color,
    pub status_warning: Color,
    pub status_danger: Color,

    // Spectrum Visualizer Gradient
    pub spectrum_low: Color,
    pub spectrum_mid: Color,
    pub spectrum_high: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::audiophile_dark()
    }
}

impl Theme {
    /// Reso signature default: Deep slate + Cyan + Emerald + Amber.
    pub fn audiophile_dark() -> Self {
        Self {
            name: "audiophile_dark".to_string(),
            primary: Color::Cyan,
            secondary: Color::Rgb(140, 200, 255),
            background: None,
            foreground: Color::White,
            text_dim: Color::DarkGray,
            border: Color::DarkGray,
            border_active: Color::Cyan,
            selection_bg: Color::Rgb(30, 45, 60),
            selection_fg: Color::White,
            status_lossless: Color::Rgb(0, 180, 80),
            status_warning: Color::Yellow,
            status_danger: Color::Red,
            spectrum_low: Color::Rgb(0, 150, 195),   // Deep ocean teal
            spectrum_mid: Color::Cyan,
            spectrum_high: Color::Rgb(255, 195, 75), // Amber peak glow
        }
    }

    /// Cyberpunk theme: Neon Magenta + Neon Cyan + Electric Yellow.
    pub fn cyberpunk() -> Self {
        Self {
            name: "cyberpunk".to_string(),
            primary: Color::Rgb(255, 0, 128),     // Hot Pink / Magenta
            secondary: Color::Rgb(0, 255, 255),   // Neon Cyan
            background: None,
            foreground: Color::Rgb(240, 240, 255),
            text_dim: Color::Rgb(110, 90, 130),
            border: Color::Rgb(90, 50, 110),
            border_active: Color::Rgb(255, 0, 128),
            selection_bg: Color::Rgb(70, 20, 90),
            selection_fg: Color::Rgb(0, 255, 255),
            status_lossless: Color::Rgb(0, 255, 128),
            status_warning: Color::Rgb(255, 230, 0),
            status_danger: Color::Rgb(255, 40, 40),
            spectrum_low: Color::Rgb(255, 0, 128),
            spectrum_mid: Color::Rgb(0, 255, 255),
            spectrum_high: Color::Rgb(255, 230, 0),
        }
    }

    /// Clean monochrome grayscale theme.
    pub fn monochrome() -> Self {
        Self {
            name: "monochrome".to_string(),
            primary: Color::White,
            secondary: Color::Gray,
            background: None,
            foreground: Color::White,
            text_dim: Color::DarkGray,
            border: Color::DarkGray,
            border_active: Color::White,
            selection_bg: Color::Rgb(50, 50, 50),
            selection_fg: Color::White,
            status_lossless: Color::White,
            status_warning: Color::Gray,
            status_danger: Color::LightRed,
            spectrum_low: Color::DarkGray,
            spectrum_mid: Color::Gray,
            spectrum_high: Color::White,
        }
    }

    /// Loads a theme by name or custom file path, falling back to `audiophile_dark` if missing.
    pub fn load(theme_name: &str, custom_path: Option<&str>) -> Self {
        match theme_name {
            "wal" => {
                let resolved = custom_path
                    .map(expand_home_dir)
                    .unwrap_or_else(default_wal_cache_path);

                if resolved.exists() {
                    match Self::load_from_path(&resolved) {
                        Ok(mut t) => {
                            t.name = "wal".to_string();
                            t
                        }
                        Err(e) => {
                            eprintln!("Failed to load wal theme from {:?}: {}", resolved, e);
                            Self::audiophile_dark()
                        }
                    }
                } else {
                    Self::audiophile_dark()
                }
            }
            "cyberpunk" => Self::cyberpunk(),
            "monochrome" => Self::monochrome(),
            "audiophile_dark" => Self::audiophile_dark(),
            _ => {
                // If custom path is provided, try loading it
                if let Some(path_str) = custom_path {
                    let path = expand_home_dir(path_str);
                    if path.exists() {
                        if let Ok(mut t) = Self::load_from_path(&path) {
                            t.name = theme_name.to_string();
                            return t;
                        }
                    }
                }
                Self::audiophile_dark()
            }
        }
    }

    /// Loads theme from a filesystem path (supports JSON or TOML).
    pub fn load_from_path(path: &Path) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Could not read file {}: {}", path.display(), e))?;

        let is_json = path.extension().and_then(|s| s.to_str()) == Some("json")
            || content.trim_start().starts_with('{');

        if is_json {
            Self::from_wal_json_str(&content)
        } else {
            Self::from_toml_str(&content).or_else(|_| Self::from_wal_json_str(&content))
        }
    }

    /// Parses native pywal / pywal-16 `colors.json`.
    pub fn from_wal_json_str(content: &str) -> Result<Self, String> {
        let wal: WalJson = serde_json::from_str(content)
            .map_err(|e| format!("Invalid pywal JSON format: {}", e))?;

        let colors = wal.colors.unwrap_or_default();
        let special = wal.special.unwrap_or_default();

        let get_color = |key: &str| -> Option<Color> {
            colors.get(key).and_then(|hex| parse_hex_color(hex))
        };

        let bg_color = special
            .background
            .as_deref()
            .and_then(parse_hex_color)
            .or_else(|| get_color("color0"));

        let fg_color = special
            .foreground
            .as_deref()
            .and_then(parse_hex_color)
            .or_else(|| get_color("color15"))
            .unwrap_or(Color::White);

        // Smart ANSI mapping to semantic roles:
        // color4 (Blue) or color6 (Cyan) for primary accent
        let primary = get_color("color4")
            .or_else(|| get_color("color6"))
            .unwrap_or(Color::Cyan);

        // color6 (Cyan) or color14 (Bright Cyan) for secondary
        let secondary = get_color("color6")
            .or_else(|| get_color("color14"))
            .unwrap_or(Color::Rgb(140, 200, 255));

        // color8 (Bright Black / Dark Gray) for inactive borders
        let border = get_color("color8").unwrap_or(Color::DarkGray);

        // color4 / color12 for active border
        let border_active = get_color("color12")
            .or_else(|| get_color("color4"))
            .unwrap_or(primary);

        // Selection background and foreground
        let selection_bg = get_color("color4").unwrap_or(Color::Rgb(30, 45, 60));
        let selection_fg = bg_color.unwrap_or(Color::Black);

        // Status indicators:
        // color2 (Green) = Lossless
        let status_lossless = get_color("color2")
            .or_else(|| get_color("color10"))
            .unwrap_or(Color::Rgb(0, 180, 80));

        // color3 (Yellow) = Warning / Bit-rate badge / Star
        let status_warning = get_color("color3")
            .or_else(|| get_color("color11"))
            .unwrap_or(Color::Yellow);

        // color1 (Red) = Danger / Recording / Errors
        let status_danger = get_color("color1")
            .or_else(|| get_color("color9"))
            .unwrap_or(Color::Red);

        // Spectrum visualizer gradient:
        let spectrum_low = get_color("color2").unwrap_or(Color::Rgb(0, 220, 120));
        let spectrum_mid = primary;
        let spectrum_high = get_color("color5")
            .or_else(|| get_color("color13"))
            .unwrap_or(Color::Rgb(160, 100, 255));

        Ok(Self {
            name: "wal".to_string(),
            primary,
            secondary,
            background: None, // Preserves terminal transparency
            foreground: fg_color,
            text_dim: border,
            border,
            border_active,
            selection_bg,
            selection_fg,
            status_lossless,
            status_warning,
            status_danger,
            spectrum_low,
            spectrum_mid,
            spectrum_high,
        })
    }

    /// Parses a custom TOML theme generated from a pywal template or standalone config.
    pub fn from_toml_str(content: &str) -> Result<Self, String> {
        let toml_file: TomlThemeFile =
            toml::from_str(content).map_err(|e| format!("Invalid TOML theme: {}", e))?;

        let mut base = Self::audiophile_dark();
        if let Some(meta) = toml_file.theme {
            if let Some(n) = meta.name {
                base.name = n;
            }
        }

        let palette = toml_file.palette.unwrap_or_default();
        let resolve_color = |val: &str| -> Option<Color> {
            let clean = val.trim();
            let key = clean.trim_start_matches('{').trim_end_matches('}');
            if let Some(hex) = palette.get(key) {
                parse_hex_color(hex)
            } else if let Some(hex) = palette.get(clean) {
                parse_hex_color(hex)
            } else {
                parse_hex_color(clean)
            }
        };

        if let Some(ui) = toml_file.ui {
            if let Some(c) = ui.primary.as_deref().and_then(&resolve_color) {
                base.primary = c;
            }
            if let Some(c) = ui.secondary.as_deref().and_then(&resolve_color) {
                base.secondary = c;
            }
            if let Some(c) = ui.background.as_deref().and_then(&resolve_color) {
                base.background = Some(c);
            }
            if let Some(c) = ui.foreground.as_deref().and_then(&resolve_color) {
                base.foreground = c;
            }
            if let Some(c) = ui.border.as_deref().and_then(&resolve_color) {
                base.border = c;
                base.text_dim = c;
            }
            if let Some(c) = ui.border_active.as_deref().and_then(&resolve_color) {
                base.border_active = c;
            }
            if let Some(c) = ui.selection_bg.as_deref().and_then(&resolve_color) {
                base.selection_bg = c;
            }
            if let Some(c) = ui.selection_fg.as_deref().and_then(&resolve_color) {
                base.selection_fg = c;
            }
            if let Some(c) = ui.status_lossless.as_deref().and_then(&resolve_color) {
                base.status_lossless = c;
            }
            if let Some(c) = ui.status_warning.as_deref().and_then(&resolve_color) {
                base.status_warning = c;
            }
            if let Some(c) = ui.status_danger.as_deref().and_then(&resolve_color) {
                base.status_danger = c;
            }
            if let Some(c) = ui.spectrum_low.as_deref().and_then(&resolve_color) {
                base.spectrum_low = c;
            }
            if let Some(c) = ui.spectrum_mid.as_deref().and_then(&resolve_color) {
                base.spectrum_mid = c;
            }
            if let Some(c) = ui.spectrum_high.as_deref().and_then(&resolve_color) {
                base.spectrum_high = c;
            }
        } else if !palette.is_empty() {
            // If only [palette] is defined, map pywal colors automatically
            let get_palette = |key: &str| -> Option<Color> {
                palette.get(key).and_then(|s| parse_hex_color(s))
            };

            if let Some(c) = get_palette("color4").or_else(|| get_palette("color6")) {
                base.primary = c;
            }
            if let Some(c) = get_palette("color6") {
                base.secondary = c;
            }
            if let Some(c) = get_palette("background") {
                base.background = Some(c);
            }
            if let Some(c) = get_palette("foreground") {
                base.foreground = c;
            }
            if let Some(c) = get_palette("color8") {
                base.border = c;
                base.text_dim = c;
            }
            if let Some(c) = get_palette("color12").or_else(|| get_palette("color4")) {
                base.border_active = c;
            }
            if let Some(c) = get_palette("color2") {
                base.status_lossless = c;
                base.spectrum_low = c;
            }
            if let Some(c) = get_palette("color3") {
                base.status_warning = c;
            }
            if let Some(c) = get_palette("color1") {
                base.status_danger = c;
            }
        }

        Ok(base)
    }
}

/// Parses hex strings (`#RRGGBB`, `#RGB`, `RRGGBB`) or named colors into `ratatui::style::Color`.
pub fn parse_hex_color(input: &str) -> Option<Color> {
    let clean = input.trim();
    if clean.is_empty() {
        return None;
    }

    // Check named colors
    match clean.to_lowercase().as_str() {
        "black" => return Some(Color::Black),
        "red" => return Some(Color::Red),
        "green" => return Some(Color::Green),
        "yellow" => return Some(Color::Yellow),
        "blue" => return Some(Color::Blue),
        "magenta" => return Some(Color::Magenta),
        "cyan" => return Some(Color::Cyan),
        "gray" => return Some(Color::Gray),
        "darkgray" | "dark_gray" => return Some(Color::DarkGray),
        "lightred" | "light_red" => return Some(Color::LightRed),
        "lightgreen" | "light_green" => return Some(Color::LightGreen),
        "lightyellow" | "light_yellow" => return Some(Color::LightYellow),
        "lightblue" | "light_blue" => return Some(Color::LightBlue),
        "lightmagenta" | "light_magenta" => return Some(Color::LightMagenta),
        "lightcyan" | "light_cyan" => return Some(Color::LightCyan),
        "white" => return Some(Color::White),
        "reset" => return Some(Color::Reset),
        _ => {}
    }

    let hex = clean.strip_prefix('#').unwrap_or(clean);
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color::Rgb(r, g, b))
        }
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            Some(Color::Rgb(r, g, b))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Serde helper models
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct WalJson {
    special: Option<WalSpecial>,
    colors: Option<HashMap<String, String>>,
}

#[derive(Debug, Default, Deserialize)]
struct WalSpecial {
    background: Option<String>,
    foreground: Option<String>,
    #[allow(dead_code)]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TomlThemeFile {
    theme: Option<TomlThemeMeta>,
    palette: Option<HashMap<String, String>>,
    ui: Option<TomlUiMapping>,
}

#[derive(Debug, Deserialize)]
struct TomlThemeMeta {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TomlUiMapping {
    primary: Option<String>,
    secondary: Option<String>,
    background: Option<String>,
    foreground: Option<String>,
    border: Option<String>,
    border_active: Option<String>,
    selection_bg: Option<String>,
    selection_fg: Option<String>,
    status_lossless: Option<String>,
    status_warning: Option<String>,
    status_danger: Option<String>,
    spectrum_low: Option<String>,
    spectrum_mid: Option<String>,
    spectrum_high: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_color() {
        assert_eq!(parse_hex_color("#ff0000"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_hex_color("#00ff00"), Some(Color::Rgb(0, 255, 0)));
        assert_eq!(parse_hex_color("#0000ff"), Some(Color::Rgb(0, 0, 255)));
        assert_eq!(parse_hex_color("#fff"), Some(Color::Rgb(255, 255, 255)));
        assert_eq!(parse_hex_color("cyan"), Some(Color::Cyan));
        assert_eq!(parse_hex_color("invalid"), None);
    }

    #[test]
    fn test_load_wal_json() {
        let json_data = r##"{
            "special": {
                "background": "#121212",
                "foreground": "#e0e0e0"
            },
            "colors": {
                "color0": "#121212",
                "color1": "#ff5555",
                "color2": "#50fa7b",
                "color3": "#f1fa8c",
                "color4": "#bd93f9",
                "color5": "#ff79c6",
                "color6": "#8be9fd",
                "color7": "#bfbfbf",
                "color8": "#4d4d4d",
                "color9": "#ff6e6e",
                "color10": "#69ff94",
                "color11": "#ffffa5",
                "color12": "#d6acff",
                "color13": "#ff92df",
                "color14": "#a4ffff",
                "color15": "#e6e6e6"
            }
        }"##;

        let theme = Theme::from_wal_json_str(json_data).expect("Should parse wal json");
        assert_eq!(theme.name, "wal");
        assert_eq!(theme.background, None);
        assert_eq!(theme.foreground, Color::Rgb(0xe0, 0xe0, 0xe0));
        assert_eq!(theme.primary, Color::Rgb(0xbd, 0x93, 0xf9));
        assert_eq!(theme.status_lossless, Color::Rgb(0x50, 0xfa, 0x7b));
        assert_eq!(theme.status_warning, Color::Rgb(0xf1, 0xfa, 0x8c));
        assert_eq!(theme.status_danger, Color::Rgb(0xff, 0x55, 0x55));
    }

    #[test]
    fn test_load_toml_template() {
        let toml_data = r##"
            [theme]
            name = "custom-wal"

            [palette]
            color1 = "#e06c75"
            color2 = "#98c379"
            color4 = "#61afef"

            [ui]
            primary = "{color4}"
            status_lossless = "{color2}"
            status_danger = "{color1}"
        "##;

        let theme = Theme::from_toml_str(toml_data).expect("Should parse toml theme");
        assert_eq!(theme.name, "custom-wal");
        assert_eq!(theme.primary, Color::Rgb(0x61, 0xaf, 0xef));
        assert_eq!(theme.status_lossless, Color::Rgb(0x98, 0xc3, 0x79));
        assert_eq!(theme.status_danger, Color::Rgb(0xe0, 0x6c, 0x75));
    }
}
