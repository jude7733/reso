use crate::error::{ResoError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::Command;

/// Summary of an active PipeWire audio sink.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PipeWireSink {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub state: String,
    pub active_rate: Option<u32>,
    pub format: Option<String>,
    pub channels: Option<u32>,
    pub volume: Option<f64>,
    pub is_running: bool,
}

/// An active audio stream in the PipeWire graph.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PipeWireStream {
    pub id: u64,
    pub name: String,
    pub app_name: String,
    pub binary: String,
    pub pid: Option<u64>,
    pub rate: Option<u32>,
    pub format: Option<String>,
    pub state: String,
    pub is_mpv: bool,
    pub is_running: bool,
}

/// Conflicting audio client locking the PipeWire clock graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CulpritClient {
    pub name: String,
    pub pid: Option<u64>,
    pub locking_rate: u32,
    pub description: String,
}

/// PipeWire graph diagnostic inspection report.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PipeWireGraphReport {
    pub active_sink: Option<PipeWireSink>,
    pub mpv_stream: Option<PipeWireStream>,
    pub all_active_streams: Vec<PipeWireStream>,
    pub allowed_clock_rates: Vec<u32>,
    pub default_clock_rate: Option<u32>,
    pub dsp_filters_active: Vec<String>,
    pub culprits: Vec<CulpritClient>,
}

/// Executes `pw-dump` and parses the JSON output into a comprehensive graph report.
pub fn inspect_pipewire_graph(target_source_rate: Option<u32>) -> Result<PipeWireGraphReport> {
    let output = Command::new("pw-dump")
        .output()
        .map_err(|e| ResoError::PipeWire(format!("Failed to execute pw-dump: {}", e)))?;

    if !output.status.success() {
        return Err(ResoError::PipeWire(format!(
            "pw-dump failed with exit code: {:?}",
            output.status.code()
        )));
    }

    let json_val: Value = serde_json::from_slice(&output.stdout)?;
    Ok(parse_pw_dump_json(&json_val, target_source_rate))
}

