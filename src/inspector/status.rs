//! 3-Stage Audiophile Pipeline Verification & Diagnostics Engine.

use crate::events::AudioStreamParams;
use crate::inspector::alsa::{
    read_dac_capabilities, read_hw_params, scan_alsa_cards, AlsaHwParams, DacCapabilities,
};
use crate::inspector::pipewire::{inspect_pipewire_graph, CulpritClient, PipeWireSink};
use crate::util::format_sample_rate;
use serde::{Deserialize, Serialize};

/// Verdict on the bit-perfect fidelity of the audio pipeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BitPerfectVerdict {
    /// Perfectly preserving native sample rate and bit depth.
    NativeBitPerfect,

    /// Audio is being resampled due to graph or device rate mismatch.
    Resampled {
        source_rate: u32,
        sink_rate: u32,
        culprit: Option<CulpritClient>,
    },

    /// Volume in MPV is below 100%, causing software bit reduction.
    VolumeDegraded { mpv_volume: f64 },

    /// Software DSP filters / equalizers are active in PipeWire.
    DspFilterActive { filters: Vec<String> },

    /// Audio pipeline is inactive or idle.
    Idle,
}

/// Comprehensive snapshot of all 3 stages in the audio pipeline.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PipelineStatus {
    // Stage 1: Source / MPV
    pub source_codec: String,
    pub source_rate: u32,
    pub source_channels: u32,
    pub source_bit_depth: Option<u8>,
    pub source_bitrate_kbps: Option<u32>,
    pub mpv_volume: f64,

    /// The station's configured native sample rate (ground truth).
    /// This is the true media rate, unlike `source_rate` which is MPV's
    /// negotiated output rate and may differ when PipeWire forces resampling.
    pub media_sample_rate: Option<u32>,

    // Stage 2: PipeWire Graph
    pub pw_sink: Option<PipeWireSink>,
    pub pw_sink_rate: Option<u32>,
    pub pw_allowed_rates: Vec<u32>,
    pub pw_dsp_filters: Vec<String>,
    pub pw_culprits: Vec<CulpritClient>,

    // Stage 3: ALSA / Hardware DAC
    pub alsa_card_name: Option<String>,
    pub alsa_card_index: Option<u32>,
    pub alsa_hw_params: Option<AlsaHwParams>,
    pub dac_capabilities: Option<DacCapabilities>,

    // Verification Result
    pub verdict: Option<BitPerfectVerdict>,
}

impl PipelineStatus {
    /// Returns the reference sample rate: prefers `media_sample_rate` (ground truth from config)
    /// if present and non-zero, falling back to MPV's `source_rate`.
    pub fn reference_rate(&self) -> u32 {
        self.media_sample_rate.unwrap_or(self.source_rate)
    }

    /// Evaluates the complete 3-stage audio pipeline.
    ///
    /// `media_sample_rate` is the station's configured native sample rate (ground truth).
    /// This is used instead of MPV's `audio-params.samplerate` which may reflect a
    /// negotiated output rate when PipeWire forces the graph to a different clock.
    pub fn evaluate(
        stream_params: &AudioStreamParams,
        mpv_volume: f64,
        media_sample_rate: Option<u32>,
    ) -> Self {
        let mut status = Self {
            source_codec: stream_params.codec.clone(),
            source_rate: stream_params.sample_rate,
            source_channels: stream_params.channels,
            source_bit_depth: stream_params.bit_depth,
            source_bitrate_kbps: stream_params.bitrate_kbps,
            mpv_volume,
            media_sample_rate: media_sample_rate.filter(|&r| r > 0),
            ..Default::default()
        };

        // The reference rate for pipeline checks: prefer the station's known
        // media rate over MPV's negotiated output rate.
        let reference_rate = status.media_sample_rate.unwrap_or(status.source_rate);

        // If no source stream is active
        if reference_rate == 0 {
            status.verdict = Some(BitPerfectVerdict::Idle);
            return status;
        }

        // Query PipeWire — pass the TRUE media rate so culprit detection
        // correctly identifies streams locking the graph away from it.
        if let Ok(pw_report) = inspect_pipewire_graph(Some(reference_rate)) {
            if let Some(sink) = &pw_report.active_sink {
                status.pw_sink_rate = sink.active_rate;
            }
            status.pw_sink = pw_report.active_sink;
            status.pw_allowed_rates = pw_report.allowed_clock_rates;
            status.pw_dsp_filters = pw_report.dsp_filters_active;
            status.pw_culprits = pw_report.culprits;
        }

        // Query ALSA
        if let Ok(cards) = scan_alsa_cards() {
            // Find USB DAC or active card
            let target_card = cards
                .iter()
                .find(|c| c.is_usb || c.id.contains("DSP") || c.name.contains("Chu2"))
                .or_else(|| cards.iter().find(|c| c.index == 1))
                .or_else(|| cards.first());

            if let Some(card) = target_card {
                status.alsa_card_name = Some(card.name.clone());
                status.alsa_card_index = Some(card.index);

                if let Ok(hw) = read_hw_params(card.index) {
                    status.alsa_hw_params = Some(hw);
                }

                if let Ok(caps) = read_dac_capabilities(card.index) {
                    status.dac_capabilities = Some(caps);
                }
            }
        }

        // Determine Verdict
        status.verdict = Some(status.calculate_verdict());
        status
    }

