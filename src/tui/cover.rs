//! Album cover art loader and terminal graphics renderer.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::StatefulWidget;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::StatefulImage;
use std::collections::{HashMap, VecDeque};

/// Maximum number of decoded album covers kept in memory.
const MAX_CACHE_SIZE: usize = 16;

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
    cache_order: VecDeque<String>,
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
            cache_order: VecDeque::new(),
            current_image_key: None,
        }
    }

    /// Loads image bytes into the renderer cache with FIFO/LRU eviction when full.
    pub fn load_image(&mut self, key: &str, image_bytes: &[u8]) {
        if let Ok(dyn_img) = image::load_from_memory(image_bytes) {
            if let Some(picker) = &mut self.picker {
                let protocol = picker.new_resize_protocol(dyn_img);

                // Evict if at capacity and key is new
                if self.cache.contains_key(key) {
                    self.cache_order.retain(|k| k != key);
                } else if self.cache.len() >= MAX_CACHE_SIZE {
                    if let Some(oldest) = self.cache_order.pop_front() {
                        self.cache.remove(&oldest);
                    }
                }

                self.cache.insert(key.to_string(), protocol);
                self.cache_order.push_back(key.to_string());
                self.current_image_key = Some(key.to_string());
            }
        }
    }

    /// Returns the number of images currently held in cache.
    pub fn cache_len(&self) -> usize {
        self.cache.len()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cover_art_manager_initial_state() {
        let mut mgr = CoverArtManager::new();
        assert_eq!(mgr.cache_len(), 0);
        mgr.set_active_key(Some("test_key"));
        assert_eq!(mgr.current_image_key.as_deref(), Some("test_key"));
        mgr.set_active_key(None);
        assert_eq!(mgr.current_image_key, None);
    }

    #[test]
    fn test_cover_art_eviction_bound() {
        let mut mgr = CoverArtManager::new();
        let png = [
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0x63, 0x34, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0xf8, 0xff, 0xff, 0x3f, 0x00, 0x05, 0xfe, 0x02, 0xfe, 0xa7, 0x54, 0x38,
            0x80, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        for i in 0..20 {
            mgr.load_image(&format!("key_{}", i), &png);
        }
        assert!(mgr.cache_len() <= 16);
    }
}
