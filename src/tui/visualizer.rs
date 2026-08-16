//! Real-time dynamic audio spectrum visualizer bar widget.
//!
//! Renders live multi-band audio frequency spectrum bars powered by
//! real-time PipeWire PCM capture and zero-allocation Fast Fourier Transforms.

use crate::tui::spectrum::{AudioCapture, SpectrumAnalyzer, FFT_SIZE, NUM_BANDS};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Widget;

/// Unicode vertical bar blocks: from 1/8th to full block.
const BARS: [char; 8] = [' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// State for the dynamic audio spectrum visualizer.
pub struct VisualizerState {
    pub is_playing: bool,
    pub step: u64,
    pub bands: Vec<f32>,
    pub peak_caps: Vec<f32>,
    pub capture: AudioCapture,
    pub analyzer: SpectrumAnalyzer,
    sample_scratch: [f32; FFT_SIZE],
    target_bands: [f32; NUM_BANDS],
}

impl std::fmt::Debug for VisualizerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VisualizerState")
            .field("is_playing", &self.is_playing)
            .field("step", &self.step)
            .field("bands_len", &self.bands.len())
            .field("is_capturing", &self.capture.is_active())
            .finish()
    }
}

impl Default for VisualizerState {
    fn default() -> Self {
        Self {
            is_playing: false,
            step: 0,
            bands: vec![0.0; NUM_BANDS],
            peak_caps: vec![0.0; NUM_BANDS],
            capture: AudioCapture::new(),
            analyzer: SpectrumAnalyzer::new(),
            sample_scratch: [0.0; FFT_SIZE],
            target_bands: [0.0; NUM_BANDS],
        }
    }
}

impl VisualizerState {
    /// Creates a new visualizer state with pre-allocated FFT engine and audio capture.
    pub fn new() -> Self {
        Self::default()
    }

    /// Explicitly starts the PipeWire audio capture stream.
    pub fn start_capture(&mut self) {
        if !self.capture.is_active() {
            self.capture.start();
        }
    }

    /// Explicitly stops the PipeWire audio capture stream.
    pub fn stop_capture(&mut self) {
        if self.capture.is_active() {
            self.capture.stop();
        }
    }

    /// Progresses the real-time audio spectrum analysis on every UI tick.
    pub fn tick(&mut self) {
        self.step = self.step.wrapping_add(1);

        if !self.is_playing {
            // Stop audio capture when playback is paused or stopped
            if self.capture.is_active() {
                self.capture.stop();
            }

            // Decay bands and peak caps smoothly to zero
            for band in &mut self.bands {
                *band *= 0.80;
                if *band < 0.005 {
                    *band = 0.0;
                }
            }
            for cap in &mut self.peak_caps {
                *cap = (*cap - 0.04).max(0.0);
            }
            return;
        }

        // Ensure capture is running when playing
        if !self.capture.is_active() {
            self.capture.start();
        }

        // Fetch latest PCM samples and compute live FFT spectrum
        let got_samples = self.capture.get_latest_samples(&mut self.sample_scratch);

        if got_samples {
            self.analyzer
                .compute_spectrum(&self.sample_scratch, &mut self.target_bands);

            for i in 0..NUM_BANDS {
                let target = self.target_bands[i];
                if target > self.bands[i] {
                    // Snappy attack: jump quickly on beats/transients
                    self.bands[i] = self.bands[i] * 0.25 + target * 0.75;
                } else {
                    // Smooth exponential decay: gravity falloff
                    self.bands[i] = (self.bands[i] * 0.82).max(0.0);
                }

                // Update peak hold caps
                if self.bands[i] >= self.peak_caps[i] {
                    self.peak_caps[i] = self.bands[i];
                } else {
                    self.peak_caps[i] = (self.peak_caps[i] - 0.025).max(self.bands[i]).max(0.0);
                }
            }
        } else {
            // Audio stream buffering or silence: smooth decay
            for band in &mut self.bands {
                *band *= 0.85;
                if *band < 0.005 {
                    *band = 0.0;
                }
            }
            for cap in &mut self.peak_caps {
                *cap = (*cap - 0.04).max(0.0);
            }
        }
    }
}

/// Ratatui widget that renders the audio spectrum bars with peak indicators.
pub struct VisualizerWidget<'a> {
    pub state: &'a VisualizerState,
    pub primary_color: Color,
    pub highlight_color: Color,
}

impl Widget for VisualizerWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let num_bars = area.width as usize;
        let max_height = area.height as f32;
        let num_bands = self.state.bands.len();

        if num_bands == 0 {
            return;
        }

        for x in 0..num_bars {
            let band_idx = (x * num_bands) / num_bars;
            let amp = self
                .state
                .bands
                .get(band_idx)
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);

            let peak_amp = self
                .state
                .peak_caps
                .get(band_idx)
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);

            let total_height = amp * max_height;
            let full_cells = total_height.floor() as u16;
            let remainder = total_height - full_cells as f32;

            let col = area.left() + x as u16;

            for y in 0..area.height {
                let cell_y = area.bottom().saturating_sub(1 + y);

                if y < full_cells {
                    // Full block
                    let color = if y > area.height * 2 / 3 {
                        self.highlight_color
                    } else {
                        self.primary_color
                    };
                    buf[(col, cell_y)]
                        .set_char('█')
                        .set_style(Style::default().fg(color));
                } else if y == full_cells && remainder > 0.05 {
                    // Partial block fraction
                    let idx = ((remainder * 8.0).floor() as usize).clamp(0, 7);
                    let color = if y > area.height * 2 / 3 {
                        self.highlight_color
                    } else {
                        self.primary_color
                    };
                    buf[(col, cell_y)]
                        .set_char(BARS[idx])
                        .set_style(Style::default().fg(color));
                }
            }

            // Draw floating peak cap dot if above current bar
            let peak_y = (peak_amp * max_height).floor() as u16;
            if peak_y > full_cells && peak_y < area.height && peak_amp > 0.08 {
                let cell_y = area.bottom().saturating_sub(1 + peak_y);
                buf[(col, cell_y)]
                    .set_char('▔')
                    .set_style(Style::default().fg(self.highlight_color));
            }
        }
    }
}
