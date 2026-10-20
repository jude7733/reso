//! Ratatui rendering engine for `reso` TUI dashboard.

use crate::events::PlaybackState;
use crate::inspector::status::BitPerfectVerdict;
use crate::tui::app::{
    ActiveTab, AddStationState, AddStationStep, App, EditStationState, ModalState, StationSortMode,
    ToastLevel,
};
use crate::tui::visualizer::VisualizerWidget;
use crate::util::{format_duration, format_sample_rate};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState,
};
use ratatui::Frame;

/// Renders the complete TUI application frame.
pub fn render(f: &mut Frame, app: &mut App) {
    let area = f.area();

    let viz_height = if app.config_manager.config.ui.show_visualizer {
        4
    } else {
        0
    };

    // Main layout: Header, Body, Visualizer/Footer
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),          // Header & Badges
            Constraint::Min(12),            // Main Dashboard Body
            Constraint::Length(viz_height), // Spectrum Visualizer
            Constraint::Length(1),          // Keybinds footer
        ])
        .split(area);

    render_header(f, app, main_chunks[0]);
    render_body(f, app, main_chunks[1]);
    if app.config_manager.config.ui.show_visualizer {
        render_visualizer(f, app, main_chunks[2]);
    }
    render_footer(f, app, main_chunks[3]);

    // Render Modals if active
    match &app.modal_state {
        ModalState::Help => render_help_modal(f, app, area),
        ModalState::AddStation(state) => render_add_station_modal(f, state, app.tick_count, area),
        ModalState::EditStation(state) => render_edit_station_modal(f, state, area),
        ModalState::ConfirmDeleteStation { station_name, .. } => {
            render_confirm_delete_station_modal(f, station_name, area);
        }
        ModalState::ConfirmDeleteLiked { track_title, .. } => {
            render_confirm_delete_liked_modal(f, track_title, area);
        }
        ModalState::None => {}
    }
}

