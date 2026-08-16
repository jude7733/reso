//! Album cover art loader and terminal graphics renderer.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::StatefulWidget;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::StatefulImage;
use std::collections::HashMap;

/// Unicode vinyl disc placeholder when image is loading or unavailable.
const VINYL_ART: [&str; 6] = [
    "  .───────────.  ",
    " /   .─────.   \\ ",
    "|   /   ●   \\   |",
    "|   \\       /   |",
    " \\   `─────'   / ",
    "  `───────────'  ",
];

/// Manages downloading, caching, and rendering album art in the TUI.
pub struct CoverArtManager {
    picker: Option<Picker>,
    cache: HashMap<String, StatefulProtocol>,
    current_image_key: Option<String>,
}

impl Default for CoverArtManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CoverArtManager {
    /// Creates a new `CoverArtManager`, automatically querying terminal graphics protocol support.
    pub fn new() -> Self {
        let picker = Picker::from_query_stdio().ok();
        Self {
            picker,
            cache: HashMap::new(),
            current_image_key: None,
        }
    }

    /// Loads image bytes into the renderer cache.
    pub fn load_image(&mut self, key: &str, image_bytes: &[u8]) {
        if let Ok(dyn_img) = image::load_from_memory(image_bytes) {
            if let Some(picker) = &mut self.picker {
                let protocol = picker.new_resize_protocol(dyn_img);
                self.cache.insert(key.to_string(), protocol);
                self.current_image_key = Some(key.to_string());
            }
        }
    }

    /// Sets the active image key.
    pub fn set_active_key(&mut self, key: Option<&str>) {
        self.current_image_key = key.map(|s| s.to_string());
    }

    /// Renders either the graphical cover or the stylized vinyl placeholder.
    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if area.width < 4 || area.height < 4 {
            return;
        }

        if let Some(key) = &self.current_image_key {
            if let Some(protocol) = self.cache.get_mut(key) {
                let image_widget = StatefulImage::new(None);
                StatefulWidget::render(image_widget, area, buf, protocol);
                return;
            }
        }

        // Fallback: render stylized Vinyl / Hi-Res Disc ASCII art
        let start_y = area.top() + (area.height.saturating_sub(VINYL_ART.len() as u16)) / 2;
        for (i, line) in VINYL_ART.iter().enumerate() {
            let y = start_y + i as u16;
            if y < area.bottom() {
                let line_len = line.chars().count() as u16;
                let start_x = area.left() + (area.width.saturating_sub(line_len)) / 2;

                for (cx, ch) in line.chars().enumerate() {
                    let x = start_x + cx as u16;
                    if x < area.right() {
                        let color = if ch == '●' {
                            Color::Cyan
                        } else {
                            Color::Rgb(120, 140, 160)
                        };
                        buf[(x, y)]
                            .set_char(ch)
                            .set_style(Style::default().fg(color).add_modifier(Modifier::BOLD));
                    }
                }
            }
        }
    }
}
