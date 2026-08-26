//! Asynchronous audio stream prober and endpoint inspector for internet radio.
//!
//! Connects to radio endpoint URLs, requests ICY metadata, reads initial audio packet
//! headers (FLAC, Ogg, AAC, MP3), and extracts precise codec, sample rate, bit depth, and station info.

use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Result of probing an audio stream endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamProbeResult {
    /// Resolved streaming URL.
    pub url: String,

    /// Inferred or metadata-extracted station name.
    pub suggested_name: String,

    /// Audio codec (e.g. `FLAC`, `AAC`, `MP3`, `OGG`, `OPUS`).
    pub codec: String,

    /// Sample rate in Hz (e.g. 44100, 48000, 96000, 192000).
    pub sample_rate: u32,

    /// Audio bit depth (e.g. 16, 24).
    pub bit_depth: u8,

    /// Bitrate in kbps if available.
    pub bitrate_kbps: Option<u32>,

    /// Genre or station tags.
    pub tags: Vec<String>,

    /// Whether this stream is audiophile lossless.
    pub is_lossless: bool,

    /// MIME content type reported by HTTP headers.
    pub content_type: String,

    /// Homepage or station URL if reported by ICY headers.
    pub homepage: Option<String>,
}

/// Audio stream endpoint prober.
pub struct StreamProber;

impl StreamProber {
    /// Probes a radio endpoint URL by performing an async HTTP handshake and packet inspection.
    pub async fn probe_endpoint(url: &str) -> Result<StreamProbeResult, String> {
        let clean_url = url.trim();
        if clean_url.is_empty() {
            return Err("Stream URL cannot be empty".to_string());
        }

        if !clean_url.starts_with("http://") && !clean_url.starts_with("https://") {
            return Err("Invalid stream URL (must start with http:// or https://)".to_string());
        }

        // Build HTTP client with reasonable timeout and standard ICY metadata headers
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(6))
            .connect_timeout(Duration::from_secs(4))
            .user_agent("Reso/0.1.0 (Lossless Audiophile Radio Player)")
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        let mut headers = HeaderMap::new();
        headers.insert("Icy-MetaData", HeaderValue::from_static("1"));
        headers.insert(
            "Accept",
            HeaderValue::from_static("audio/*, application/ogg, */*"),
        );

        let response = client
            .get(clean_url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| format!("Connection failed: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            return Err(format!("HTTP Error: {}", status));
        }

        let resp_headers = response.headers().clone();
        let content_type = resp_headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        // Check if response is a playlist (.m3u, .pls)
        let is_playlist = content_type.contains("mpegurl")
            || content_type.contains("scpls")
            || clean_url.ends_with(".m3u")
            || clean_url.ends_with(".m3u8")
            || clean_url.ends_with(".pls");

        if is_playlist {
            if let Ok(body_text) = response.text().await {
                if let Some(first_target) = extract_stream_from_playlist(&body_text) {
                    if first_target != clean_url {
                        return Box::pin(Self::probe_endpoint(&first_target)).await;
                    }
                }
            }
            return Err("Playlist does not contain valid audio stream URLs".to_string());
        }

        // Extract ICY Headers
        let icy_name = resp_headers
            .get("icy-name")
            .or_else(|| resp_headers.get("ice-name"))
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let icy_genre = resp_headers
            .get("icy-genre")
            .or_else(|| resp_headers.get("ice-genre"))
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let icy_br = resp_headers
            .get("icy-br")
            .or_else(|| resp_headers.get("ice-bitrate"))
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u32>().ok());