/// Renders the top header bar with title, tabs, and status badges.
fn render_header(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(13), // Logo & App Name (reso.png Icon + RESO)
            Constraint::Min(45),    // Navigation Tabs
            Constraint::Length(35), // Status & Bit-Perfect Badges
        ])
        .split(area);

    // 1. Logo Block with embedded assets/reso.png icon
    let logo_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(app.theme.primary));
    let inner_logo = logo_block.inner(chunks[0]);
    f.render_widget(logo_block, chunks[0]);

    if inner_logo.width >= 6 && inner_logo.height >= 1 {
        let icon_rect = Rect {
            x: inner_logo.x + 1,
            y: inner_logo.y,
            width: 2,
            height: 1,
        };
        let icon_rendered = app.cover_art_manager.render_app_icon(icon_rect, f.buffer_mut());

        let text_x = if icon_rendered {
            inner_logo.x + 4
        } else {
            inner_logo.x + (inner_logo.width.saturating_sub(4)) / 2
        };
        let text_rect = Rect {
            x: text_x,
            y: inner_logo.y,
            width: 4.min(inner_logo.width),
            height: 1,
        };
        let logo_text = Paragraph::new(Span::styled(
            "RESO",
            Style::default()
                .fg(app.theme.primary)
                .add_modifier(Modifier::BOLD),
        ));
        f.render_widget(logo_text, text_rect);
    }

    // 2. Navigation Tabs
    let tab_items = [
        ("1:Stations", ActiveTab::Stations),
        ("2:Search", ActiveTab::Search),
        ("3:Inspector", ActiveTab::Inspector),
        ("4:Liked", ActiveTab::Liked),
        ("5:Settings", ActiveTab::Settings),
    ];
    let tab_spans: Vec<Span> = tab_items
        .iter()
        .flat_map(|(name, tab)| {
            let is_active = app.active_tab == *tab;
            let style = if is_active {
                Style::default()
                    .fg(app.theme.selection_fg)
                    .bg(app.theme.primary)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.foreground)
            };
            vec![Span::styled(format!(" {} ", name), style), Span::raw(" ")]
        })
        .collect();

    let tabs_widget = Paragraph::new(Line::from(tab_spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Views ")
            .border_style(Style::default().fg(app.theme.border)),
    );
    f.render_widget(tabs_widget, chunks[1]);

    // 3. Status Badges (Bit-Perfect, Recording, Sleep Timer)
    let mut badge_spans = Vec::new();

    // Sleep Timer
    if let Some(secs) = app.sleep_timer_secs_remaining {
        badge_spans.push(Span::styled(
            format!(" ⏳{} ", format_duration(secs)),
            Style::default()
                .fg(app.theme.status_warning)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Recording Badge
    if app.is_recording {
        let pulse = if (app.tick_count / 5) % 2 == 0 {
            app.theme.status_danger
        } else {
            Color::LightRed
        };
        badge_spans.push(Span::styled(
            " 🔴 REC ",
            Style::default()
                .fg(Color::White)
                .bg(pulse)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Bit-Perfect Badge — vivid colored pill with background
    let badge_border_color;
    match &app.pipeline_status.verdict {
        Some(BitPerfectVerdict::NativeBitPerfect) => {
            let rate_str = format_sample_rate(app.pipeline_status.reference_rate());
            // Subtle pulse between two greens for a "live" feel
            let bg = if (app.tick_count / 10) % 2 == 0 {
                Color::Rgb(0, 140, 60)
            } else {
                Color::Rgb(0, 160, 70)
            };
            badge_border_color = app.theme.status_lossless;
            badge_spans.push(Span::styled(
                format!(" ✓ BIT-PERFECT: {} ", rate_str),
                Style::default()
                    .fg(Color::White)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        Some(BitPerfectVerdict::Resampled {
            source_rate,
            sink_rate,
            ..
        }) => {
            badge_border_color = app.theme.status_warning;
            badge_spans.push(Span::styled(
                format!(
                    " ⚠ RESAMPLED: {}→{} ",
                    format_sample_rate(*source_rate),
                    format_sample_rate(*sink_rate)
                ),
                Style::default()
                    .fg(Color::Black)
                    .bg(app.theme.status_warning)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        Some(BitPerfectVerdict::VolumeDegraded { .. }) => {
            badge_border_color = app.theme.status_warning;
            badge_spans.push(Span::styled(
                " ⚠ VOL < 100% ",
                Style::default()
                    .fg(Color::Black)
                    .bg(app.theme.status_warning)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        Some(BitPerfectVerdict::DspFilterActive { .. }) => {
            badge_border_color = app.theme.status_danger;
            badge_spans.push(Span::styled(
                " ⚠ DSP ACTIVE ",
                Style::default()
                    .fg(Color::White)
                    .bg(app.theme.status_danger)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        _ => {
            badge_border_color = app.theme.border;
            badge_spans.push(Span::styled(
                " ■ STANDBY ",
                Style::default().fg(Color::Gray).bg(Color::Rgb(40, 40, 40)),
            ));
        }
    }

    let badges_widget = Paragraph::new(Line::from(badge_spans))
        .alignment(Alignment::Right)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(badge_border_color)),
        );
    f.render_widget(badges_widget, chunks[2]);
}

/// Renders the main dashboard body depending on the active tab.
fn render_body(f: &mut Frame, app: &mut App, area: Rect) {
    match app.active_tab {
        ActiveTab::Stations => {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(42), // Stations List
                    Constraint::Percentage(58), // Now Playing + Compact Audiophile Path
                ])
                .split(area);

            render_stations_list(f, app, chunks[0]);
            render_right_pane(f, app, chunks[1]);
        }
        ActiveTab::Search => {
            render_search_tab(f, app, area);
        }
        ActiveTab::Inspector => {
            render_inspector_tab(f, app, area);
        }
        ActiveTab::Liked => {
            render_liked_tab(f, app, area);
        }
        ActiveTab::Settings => {
            render_settings_tab(f, app, area);
        }
    }
}

/// Renders the station catalog list on the left with selected station details & tags.
fn render_stations_list(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.active_tab == ActiveTab::Stations && !app.search_input_active;
    let filtered_indices: Vec<usize> = app.filtered_station_indices().to_vec();
    let total_len = filtered_indices.len();

    if total_len > 0 && app.station_list_index >= total_len {
        app.station_list_index = total_len - 1;
    }
    if total_len == 0 {
        app.station_list_state.select(None);
    } else {
        app.station_list_state.select(Some(app.station_list_index));
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),    // Stations List
            Constraint::Length(4), // Selected Station Tags & Info Card
        ])
        .split(area);

    let items: Vec<ListItem> = filtered_indices
        .iter()
        .enumerate()
        .filter_map(|(idx, &station_idx)| {
            let station = app.catalog.stations.get(station_idx)?;
            let is_selected = idx == app.station_list_index;
            let is_playing = app.current_station.as_ref().map(|s| &s.id) == Some(&station.id);

            let play_icon = if is_playing {
                match app.playback_state {
                    PlaybackState::Playing => "▶ ",
                    PlaybackState::Paused => "⏸ ",
                    _ => "■ ",
                }
            } else {
                "  "
            };

            let fav_icon = if station.favorite { "★ " } else { "  " };

            let rate_badge = format!(
                " [{} {}]",
                station.codec,
                format_sample_rate(station.sample_rate)
            );

            let base_style = if is_selected {
                Style::default()
                    .fg(app.theme.selection_fg)
                    .bg(app.theme.selection_bg)
                    .add_modifier(Modifier::BOLD)
            } else if is_playing {
                Style::default()
                    .fg(app.theme.primary)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.foreground)
            };

            // Calculate name width to never collide with rate badge
            let badge_width = rate_badge.chars().count();
            let prefix_width = 4; // play_icon + fav_icon
            let max_name_width = (chunks[0]
                .width
                .saturating_sub(badge_width as u16 + prefix_width + 4)
                as usize)
                .max(12);
            let truncated_name = crate::util::truncate_str(&station.name, max_name_width);
            let padded_name = format!("{:<width$}", truncated_name, width = max_name_width);

            let content = Line::from(vec![
                Span::styled(
                    play_icon,
                    if is_playing {
                        Style::default().fg(app.theme.status_lossless)
                    } else {
                        base_style
                    },
                ),
                Span::styled(fav_icon, Style::default().fg(app.theme.status_warning)),
                Span::styled(padded_name, base_style),
                Span::styled(
                    rate_badge,
                    if is_selected {
                        Style::default().fg(app.theme.secondary)
                    } else {
                        Style::default().fg(app.theme.text_dim)
                    },
                ),
            ]);

            Some(ListItem::new(content))
        })
        .collect();

    let border_color = if is_focused {
        app.theme.border_active
    } else {
        app.theme.border
    };
    let title = if !app.filter_query.is_empty() {
        format!(
            " Stations ({}/{}) [Filter: \"{}\"] [Sort: {}] ",
            total_len,
            app.catalog.stations.len(),
            app.filter_query,
            app.station_sort_mode.display_label()
        )
    } else {
        format!(
            " Stations ({}) [Sort: {}] ",
            total_len,
            app.station_sort_mode.display_label()
        )
    };

    let list_widget = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(title)
                .border_style(Style::default().fg(border_color)),
        )
        .highlight_style(
            Style::default()
                .fg(app.theme.selection_fg)
                .bg(app.theme.selection_bg)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list_widget, chunks[0], &mut app.station_list_state);

    let visible_height = chunks[0].height.saturating_sub(2) as usize;
    if total_len > visible_height {
        let mut scrollbar_state = ScrollbarState::new(total_len).position(app.station_list_index);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .track_symbol(Some("│"))
            .thumb_symbol("█");
        f.render_stateful_widget(scrollbar, chunks[0], &mut scrollbar_state);
    }

    // Render Selected Station Tags & Info Card
    let selected_station = filtered_indices
        .get(app.station_list_index)
        .and_then(|&idx| app.catalog.stations.get(idx));
    let card_lines = if let Some(st) = selected_station {
        let mut tag_spans = vec![Span::styled(
            "🏷 Tags: ",
            Style::default()
                .fg(app.theme.text_dim)
                .add_modifier(Modifier::BOLD),
        )];
        if st.tags.is_empty() {
            tag_spans.push(Span::styled("none", Style::default().fg(app.theme.text_dim)));
        } else {
            for (ti, tag) in st.tags.iter().enumerate() {
                if ti > 0 {
                    tag_spans.push(Span::raw(" "));
                }
                tag_spans.push(Span::styled(
                    format!("[{}]", tag),
                    Style::default().fg(app.theme.secondary),
                ));
            }
        }

        let max_url_len = chunks[1].width.saturating_sub(8) as usize;
        let url_truncated = crate::util::truncate_str(&st.url, max_url_len);
        let url_line = vec![
            Span::styled("🔗 URL:  ", Style::default().fg(app.theme.text_dim)),
            Span::styled(
                url_truncated,
                Style::default().fg(app.theme.foreground),
            ),
        ];

        vec![Line::from(tag_spans), Line::from(url_line)]
    } else {
        vec![
            Line::styled(" No station selected", Style::default().fg(app.theme.text_dim)),
            Line::styled(
                " Press [a] to add a radio endpoint",
                Style::default().fg(app.theme.text_dim),
            ),
        ]
    };

    let tags_card = Paragraph::new(card_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Station Tags & Stream Endpoint ")
            .border_style(Style::default().fg(app.theme.border)),
    );
    f.render_widget(tags_card, chunks[1]);
}

/// Renders the right pane: Unified Now Playing (music player showcase) + Audiophile Diagnostics.
fn render_right_pane(f: &mut Frame, app: &mut App, area: Rect) {
    let diag_height = if area.height >= 20 { 6 } else { 5 };
    let player_height = area.height.saturating_sub(diag_height).max(8);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(player_height), // Unified Now Playing Card (Hero showcase)
            Constraint::Length(diag_height),   // Compact Audiophile Audio Path
        ])
        .split(area);

    render_now_playing(f, app, chunks[0]);
    render_audio_path_diagram(f, app, chunks[1]);
}

/// Renders the unified Now Playing card with large cover art, music player typography, and individual format pills in a vertical layout.
fn render_now_playing(f: &mut Frame, app: &mut App, area: Rect) {
    let is_playing = app.playback_state == PlaybackState::Playing;
    let card_border_color = if is_playing {
        app.theme.border_active
    } else {
        app.theme.border
    };

    let main_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Now Playing ")
        .border_style(Style::default().fg(card_border_color));

    let inner_area = main_block.inner(area);
    f.render_widget(main_block, area);

    if inner_area.width < 10 || inner_area.height < 3 {
        return;
    }

    // Playback State Badge
    let playback_badge = match app.playback_state {
        PlaybackState::Playing => Span::styled(
            " ▶ PLAYING ",
            Style::default()
                .fg(Color::White)
                .bg(app.theme.status_lossless)
                .add_modifier(Modifier::BOLD),
        ),
        PlaybackState::Paused => Span::styled(
            " ⏸ PAUSED ",
            Style::default()
                .fg(Color::Black)
                .bg(app.theme.status_warning)
                .add_modifier(Modifier::BOLD),
        ),
        _ => Span::styled(
            " ■ STOPPED ",
            Style::default()
                .fg(Color::White)
                .bg(app.theme.border)
                .add_modifier(Modifier::BOLD),
        ),
    };

    let is_fav = app
        .current_station
        .as_ref()
        .map(|s| s.favorite)
        .unwrap_or(false);
    let fav_span = if is_fav {
        Some(Span::styled(
            " ★ FAVORITE ",
            Style::default()
                .fg(app.theme.status_warning)
                .add_modifier(Modifier::BOLD),
        ))
    } else {
        None
    };

    let rec_badge = if app.is_recording {
        Some(Span::styled(
            " [● REC] ",
            Style::default()
                .fg(Color::White)
                .bg(app.theme.status_danger)
                .add_modifier(Modifier::BOLD),
        ))
    } else {
        None
    };

    let sleep_badge = app.sleep_timer_secs_remaining.map(|s| {
        let mins = s / 60;
        let secs = s % 60;
        Span::styled(
            format!(" ⏳ {:02}:{:02} ", mins, secs),
            Style::default()
                .fg(Color::Black)
                .bg(app.theme.status_warning)
                .add_modifier(Modifier::BOLD),
        )
    });

    let elapsed_badge = if app.playback_state == PlaybackState::Playing {
        if let Some(started) = app.track_started_at {
            let elapsed_secs = started.elapsed().as_secs();
            let mins = elapsed_secs / 60;
            let secs = elapsed_secs % 60;
            Some(Span::styled(
                format!(" ⏱ {:02}:{:02} ", mins, secs),
                Style::default()
                    .fg(app.theme.primary)
                    .bg(Color::Rgb(20, 35, 50))
                    .add_modifier(Modifier::BOLD),
            ))
        } else {
            None
        }
    } else {
        None
    };

    // Track Metadata & Music Player Typography
    let station_name = app
        .current_station
        .as_ref()
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "No Station Selected".to_string());

    let raw_title_opt = app
        .track_metadata
        .title
        .clone()
        .or_else(|| app.track_metadata.raw_title.clone());

    let track_title = if let Some(raw) = raw_title_opt {
        if raw.contains(".flac?")
            || raw.contains(".aac?")
            || raw.starts_with("http://")
            || raw.starts_with("https://")
            || (raw.ends_with(".flac") && raw.contains('-'))
        {
            if station_name != "No Station Selected" {
                format!("Live Broadcast • {}", station_name)
            } else {
                "Connecting to stream...".to_string()
            }
        } else {
            raw
        }
    } else {
        "No Track Playing".to_string()
    };

    let artist_name = app.track_metadata.artist.clone();

    let album_year = match (&app.track_metadata.album, &app.track_metadata.year) {
        (Some(alb), Some(yr)) => format!("{} ({})", alb, yr),
        (Some(alb), None) => alb.clone(),
        _ => String::new(),
    };

    // Format Spec Pills
    let codec_str = if app.stream_params.codec.is_empty() {
        "FLAC"
    } else {
        &app.stream_params.codec
    };
    let codec_pill = Span::styled(
        format!("[{}]", codec_str.to_uppercase()),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );

    let is_hi_res = app.stream_params.sample_rate >= 88200;
    let rate_color = if is_hi_res {
        Color::Rgb(255, 140, 255)
    } else {
        Color::Rgb(100, 220, 255)
    };
    let rate_pill = Span::styled(
        format!("[{}]", format_sample_rate(app.stream_params.sample_rate)),
        Style::default().fg(rate_color).add_modifier(Modifier::BOLD),
    );

    let bit_depth_val = app.stream_params.bit_depth.unwrap_or(16);
    let depth_str = app
        .stream_params
        .bit_depth
        .map(|b| format!("{}-bit", b))
        .unwrap_or_else(|| "16-bit".to_string());
    let depth_pill = Span::styled(
        format!("[{}]", depth_str),
        Style::default()
            .fg(Color::Rgb(255, 215, 100))
            .add_modifier(Modifier::BOLD),
    );

    let bitrate_val = app.stream_params.bitrate_kbps.unwrap_or(0);
    let bitrate_pill = Span::styled(
        format!("[{}]", crate::util::format_bitrate(bitrate_val)),
        Style::default().fg(Color::DarkGray),
    );

    let quality_badge = if is_hi_res || bit_depth_val >= 24 {
        Some(Span::styled(
            "[HI-RES LOSSLESS]",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(255, 200, 50))
                .add_modifier(Modifier::BOLD),
        ))
    } else if codec_str.to_uppercase().contains("FLAC") {
        Some(Span::styled(
            "[LOSSLESS]",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(50, 200, 120))
                .add_modifier(Modifier::BOLD),
        ))
    } else {
        None
    };

    // Header status badges
    let mut header_spans = vec![playback_badge];
    if let Some(f_span) = fav_span {
        header_spans.push(Span::raw(" "));
        header_spans.push(f_span);
    }
    if let Some(r_span) = rec_badge {
        header_spans.push(Span::raw(" "));
        header_spans.push(r_span);
    }
    if let Some(s_span) = sleep_badge {
        header_spans.push(Span::raw(" "));
        header_spans.push(s_span);
    }
    if let Some(e_span) = elapsed_badge {
        header_spans.push(Span::raw(" "));
        header_spans.push(e_span);
    }

    // Artist & Album info
    let mut artist_spans = Vec::new();
    if let Some(art) = artist_name {
        if !art.trim().is_empty() && art != "—" && art != "-" {
            artist_spans.push(Span::styled("♪ ", Style::default().fg(Color::Green)));
            artist_spans.push(Span::styled(
                art,
                Style::default()
                    .fg(Color::Rgb(100, 230, 160))
                    .add_modifier(Modifier::BOLD),
            ));
        }
    }
    if !album_year.is_empty() && album_year != "—" {
        if !artist_spans.is_empty() {
            artist_spans.push(Span::raw("  "));
        }
        artist_spans.push(Span::styled("💿 ", Style::default().fg(Color::DarkGray)));
        artist_spans.push(Span::styled(album_year, Style::default().fg(Color::Gray)));
    }

    // Assemble Format Component Pills
    let mut format_spans = vec![
        codec_pill,
        Span::raw(" "),
        rate_pill,
        Span::raw(" "),
        depth_pill,
        Span::raw(" "),
        bitrate_pill,
    ];
    if let Some(badge) = quality_badge {
        format_spans.push(Span::raw(" "));
        format_spans.push(badge);
    }

    // Adaptive Layout: Classic Left-Anchored Player when wide enough, vertical stack when narrow
    if inner_area.width >= 55 {
        // Square cover art: 1 character cell height ~ 2 cell width
        let max_allowed_width = (inner_area.width * 36) / 100;
        let available_height = inner_area.height.saturating_sub(1);
        let art_width = ((available_height * 2) as u16).min(max_allowed_width).max(10);
        let art_height = (art_width / 2).min(available_height);
        let left_col_width = art_width + 3; // Artwork width + 3 cols margin

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(left_col_width), // Left: Album Art
                Constraint::Min(25),                // Right: Track Details & Badges
            ])
            .split(inner_area);

        // 1. Render Cover Art on Left, centered vertically
        if chunks[0].width >= 8 && chunks[0].height >= 4 {
            let art_y = chunks[0].y + (chunks[0].height.saturating_sub(art_height)) / 2;
            let art_x = chunks[0].x + 1; // 1 col margin from left card border
            let art_rect = Rect {
                x: art_x,
                y: art_y,
                width: art_width,
                height: art_height,
            };
            app.cover_art_manager.render(art_rect, f.buffer_mut());
        }

        // 2. Assemble Metadata lines with balanced vertical breathing room
        let mut meta_lines = Vec::new();
        let add_spacing = chunks[1].height >= 8;

        // Line 1: Header status badges (Playback, Fav, Rec, Sleep, Elapsed Timer)
        meta_lines.push(Line::from(header_spans));

        if add_spacing {
            meta_lines.push(Line::raw(""));
        }

        // Line 2: Track Title (Large / Bold Hero)
        meta_lines.push(Line::from(vec![Span::styled(
            track_title,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )]));

        // Line 3: Artist & Album info
        if !artist_spans.is_empty() {
            meta_lines.push(Line::from(artist_spans));
        }

        // Line 4: Station Name
        meta_lines.push(Line::from(vec![
            Span::styled("📻 ", Style::default().fg(Color::Cyan)),
            Span::styled(
                station_name,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        if add_spacing {
            meta_lines.push(Line::raw(""));
        }

        // Line 5: Format Component Pills (Codec, Rate, Bits, Bitrate, Quality)
        meta_lines.push(Line::from(format_spans));

        // Center metadata vertically inside right pane so it balances with the left cover art
        let meta_height = meta_lines.len() as u16;
        let y_offset = (chunks[1].height.saturating_sub(meta_height)) / 2;
        let text_area = Rect {
            x: chunks[1].x,
            y: chunks[1].y + y_offset,
            width: chunks[1].width,
            height: chunks[1].height.saturating_sub(y_offset),
        };
        f.render_widget(Paragraph::new(meta_lines), text_area);
    } else {
        // Vertical Split: Metadata on top, Cover Art on bottom
        let mut meta_lines = Vec::new();
        meta_lines.push(Line::from(header_spans));
        meta_lines.push(Line::from(vec![Span::styled(
            track_title,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )]));
        if !artist_spans.is_empty() {
            meta_lines.push(Line::from(artist_spans));
        }
        meta_lines.push(Line::from(vec![
            Span::styled("📻 ", Style::default().fg(Color::Cyan)),
            Span::styled(
                station_name,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        meta_lines.push(Line::from(format_spans));

        let meta_height = (meta_lines.len() as u16).min(inner_area.height);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(meta_height), Constraint::Min(3)])
            .split(inner_area);

        f.render_widget(Paragraph::new(meta_lines), chunks[0]);

        if chunks.len() > 1 && chunks[1].height >= 4 && chunks[1].width >= 8 {
            let art_height = chunks[1].height.saturating_sub(1).min(12);
            let art_width = (art_height * 2).min(chunks[1].width);
            let art_x = chunks[1].x + (chunks[1].width.saturating_sub(art_width)) / 2;
            let art_y = chunks[1].y + (chunks[1].height.saturating_sub(art_height)) / 2;
            let art_rect = Rect {
                x: art_x,
                y: art_y,
                width: art_width,
                height: art_height,
            };
            app.cover_art_manager.render(art_rect, f.buffer_mut());
        }
    }
}

/// Renders the compact 3-stage Audio Path diagram and hardware diagnostics.
fn render_audio_path_diagram(f: &mut Frame, app: &App, area: Rect) {
    let status = &app.pipeline_status;

    let src_rate = format_sample_rate(status.reference_rate());
    let pw_rate = format_sample_rate(status.pw_sink_rate.unwrap_or(0));
    let dac_rate = format_sample_rate(
        status
            .alsa_hw_params
            .as_ref()
            .and_then(|h| h.rate)
            .unwrap_or(0),
    );
    let dac_fmt = status
        .alsa_hw_params
        .as_ref()
        .and_then(|h| h.format.as_deref())
        .unwrap_or("S24_3LE");

    let diagram_line = Line::from(vec![
        Span::styled(
            format!(" [1] Source: {} ", src_rate),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ──▶ ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!(" [2] PipeWire Sink: {} ", pw_rate),
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ──▶ ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!(" [3] USB DAC: {} ({}) ", dac_rate, dac_fmt),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let dac_name = status
        .alsa_card_name
        .as_deref()
        .unwrap_or("USB DAC / ALSA Hardware");

    let mut lines = vec![
        diagram_line,
        Line::from(vec![
            Span::styled("Hardware Target: ", Style::default().fg(Color::DarkGray)),
            Span::styled(dac_name, Style::default().fg(Color::White)),
        ]),
    ];

    // Hardware Volume Bar (wpctl) and optional culprit warning
    let vol = app.hardware_volume;
    let vol_bars = (vol * 20.0).round() as usize;
    let bar_str: String =
        "█".repeat(vol_bars.min(20)) + &"░".repeat(20usize.saturating_sub(vol_bars));

    let mut vol_spans = vec![
        Span::styled(
            "Hardware Vol (wpctl): ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("{:.0}% [{}]", vol * 100.0, bar_str),
            Style::default().fg(Color::Cyan),
        ),
    ];

    if app.is_muted {
        vol_spans.push(Span::styled(
            " [MUTED]",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    }

    if let Some(culprit) = status.pw_culprits.first() {
        vol_spans.push(Span::styled(
            format!("  [⚠ Culprit: {}]", culprit.description),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }

    lines.push(Line::from(vol_spans));

    // If bottom pane has extra vertical space (e.g. height >= 7), show signal integrity status
    if area.height >= 7 {
        lines.push(Line::raw(""));
        let (verdict_desc, verdict_color) = match &status.verdict {
            Some(BitPerfectVerdict::NativeBitPerfect) => (
                "● Signal Integrity: 1:1 Direct Bit-Perfect. Native hardware clock locked, 0 DSP.",
                Color::Green,
            ),
            Some(BitPerfectVerdict::Resampled { .. }) => (
                "▲ Signal Integrity: Audio resampled by PipeWire graph to match sink rate.",
                Color::Yellow,
            ),
            Some(BitPerfectVerdict::VolumeDegraded { .. }) => (
                "▲ Signal Integrity: Digital volume attenuation (<100%). Set hw vol to max for bit-perfect.",
                Color::Yellow,
            ),
            _ => (
                "● Signal Status: ALSA kernel node active. Press '3' for full Inspector matrix.",
                Color::DarkGray,
            ),
        };
        lines.push(Line::from(vec![Span::styled(
            verdict_desc,
            Style::default().fg(verdict_color),
        )]));
    }

    let diag_widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Audiophile Audio Path & PipeWire Diagnostics ")
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(diag_widget, area);
}

/// Renders the real-time spectrum visualizer bar.
fn render_visualizer(f: &mut Frame, app: &App, area: Rect) {
    let viz_widget = VisualizerWidget {
        state: &app.visualizer_state,
        primary_color: app.theme.primary,
        highlight_color: app.theme.secondary,
        low_color: Some(app.theme.spectrum_low),
        peak_color: Some(app.theme.spectrum_high),
    };
    f.render_widget(viz_widget, area);
}

/// Renders the bottom keybindings status bar or active toast notification.
fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    if let Some(ref toast) = app.toast {
        let (fg, bg, icon) = match toast.level {
            ToastLevel::Success => (Color::White, Color::Rgb(0, 140, 60), " ✓ "),
            ToastLevel::Error => (Color::White, Color::Rgb(200, 30, 30), " ✗ "),
            ToastLevel::Warning => (Color::Black, Color::Yellow, " ⚠ "),
            ToastLevel::Info => (Color::Black, Color::Cyan, " ℹ "),
        };
        let line = Line::from(vec![
            Span::styled(
                icon,
                Style::default()
                    .fg(fg)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", toast.message),
                Style::default()
                    .fg(fg)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        let toast_widget = Paragraph::new(line).alignment(Alignment::Center);
        f.render_widget(toast_widget, area);
        return;
    }

    let keys = vec![
        ("Space", "Pause"),
        ("Enter", "Play"),
        ("a", "Add"),
        ("e", "Edit"),
        ("d/x", "Del"),
        ("S", "Sort"),
        ("y/c", "Copy"),
        ("f", "Fav"),
        ("r", "Rec"),
        ("l", "Like"),
        ("+/-", "Vol"),
        ("/", "Filter"),
        ("1-5", "Tabs"),
        ("?", "Help"),
        ("q", "Quit"),
    ];

    let spans: Vec<Span> = keys
        .iter()
        .flat_map(|(k, v)| {
            vec![
                Span::styled(
                    format!("[{}]", k),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{} ", v), Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
            ]
        })
        .collect();

    let footer_widget = Paragraph::new(Line::from(spans)).alignment(Alignment::Center);
    f.render_widget(footer_widget, area);
}

/// Renders the Search tab targeting Radio-Browser API with 2-pane preview and discovery guide.
fn render_search_tab(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Query Input
            Constraint::Min(8),    // Results / Discovery Body
        ])
        .split(area);

    let query_text = format!("Search Query: {}█", app.search_query);
    let search_input = Paragraph::new(query_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Radio-Browser.info Lossless Search (Press Enter to query) ")
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(search_input, chunks[0]);

    let total_len = app.search_results.len();
    if total_len > 0 && app.search_results_index >= total_len {
        app.search_results_index = total_len - 1;
    }
    if total_len == 0 {
        app.search_list_state.select(None);
    } else {
        app.search_list_state.select(Some(app.search_results_index));
    }

    if total_len == 0 {
        // Helpful Discovery Guide when idle or no results
        let guide_lines = vec![
            Line::from(vec![
                Span::styled("📻 ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    "Lossless Radio Discovery Guide",
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("Type a query above and press ", Style::default().fg(Color::Gray)),
                Span::styled("[Enter]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(" to query 30,000+ public radio streams via Radio-Browser.info.", Style::default().fg(Color::Gray)),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("Curated Audiophile & Lossless Search Terms:", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("  • ", Style::default().fg(Color::Cyan)),
                Span::styled("flac", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("       - Lossless FLAC streams worldwide (CD-quality / Hi-Res)", Style::default().fg(Color::Gray)),
            ]),
            Line::from(vec![
                Span::styled("  • ", Style::default().fg(Color::Cyan)),
                Span::styled("jazz", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("       - High fidelity jazz, blues, and acoustic performances", Style::default().fg(Color::Gray)),
            ]),
            Line::from(vec![
                Span::styled("  • ", Style::default().fg(Color::Cyan)),
                Span::styled("classical", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("  - Symphonies, concertos, opera, and baroque archives", Style::default().fg(Color::Gray)),
            ]),
            Line::from(vec![
                Span::styled("  • ", Style::default().fg(Color::Cyan)),
                Span::styled("ambient", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("    - Atmospheric soundscapes, meditation, and chillout", Style::default().fg(Color::Gray)),
            ]),
            Line::from(vec![
                Span::styled("  • ", Style::default().fg(Color::Cyan)),
                Span::styled("electronic", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(" - Techno, synthwave, trance, and IDM broadcasts", Style::default().fg(Color::Gray)),
            ]),
            Line::from(vec![
                Span::styled("  • ", Style::default().fg(Color::Cyan)),
                Span::styled("96khz", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("      - Studio Master 24-bit 96 kHz high-resolution feeds", Style::default().fg(Color::Gray)),
            ]),
            Line::from(vec![
                Span::styled("  • ", Style::default().fg(Color::Cyan)),
                Span::styled("paradise", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("   - Radio Paradise commercial-free eclectic mixes", Style::default().fg(Color::Gray)),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("Actions: ", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
                Span::styled("[Enter] ", Style::default().fg(Color::Cyan)),
                Span::styled("Play Stream  •  ", Style::default().fg(Color::Gray)),
                Span::styled("['a'] ", Style::default().fg(Color::Yellow)),
                Span::styled("Add to Catalog  •  ", Style::default().fg(Color::Gray)),
                Span::styled("[j/k] ", Style::default().fg(Color::Cyan)),
                Span::styled("Browse Results", Style::default().fg(Color::Gray)),
            ]),
        ];

        let guide_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Radio Discovery ")
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = guide_block.inner(chunks[1]);
        f.render_widget(guide_block, chunks[1]);
        f.render_widget(Paragraph::new(guide_lines), inner);
        return;
    }

    // 2-Pane Results + Preview Layout when wide enough
    let (list_area, preview_area) = if chunks[1].width >= 90 {
        let sub = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(55), // Results List
                Constraint::Percentage(45), // Station Preview Card
            ])
            .split(chunks[1]);
        (sub[0], Some(sub[1]))
    } else {
        (chunks[1], None)
    };

    let items: Vec<ListItem> = app
        .search_results
        .iter()
        .enumerate()
        .map(|(idx, s)| {
            let is_sel = idx == app.search_results_index;
            let style = if is_sel {
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Rgb(30, 45, 60))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            let rate_badge = format!("[{}]", s.codec);
            let badge_width = rate_badge.len() + 2;
            let max_name = (list_area.width.saturating_sub(badge_width as u16 + 4) as usize).max(10);
            let trunc_name = truncate_str(&s.name, max_name);
            let padded_name = format!("{:<width$}", trunc_name, width = max_name);

            let line = Line::from(vec![
                Span::styled(padded_name, style),
                Span::styled(
                    format!(" {}", rate_badge),
                    if is_sel {
                        Style::default().fg(Color::Rgb(140, 200, 255)).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Cyan)
                    },
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let results_widget = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(format!(
                    " Search Results ({}) [Enter: Play | 'a': Add to Catalog] ",
                    total_len
                ))
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .highlight_style(
            Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(30, 45, 60))
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(results_widget, list_area, &mut app.search_list_state);

    let visible_height = list_area.height.saturating_sub(2) as usize;
    if total_len > visible_height {
        let mut scrollbar_state = ScrollbarState::new(total_len).position(app.search_results_index);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .track_symbol(Some("│"))
            .thumb_symbol("█");
        f.render_stateful_widget(scrollbar, list_area, &mut scrollbar_state);
    }

    // Right Preview Card for Selected Search Result
    if let Some(p_area) = preview_area {
        let selected_station = app.search_results.get(app.search_results_index);
        let preview_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Station Preview & Details ")
            .border_style(Style::default().fg(Color::Cyan));

        let inner = preview_block.inner(p_area);
        f.render_widget(preview_block, p_area);

        if let Some(s) = selected_station {
            let rate_str = if s.sample_rate > 0 {
                format_sample_rate(s.sample_rate)
            } else {
                "Lossless".to_string()
            };
            let bit_str = if s.bit_depth > 0 {
                format!("{}-bit", s.bit_depth)
            } else {
                "16/24-bit".to_string()
            };
            let tags_str = if s.tags.is_empty() {
                "none".to_string()
            } else {
                s.tags.join(", ")
            };
            let hp_str = s.homepage.as_deref().unwrap_or("—");

            let preview_lines = vec![
                Line::from(vec![
                    Span::styled("📻 Station:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&s.name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                ]),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("🎵 Audio:    ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("[{}] ", s.codec), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("[{}] ", rate_str), Style::default().fg(Color::Rgb(100, 220, 255)).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("[{}]", bit_str), Style::default().fg(Color::Rgb(255, 215, 100)).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("🏷 Tags:     ", Style::default().fg(Color::DarkGray)),
                    Span::styled(tags_str, Style::default().fg(Color::Yellow)),
                ]),
                Line::from(vec![
                    Span::styled("🔗 Stream:   ", Style::default().fg(Color::DarkGray)),
                    Span::styled(truncate_str(&s.url, p_area.width.saturating_sub(15) as usize), Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    Span::styled("🏠 Homepage: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(truncate_str(hp_str, p_area.width.saturating_sub(15) as usize), Style::default().fg(Color::Gray)),
                ]),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Actions: ", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
                    Span::styled("[Enter] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::styled("Play Stream  •  ", Style::default().fg(Color::Gray)),
                    Span::styled("['a'] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::styled("Add to Catalog", Style::default().fg(Color::Gray)),
                ]),
            ];

            f.render_widget(Paragraph::new(preview_lines), inner);
        }
    }
}

/// Truncates a string to max_len characters with ellipsis.
fn truncate_str(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
        format!("{}…", truncated)
    }
}

/// Renders the detailed Audiophile Inspector tab with visual pipeline stages and DAC capability matrix.
fn render_inspector_tab(f: &mut Frame, app: &App, area: Rect) {
    let vertical_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // 1. Verdict & Status Banner (3 inner lines)
            Constraint::Length(9), // 2. 3-Stage Visual Pipeline Graph (7 inner lines)
            Constraint::Min(8),    // 3. Diagnostics & Hardware Capability Matrix
        ])
        .split(area);

    render_inspector_verdict_banner(f, app, vertical_chunks[0]);
    render_inspector_pipeline_stages(f, app, vertical_chunks[1]);
    render_inspector_diagnostics_and_matrix(f, app, vertical_chunks[2]);
}

/// Renders the top status & bit-perfect verdict banner in the Inspector tab.
fn render_inspector_verdict_banner(f: &mut Frame, app: &App, area: Rect) {
    let status = &app.pipeline_status;
    let (border_color, title_badge, title_style, desc_lines) = match &status.verdict {
        Some(BitPerfectVerdict::NativeBitPerfect) => {
            let rate_str = format_sample_rate(status.reference_rate());
            let pw_sink_rate_str = format_sample_rate(status.pw_sink_rate.unwrap_or_else(|| status.reference_rate()));
            let dac_rate_str = format_sample_rate(status.alsa_hw_params.as_ref().and_then(|h| h.rate).unwrap_or_else(|| status.reference_rate()));
            (
                Color::Green,
                " [✓ DIRECT BIT-PERFECT PLAYBACK] ",
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                vec![
                    Line::from(vec![
                        Span::styled("SIGNAL PATH: ", Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("Source (FLAC {})", rate_str), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::styled(" ──▶ ", Style::default().fg(Color::Green)),
                        Span::styled(format!("PipeWire ({})", pw_sink_rate_str), Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                        Span::styled(" ──▶ ", Style::default().fg(Color::Green)),
                        Span::styled(format!("DAC ({})", dac_rate_str), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                        Span::styled(" [1:1 SYNC]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    ]),
                    Line::from(vec![
                        Span::styled("INTEGRITY:   ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Native clock locked. Zero software resampling, DSP equalizers, or bit degradation.", Style::default().fg(Color::White)),
                    ]),
                ],
            )
        }
        Some(BitPerfectVerdict::Resampled { source_rate, sink_rate, culprit }) => {
            let src_str = format_sample_rate(*source_rate);
            let snk_str = format_sample_rate(*sink_rate);
            let culprit_str = culprit.as_ref().map(|c| format!(" (Locked by: {})", c.name)).unwrap_or_default();
            (
                Color::Yellow,
                " [⚠ RESAMPLING DETECTED] ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                vec![
                    Line::from(vec![
                        Span::styled("SIGNAL PATH: ", Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("Source ({})", src_str), Style::default().fg(Color::Yellow)),
                        Span::styled(" ──▶ [Resampling Filter] ──▶ ", Style::default().fg(Color::Red)),
                        Span::styled(format!("DAC ({})", snk_str), Style::default().fg(Color::Yellow)),
                        Span::styled(" [RATE MISMATCH]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    ]),
                    Line::from(vec![
                        Span::styled("DIAGNOSIS:   ", Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("Stream sample rate ({}) is resampled to ({}) by PipeWire{}", src_str, snk_str, culprit_str), Style::default().fg(Color::White)),
                    ]),
                ],
            )
        }
        Some(BitPerfectVerdict::VolumeDegraded { mpv_volume }) => {
            (
                Color::Yellow,
                " [⚠ SOFTWARE VOLUME ATTENUATION] ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                vec![
                    Line::from(vec![
                        Span::styled("STATUS:      ", Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("MPV software decoder volume is at {:.1}% (Digital bit depth truncation).", mpv_volume), Style::default().fg(Color::Yellow)),
                    ]),
                    Line::from(vec![
                        Span::styled("ADVICE:      ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Set MPV volume to 100% and adjust listening level with hardware volume (`+/-`).", Style::default().fg(Color::Cyan)),
                    ]),
                ],
            )
        }
        Some(BitPerfectVerdict::DspFilterActive { filters }) => {
            (
                Color::Red,
                " [⚠ ACTIVE SOFTWARE DSP FILTERS] ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                vec![
                    Line::from(vec![
                        Span::styled("STATUS:      ", Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("Software DSP / Equalizer nodes active in graph: {}", filters.join(", ")), Style::default().fg(Color::Red)),
                    ]),
                    Line::from(vec![
                        Span::styled("ADVICE:      ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Bypass EasyEffects or equalizer filters for bit-exact audiophile reproduction.", Style::default().fg(Color::Cyan)),
                    ]),
                ],
            )
        }
        Some(BitPerfectVerdict::Idle) | None => {
            (
                Color::DarkGray,
                " [○ PIPELINE STANDBY / IDLE] ",
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
                vec![
                    Line::from(vec![
                        Span::styled("STATUS:      ", Style::default().fg(Color::DarkGray)),
                        Span::styled("No active lossless radio stream playing. Audio pipeline is in standby.", Style::default().fg(Color::Gray)),
                    ]),
                    Line::from(vec![
                        Span::styled("TIP:         ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Select and play a FLAC lossless station from Tab 1 to inspect live stream graph.", Style::default().fg(Color::Cyan)),
                    ]),
                ],
            )
        }
    };

    let title_line = Line::from(vec![
        Span::styled(title_badge, title_style),
        Span::raw(" "),
        Span::styled(
            "Audiophile Signal Integrity Report",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let mut content_lines = vec![title_line];
    content_lines.extend(desc_lines);

    let banner = Paragraph::new(content_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color)),
    );
    f.render_widget(banner, area);
}

/// Renders the 3-stage visual pipeline graph cards (Source ➔ PipeWire ➔ DAC).
fn render_inspector_pipeline_stages(f: &mut Frame, app: &App, area: Rect) {
    let status = &app.pipeline_status;

    let stage_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33), // Stage 1: Source (MPV)
            Constraint::Percentage(34), // Stage 2: PipeWire Graph
            Constraint::Percentage(33), // Stage 3: Hardware DAC
        ])
        .split(area);

    // Stage 1: Source (MPV Decoder)
    let codec = if status.source_codec.is_empty() {
        "FLAC"
    } else {
        &status.source_codec
    };
    let sample_rate = format_sample_rate(status.reference_rate());
    let bit_depth = status
        .source_bit_depth
        .map(|b| format!("{}-bit", b))
        .unwrap_or_else(|| "16/24-bit".to_string());
    let channels = if status.source_channels == 0 {
        "2 (Stereo)".to_string()
    } else {
        format!("{} ch", status.source_channels)
    };
    let bitrate = crate::util::format_bitrate(status.source_bitrate_kbps.unwrap_or(0));
    let vol_str = format!("{:.1}%", status.mpv_volume);
    let vol_style = if status.mpv_volume >= 99.5 {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Yellow)
    };

    let stage1_lines = vec![
        Line::from(vec![
            Span::styled("Codec:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                codec,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Rate:      ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                sample_rate,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Depth:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(bit_depth, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("Channels:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(channels, Style::default().fg(Color::Gray)),
        ]),
        Line::from(vec![
            Span::styled("Bitrate:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(bitrate, Style::default().fg(Color::Gray)),
        ]),
        Line::from(vec![
            Span::styled("MPV Vol:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(vol_str, vol_style.add_modifier(Modifier::BOLD)),
            Span::styled(
                if status.mpv_volume >= 99.5 {
                    " [✓ Exact]"
                } else {
                    " [⚠ Lossy]"
                },
                vol_style,
            ),
        ]),
        Line::from(vec![
            Span::styled("Stage 1:   ", Style::default().fg(Color::DarkGray)),
            Span::styled("──▶ Direct Stream Out", Style::default().fg(Color::Green)),
        ]),
    ];

    let stage1_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" 1. Stream Source (MPV) ")
        .border_style(Style::default().fg(Color::Cyan));
    f.render_widget(
        Paragraph::new(stage1_lines).block(stage1_block),
        stage_chunks[0],
    );

    // Stage 2: PipeWire Audio Graph
    let sink_desc = status
        .pw_sink
        .as_ref()
        .map(|s| s.description.clone())
        .unwrap_or_else(|| "No active sink".to_string());
    let sink_rate_val = status.pw_sink_rate.unwrap_or(0);
    let sink_rate = format_sample_rate(sink_rate_val);
    let ref_rate = status.reference_rate();
    let sink_rate_color = if sink_rate_val == ref_rate && ref_rate > 0 {
        Color::Green
    } else if sink_rate_val > 0 {
        Color::Yellow
    } else {
        Color::DarkGray
    };
    let sink_fmt = status
        .pw_sink
        .as_ref()
        .and_then(|s| s.format.clone())
        .unwrap_or_else(|| "Native".to_string());
    let dsp_str = if status.pw_dsp_filters.is_empty() {
        "None (Bit-exact)".to_string()
    } else {
        format!("Active ({})", status.pw_dsp_filters.len())
    };
    let dsp_color = if status.pw_dsp_filters.is_empty() {
        Color::Green
    } else {
        Color::Red
    };

    let allowed_str = if status.pw_allowed_rates.is_empty() {
        "Dynamic rates".to_string()
    } else {
        status
            .pw_allowed_rates
            .iter()
            .map(|r| {
                if r % 1000 == 0 {
                    format!("{}k", r / 1000)
                } else {
                    format!("{:.1}k", *r as f64 / 1000.0)
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };

    let sink_running = status
        .pw_sink
        .as_ref()
        .map(|s| s.is_running)
        .unwrap_or(false);

    let stage2_lines = vec![
        Line::from(vec![
            Span::styled("Sink Node: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                truncate_str(&sink_desc, 18),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::styled("Sink Rate: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                sink_rate,
                Style::default()
                    .fg(sink_rate_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if sink_rate_val == ref_rate && ref_rate > 0 {
                    " [MATCH]"
                } else {
                    ""
                },
                Style::default().fg(Color::Green),
            ),
        ]),
        Line::from(vec![
            Span::styled("Format:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(sink_fmt, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Clocks:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("[{}]", allowed_str),
                Style::default().fg(Color::Gray),
            ),
        ]),
        Line::from(vec![
            Span::styled("DSP Path:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(dsp_str, Style::default().fg(dsp_color)),
        ]),
        Line::from(vec![
            Span::styled("Graph:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                if sink_running {
                    "● Active Link"
                } else {
                    "○ Standby"
                },
                if sink_running {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
        ]),
        Line::from(vec![
            Span::styled("Stage 2:   ", Style::default().fg(Color::DarkGray)),
            Span::styled("──▶ Direct ALSA Link", Style::default().fg(Color::Green)),
        ]),
    ];

    let stage2_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" 2. PipeWire Audio Graph ")
        .border_style(Style::default().fg(Color::Blue));
    f.render_widget(
        Paragraph::new(stage2_lines).block(stage2_block),
        stage_chunks[1],
    );

    // Stage 3: Hardware DAC (ALSA)
    let dac_name = status
        .alsa_card_name
        .as_deref()
        .unwrap_or("ALSA Hardware Target");
    let hw = status.alsa_hw_params.as_ref();
    let hw_rate_val = hw.and_then(|h| h.rate).unwrap_or(0);
    let hw_rate = format_sample_rate(hw_rate_val);
    let hw_rate_color = if hw_rate_val == ref_rate && ref_rate > 0 {
        Color::Green
    } else if hw_rate_val > 0 {
        Color::Yellow
    } else {
        Color::DarkGray
    };
    let hw_fmt = hw
        .and_then(|h| h.format.clone())
        .unwrap_or_else(|| "Unknown".to_string());
    let hw_active = hw.map(|h| h.is_active).unwrap_or(false);
    let buffer_info = hw
        .map(|h| {
            format!(
                "{}/{}",
                h.period_size.unwrap_or(0),
                h.buffer_size.unwrap_or(0)
            )
        })
        .unwrap_or_else(|| "—".to_string());

    let stage3_lines = vec![
        Line::from(vec![
            Span::styled("DAC Card:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                truncate_str(dac_name, 18),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("HW Clock:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                hw_rate,
                Style::default()
                    .fg(hw_rate_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if hw_rate_val == ref_rate && ref_rate > 0 {
                    " [MATCH]"
                } else {
                    ""
                },
                Style::default().fg(Color::Green),
            ),
        ]),
        Line::from(vec![
            Span::styled("HW Format: ", Style::default().fg(Color::DarkGray)),
            Span::styled(hw_fmt, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Buffer:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(buffer_info, Style::default().fg(Color::Gray)),
        ]),
        Line::from(vec![
            Span::styled("ALSA State:", Style::default().fg(Color::DarkGray)),
            Span::styled(
                if hw_active {
                    "● Direct DMA"
                } else {
                    "○ Idle"
                },
                if hw_active {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
        ]),
        Line::from(vec![
            Span::styled("Physical:  ", Style::default().fg(Color::DarkGray)),
            Span::styled("USB Async DAC", Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("Stage 3:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "✓ Analog Audio Out",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    let stage3_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" 3. Hardware DAC / ALSA ")
        .border_style(Style::default().fg(Color::Green));
    f.render_widget(
        Paragraph::new(stage3_lines).block(stage3_block),
        stage_chunks[2],
    );
}

/// Renders the bottom diagnostic insights & DAC capability matrix.
fn render_inspector_diagnostics_and_matrix(f: &mut Frame, app: &App, area: Rect) {
    let status = &app.pipeline_status;

    let diag_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Left: PipeWire Graph & Conflict Analysis
            Constraint::Percentage(50), // Right: DAC Hardware Capability Matrix
        ])
        .split(area);

    // Left Box: PipeWire Graph & Conflict Analysis
    let mut left_lines = Vec::new();

    if !status.pw_culprits.is_empty() {
        left_lines.push(Line::from(vec![Span::styled(
            "⚠ ACTIVE CLOCK CONFLICT DETECTED:",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )]));
        for culprit in &status.pw_culprits {
            left_lines.push(Line::from(vec![
                Span::styled("  • Process:      ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    &culprit.name,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(
                        " (PID {})",
                        culprit
                            .pid
                            .map(|p| p.to_string())
                            .unwrap_or_else(|| "?".to_string())
                    ),
                    Style::default().fg(Color::Gray),
                ),
            ]));
            left_lines.push(Line::from(vec![
                Span::styled("  • Locking Rate: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format_sample_rate(culprit.locking_rate),
                    Style::default().fg(Color::Yellow),
                ),
            ]));
        }
        left_lines.push(Line::from(vec![Span::styled(
            "  ➜ Action:       Pause other audio apps to unlock dynamic 44.1k/96k rate switching.",
            Style::default().fg(Color::Cyan),
        )]));
    } else if !status.pw_dsp_filters.is_empty() {
        left_lines.push(Line::from(vec![Span::styled(
            "⚠ ACTIVE DSP FILTERS IN AUDIO PATH:",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )]));
        for f in &status.pw_dsp_filters {
            left_lines.push(Line::from(vec![
                Span::styled(
                    "  • Active Filter Node: ",
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(f, Style::default().fg(Color::Yellow)),
            ]));
        }
        left_lines.push(Line::from(vec![Span::styled(
            "  ➜ Action:             Disable DSP / equalizer filters for bit-exact reproduction.",
            Style::default().fg(Color::Cyan),
        )]));
    } else {
        left_lines.push(Line::from(vec![
            Span::styled("Routing:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "Direct Stream (No resamplers)",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        left_lines.push(Line::from(vec![
            Span::styled("Clocks:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "Dynamic frequency negotiation enabled",
                Style::default().fg(Color::White),
            ),
        ]));
        if let Some(sink) = &status.pw_sink {
            left_lines.push(Line::from(vec![
                Span::styled("Sink Node:  ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    truncate_str(&sink.name, 28),
                    Style::default().fg(Color::Cyan),
                ),
            ]));
        }
        left_lines.push(Line::from(vec![
            Span::styled("Hardware:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.0}% Master (wpctl)", app.hardware_volume * 100.0),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" (Bit-transparent)", Style::default().fg(Color::DarkGray)),
        ]));
        left_lines.push(Line::from(vec![
            Span::styled("DSP Chains: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "Bypassed (Bit-exact path)",
                Style::default().fg(Color::Green),
            ),
        ]));
        left_lines.push(Line::from(vec![
            Span::styled("Conflicts:  ", Style::default().fg(Color::DarkGray)),
            Span::styled("None (Clock unlocked)", Style::default().fg(Color::Cyan)),
        ]));
    }

    let left_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" PipeWire Diagnostics ")
        .border_style(Style::default().fg(if !status.pw_culprits.is_empty() {
            Color::Yellow
        } else {
            Color::DarkGray
        }));
    f.render_widget(Paragraph::new(left_lines).block(left_block), diag_chunks[0]);

    // Right Box: DAC Hardware Capability Matrix
    let mut right_lines = Vec::new();

    if let Some(caps) = &status.dac_capabilities {
        // Supported Rates Chips
        let mut rate_spans = vec![Span::styled(
            "Rates:      ",
            Style::default().fg(Color::DarkGray),
        )];
        let active_hw_rate = status
            .alsa_hw_params
            .as_ref()
            .and_then(|h| h.rate)
            .unwrap_or(0);

        for rate in &caps.supported_rates {
            let is_active = *rate == active_hw_rate && active_hw_rate > 0;
            let rate_label = if rate % 1000 == 0 {
                format!("{}k", rate / 1000)
            } else {
                format!("{:.1}k", *rate as f64 / 1000.0)
            };

            if is_active {
                rate_spans.push(Span::styled(
                    format!("[{}: ACTIVE]", rate_label),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                rate_spans.push(Span::styled(
                    format!("[{}]", rate_label),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            rate_spans.push(Span::raw(" "));
        }
        right_lines.push(Line::from(rate_spans));

        // Bit Depths Chips
        let mut depth_spans = vec![Span::styled(
            "Depths:     ",
            Style::default().fg(Color::DarkGray),
        )];
        for d in &caps.supported_bit_depths {
            let is_current = status.source_bit_depth == Some(*d);
            if is_current {
                depth_spans.push(Span::styled(
                    format!("[{}-bit: ACTIVE]", d),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                depth_spans.push(Span::styled(
                    format!("[{}-bit]", d),
                    Style::default().fg(Color::White),
                ));
            }
            depth_spans.push(Span::raw(" "));
        }
        right_lines.push(Line::from(depth_spans));

        // Hardware formats
        let formats_str: Vec<String> = caps.supported_formats.iter().cloned().collect();
        if !formats_str.is_empty() {
            right_lines.push(Line::from(vec![
                Span::styled("Formats:    ", Style::default().fg(Color::DarkGray)),
                Span::styled(formats_str.join(", "), Style::default().fg(Color::Gray)),
            ]));
        }

        // Buffer info
        if let Some(hw) = &status.alsa_hw_params {
            right_lines.push(Line::from(vec![
                Span::styled("Buffer:     ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!(
                        "{}p / {}f (Direct DMA Ring)",
                        hw.period_size.unwrap_or(0),
                        hw.buffer_size.unwrap_or(0)
                    ),
                    Style::default().fg(Color::White),
                ),
            ]));
        }

        // Card hardware model
        right_lines.push(Line::from(vec![
            Span::styled("Card Model: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                truncate_str(&caps.card_name, 28),
                Style::default().fg(Color::White),
            ),
        ]));

        // ALSA Stream path
        if let Some(card_idx) = status.alsa_card_index {
            right_lines.push(Line::from(vec![
                Span::styled("ALSA Node:  ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("/proc/asound/card{}/stream0", card_idx),
                    Style::default().fg(Color::Cyan),
                ),
            ]));
        }
    } else {
        right_lines.push(Line::from(vec![
            Span::styled("Rates:      ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "[ 44.1k ] [ 48.0k ] [ 88.2k ] [ 96.0k ] [ 192.0k ]",
                Style::default().fg(Color::Cyan),
            ),
        ]));
        right_lines.push(Line::from(vec![
            Span::styled("Depths:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "[ 16-bit ] [ 24-bit ] [ 32-bit ]",
                Style::default().fg(Color::White),
            ),
        ]));
        right_lines.push(Line::from(vec![
            Span::styled("DAC Device: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                status
                    .alsa_card_name
                    .as_deref()
                    .unwrap_or("Standard Linux Audio Device"),
                Style::default().fg(Color::Gray),
            ),
        ]));
        right_lines.push(Line::from(vec![
            Span::styled("ALSA Node:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "/proc/asound/cards (Active Sound Core)",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    let right_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" DAC Hardware Matrix ")
        .border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(
        Paragraph::new(right_lines).block(right_block),
        diag_chunks[1],
    );
}

/// Cleans markdown artifacts from legacy imported liked tracks.
fn clean_legacy_str(s: &str) -> String {
    let mut cleaned = s
        .replace("**", "")
        .replace('*', "")
        .replace('`', "");
    if let Some(pos) = cleaned.find("] ") {
        if cleaned.starts_with('[') && pos < 25 {
            cleaned = cleaned[pos + 2..].to_string();
        }
    }
    cleaned.trim().to_string()
}

/// Renders the Liked Tracks history tab with master list and detailed track card.
fn render_liked_tab(f: &mut Frame, app: &mut App, area: Rect) {
    let total_len = app.liked_tracks.len();
    if total_len > 0 && app.liked_list_index >= total_len {
        app.liked_list_index = total_len - 1;
    }
    if total_len == 0 {
        app.liked_list_state.select(None);
    } else {
        app.liked_list_state.select(Some(app.liked_list_index));
    }

    let (list_area, detail_area) = if area.width >= 90 {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(56), // List
                Constraint::Percentage(44), // Detail Card
            ])
            .split(area);
        (chunks[0], Some(chunks[1]))
    } else {
        (area, None)
    };

    let items: Vec<ListItem> = app
        .liked_tracks
        .iter()
        .enumerate()
        .map(|(idx, track)| {
            let is_sel = idx == app.liked_list_index;
            let text_style = if is_sel {
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Rgb(30, 45, 60))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            let clean_artist = clean_legacy_str(&track.artist);
            let clean_title = clean_legacy_str(&track.title);

            let rate_str = track
                .sample_rate
                .map(|r| format!(" {}", format_sample_rate(r)))
                .unwrap_or_default();
            let codec_badge = format!("[{}{}]", track.codec.as_deref().unwrap_or("FLAC"), rate_str);

            let line = Line::from(vec![
                Span::styled(
                    format!("[{}] ", track.timestamp),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(format!("{} - {}", clean_artist, clean_title), text_style),
                Span::styled(
                    format!(" [{}]", track.station),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!(" {}", codec_badge),
                    Style::default().fg(Color::Rgb(120, 180, 240)),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list_title = format!(
        " Liked Tracks ({}) [d/x: Delete | y/c: Copy | Enter: Play Station] ",
        total_len
    );

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(list_title)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .highlight_style(
            Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(30, 45, 60))
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, list_area, &mut app.liked_list_state);

    let visible_height = list_area.height.saturating_sub(2) as usize;
    if total_len > visible_height {
        let mut scrollbar_state = ScrollbarState::new(total_len).position(app.liked_list_index);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .track_symbol(Some("│"))
            .thumb_symbol("█");
        f.render_stateful_widget(scrollbar, list_area, &mut scrollbar_state);
    }

    // Detail Panel
    if let Some(d_area) = detail_area {
        let selected_track = app.liked_tracks.get(app.liked_list_index);
        let detail_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Bookmarked Track Details ")
            .border_style(Style::default().fg(Color::Cyan));

        let inner = detail_block.inner(d_area);
        f.render_widget(detail_block, d_area);

        if let Some(track) = selected_track {
            let clean_artist = clean_legacy_str(&track.artist);
            let clean_title = clean_legacy_str(&track.title);

            let rate_str = track
                .sample_rate
                .map(|r| format_sample_rate(r))
                .unwrap_or_else(|| "44.1 kHz".to_string());
            let bit_depth_str = track
                .bit_depth
                .map(|b| format!("{}-bit", b))
                .unwrap_or_else(|| "16-bit".to_string());
            let codec_str = track.codec.as_deref().unwrap_or("FLAC");

            let lines = vec![
                Line::from(vec![
                    Span::styled("🎵 Track:   ", Style::default().fg(Color::DarkGray)),
                    Span::styled(clean_title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("♪  Artist:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(clean_artist, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("💿 Album:   ", Style::default().fg(Color::DarkGray)),
                    Span::styled(track.album.as_deref().unwrap_or("Single / Live Broadcast"), Style::default().fg(Color::Gray)),
                ]),
                Line::from(vec![
                    Span::styled("📻 Station: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&track.station, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                ]),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Audio Spec: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("[{}] ", codec_str), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("[{}] ", rate_str), Style::default().fg(Color::Rgb(100, 220, 255)).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("[{}]", bit_depth_str), Style::default().fg(Color::Rgb(255, 215, 100)).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("🗓 Bookmarked: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&track.timestamp, Style::default().fg(Color::DarkGray)),
                ]),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Actions: ", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
                    Span::styled("[Enter] ", Style::default().fg(Color::Cyan)),
                    Span::styled("Tune Station  •  ", Style::default().fg(Color::Gray)),
                    Span::styled("[c/y] ", Style::default().fg(Color::Cyan)),
                    Span::styled("Copy  •  ", Style::default().fg(Color::Gray)),
                    Span::styled("[d/x] ", Style::default().fg(Color::Yellow)),
                    Span::styled("Delete", Style::default().fg(Color::Gray)),
                ]),
            ];

            let p = Paragraph::new(lines);
            f.render_widget(p, inner);
        } else {
            let empty_text = Paragraph::new("No track selected.\nPress 'l' while listening to bookmark tracks here.")
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(empty_text, inner);
        }
    }
}

/// Renders the Settings tab (Tab 5) for tuning reso options interactively.
fn render_settings_tab(f: &mut Frame, app: &mut App, area: Rect) {
    let s = &app.settings_state;

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(28), // Categories Navigation List
            Constraint::Min(40),    // Settings Form Panel
        ])
        .split(area);

    // Left Column: Categories
    let sections = [
        ("🎨 UI & Appearance", 0),
        ("📻 Playback & General", 1),
        ("📊 Scrobbler Services", 2),
        ("💾 Save Configuration", 3),
    ];

    let category_items: Vec<ListItem> = sections
        .iter()
        .map(|(label, idx)| {
            let is_sel = s.selected_section == *idx;
            let style = if is_sel {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            ListItem::new(format!("  {}  ", label)).style(style)
        })
        .collect();

    let category_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Sections [h/l: Tab] ")
        .border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(List::new(category_items).block(category_block), chunks[0]);

    // Right Column: Active Category Form
    let right_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Tune Configuration [j/k: Select | Space/Enter: Toggle/Edit] ")
        .border_style(Style::default().fg(Color::Cyan));

    let form_area = right_block.inner(chunks[1]);
    f.render_widget(right_block, chunks[1]);

    let mut form_lines: Vec<Line> = Vec::new();

    match s.selected_section {
        0 => {
            // UI & Appearance
            form_lines.push(Line::from(vec![
                Span::styled("APPEARANCE & DISPLAY PREFERENCES", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]));
            form_lines.push(Line::raw(""));

            // Field 0: Theme
            let sel0 = s.selected_field == 0;
            let cursor0 = if sel0 { "▶ " } else { "  " };
            let style0 = if sel0 { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Gray) };
            let hint = if s.theme == "wal" {
                " (Dynamic wallpaper palette from ~/.cache/wal)"
            } else {
                " (Space to cycle: audiophile_dark, cyberpunk, monochrome, wal)"
            };
            form_lines.push(Line::from(vec![
                Span::styled(cursor0, Style::default().fg(Color::Cyan)),
                Span::styled("Color Theme:                ", style0),
                Span::styled(format!("[ < {} > ]", s.theme), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(hint, Style::default().fg(Color::DarkGray)),
            ]));
            form_lines.push(Line::raw(""));

            // Field 1: Visualizer
            let sel1 = s.selected_field == 1;
            let cursor1 = if sel1 { "▶ " } else { "  " };
            let style1 = if sel1 { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Gray) };
            let (viz_check, viz_col) = if s.show_visualizer { ("[✓ Enabled]", Color::Green) } else { ("[✗ Disabled]", Color::DarkGray) };
            form_lines.push(Line::from(vec![
                Span::styled(cursor1, Style::default().fg(Color::Cyan)),
                Span::styled("Spectrum Visualizer:        ", style1),
                Span::styled(viz_check, Style::default().fg(viz_col).add_modifier(Modifier::BOLD)),
                Span::styled(" (Space to toggle)", Style::default().fg(Color::DarkGray)),
            ]));
            form_lines.push(Line::raw(""));

            // Field 2: Default Sort Mode
            let sel2 = s.selected_field == 2;
            let cursor2 = if sel2 { "▶ " } else { "  " };
            let style2 = if sel2 { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Gray) };
            let sort_label = StationSortMode::from_str_key(&s.default_sort_mode).display_label();
            form_lines.push(Line::from(vec![
                Span::styled(cursor2, Style::default().fg(Color::Cyan)),
                Span::styled("Default Station Sort Mode:  ", style2),
                Span::styled(format!("[ < {} > ]", sort_label), Style::default().fg(Color::Rgb(100, 220, 255)).add_modifier(Modifier::BOLD)),
                Span::styled(" (Space to cycle)", Style::default().fg(Color::DarkGray)),
            ]));
            form_lines.push(Line::raw(""));

            // Field 3: Cover Art Mode
            let sel3 = s.selected_field == 3;
            let cursor3 = if sel3 { "▶ " } else { "  " };
            let style3 = if sel3 { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Gray) };
            form_lines.push(Line::from(vec![
                Span::styled(cursor3, Style::default().fg(Color::Cyan)),
                Span::styled("Album Cover Renderer:       ", style3),
                Span::styled(format!("[ < {} > ]", s.cover_mode), Style::default().fg(Color::Rgb(255, 140, 255)).add_modifier(Modifier::BOLD)),
                Span::styled(" (auto, kitty, sixel, halfblocks, off)", Style::default().fg(Color::DarkGray)),
            ]));
        }
        1 => {
            // Playback & General
            form_lines.push(Line::from(vec![
                Span::styled("PLAYBACK & DIRECTORY CONFIGURATION", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]));
            form_lines.push(Line::raw(""));

            // Field 0: Default Station ID
            let sel0 = s.selected_field == 0;
            let cursor0 = if sel0 { "▶ " } else { "  " };
            let style0 = if sel0 { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Gray) };
            let val0 = if sel0 && s.is_editing_text { format!("{}█", s.text_buffer) } else { s.default_station_id.clone() };
            form_lines.push(Line::from(vec![
                Span::styled(cursor0, Style::default().fg(Color::Cyan)),
                Span::styled("Default Station Slug:       ", style0),
                Span::styled(format!("\"{}\"", val0), if sel0 && s.is_editing_text { Style::default().fg(Color::Yellow) } else { Style::default().fg(Color::Cyan) }),
                Span::styled(" (e.g. rp-main, Enter to edit)", Style::default().fg(Color::DarkGray)),
            ]));
            form_lines.push(Line::raw(""));

            // Field 1: Recording Directory
            let sel1 = s.selected_field == 1;
            let cursor1 = if sel1 { "▶ " } else { "  " };
            let style1 = if sel1 { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Gray) };
            let val1 = if sel1 && s.is_editing_text { format!("{}█", s.text_buffer) } else { s.record_dir.clone() };
            form_lines.push(Line::from(vec![
                Span::styled(cursor1, Style::default().fg(Color::Cyan)),
                Span::styled("Stream Recording Directory: ", style1),
                Span::styled(format!("\"{}\"", val1), if sel1 && s.is_editing_text { Style::default().fg(Color::Yellow) } else { Style::default().fg(Color::Cyan) }),
                Span::styled(" (Enter to edit)", Style::default().fg(Color::DarkGray)),
            ]));
            form_lines.push(Line::raw(""));

            // Field 2: MPV Socket Path
            let sel2 = s.selected_field == 2;
            let cursor2 = if sel2 { "▶ " } else { "  " };
            let style2 = if sel2 { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Gray) };
            let val2 = if sel2 && s.is_editing_text { format!("{}█", s.text_buffer) } else { s.socket_path.clone() };
            form_lines.push(Line::from(vec![
                Span::styled(cursor2, Style::default().fg(Color::Cyan)),
                Span::styled("MPV Domain Socket Path:     ", style2),
                Span::styled(format!("\"{}\"", val2), if sel2 && s.is_editing_text { Style::default().fg(Color::Yellow) } else { Style::default().fg(Color::Cyan) }),
                Span::styled(" (Enter to edit)", Style::default().fg(Color::DarkGray)),
            ]));
        }
        2 => {
            // Scrobbler Services
            form_lines.push(Line::from(vec![
                Span::styled("LAST.FM & LISTENBRAINZ AUDIOSCROBBLER", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]));
            form_lines.push(Line::raw(""));

            // Field 0: Last.fm Enabled
            let sel0 = s.selected_field == 0;
            let (l_check, l_col) = if s.lastfm_enabled { ("[✓ Enabled]", Color::Green) } else { ("[✗ Disabled]", Color::DarkGray) };
            form_lines.push(Line::from(vec![
                Span::styled(if sel0 { "▶ " } else { "  " }, Style::default().fg(Color::Cyan)),
                Span::styled("Last.fm Scrobbling:         ", if sel0 { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Gray) }),
                Span::styled(l_check, Style::default().fg(l_col).add_modifier(Modifier::BOLD)),
                Span::styled(" (Space to toggle)", Style::default().fg(Color::DarkGray)),
            ]));
            form_lines.push(Line::raw(""));

            // Field 1: Last.fm API Key
            let sel1 = s.selected_field == 1;
            let val1 = if sel1 && s.is_editing_text { format!("{}█", s.text_buffer) } else { if s.lastfm_api_key.is_empty() { "(not set)".to_string() } else { "••••••••••••••••".to_string() } };
            form_lines.push(Line::from(vec![
                Span::styled(if sel1 { "▶ " } else { "  " }, Style::default().fg(Color::Cyan)),
                Span::styled("Last.fm API Key:            ", if sel1 { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Gray) }),
                Span::styled(val1, Style::default().fg(Color::Cyan)),
                Span::styled(" (Enter to edit)", Style::default().fg(Color::DarkGray)),
            ]));
            form_lines.push(Line::raw(""));

            // Field 2: Last.fm Shared Secret
            let sel2 = s.selected_field == 2;
            let val2 = if sel2 && s.is_editing_text { format!("{}█", s.text_buffer) } else { if s.lastfm_api_secret.is_empty() { "(not set)".to_string() } else { "••••••••••••••••".to_string() } };
            form_lines.push(Line::from(vec![
                Span::styled(if sel2 { "▶ " } else { "  " }, Style::default().fg(Color::Cyan)),
                Span::styled("Last.fm Shared Secret:      ", if sel2 { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Gray) }),
                Span::styled(val2, Style::default().fg(Color::Cyan)),
                Span::styled(" (Enter to edit)", Style::default().fg(Color::DarkGray)),
            ]));
            form_lines.push(Line::raw(""));

            // Field 3: Last.fm Authorization Status
            let sel3 = s.selected_field == 3;
            let (auth_badge, auth_color, auth_hint) = if !s.lastfm_session_key.is_empty() {
                let user_disp = if s.lastfm_username.is_empty() { "Authorized".to_string() } else { format!("@{}", s.lastfm_username) };
                (format!("[✓ Linked as {}]", user_disp), Color::Green, " (Enter to re-authenticate)")
            } else if s.lastfm_pending_token.is_some() {
                ("[⏳ Pending Browser Approval...]".to_string(), Color::Yellow, " (Enter to finish linking)")
            } else {
                ("[✗ Not Linked]".to_string(), Color::Red, " (Enter to Authorize in Browser)")
            };
            form_lines.push(Line::from(vec![
                Span::styled(if sel3 { "▶ " } else { "  " }, Style::default().fg(Color::Cyan)),
                Span::styled("Last.fm Authorization:      ", if sel3 { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Gray) }),
                Span::styled(auth_badge, Style::default().fg(auth_color).add_modifier(Modifier::BOLD)),
                Span::styled(auth_hint, Style::default().fg(Color::DarkGray)),
            ]));
            form_lines.push(Line::raw(""));

            // Field 4: ListenBrainz Enabled
            let sel4 = s.selected_field == 4;
            let (b_check, b_col) = if s.listenbrainz_enabled { ("[✓ Enabled]", Color::Green) } else { ("[✗ Disabled]", Color::DarkGray) };
            form_lines.push(Line::from(vec![
                Span::styled(if sel4 { "▶ " } else { "  " }, Style::default().fg(Color::Cyan)),
                Span::styled("ListenBrainz Scrobbling:    ", if sel4 { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Gray) }),
                Span::styled(b_check, Style::default().fg(b_col).add_modifier(Modifier::BOLD)),
                Span::styled(" (Space to toggle)", Style::default().fg(Color::DarkGray)),
            ]));
            form_lines.push(Line::raw(""));

            // Field 5: ListenBrainz User Token
            let sel5 = s.selected_field == 5;
            let val5 = if sel5 && s.is_editing_text { format!("{}█", s.text_buffer) } else { if s.listenbrainz_token.is_empty() { "(not set)".to_string() } else { "••••••••••••••••".to_string() } };
            form_lines.push(Line::from(vec![
                Span::styled(if sel5 { "▶ " } else { "  " }, Style::default().fg(Color::Cyan)),
                Span::styled("ListenBrainz User Token:    ", if sel5 { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Gray) }),
                Span::styled(val5, Style::default().fg(Color::Cyan)),
                Span::styled(" (Enter to edit)", Style::default().fg(Color::DarkGray)),
            ]));
        }
        3 => {
            // Save & System
            form_lines.push(Line::from(vec![
                Span::styled("SYSTEM DIAGNOSTICS & PERSISTENCE", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]));
            form_lines.push(Line::raw(""));

            form_lines.push(Line::from(vec![
                Span::styled("• Audio Engine:        ", Style::default().fg(Color::DarkGray)),
                Span::styled("MPV Lossless Decoder (Bit-Exact Unity Volume)", Style::default().fg(Color::White)),
            ]));
            form_lines.push(Line::from(vec![
                Span::styled("• Sound Server:        ", Style::default().fg(Color::DarkGray)),
                Span::styled("PipeWire Dynamic Multi-Rate Clock Graph", Style::default().fg(Color::Cyan)),
            ]));
            form_lines.push(Line::from(vec![
                Span::styled("• Hardware Target:     ", Style::default().fg(Color::DarkGray)),
                Span::styled(app.pipeline_status.alsa_card_name.as_deref().unwrap_or("ALSA USB DAC"), Style::default().fg(Color::Green)),
            ]));
            form_lines.push(Line::from(vec![
                Span::styled("• Stations in Catalog:  ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{} lossless & hi-res stations", app.catalog.stations.len()), Style::default().fg(Color::Cyan)),
            ]));
            form_lines.push(Line::from(vec![
                Span::styled("• Bookmarked Tracks:   ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{} tracks in liked_tracks.toml", app.liked_tracks.len()), Style::default().fg(Color::Yellow)),
            ]));
            form_lines.push(Line::raw(""));
            form_lines.push(Line::raw(""));

            let save_btn = Line::from(vec![
                Span::styled("▶ ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    " [ SAVE SETTINGS TO DISK (~/.config/reso/config.toml) ] ",
                    Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  (Press Enter or Ctrl+S)", Style::default().fg(Color::DarkGray)),
            ]);
            form_lines.push(save_btn);
        }
        _ => {}
    }

    let form_paragraph = Paragraph::new(form_lines);
    f.render_widget(form_paragraph, form_area);
}

/// Renders the Add Station modal wizard (Input URL -> Probing -> Verified / Confirm -> Save).
fn render_add_station_modal(f: &mut Frame, state: &AddStationState, tick_count: u64, area: Rect) {
    let popup_width = (area.width * 80 / 100).clamp(58, 88);
    let popup_height = 15;
    let popup_area = Rect {
        x: (area.width.saturating_sub(popup_width)) / 2,
        y: (area.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, popup_area);

    match state.step {
        AddStationStep::Input => {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .title(" Add New Radio Endpoint URL ")
                .border_style(Style::default().fg(Color::Cyan));
            let inner = block.inner(popup_area);
            f.render_widget(block, popup_area);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // URL Input
                    Constraint::Length(3), // Name Input
                    Constraint::Length(3), // Tags Input
                    Constraint::Min(3),    // Helper / Duplicate warning
                ])
                .split(inner);

            // Field 0: URL
            let url_cursor = if state.active_field == 0 { "█" } else { "" };
            let url_style = if state.active_field == 0 {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let url_text = format!("{}{}", state.url_input, url_cursor);
            let url_widget = Paragraph::new(url_text).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" 1. Stream Endpoint URL (http:// or https://) * ")
                    .border_style(url_style),
            );
            f.render_widget(url_widget, chunks[0]);

            // Field 1: Name
            let name_cursor = if state.active_field == 1 { "█" } else { "" };
            let name_style = if state.active_field == 1 {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let name_text = format!("{}{}", state.name_input, name_cursor);
            let name_widget = Paragraph::new(name_text).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" 2. Station Name (Optional - auto-detected if blank) ")
                    .border_style(name_style),
            );
            f.render_widget(name_widget, chunks[1]);

            // Field 2: Tags
            let tags_cursor = if state.active_field == 2 { "█" } else { "" };
            let tags_style = if state.active_field == 2 {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let tags_text = format!("{}{}", state.tags_input, tags_cursor);
            let tags_widget = Paragraph::new(tags_text).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" 3. Genre Tags (Optional, e.g. jazz, hi-res, lossless) ")
                    .border_style(tags_style),
            );
            f.render_widget(tags_widget, chunks[2]);

            // Helper / Duplicate warning
            let mut helper_lines = Vec::new();
            if let Some(ref existing_name) = state.duplicate_warning {
                helper_lines.push(Line::from(vec![
                    Span::styled(
                        " ⚠ Note: ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("Station already exists in catalog: \"{}\"", existing_name),
                        Style::default().fg(Color::Yellow),
                    ),
                ]));
            } else {
                helper_lines.push(Line::raw(""));
            }

            helper_lines.push(Line::from(vec![
                Span::styled(
                    "[Enter] ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "Test Stream & Live Audio Preview     ",
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    "[Tab / ↑↓] ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("Cycle Fields", Style::default().fg(Color::DarkGray)),
            ]));
            helper_lines.push(Line::from(vec![
                Span::styled(
                    "[Ctrl+U] ",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "Clear Field                         ",
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    "[Esc] ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled("Cancel", Style::default().fg(Color::DarkGray)),
            ]));

            f.render_widget(
                Paragraph::new(helper_lines).alignment(Alignment::Center),
                chunks[3],
            );
        }

        AddStationStep::Probing => {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .title(" Probing Radio Stream & Starting Live Audio Preview... ")
                .border_style(Style::default().fg(Color::Yellow));
            let inner = block.inner(popup_area);
            f.render_widget(block, popup_area);

            let spinner_frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let spinner = spinner_frames[(tick_count as usize / 2) % spinner_frames.len()];

            let content = vec![
                Line::raw(""),
                Line::from(vec![
                    Span::styled(
                        format!("  {} ", spinner),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "Connecting to endpoint & inspecting stream audio format...",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("  Target URL: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&state.url_input, Style::default().fg(Color::Cyan)),
                ]),
                Line::raw(""),
                Line::from(vec![Span::styled(
                    "  🔊 Live audio preview playing via MPV client while probing ICY headers...",
                    Style::default().fg(Color::Rgb(140, 200, 255)),
                )]),
                Line::raw(""),
                Line::from(vec![
                    Span::styled(
                        "  [Esc] ",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "Cancel Probing & Restore Previous Playback",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
            ];
            f.render_widget(Paragraph::new(content), inner);
        }

        AddStationStep::Confirm => {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .title(" ✓ Stream Verified - Confirm Addition ")
                .border_style(Style::default().fg(Color::Green));
            let inner = block.inner(popup_area);
            f.render_widget(block, popup_area);

            let probe = state.probe_result.as_ref();
            let codec_str = probe.map(|p| p.codec.as_str()).unwrap_or("FLAC");
            let sr_str = probe
                .map(|p| format_sample_rate(p.sample_rate))
                .unwrap_or_else(|| "44.1 kHz".to_string());
            let bit_depth_str = probe
                .map(|p| format!("{}-bit", p.bit_depth))
                .unwrap_or_else(|| "16-bit".to_string());
            let is_lossless = probe.map(|p| p.is_lossless).unwrap_or(true);

            let codec_badge = if is_lossless {
                Span::styled(
                    format!(" [{} LOSSLESS] ", codec_str),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    format!(" [{}] ", codec_str),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
            };

            let rate_badge = Span::styled(
                format!(" [{} | {}] ", sr_str, bit_depth_str),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );

            let name_cursor = if state.active_field == 1 { "█" } else { "" };
            let tags_cursor = if state.active_field == 2 { "█" } else { "" };

            let content = vec![
                Line::from(vec![
                    Span::styled("Format:  ", Style::default().fg(Color::DarkGray)),
                    codec_badge,
                    Span::raw(" "),
                    rate_badge,
                ]),
                Line::from(vec![
                    Span::styled("URL:     ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&state.url_input, Style::default().fg(Color::White)),
                ]),
                Line::from(vec![
                    Span::styled("Name:    ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{}{}", state.name_input, name_cursor),
                        if state.active_field == 1 {
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        },
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Tags:    ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{}{}", state.tags_input, tags_cursor),
                        if state.active_field == 2 {
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        },
                    ),
                ]),
                Line::raw(""),
                Line::from(vec![
                    Span::styled(
                        "[Enter / y] ",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "Confirm & Save to stations.toml     ",
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        "[t] ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("Re-test Stream", Style::default().fg(Color::Gray)),
                ]),
                Line::from(vec![
                    Span::styled(
                        "[Tab / ↑↓] ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "Edit Station Name / Genre Tags     ",
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        "[Esc] ",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("Cancel & Discard", Style::default().fg(Color::DarkGray)),
                ]),
            ];

            f.render_widget(Paragraph::new(content), inner);
        }

        AddStationStep::Error => {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .title(" ✗ Stream Test Failed ")
                .border_style(Style::default().fg(Color::Red));
            let inner = block.inner(popup_area);
            f.render_widget(block, popup_area);

            let err_msg = state
                .probe_error
                .as_deref()
                .unwrap_or("Unable to connect to audio stream");

            let content = vec![
                Line::raw(""),
                Line::from(vec![
                    Span::styled(
                        "Error:  ",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(err_msg, Style::default().fg(Color::White)),
                ]),
                Line::from(vec![
                    Span::styled("Target: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&state.url_input, Style::default().fg(Color::Cyan)),
                ]),
                Line::raw(""),
                Line::from(vec![
                    Span::styled(
                        "[t / Enter] ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "Retry Probe Connection     ",
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        "[e / Tab] ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("Edit URL", Style::default().fg(Color::White)),
                ]),
                Line::from(vec![
                    Span::styled(
                        "[f] ",
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "Force Add Anyway                 ",
                        Style::default().fg(Color::Gray),
                    ),
                    Span::styled(
                        "[Esc] ",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("Cancel & Discard", Style::default().fg(Color::Gray)),
                ]),
            ];

            f.render_widget(Paragraph::new(content), inner);
        }
    }
}

/// Renders the Edit Station modal dialog.
fn render_edit_station_modal(f: &mut Frame, state: &EditStationState, area: Rect) {
    let popup_width = (area.width * 80 / 100).clamp(58, 88);
    let popup_height = 17;
    let popup_area = Rect {
        x: (area.width.saturating_sub(popup_width)) / 2,
        y: (area.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(format!(" Edit Station: {} ", state.original_id))
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Field 0: Name
            Constraint::Length(2), // Field 1: URL
            Constraint::Length(2), // Fields 2, 3, 4: Codec, Sample Rate, Bit Depth (Horizontal)
            Constraint::Length(2), // Field 5: Tags
            Constraint::Min(4),    // Status / Help
        ])
        .split(inner);

    // Field 0: Name
    let cur0 = if state.active_field == 0 { "█" } else { "" };
    let sty0 = if state.active_field == 0 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let name_line = Line::from(vec![
        Span::styled(" Name:       ", sty0),
        Span::styled(
            format!("{}{}", state.name_input, cur0),
            if state.active_field == 0 {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            },
        ),
    ]);
    f.render_widget(Paragraph::new(name_line), chunks[0]);

    // Field 1: URL
    let cur1 = if state.active_field == 1 { "█" } else { "" };
    let sty1 = if state.active_field == 1 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let url_line = Line::from(vec![
        Span::styled(" URL:        ", sty1),
        Span::styled(
            format!("{}{}", state.url_input, cur1),
            if state.active_field == 1 {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            },
        ),
    ]);
    f.render_widget(Paragraph::new(url_line), chunks[1]);

    // Row for Codec, Sample Rate, Bit Depth
    let row_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33), // Codec
            Constraint::Percentage(37), // Sample Rate
            Constraint::Percentage(30), // Bit Depth
        ])
        .split(chunks[2]);

    // Field 2: Codec
    let cur2 = if state.active_field == 2 { "█" } else { "" };
    let sty2 = if state.active_field == 2 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let codec_line = Line::from(vec![
        Span::styled(" Codec: ", sty2),
        Span::styled(
            format!("{}{}", state.codec_input, cur2),
            if state.active_field == 2 {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            },
        ),
    ]);
    f.render_widget(Paragraph::new(codec_line), row_chunks[0]);

    // Field 3: Sample Rate
    let cur3 = if state.active_field == 3 { "█" } else { "" };
    let sty3 = if state.active_field == 3 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let sr_line = Line::from(vec![
        Span::styled(" Rate(Hz): ", sty3),
        Span::styled(
            format!("{}{}", state.sample_rate_input, cur3),
            if state.active_field == 3 {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            },
        ),
    ]);
    f.render_widget(Paragraph::new(sr_line), row_chunks[1]);

    // Field 4: Bit Depth
    let cur4 = if state.active_field == 4 { "█" } else { "" };
    let sty4 = if state.active_field == 4 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let bits_line = Line::from(vec![
        Span::styled(" Bits: ", sty4),
        Span::styled(
            format!("{}{}", state.bit_depth_input, cur4),
            if state.active_field == 4 {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            },
        ),
    ]);
    f.render_widget(Paragraph::new(bits_line), row_chunks[2]);

    // Field 5: Tags
    let cur5 = if state.active_field == 5 { "█" } else { "" };
    let sty5 = if state.active_field == 5 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let tags_line = Line::from(vec![
        Span::styled(" Tags:       ", sty5),
        Span::styled(
            format!("{}{}", state.tags_input, cur5),
            if state.active_field == 5 {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            },
        ),
    ]);
    f.render_widget(Paragraph::new(tags_line), chunks[3]);

    // Status / Helper
    let mut footer_lines = Vec::new();
    if let Some(ref msg) = state.probe_message {
        footer_lines.push(Line::from(vec![Span::styled(
            format!(" {}", msg),
            if msg.contains("✓") {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Yellow)
            },
        )]));
    } else {
        footer_lines.push(Line::raw(""));
    }

    footer_lines.push(Line::from(vec![
        Span::styled(
            "[Enter] ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Save to stations.toml    ",
            Style::default().fg(Color::White),
        ),
        Span::styled(
            "[Ctrl+T / t] ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Re-probe URL Format", Style::default().fg(Color::Gray)),
    ]));
    footer_lines.push(Line::from(vec![
        Span::styled(
            "[Tab / ↑↓] ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Next Field    ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "[Ctrl+U] ",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Clear    ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "[Esc] ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::styled("Cancel", Style::default().fg(Color::DarkGray)),
    ]));

    f.render_widget(Paragraph::new(footer_lines), chunks[4]);
}

/// Renders confirmation modal for deleting a station.
fn render_confirm_delete_station_modal(f: &mut Frame, station_name: &str, area: Rect) {
    let popup_width = 60.min(area.width.saturating_sub(4));
    let popup_height = 8;
    let popup_area = Rect {
        x: (area.width.saturating_sub(popup_width)) / 2,
        y: (area.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(" Delete Station ")
        .border_style(Style::default().fg(Color::Red));
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    let content = vec![
        Line::raw(""),
        Line::from(vec![Span::styled(
            "Are you sure you want to remove this station?",
            Style::default().fg(Color::White),
        )]),
        Line::from(vec![
            Span::styled("Station: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                station_name,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "[y / Enter / d] ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled("Confirm Delete   ", Style::default().fg(Color::White)),
            Span::styled(
                "[n / Esc / c] ",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Cancel", Style::default().fg(Color::Gray)),
        ]),
    ];

    f.render_widget(Paragraph::new(content).alignment(Alignment::Center), inner);
}

/// Renders confirmation modal for deleting a liked track.
fn render_confirm_delete_liked_modal(f: &mut Frame, track_title: &str, area: Rect) {
    let popup_width = 60.min(area.width.saturating_sub(4));
    let popup_height = 8;
    let popup_area = Rect {
        x: (area.width.saturating_sub(popup_width)) / 2,
        y: (area.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(" Delete Liked Track ")
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    let content = vec![
        Line::raw(""),
        Line::from(vec![Span::styled(
            "Remove this track from liked history?",
            Style::default().fg(Color::White),
        )]),
        Line::from(vec![
            Span::styled("Track: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                track_title,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "[y / Enter / d] ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Confirm Delete   ", Style::default().fg(Color::White)),
            Span::styled(
                "[n / Esc / c] ",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Cancel", Style::default().fg(Color::Gray)),
        ]),
    ];

    f.render_widget(Paragraph::new(content).alignment(Alignment::Center), inner);
}

/// Renders the scrollable Help modal popup.
fn render_help_modal(f: &mut Frame, app: &App, area: Rect) {
    let popup_area = Rect {
        x: area.width / 8,
        y: area.height / 10,
        width: (area.width * 3 / 4).max(50).min(area.width),
        height: (area.height * 4 / 5).max(12).min(area.height),
    };

    f.render_widget(Clear, popup_area);

    let help_text = r#"
=============================================================================
                          RESO KEYBOARD SHORTCUTS
=============================================================================

  Playback Controls:
    Space               Toggle Play / Pause
    Enter / o           Play selected station / liked track / search result
    s                   Stop playback
    n / p               Next / Previous station
    + / =               Increase hardware volume (wpctl)
    - / _               Decrease hardware volume (wpctl)
    m                   Toggle mute

  Station & Playlist Management (Vim-Style):
    a                   Add new station endpoint URL (probe & verify format)
    e / c               Edit selected station parameters (codec, sample rate, bits, tags)
    d / x               Delete selected station / liked track (with confirmation)
    S                   Cycle station list sort mode (Default, Quality, Favs, A-Z, Codec)
    f                   Toggle favorite on selected station
    y / c               Yank / Copy station stream URL or track title to clipboard
    r                   Toggle lossless stream recording (~/Music/Radio/)
    l                   Like / Bookmark playing track (~/.config/reso/liked_tracks.toml)
    /                   Search Radio-Browser catalog / filter stations

  Navigation & Views:
    1                   Switch to Stations tab
    2                   Switch to Search tab (radio-browser.info discovery)
    3                   Switch to Audiophile Inspector tab
    4                   Switch to Liked Tracks history tab
    5                   Switch to Settings & Preferences tab
    Tab / BackTab       Cycle next / previous tab
    j / Down            Navigate down / Scroll help down
    k / Up              Navigate up / Scroll help up
    PgDn / d            Scroll down by page (5 lines)
    PgUp / u            Scroll up by page (5 lines)
    g / Home            Jump to top
    G / End             Jump to bottom
    ?                   Toggle this Help screen
    q / Esc             Quit Reso / Close modal

  Settings Tab Controls:
    Tab / BackTab       Cycle between settings categories (UI, Playback, Scrobbler, System)
    j / k               Select configuration field
    Space               Toggle boolean option or cycle setting choices
    Enter               Edit text field / activate button
    Ctrl+S              Save settings to disk (~/.config/reso/config.toml)

=============================================================================
             Scroll: [j/k/PgUp/PgDn/g/G]  •  Close: [?/Esc/q]
"#;

    let total_lines = help_text.lines().count();
    let inner_height = popup_area.height.saturating_sub(2) as usize;
    let max_scroll = total_lines.saturating_sub(inner_height);
    let scroll_pos = app.help_scroll.min(max_scroll);

    let help_widget = Paragraph::new(help_text)
        .scroll((scroll_pos as u16, 0))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .title(" Reso Audiophile Help ")
                .border_style(Style::default().fg(Color::Cyan)),
        );
    f.render_widget(help_widget, popup_area);

    if total_lines > inner_height {
        let mut scrollbar_state = ScrollbarState::new(max_scroll).position(scroll_pos);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .track_symbol(Some("│"))
            .thumb_symbol("█");
        f.render_stateful_widget(scrollbar, popup_area, &mut scrollbar_state);
    }
}
