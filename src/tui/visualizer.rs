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
    pub show_visualizer: bool,
    pub step: u64,
    pub bands: Vec<f32>,
    pub peak_caps: Vec<f32>,
    pub peak_hold_timers: [u8; NUM_BANDS],
    pub peak_velocities: [f32; NUM_BANDS],
    pub capture: AudioCapture,
    pub analyzer: SpectrumAnalyzer,
    sample_scratch: [f32; FFT_SIZE],
    target_bands: [f32; NUM_BANDS],
}

impl std::fmt::Debug for VisualizerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VisualizerState")
            .field("is_playing", &self.is_playing)
            .field("show_visualizer", &self.show_visualizer)
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
            show_visualizer: true,
            step: 0,
            bands: vec![0.0; NUM_BANDS],
            peak_caps: vec![0.0; NUM_BANDS],
            peak_hold_timers: [0; NUM_BANDS],
            peak_velocities: [0.0; NUM_BANDS],
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

        if !self.is_playing || !self.show_visualizer {
            // Stop audio capture when playback is paused or stopped, or visualizer is disabled
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
            self.peak_hold_timers.fill(0);
            self.peak_velocities.fill(0.0);
            return;
        }

        // Ensure capture is running and process is genuinely alive (self-healing)
        if !self.capture.is_alive() {
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

                // Update peak hold caps with realistic physics (hold suspended, then gravity fall)
                if self.bands[i] >= self.peak_caps[i] {
                    self.peak_caps[i] = self.bands[i];
                    self.peak_hold_timers[i] = 4; // Hold peak floating for 4 ticks (~200ms)
                    self.peak_velocities[i] = 0.0;
                } else if self.peak_hold_timers[i] > 0 {
                    // Still in hold phase: keep peak cap suspended in air
                    self.peak_hold_timers[i] -= 1;
                } else {
                    // Gravity falloff: accelerate downward
                    self.peak_velocities[i] = (self.peak_velocities[i] + 0.008).min(0.06);
                    self.peak_caps[i] = (self.peak_caps[i] - self.peak_velocities[i])
                        .max(self.bands[i])
                        .max(0.0);
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
            self.peak_hold_timers.fill(0);
            self.peak_velocities.fill(0.0);
        }
    }
}

/// Ratatui widget that renders the audio spectrum bars with peak indicators.
pub struct VisualizerWidget<'a> {
    pub state: &'a VisualizerState,
    pub primary_color: Color,
    pub highlight_color: Color,
    pub low_color: Option<Color>,
    pub peak_color: Option<Color>,
}

impl<'a> VisualizerWidget<'a> {
    pub fn new(state: &'a VisualizerState, primary_color: Color, highlight_color: Color) -> Self {
        Self {
            state,
            primary_color,
            highlight_color,
            low_color: None,
            peak_color: None,
        }
    }
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

        let low_c = self.low_color.unwrap_or_else(|| {
            if self.primary_color == Color::Cyan {
                Color::Rgb(0, 150, 195)
            } else {
                self.primary_color
            }
        });
        let peak_c = self.peak_color.unwrap_or_else(|| {
            if self.primary_color == Color::Cyan {
                Color::Rgb(255, 195, 75)
            } else {
                self.highlight_color
            }
        });

        for x in 0..num_bars {
            // Smooth cosine curve interpolation across terminal width
            let (amp, peak_amp) = if num_bars > 1 && num_bands > 1 {
                let pos = (x as f32 / (num_bars - 1) as f32) * (num_bands - 1) as f32;
                let idx0 = (pos.floor() as usize).min(num_bands - 1);
                let idx1 = (idx0 + 1).min(num_bands - 1);
                let frac = pos - idx0 as f32;
                // Cosine curve easing: s-curve transition between bands
                let t = (1.0 - (frac * std::f32::consts::PI).cos()) * 0.5;
                let a0 = self.state.bands[idx0];
                let a1 = self.state.bands[idx1];
                let p0 = self.state.peak_caps[idx0];
                let p1 = self.state.peak_caps[idx1];
                (
                    (a0 * (1.0 - t) + a1 * t).clamp(0.0, 1.0),
                    (p0 * (1.0 - t) + p1 * t).clamp(0.0, 1.0),
                )
            } else {
                (
                    self.state.bands.first().copied().unwrap_or(0.0).clamp(0.0, 1.0),
                    self.state.peak_caps.first().copied().unwrap_or(0.0).clamp(0.0, 1.0),
                )
            };

            let total_height = amp * max_height;
            let full_cells = total_height.floor() as u16;
            let remainder = total_height - full_cells as f32;

            let col = area.left() + x as u16;

            for y in 0..area.height {
                let cell_y = area.bottom().saturating_sub(1 + y);

                // Multi-tier gradient based on vertical elevation
                let norm_y = if area.height > 1 {
                    y as f32 / (area.height - 1) as f32
                } else {
                    0.0
                };

                let cell_color = if norm_y > 0.85 {
                    peak_c // Amber peak glow / high tier
                } else if norm_y > 0.60 {
                    self.highlight_color // Upper bar tier (ice blue / secondary accent)
                } else if norm_y > 0.30 {
                    self.primary_color // Mid bar tier (primary accent)
                } else {
                    low_c // Deep bottom tier
                };

                if y < full_cells {
                    // Full block
                    buf[(col, cell_y)]
                        .set_char('█')
                        .set_style(Style::default().fg(cell_color));
                } else if y == full_cells && remainder > 0.05 {
                    // Partial block fraction
                    let idx = ((remainder * 8.0).floor() as usize).clamp(0, 7);
                    buf[(col, cell_y)]
                        .set_char(BARS[idx])
                        .set_style(Style::default().fg(cell_color));
                }
            }

            // Draw floating peak cap dot if above current bar with glowing highlight
            let peak_y = (peak_amp * max_height).floor() as u16;
            if peak_y > full_cells && peak_y < area.height && peak_amp > 0.06 {
                let cell_y = area.bottom().saturating_sub(1 + peak_y);
                let cap_color = self.peak_color.unwrap_or_else(|| {
                    if self.primary_color == Color::Cyan {
                        Color::Rgb(255, 230, 140)
                    } else {
                        self.highlight_color
                    }
                });
                buf[(col, cell_y)]
                    .set_char('▔')
                    .set_style(Style::default().fg(cap_color));
            }
        }
    }
}