        let icy_sr = resp_headers
            .get("icy-sr")
            .or_else(|| resp_headers.get("ice-samplerate"))
            .or_else(|| resp_headers.get("icy-samplerate"))
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u32>().ok());

        let icy_url = resp_headers
            .get("icy-url")
            .or_else(|| resp_headers.get("ice-url"))
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        // Stream initial chunk to inspect audio magic bytes
        let mut bytes_chunk = Vec::new();
        let mut response = response;

        while let Ok(Some(chunk)) = response.chunk().await {
            bytes_chunk.extend_from_slice(&chunk);
            if bytes_chunk.len() >= 32768 {
                break;
            }
        }

        // Decode audio payload metadata
        let (codec, sample_rate, bit_depth) =
            inspect_audio_bytes(&bytes_chunk, &content_type, icy_sr);

        let suggested_name = icy_name.unwrap_or_else(|| infer_name_from_url(clean_url));

        let mut tags = Vec::new();
        if let Some(g) = icy_genre {
            for tag in g.split([';', ',', '/']) {
                let trimmed = tag.trim().to_lowercase();
                if !trimmed.is_empty() && !tags.contains(&trimmed) {
                    tags.push(trimmed);
                }
            }
        }

        let is_lossless = codec.to_uppercase().contains("FLAC")
            || codec.to_uppercase().contains("ALAC")
            || (bit_depth >= 16 && sample_rate >= 44100 && codec == "FLAC");

        if is_lossless && !tags.contains(&"lossless".to_string()) {
            tags.push("lossless".to_string());
        }
        if sample_rate >= 88200 && !tags.contains(&"hi-res".to_string()) {
            tags.push("hi-res".to_string());
        }

        Ok(StreamProbeResult {
            url: clean_url.to_string(),
            suggested_name,
            codec,
            sample_rate,
            bit_depth,
            bitrate_kbps: icy_br,
            tags,
            is_lossless,
            content_type,
            homepage: icy_url,
        })
    }
}

/// Inspects audio magic bytes and HTTP Content-Type to determine codec, sample rate, and bit depth.
pub fn inspect_audio_bytes(
    bytes: &[u8],
    content_type: &str,
    hint_sample_rate: Option<u32>,
) -> (String, u32, u8) {
    let ct_lower = content_type.to_lowercase();

    // 1. Direct FLAC Magic Header (`fLaC`) or Ogg FLAC
    if let Some(pos) = find_subsequence(bytes, b"fLaC") {
        // In native FLAC, METADATA_BLOCK_HEADER (4 bytes) follows `fLaC`, then 34-byte STREAMINFO.
        // Byte 0 of header: bit 7 is is_last, bits 0..6 is block_type (0 = STREAMINFO).
        let offset = if bytes.len() >= pos + 8 && (bytes[pos + 4] & 0x7F == 0) {
            pos + 8
        } else {
            pos + 4
        };

        if bytes.len() >= offset + 18 {
            let info = &bytes[offset..];
            let b10 = info[10] as u32;
            let b11 = info[11] as u32;
            let b12 = info[12] as u32;
            let b13 = info[13] as u32;

            let sr = (b10 << 12) | (b11 << 4) | (b12 >> 4);
            let bits = (((b12 & 0x01) << 4) | (b13 >> 4)) + 1;

            let valid_sr = if sr > 0 {
                sr
            } else {
                hint_sample_rate.unwrap_or(44100)
            };
            let valid_bits = if bits > 0 && bits <= 32 {
                bits as u8
            } else {
                16
            };

            return ("FLAC".to_string(), valid_sr, valid_bits);
        }
        return ("FLAC".to_string(), hint_sample_rate.unwrap_or(44100), 16);
    }

    // 2. Ogg Container Header (`OggS`)
    if let Some(pos) = find_subsequence(bytes, b"OggS") {
        if find_subsequence(&bytes[pos..], b"\x7fFLAC").is_some() {
            return ("FLAC".to_string(), hint_sample_rate.unwrap_or(44100), 16);
        }
        if find_subsequence(&bytes[pos..], b"OpusHead").is_some() {
            return ("OPUS".to_string(), 48000, 16);
        }
        if let Some(vpos) = find_subsequence(&bytes[pos..], b"\x01vorbis") {
            let abs_vpos = pos + vpos;
            if bytes.len() >= abs_vpos + 16 {
                let sr = u32::from_le_bytes([
                    bytes[abs_vpos + 11],
                    bytes[abs_vpos + 12],
                    bytes[abs_vpos + 13],
                    bytes[abs_vpos + 14],
                ]);
                let valid_sr = if sr > 0 && sr <= 384000 {
                    sr
                } else {
                    hint_sample_rate.unwrap_or(44100)
                };
                return ("OGG".to_string(), valid_sr, 16);
            }
            return ("OGG".to_string(), hint_sample_rate.unwrap_or(44100), 16);
        }
        return ("OGG".to_string(), hint_sample_rate.unwrap_or(44100), 16);
    }

    // 3. AAC (ADTS frame sync 0xFFF) or Content-Type
    if ct_lower.contains("aac") || ct_lower.contains("audio/aac") {
        let sr = hint_sample_rate.unwrap_or(44100);
        return ("AAC".to_string(), sr, 16);
    }

    if bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xF0) == 0xF0 {
        let sr = hint_sample_rate.unwrap_or(44100);
        return ("AAC".to_string(), sr, 16);
    }

    // 4. MP3 (ID3 header or MPEG sync)
    if ct_lower.contains("mpeg") || ct_lower.contains("mp3") || bytes.starts_with(b"ID3") {
        let sr = hint_sample_rate.unwrap_or(44100);
        return ("MP3".to_string(), sr, 16);
    }

    // 5. Fallback based on content type or defaults
    if ct_lower.contains("flac") {
        ("FLAC".to_string(), hint_sample_rate.unwrap_or(44100), 16)
    } else if ct_lower.contains("opus") {
        ("OPUS".to_string(), 48000, 16)
    } else {
        ("MP3".to_string(), hint_sample_rate.unwrap_or(44100), 16)
    }
}