    /// Determines the bit-perfect verdict based on the 3 stages.
    ///
    /// Uses the station's known media sample rate as the reference (ground truth)
    /// rather than MPV's negotiated output rate, which can be changed by PipeWire.
    fn calculate_verdict(&self) -> BitPerfectVerdict {
        // The reference rate: station config (ground truth) > MPV output rate.
        let reference_rate = self.media_sample_rate.unwrap_or(self.source_rate);

        if reference_rate == 0 {
            return BitPerfectVerdict::Idle;
        }

        // 1. Check software DSP filters
        if !self.pw_dsp_filters.is_empty() {
            return BitPerfectVerdict::DspFilterActive {
                filters: self.pw_dsp_filters.clone(),
            };
        }

        // 2. Check MPV volume bit-depth scaling
        if self.mpv_volume < 99.5 {
            return BitPerfectVerdict::VolumeDegraded {
                mpv_volume: self.mpv_volume,
            };
        }

        // 3. Cross-check: if media rate is known and MPV's output rate differs,
        //    MPV itself was forced to resample (e.g. PipeWire negotiated a different rate).
        if let Some(media_rate) = self.media_sample_rate {
            if self.source_rate != 0 && self.source_rate != media_rate {
                let culprit = self.pw_culprits.first().cloned();
                return BitPerfectVerdict::Resampled {
                    source_rate: media_rate,
                    sink_rate: self.source_rate,
                    culprit,
                };
            }
        }

        // 4. Check PipeWire sink rate vs reference rate.
        //    Only compare when pw_sink_rate is actually known — do NOT
        //    fallback to source_rate, which hides mismatches.
        if let Some(sink_rate) = self.pw_sink_rate {
            if sink_rate != reference_rate {
                let culprit = self.pw_culprits.first().cloned();
                return BitPerfectVerdict::Resampled {
                    source_rate: reference_rate,
                    sink_rate,
                    culprit,
                };
            }
        }

        // 5. Check ALSA DAC rate vs reference rate
        if let Some(hw) = &self.alsa_hw_params {
            if let Some(alsa_rate) = hw.rate {
                if alsa_rate != reference_rate {
                    return BitPerfectVerdict::Resampled {
                        source_rate: reference_rate,
                        sink_rate: alsa_rate,
                        culprit: None,
                    };
                }
            }
        }

        BitPerfectVerdict::NativeBitPerfect
    }

