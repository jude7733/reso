use crate::error::{ResoError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Information about a detected ALSA sound card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlsaCard {
    pub index: u32,
    pub id: String,
    pub name: String,
    pub is_usb: bool,
}

/// Active hardware parameters read from `/proc/asound/card<X>/pcm0p/sub0/hw_params`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AlsaHwParams {
    pub is_active: bool,
    pub access: Option<String>,
    pub format: Option<String>,
    pub channels: Option<u32>,
    pub rate: Option<u32>,
    pub period_size: Option<u32>,
    pub buffer_size: Option<u32>,
}

/// Supported audio capability profile parsed from `/proc/asound/card<X>/stream0`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DacCapabilities {
    pub card_name: String,
    pub status: String,
    pub momentary_freq_hz: Option<u32>,
    pub supported_rates: BTreeSet<u32>,
    pub supported_bit_depths: BTreeSet<u8>,
    pub supported_formats: BTreeSet<String>,
}

/// Scans `/proc/asound/cards` to list all available ALSA sound cards.
pub fn scan_alsa_cards() -> Result<Vec<AlsaCard>> {
    let path = Path::new("/proc/asound/cards");
    if !path.exists() {
        return Err(ResoError::Alsa("ALSA /proc/asound/cards not found".to_string()));
    }

    let content = fs::read_to_string(path)?;
    let mut cards = Vec::new();

    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if line.is_empty() {
            i += 1;
            continue;
        }

        // Example line: " 1 [DSP            ]: USB-Audio - Chu2 DSP"
        if let Some((idx_str, rest)) = line.split_once('[') {
            if let Ok(index) = idx_str.trim().parse::<u32>() {
                if let Some((id_str, desc_str)) = rest.split_once(']') {
                    let id = id_str.trim().to_string();
                    let desc = desc_str.trim_start_matches(':').trim().to_string();
                    let is_usb = desc.contains("USB") || id.contains("DSP");

                    let mut full_name = desc;
                    if i + 1 < lines.len() && !lines[i + 1].trim().starts_with(|c: char| c.is_ascii_digit()) {
                        let next_line = lines[i + 1].trim();
                        if !next_line.is_empty() {
                            full_name = format!("{} ({})", full_name, next_line);
                        }
                        i += 1;
                    }

                    cards.push(AlsaCard {
                        index,
                        id,
                        name: full_name,
                        is_usb,
                    });
                }
            }
        }
        i += 1;
    }

    Ok(cards)
}

/// Reads the active hardware parameters for a given card index.
pub fn read_hw_params(card_index: u32) -> Result<AlsaHwParams> {
    // Try pcm0p, pcm1p, pcm2p
    for pcm in 0..4 {
        let path = PathBuf::from(format!("/proc/asound/card{}/pcm{}p/sub0/hw_params", card_index, pcm));
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if content.contains("closed") || content.trim().is_empty() {
                    continue;
                }
                return Ok(parse_hw_params_content(&content));
            }
        }
    }

    Ok(AlsaHwParams {
        is_active: false,
        ..Default::default()
    })
}

/// Parses the contents of `/proc/asound/card<X>/pcm<Y>p/sub0/hw_params`.
pub fn parse_hw_params_content(content: &str) -> AlsaHwParams {
    let mut params = AlsaHwParams {
        is_active: true,
        ..Default::default()
    };

    for line in content.lines() {
        if let Some((key, val)) = line.split_once(':') {
            let key = key.trim();
            let val = val.trim();
            match key {
                "access" => params.access = Some(val.to_string()),
                "format" => params.format = Some(val.to_string()),
                "channels" => params.channels = val.parse().ok(),
                "rate" => {
                    // Example: "44100 (44100/1)"
                    let rate_str = val.split_whitespace().next().unwrap_or(val);
                    params.rate = rate_str.parse().ok();
                }
                "period_size" => params.period_size = val.parse().ok(),
                "buffer_size" => params.buffer_size = val.parse().ok(),
                _ => {}
            }
        }
    }

    params
}