/// Helper to search for a byte pattern inside a buffer.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Extracts the first stream URL from an M3U or PLS playlist text.
fn extract_stream_from_playlist(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            return Some(trimmed.to_string());
        }
        if let Some(rest) = trimmed.strip_prefix("File1=") {
            let t = rest.trim();
            if t.starts_with("http://") || t.starts_with("https://") {
                return Some(t.to_string());
            }
        }
    }
    None
}

/// Infers a human-readable station name from a stream URL.
pub fn infer_name_from_url(url: &str) -> String {
    if let Ok(parsed) = reqwest::Url::parse(url) {
        if let Some(host) = parsed.host_str() {
            let mut parts: Vec<&str> = host.split('.').collect();
            while parts.len() > 2 {
                let first = parts[0];
                if first == "www"
                    || first == "stream"
                    || first == "streams"
                    || first.starts_with("stream")
                    || first == "live"
                    || first.starts_with("live")
                    || first == "audio"
                    || first == "icecast"
                    || first == "amp"
                    || first == "bcast"
                    || first == "admin"
                {
                    parts.remove(0);
                } else {
                    break;
                }
            }

            if let Some(main_name) = parts.first() {
                let path = parsed.path().trim_matches('/');
                if !path.is_empty() && path.len() < 24 {
                    return format!("{} ({})", capitalize_words(main_name), path);
                }
                return capitalize_words(main_name);
            }
        }
    }
    "Custom Radio Stream".to_string()
}

fn capitalize_words(s: &str) -> String {
    s.split(['-', '_'])
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_name_from_url() {
        assert_eq!(
            infer_name_from_url("http://stream.radioparadise.com/flac"),
            "Radioparadise (flac)"
        );
        assert_eq!(
            infer_name_from_url("https://live.kexp.org/kexp160.aac"),
            "Kexp (kexp160.aac)"
        );
    }

    #[test]
    fn test_flac_magic_bytes_detection() {
        let mut sample_flac = vec![b'f', b'L', b'a', b'C'];
        // 4-byte METADATA_BLOCK_HEADER (block_type 0, length 34)
        sample_flac.extend_from_slice(&[0x00, 0x00, 0x00, 0x22]);
        sample_flac.extend_from_slice(&[0u8; 34]);
        // Put 48000 Hz in STREAMINFO bytes 10..12: 48000 = 0x0BB80 -> (0x0B << 12) | (0xB8 << 4) | 0
        sample_flac[8 + 10] = 0x0B;
        sample_flac[8 + 11] = 0xB8;
        sample_flac[8 + 12] = 0x00;
        sample_flac[8 + 13] = 0xF0; // 16-bit

        let (codec, sr, bits) = inspect_audio_bytes(&sample_flac, "audio/flac", None);
        assert_eq!(codec, "FLAC");
        assert_eq!(sr, 48000);
        assert_eq!(bits, 16);

        // Test 24-bit 96kHz FLAC: 96000 = 0x17700. 24-bit = 23 -> b12 bit 0 is 1, b13>>4 is 7
        let mut flac_24b = vec![b'f', b'L', b'a', b'C'];
        flac_24b.extend_from_slice(&[0x00, 0x00, 0x00, 0x22]);
        flac_24b.extend_from_slice(&[0u8; 34]);
        flac_24b[8 + 10] = 0x17;
        flac_24b[8 + 11] = 0x70;
        flac_24b[8 + 12] = 0x01; // sample rate lower nibble 0 + bit_depth high bit 1
        flac_24b[8 + 13] = 0x70; // bit_depth lower 4 bits (7)

        let (_, sr24, bits24) = inspect_audio_bytes(&flac_24b, "audio/flac", None);
        assert_eq!(sr24, 96000);
        assert_eq!(bits24, 24);
    }
}