/// Parses raw JSON array returned by `pw-dump`.
pub fn parse_pw_dump_json(val: &Value, target_source_rate: Option<u32>) -> PipeWireGraphReport {
    let mut report = PipeWireGraphReport::default();

    let items = match val.as_array() {
        Some(arr) => arr,
        None => return report,
    };

    let mut sinks: Vec<PipeWireSink> = Vec::new();
    let mut streams: Vec<PipeWireStream> = Vec::new();

    for item in items {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");

        // Check PipeWire Core properties for allowed clock rates
        if item_type == "PipeWire:Interface:Core" {
            if let Some(props) = item.get("info").and_then(|i| i.get("props")) {
                if let Some(rates_str) = props
                    .get("default.clock.allowed-rates")
                    .and_then(|r| r.as_str())
                {
                    report.allowed_clock_rates = parse_allowed_rates(rates_str);
                }
                if let Some(def_rate) = props.get("default.clock.rate").and_then(|r| r.as_u64()) {
                    report.default_clock_rate = Some(def_rate as u32);
                }
            }
            continue;
        }

        // Node inspection
        if item_type == "PipeWire:Interface:Node" {
            let id = item.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let info = match item.get("info") {
                Some(i) => i,
                None => continue,
            };

            let state = info
                .get("state")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown")
                .to_string();
            let is_running = state == "running";

            let props = info.get("props");
            let media_class = props
                .and_then(|p| p.get("media.class"))
                .and_then(|c| c.as_str())
                .unwrap_or("");
            let node_name = props
                .and_then(|p| p.get("node.name"))
                .and_then(|n| n.as_str())
                .unwrap_or("");
            let node_desc = props
                .and_then(|p| p.get("node.description"))
                .and_then(|d| d.as_str())
                .unwrap_or(node_name);
            let app_name = props
                .and_then(|p| p.get("application.name"))
                .and_then(|a| a.as_str())
                .unwrap_or("");
            let binary = props
                .and_then(|p| p.get("application.process.binary"))
                .and_then(|b| b.as_str())
                .unwrap_or("");
            let pid = props
                .and_then(|p| p.get("application.process.id"))
                .and_then(|pid| pid.as_u64());

            // Extract Format parameter
            let mut rate: Option<u32> = None;
            let mut format: Option<String> = None;
            let mut channels: Option<u32> = None;

            if let Some(params) = info.get("params") {
                if let Some(format_arr) = params.get("Format").and_then(|f| f.as_array()) {
                    if let Some(first_fmt) = format_arr.first() {
                        if let Some(r) = first_fmt.get("rate").and_then(|r| r.as_u64()) {
                            rate = Some(r as u32);
                        }
                        if let Some(fmt) = first_fmt.get("format").and_then(|f| f.as_str()) {
                            format = Some(fmt.to_string());
                        }
                        if let Some(ch) = first_fmt.get("channels").and_then(|c| c.as_u64()) {
                            channels = Some(ch as u32);
                        }
                    }
                }
            }

            // Fallback rate from props
            if rate.is_none() {
                if let Some(r) = props
                    .and_then(|p| p.get("audio.rate"))
                    .and_then(|r| r.as_u64())
                {
                    rate = Some(r as u32);
                }
            }

            // Detect DSP filters
            if (media_class.contains("Filter")
                || node_name.contains("filter")
                || node_name.contains("easyeffects")
                || node_name.contains("lsp"))
                && is_running
            {
                report.dsp_filters_active.push(node_desc.to_string());
            }

            // Sinks
            if media_class == "Audio/Sink" {
                sinks.push(PipeWireSink {
                    id,
                    name: node_name.to_string(),
                    description: node_desc.to_string(),
                    state: state.clone(),
                    active_rate: rate,
                    format: format.clone(),
                    channels,
                    volume: None,
                    is_running,
                });
            }

            // Audio playback streams
            if media_class == "Stream/Output/Audio"
                || (!media_class.contains("Sink")
                    && !media_class.contains("Source")
                    && (!app_name.is_empty() || !binary.is_empty()))
            {
                let is_mpv = app_name.to_lowercase().contains("mpv")
                    || binary.to_lowercase().contains("mpv")
                    || node_name.to_lowercase().contains("mpv");

                let stream = PipeWireStream {
                    id,
                    name: node_name.to_string(),
                    app_name: app_name.to_string(),
                    binary: binary.to_string(),
                    pid,
                    rate,
                    format: format.clone(),
                    state: state.clone(),
                    is_mpv,
                    is_running,
                };

                if is_mpv {
                    report.mpv_stream = Some(stream.clone());
                }

                if is_running {
                    streams.push(stream);
                }
            }
        }
    }

    // Determine active sink (prefer running USB sink, or running sink, or first sink)
    let active_sink = sinks
        .iter()
        .find(|s| {
            s.is_running
                && (s.name.contains("usb") || s.name.contains("Chu2") || s.name.contains("DSP"))
        })
        .or_else(|| sinks.iter().find(|s| s.is_running))
        .or_else(|| sinks.first())
        .cloned();

    report.active_sink = active_sink;
    report.all_active_streams = streams;

    // Culprit Detection:
    // If target source rate is known and active sink rate doesn't match target rate:
    if let (Some(target_rate), Some(sink)) = (target_source_rate, &report.active_sink) {
        if let Some(sink_rate) = sink.active_rate {
            if sink_rate != target_rate {
                // Find other running streams that might be locking the graph to sink_rate
                for stream in &report.all_active_streams {
                    if !stream.is_mpv && stream.is_running {
                        let name = if !stream.app_name.is_empty() {
                            stream.app_name.clone()
                        } else if !stream.binary.is_empty() {
                            stream.binary.clone()
                        } else {
                            stream.name.clone()
                        };

                        let culprit_rate = stream.rate.unwrap_or(sink_rate);
                        report.culprits.push(CulpritClient {
                            name: name.clone(),
                            pid: stream.pid,
                            locking_rate: culprit_rate,
                            description: format!(
                                "{} (PID {}) is locking PipeWire graph at {}",
                                name,
                                stream
                                    .pid
                                    .map(|p| p.to_string())
                                    .unwrap_or_else(|| "?".to_string()),
                                crate::util::format_sample_rate(culprit_rate)
                            ),
                        });
                    }
                }
            }
        }
    }

    report
}

/// Parses "[ 44100 48000 88200 96000 ]" or "44100, 48000" into a `Vec<u32>`.
pub fn parse_allowed_rates(raw: &str) -> Vec<u32> {
    let clean = raw.replace(['[', ']', ',', '"'], " ");
    clean
        .split_whitespace()
        .filter_map(|s| s.parse::<u32>().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_allowed_rates() {
        let raw = "[ 44100 48000 88200 96000 ]";
        let rates = parse_allowed_rates(raw);
        assert_eq!(rates, vec![44100, 48000, 88200, 96000]);
    }

    #[test]
    fn test_parse_pw_dump_sink_and_culprit() {
        let dump = json!([
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
                "id": 98,
                "type": "PipeWire:Interface:Node",
                "info": {
                    "state": "running",
                    "props": {
                        "media.class": "Stream/Output/Audio",
                        "application.name": "Firefox",
                        "application.process.binary": "firefox",
                        "application.process.id": 84229
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

        let report = parse_pw_dump_json(&dump, Some(44100));
        assert!(report.active_sink.is_some());
        let sink = report.active_sink.unwrap();
        assert_eq!(sink.id, 55);
        assert_eq!(sink.active_rate, Some(48000));

        // Firefox was running at 48000 while target is 44100 -> culprit!
        assert_eq!(report.culprits.len(), 1);
        assert_eq!(report.culprits[0].name, "Firefox");
        assert_eq!(report.culprits[0].locking_rate, 48000);
        assert_eq!(report.culprits[0].pid, Some(84229));
    }
}