/// Reads and parses the DAC capability matrix from `/proc/asound/card<X>/stream0`.
pub fn read_dac_capabilities(card_index: u32) -> Result<DacCapabilities> {
    let path = PathBuf::from(format!("/proc/asound/card{}/stream0", card_index));
    if !path.exists() {
        return Err(ResoError::Alsa(format!(
            "stream0 descriptor not found for card {}",
            card_index
        )));
    }

    let content = fs::read_to_string(&path)?;
    Ok(parse_stream0_content(&content))
}

/// Parses USB audio descriptor from `/proc/asound/card<X>/stream0`.
pub fn parse_stream0_content(content: &str) -> DacCapabilities {
    let mut caps = DacCapabilities::default();
    let mut in_playback = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Card header
        if caps.card_name.is_empty() && trimmed.contains("at usb-") {
            caps.card_name = trimmed.to_string();
            continue;
        }

        if trimmed.starts_with("Playback:") {
            in_playback = true;
            continue;
        } else if trimmed.starts_with("Capture:") {
            in_playback = false;
            continue;
        }

        if in_playback {
            if trimmed.starts_with("Status:") {
                caps.status = trimmed.trim_start_matches("Status:").trim().to_string();
            } else if trimmed.starts_with("Momentary freq =") {
                // Example: "Momentary freq = 44100 Hz (0x2c.199a)"
                if let Some(freq_part) = trimmed.strip_prefix("Momentary freq =") {
                    let first_token = freq_part.split_whitespace().next().unwrap_or("");
                    if let Ok(hz) = first_token.parse::<u32>() {
                        caps.momentary_freq_hz = Some(hz);
                    }
                }
            } else if trimmed.starts_with("Format:") {
                let fmt = trimmed.trim_start_matches("Format:").trim().to_string();
                caps.supported_formats.insert(fmt.clone());
                if fmt.contains("24") || fmt.contains("S24") {
                    caps.supported_bit_depths.insert(24);
                } else if fmt.contains("16") || fmt.contains("S16") {
                    caps.supported_bit_depths.insert(16);
                } else if fmt.contains("32") || fmt.contains("S32") {
                    caps.supported_bit_depths.insert(32);
                }
            } else if trimmed.starts_with("Bits:") {
                if let Ok(b) = trimmed.trim_start_matches("Bits:").trim().parse::<u8>() {
                    caps.supported_bit_depths.insert(b);
                }
            } else if trimmed.starts_with("Rates:") {
                // Example: "Rates: 44100, 48000, 88200, 96000"
                let rates_str = trimmed.trim_start_matches("Rates:").trim();
                for r in rates_str.split(',') {
                    if let Ok(rate) = r.trim().parse::<u32>() {
                        caps.supported_rates.insert(rate);
                    }
                }
            }
        }
    }

    caps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hw_params() {
        let raw = r#"
access: MMAP_INTERLEAVED
format: S24_3LE
subformat: STD
channels: 2
rate: 44100 (44100/1)
period_size: 512
buffer_size: 32768
"#;
        let params = parse_hw_params_content(raw);
        assert!(params.is_active);
        assert_eq!(params.format, Some("S24_3LE".to_string()));
        assert_eq!(params.channels, Some(2));
        assert_eq!(params.rate, Some(44100));
        assert_eq!(params.period_size, Some(512));
    }

    #[test]
    fn test_parse_stream0() {
        let raw = r#"
KTMicro Chu2 DSP at usb-0000:00:14.0-9, full speed : USB Audio

Playback:
  Status: Running
    Interface = 2
    Altset = 2
    Packet Size = 402
    Momentary freq = 44100 Hz (0x2c.199a)
  Interface 2
    Altset 1
    Format: S16_LE
    Channels: 2
    Rates: 44100, 48000, 88200, 96000
    Bits: 16
  Interface 2
    Altset 2
    Format: S24_3LE
    Channels: 2
    Rates: 44100, 88200, 48000, 96000
    Bits: 24
"#;
        let caps = parse_stream0_content(raw);
        assert_eq!(caps.status, "Running");
        assert_eq!(caps.momentary_freq_hz, Some(44100));
        assert!(caps.supported_rates.contains(&44100));
        assert!(caps.supported_rates.contains(&48000));
        assert!(caps.supported_rates.contains(&88200));
        assert!(caps.supported_rates.contains(&96000));
        assert!(caps.supported_bit_depths.contains(&16));
        assert!(caps.supported_bit_depths.contains(&24));
    }
}