    /// Generates a clean human-readable diagnostic summary string for CLI inspect.
    pub fn format_cli_report(&self) -> String {
        let mut out = String::new();
        out.push_str("=================================================================\n");
        out.push_str("               RESO AUDIOPHILE PIPELINE INSPECTOR                \n");
        out.push_str("=================================================================\n\n");

        // 1. Source Stage
        out.push_str("1. STREAM SOURCE (MPV DECODER)\n");
        out.push_str(&format!(
            "   Codec:        {}\n",
            if self.source_codec.is_empty() {
                "None"
            } else {
                &self.source_codec
            }
        ));
        // Show the true media rate when it differs from MPV's output rate
        let media_rate = self.media_sample_rate.unwrap_or(self.source_rate);
        out.push_str(&format!(
            "   Sample Rate:  {}\n",
            format_sample_rate(media_rate)
        ));
        if self.media_sample_rate.is_some()
            && self.source_rate != media_rate
            && self.source_rate > 0
        {
            out.push_str(&format!(
                "   MPV Output:   {} (negotiated by PipeWire)\n",
                format_sample_rate(self.source_rate)
            ));
        }
        out.push_str(&format!(
            "   Bit Depth:    {}\n",
            self.source_bit_depth
                .map(|b| format!("{}-bit", b))
                .unwrap_or_else(|| "16/24-bit VBR".to_string())
        ));
        out.push_str(&format!(
            "   Channels:     {}\n",
            if self.source_channels == 0 {
                2
            } else {
                self.source_channels
            }
        ));
        out.push_str(&format!(
            "   MPV Volume:   {:.1}% (Bit-perfect requires 100%)\n\n",
            self.mpv_volume
        ));

        // 2. PipeWire Stage
        out.push_str("2. PIPEWIRE AUDIO GRAPH\n");
        if let Some(sink) = &self.pw_sink {
            out.push_str(&format!(
                "   Sink Node:    {} [{}]\n",
                sink.description, sink.name
            ));
            out.push_str(&format!(
                "   Sink Rate:    {}\n",
                format_sample_rate(self.pw_sink_rate.unwrap_or(0))
            ));
            out.push_str(&format!(
                "   Sink Format:  {}\n",
                sink.format.as_deref().unwrap_or("Native")
            ));
        } else {
            out.push_str("   Sink Node:    None detected\n");
        }
        out.push_str(&format!("   Allowed Rates: {:?}\n", self.pw_allowed_rates));
        if !self.pw_dsp_filters.is_empty() {
            out.push_str(&format!(
                "   DSP Filters:  WARNING: {:?}\n",
                self.pw_dsp_filters
            ));
        } else {
            out.push_str("   DSP Filters:  None (Bit-exact path)\n");
        }
        out.push('\n');

        // 3. ALSA Hardware Stage
        out.push_str("3. HARDWARE DAC / ALSA\n");
        out.push_str(&format!(
            "   DAC Device:   {}\n",
            self.alsa_card_name.as_deref().unwrap_or("Unknown")
        ));
        if let Some(hw) = &self.alsa_hw_params {
            out.push_str(&format!(
                "   ALSA Status:  {}\n",
                if hw.is_active {
                    "Active (Running)"
                } else {
                    "Idle / Suspended"
                }
            ));
            out.push_str(&format!(
                "   HW Rate:      {}\n",
                format_sample_rate(hw.rate.unwrap_or(0))
            ));
            out.push_str(&format!(
                "   HW Format:    {}\n",
                hw.format.as_deref().unwrap_or("Unknown")
            ));
            out.push_str(&format!(
                "   Buffer Size:  {} periods\n",
                hw.buffer_size.unwrap_or(0)
            ));
        }
        if let Some(caps) = &self.dac_capabilities {
            out.push_str(&format!(
                "   DAC Supported Rates:  {:?}\n",
                caps.supported_rates
            ));
            out.push_str(&format!(
                "   DAC Supported Depths: {:?} bits\n",
                caps.supported_bit_depths
            ));
        }
        out.push('\n');

        // Final Verdict
        out.push_str("=================================================================\n");
        match &self.verdict {
            Some(BitPerfectVerdict::NativeBitPerfect) => {
                out.push_str(" STATUS: [ PASS ] ✓ BIT-PERFECT DIRECT PLAYBACK\n");
                out.push_str(
                    " Native source rate matches PipeWire sink and hardware DAC clocks.\n",
                );
            }
            Some(BitPerfectVerdict::Resampled {
                source_rate,
                sink_rate,
                culprit,
            }) => {
                out.push_str(" STATUS: [ WARNING ] ⚠ RESAMPLING DETECTED\n");
                out.push_str(&format!(
                    " Stream ({} Hz) is being resampled to ({} Hz).\n",
                    source_rate, sink_rate
                ));
                if let Some(c) = culprit {
                    out.push_str(&format!(" CULPRIT: {}\n", c.description));
                }
            }
            Some(BitPerfectVerdict::VolumeDegraded { mpv_volume }) => {
                out.push_str(" STATUS: [ WARNING ] ⚠ MPV SOFTWARE VOLUME ACTIVE\n");
                out.push_str(&format!(" MPV volume is at {:.1}%. Set MPV volume to 100% to avoid truncating bit depth.\n", mpv_volume));
                out.push_str(
                    " Adjust listening volume using hardware PipeWire control (`wpctl`).\n",
                );
            }
            Some(BitPerfectVerdict::DspFilterActive { filters }) => {
                out.push_str(" STATUS: [ WARNING ] ⚠ ACTIVE DSP / EQUALIZER DETECTED\n");
                out.push_str(&format!(
                    " PipeWire software filters active: {:?}\n",
                    filters
                ));
            }
            Some(BitPerfectVerdict::Idle) | None => {
                out.push_str(" STATUS: [ IDLE ] Audio pipeline is currently paused or stopped.\n");
            }
        }
        out.push_str("=================================================================\n");

        out
    }
}
