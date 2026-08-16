//! Real-time animated audio visualizer bar widget.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Widget;

/// Unicode vertical bar blocks: from 1/8th to full block.
const BARS: [char; 8] = [' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// State for the spectrum visualizer.
#[derive(Debug, Clone)]
pub struct VisualizerState {
    pub is_playing: bool,
    pub step: u64,
    pub bands: Vec<f32>,
}

impl Default for VisualizerState {
    fn default() -> Self {
        Self {
            is_playing: false,
            step: 0,
            bands: vec![0.0; 48],
        }
    }
}

impl VisualizerState {
    /// Progresses the simulated or live frequency bands on every UI tick.
    pub fn tick(&mut self) {
        self.step = self.step.wrapping_add(1);
        let count = self.bands.len();

        if !self.is_playing {
            // Decay to zero smoothly when paused/stopped
            for band in &mut self.bands {
                *band *= 0.85;
                if *band < 0.01 {
                    *band = 0.0;
                }
            }
            return;
        }

        let t = self.step as f32 * 0.12;
        for i in 0..count {
            let fi = i as f32;
            // Harmonic wave synthesis for rich, dynamic audiophile visualizer bars
            let wave1 = ((fi * 0.35 + t).sin() * 0.5 + 0.5).powi(2);
            let wave2 = (fi * 0.65 - t * 1.5).cos() * 0.5 + 0.5;
            let wave3 = (fi * 0.15 + t * 0.7).sin() * 0.5 + 0.5;

            // Emphasis on sub-bass and mid frequencies
            let envelope = (-((fi - (count as f32 * 0.2)).abs() / 12.0)).exp();
            let target = (wave1 * 0.5 + wave2 * 0.3 + wave3 * 0.2) * (0.4 + 0.6 * envelope);

            // Smooth interpolation
            self.bands[i] = self.bands[i] * 0.6 + target * 0.4;
        }
    }
}

/// Ratatui widget that renders the audio spectrum bars.
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

        for x in 0..num_bars {
            let band_idx = (x * self.state.bands.len()) / num_bars;
            let amp = self
                .state
                .bands
                .get(band_idx)
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);

            let total_height = amp * max_height;
            let full_cells = total_height.floor() as u16;
            let remainder = total_height - full_cells as f32;

            for y in 0..area.height {
                let cell_y = area.bottom().saturating_sub(1 + y);
                let col = area.left() + x as u16;

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
                    buf[(col, cell_y)]
                        .set_char(BARS[idx])
                        .set_style(Style::default().fg(self.primary_color));
                }
            }
        }
    }
}
