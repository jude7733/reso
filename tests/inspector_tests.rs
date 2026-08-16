use reso::config::StationsCatalog;
use reso::inspector::alsa::{parse_hw_params_content, parse_stream0_content};
use reso::inspector::pipewire::parse_pw_dump_json;
use reso::mpv::client::parse_artist_title;
use reso::util::{format_duration, format_sample_rate, parse_duration_str, sanitize_filename};
use serde_json::json;
use std::time::Duration;

#[test]
fn test_audio_rate_and_format_utils() {
    assert_eq!(format_sample_rate(44100), "44.1 kHz");
    assert_eq!(format_sample_rate(48000), "48.0 kHz");
    assert_eq!(format_sample_rate(88200), "88.2 kHz");
    assert_eq!(format_sample_rate(96000), "96.0 kHz");
    assert_eq!(format_sample_rate(192000), "192.0 kHz");

    assert_eq!(format_duration(65), "01:05");
    assert_eq!(format_duration(3665), "01:01:05");

    assert_eq!(parse_duration_str("45m"), Some(Duration::from_secs(45 * 60)));
    assert_eq!(parse_duration_str("1h30m"), Some(Duration::from_secs(90 * 60)));
    assert_eq!(parse_duration_str("10s"), Some(Duration::from_secs(10)));
}

#[test]
fn test_stream_sanitization_and_artist_parsing() {
    let sanitized = sanitize_filename("Audiophile.fm: Classical / FLAC (96kHz)");
    assert_eq!(sanitized, "Audiophile.fm_ Classical _ FLAC _96kHz_");

    let (a1, t1) = parse_artist_title("Pink Floyd - Comfortably Numb");
    assert_eq!(a1.as_deref(), Some("Pink Floyd"));
    assert_eq!(t1.as_deref(), Some("Comfortably Numb"));

    let (a2, t2) = parse_artist_title("Miles Davis / So What");
    assert_eq!(a2.as_deref(), Some("Miles Davis"));
    assert_eq!(t2.as_deref(), Some("So What"));
}

#[test]
fn test_alsa_hw_params_parsing() {
    let mock_hw = r#"
access: MMAP_INTERLEAVED
format: S24_3LE
subformat: STD
channels: 2
rate: 96000 (96000/1)
period_size: 1024
buffer_size: 65536
"#;
    let params = parse_hw_params_content(mock_hw);
    assert!(params.is_active);
    assert_eq!(params.format.as_deref(), Some("S24_3LE"));
    assert_eq!(params.channels, Some(2));
    assert_eq!(params.rate, Some(96000));
    assert_eq!(params.period_size, Some(1024));
    assert_eq!(params.buffer_size, Some(65536));
}

#[test]
fn test_alsa_stream0_dac_matrix_parsing() {
    let mock_stream0 = r#"
KTMicro Chu2 DSP at usb-0000:00:14.0-9, full speed : USB Audio

Playback:
  Status: Running
    Interface = 2
    Altset = 2
    Packet Size = 402
    Momentary freq = 96000 Hz (0x2c.199a)
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
    Rates: 44100, 48000, 88200, 96000, 192000
    Bits: 24
"#;
    let caps = parse_stream0_content(mock_stream0);
    assert_eq!(caps.status, "Running");
    assert_eq!(caps.momentary_freq_hz, Some(96000));
    assert!(caps.supported_rates.contains(&44100));
    assert!(caps.supported_rates.contains(&96000));
    assert!(caps.supported_rates.contains(&192000));
    assert!(caps.supported_bit_depths.contains(&16));
    assert!(caps.supported_bit_depths.contains(&24));
}

#[test]
fn test_pipewire_culprit_detection() {
    let mock_dump = json!([
        {
            "id": 0,
            "type": "PipeWire:Interface:Core",
            "info": {
                "props": {
                    "default.clock.allowed-rates": "[ 44100 48000 88200 96000 ]",
                    "default.clock.rate": 48000
                }
            }
        },
        {
            "id": 55,
            "type": "PipeWire:Interface:Node",
            "info": {
                "state": "running",
                "props": {
                    "media.class": "Audio/Sink",
                    "node.name": "alsa_output.usb-Chu2_DSP.analog-stereo",
                    "node.description": "Chu2 DSP Analog Stereo"
                },
                "params": {
                    "Format": [
                        {
                            "rate": 48000,
                            "format": "S24LE",
                            "channels": 2
                        }
                    ]
                }
            }
        },
        {
            "id": 90,
            "type": "PipeWire:Interface:Node",
            "info": {
                "state": "running",
                "props": {
                    "media.class": "Stream/Output/Audio",
                    "application.name": "Firefox",
                    "application.process.binary": "firefox",
                    "application.process.id": 93384
                },
                "params": {
                    "Format": [
                        {
                            "rate": 48000,
                            "format": "F32LE",
                            "channels": 2
                        }
                    ]
                }
            }
        }
    ]);

    // Target source rate is 44100, but sink is running at 48000
    let report = parse_pw_dump_json(&mock_dump, Some(44100));
    assert_eq!(report.allowed_clock_rates, vec![44100, 48000, 88200, 96000]);
    assert_eq!(report.culprits.len(), 1);
    assert_eq!(report.culprits[0].name, "Firefox");
    assert_eq!(report.culprits[0].pid, Some(93384));
    assert_eq!(report.culprits[0].locking_rate, 48000);
}

#[test]
fn test_default_lossless_catalog_integrity() {
    let catalog = StationsCatalog::default_lossless_stations();
    assert!(catalog.stations.len() >= 8);

    // Verify Radio Paradise mixes
    let rp_main = catalog.stations.iter().find(|s| s.id == "rp-main").unwrap();
    assert_eq!(rp_main.codec, "FLAC");
    assert_eq!(rp_main.sample_rate, 44100);
    assert_eq!(rp_main.rp_channel, Some(0));

    // Verify Czech Radio D-Dur
    let ddur = catalog.stations.iter().find(|s| s.id == "czech-d-dur").unwrap();
    assert_eq!(ddur.codec, "FLAC");
    assert_eq!(ddur.sample_rate, 48000);
    assert_eq!(ddur.bit_depth, 24);

    // Verify JB Radio-2
    let jb = catalog.stations.iter().find(|s| s.id == "jb-radio2").unwrap();
    assert_eq!(jb.codec, "FLAC");
    assert_eq!(jb.sample_rate, 96000);
    assert_eq!(jb.bit_depth, 24);
}
