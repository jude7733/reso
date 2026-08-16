//! Ratatui rendering engine for `reso` TUI dashboard.

use crate::events::PlaybackState;
use crate::inspector::status::BitPerfectVerdict;
use crate::tui::app::{ActiveTab, App};
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

    // Main layout: Header, Body, Visualizer/Footer
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header & Badges
            Constraint::Min(12),   // Main Dashboard Body
            Constraint::Length(4), // Spectrum Visualizer
            Constraint::Length(1), // Keybinds footer
        ])
        .split(area);

    render_header(f, app, main_chunks[0]);
    render_body(f, app, main_chunks[1]);
    render_visualizer(f, app, main_chunks[2]);
    render_footer(f, app, main_chunks[3]);

    // Render Search / Help Modals if active
    if app.show_help_modal {
        render_help_modal(f, area);
    }
}

/// Renders the top header bar with title, tabs, and status badges.
fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(20), // Logo & App Name
            Constraint::Min(30),    // Navigation Tabs
            Constraint::Length(35), // Status & Bit-Perfect Badges
        ])
        .split(area);

    // 1. Logo
    let logo_text = vec![Line::from(vec![
        Span::styled(
            "RESO ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("v0.1.0", Style::default().fg(Color::DarkGray)),
    ])];
    let logo_widget = Paragraph::new(logo_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(logo_widget, chunks[0]);

    // 2. Navigation Tabs
    let tab_items = [
        ("1:Stations", ActiveTab::Stations),
        ("2:Search", ActiveTab::Search),
        ("3:Inspector", ActiveTab::Inspector),
        ("4:Liked", ActiveTab::Liked),
    ];
    let tab_spans: Vec<Span> = tab_items
        .iter()
        .flat_map(|(name, tab)| {
            let is_active = app.active_tab == *tab;
            let style = if is_active {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            vec![Span::styled(format!(" {} ", name), style), Span::raw(" ")]
        })
        .collect();

    let tabs_widget = Paragraph::new(Line::from(tab_spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Views ")
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(tabs_widget, chunks[1]);

    // 3. Status Badges (Bit-Perfect, Recording, Sleep Timer)
    let mut badge_spans = Vec::new();

    // Sleep Timer
    if let Some(secs) = app.sleep_timer_secs_remaining {
        badge_spans.push(Span::styled(
            format!(" ⏳{} ", format_duration(secs)),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Recording Badge
    if app.is_recording {
        let pulse = if (app.tick_count / 5) % 2 == 0 {
            Color::Red
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

    // Bit-Perfect Badge
    match &app.pipeline_status.verdict {
        Some(BitPerfectVerdict::NativeBitPerfect) => {
            let rate_str = format_sample_rate(app.pipeline_status.source_rate);
            badge_spans.push(Span::styled(
                format!(" [✓ BIT-PERFECT: {}] ", rate_str),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        Some(BitPerfectVerdict::Resampled {
            source_rate,
            sink_rate,
            ..
        }) => {
            badge_spans.push(Span::styled(
                format!(
                    " [⚠ RESAMPLED: {}→{}] ",
                    format_sample_rate(*source_rate),
                    format_sample_rate(*sink_rate)
                ),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        Some(BitPerfectVerdict::VolumeDegraded { .. }) => {
            badge_spans.push(Span::styled(
                " [⚠ VOL < 100%] ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        Some(BitPerfectVerdict::DspFilterActive { .. }) => {
            badge_spans.push(Span::styled(
                " [⚠ DSP ACTIVE] ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ));
        }
        _ => {
            badge_spans.push(Span::styled(
                " [STANDBY] ",
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    let badges_widget = Paragraph::new(Line::from(badge_spans))
        .alignment(Alignment::Right)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
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
                    Constraint::Percentage(58), // Now Playing + Audiophile Inspector
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
    }
}

/// Renders the station catalog list on the left.
fn render_stations_list(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.active_tab == ActiveTab::Stations && !app.search_input_active;
    let filtered: Vec<crate::config::Station> =
        app.filtered_stations().into_iter().cloned().collect();
    let total_len = filtered.len();

    if total_len > 0 && app.station_list_index >= total_len {
        app.station_list_index = total_len - 1;
    }
    if total_len == 0 {
        app.station_list_state.select(None);
    } else {
        app.station_list_state.select(Some(app.station_list_index));
    }

    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(idx, station)| {
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
                "[{} {}]",
                station.codec,
                format_sample_rate(station.sample_rate)
            );

            let base_style = if is_selected {
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Rgb(30, 45, 60))
                    .add_modifier(Modifier::BOLD)
            } else if is_playing {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            let content = Line::from(vec![
                Span::styled(
                    play_icon,
                    if is_playing {
                        Style::default().fg(Color::Green)
                    } else {
                        base_style
                    },
                ),
                Span::styled(fav_icon, Style::default().fg(Color::Yellow)),
                Span::styled(format!("{:<28}", station.name), base_style),
                Span::styled(rate_badge, Style::default().fg(Color::DarkGray)),
            ]);

            ListItem::new(content)
        })
        .collect();

    let border_color = if is_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let title = format!(" Stations ({}) ", total_len);

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
                .fg(Color::White)
                .bg(Color::Rgb(30, 45, 60))
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list_widget, area, &mut app.station_list_state);

    let visible_height = area.height.saturating_sub(2) as usize;
    if total_len > visible_height {
        let mut scrollbar_state = ScrollbarState::new(total_len).position(app.station_list_index);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .track_symbol(Some("│"))
            .thumb_symbol("█");
        f.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

/// Renders the right pane: Now Playing (top) + Audiophile Diagnostics (bottom).
fn render_right_pane(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10), // Now Playing & Album Art
            Constraint::Min(8),     // Audio Path Diagram & Diagnostics
        ])
        .split(area);

    render_now_playing(f, app, chunks[0]);
    render_audio_path_diagram(f, app, chunks[1]);
}

/// Renders the Now Playing card with cover art, metadata, and stream bitrate.
fn render_now_playing(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(20), // Cover Art / Vinyl
            Constraint::Min(20),    // Track Metadata details
        ])
        .split(area);

    // Cover Art
    let cover_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Album Art ")
        .border_style(Style::default().fg(Color::DarkGray));
    let cover_inner = cover_block.inner(chunks[0]);
    f.render_widget(cover_block, chunks[0]);
    app.cover_art_manager.render(cover_inner, f.buffer_mut());

    // Track Metadata
    let station_name = app
        .current_station
        .as_ref()
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "No Station Selected".to_string());

    let track_title = app.track_metadata.title.clone().unwrap_or_else(|| {
        app.track_metadata
            .raw_title
            .clone()
            .unwrap_or_else(|| "Idle".to_string())
    });

    let artist_name = app
        .track_metadata
        .artist
        .clone()
        .unwrap_or_else(|| "—".to_string());

    let album_year = match (&app.track_metadata.album, &app.track_metadata.year) {
        (Some(alb), Some(yr)) => format!("{} ({})", alb, yr),
        (Some(alb), None) => alb.clone(),
        _ => "—".to_string(),
    };

    let stream_params = format!(
        "{} • {} • {} • {}",
        if app.stream_params.codec.is_empty() {
            "FLAC"
        } else {
            &app.stream_params.codec
        },
        format_sample_rate(app.stream_params.sample_rate),
        app.stream_params
            .bit_depth
            .map(|b| format!("{}-bit", b))
            .unwrap_or_else(|| "16/24-bit".to_string()),
        crate::util::format_bitrate(app.stream_params.bitrate_kbps.unwrap_or(0)),
    );

    let text = vec![
        Line::from(vec![
            Span::styled("Station: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                station_name,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Track:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                track_title,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Artist:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(artist_name, Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("Album:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(album_year, Style::default().fg(Color::Gray)),
        ]),
        Line::from(vec![
            Span::styled("Format:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(stream_params, Style::default().fg(Color::Yellow)),
        ]),
    ];

    let meta_widget = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Now Playing ")
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(meta_widget, chunks[1]);
}

/// Renders the 3-stage Audio Path diagram and hardware diagnostics.
fn render_audio_path_diagram(f: &mut Frame, app: &App, area: Rect) {
    let status = &app.pipeline_status;

    let src_rate = format_sample_rate(status.source_rate);
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

    // Culprit Warning
    if let Some(culprit) = status.pw_culprits.first() {
        lines.push(Line::from(vec![
            Span::styled(
                "⚠ Resampling Culprit: ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(&culprit.description, Style::default().fg(Color::Yellow)),
        ]));
    }

    // Hardware Volume Bar (wpctl)
    let vol = app.hardware_volume;
    let vol_bars = (vol * 20.0).round() as usize;
    let bar_str: String =
        "█".repeat(vol_bars.min(20)) + &"░".repeat(20usize.saturating_sub(vol_bars));
    lines.push(Line::from(vec![
        Span::styled(
            "Hardware Vol (wpctl): ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("{:.0}% [{}]", vol * 100.0, bar_str),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            if app.is_muted { " [MUTED]" } else { "" },
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
    ]));

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
        primary_color: Color::Cyan,
        highlight_color: Color::Rgb(140, 200, 255),
    };
    f.render_widget(viz_widget, area);
}

/// Renders the bottom keybindings status bar.
fn render_footer(f: &mut Frame, _app: &App, area: Rect) {
    let keys = vec![
        ("Space", "Pause"),
        ("Enter", "Play"),
        ("f", "Fav"),
        ("r", "Rec"),
        ("l", "Like"),
        ("c", "Copy"),
        ("+/-", "Vol"),
        ("/", "Filter"),
        ("1-4", "Tabs"),
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

/// Renders the Search tab targeting Radio-Browser API.
fn render_search_tab(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Query Input
            Constraint::Min(8),    // Results List
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

            let line = Line::from(vec![
                Span::styled(format!("{:<32}", s.name), style),
                Span::styled(format!("[{}] ", s.codec), Style::default().fg(Color::Cyan)),
                Span::styled(s.tags.join(", "), Style::default().fg(Color::DarkGray)),
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
                    " Search Results ({}) - [Enter: Play | 'a': Add to Catalog] ",
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

    f.render_stateful_widget(results_widget, chunks[1], &mut app.search_list_state);

    let visible_height = chunks[1].height.saturating_sub(2) as usize;
    if total_len > visible_height {
        let mut scrollbar_state = ScrollbarState::new(total_len).position(app.search_results_index);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .track_symbol(Some("│"))
            .thumb_symbol("█");
        f.render_stateful_widget(scrollbar, chunks[1], &mut scrollbar_state);
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
            let rate_str = format_sample_rate(status.source_rate);
            let pw_sink_rate_str = format_sample_rate(status.pw_sink_rate.unwrap_or(status.source_rate));
            let dac_rate_str = format_sample_rate(status.alsa_hw_params.as_ref().and_then(|h| h.rate).unwrap_or(status.source_rate));
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
    let sample_rate = format_sample_rate(status.source_rate);
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
    let sink_rate_color = if sink_rate_val == status.source_rate && status.source_rate > 0 {
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
                if sink_rate_val == status.source_rate && status.source_rate > 0 {
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
    let hw_rate_color = if hw_rate_val == status.source_rate && status.source_rate > 0 {
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
                if hw_rate_val == status.source_rate && status.source_rate > 0 {
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

/// Renders the Liked Tracks history tab.
fn render_liked_tab(f: &mut Frame, app: &mut App, area: Rect) {
    let total_len = app.liked_tracks_history.len();
    if total_len > 0 && app.liked_list_index >= total_len {
        app.liked_list_index = total_len - 1;
    }
    if total_len == 0 {
        app.liked_list_state.select(None);
    } else {
        app.liked_list_state.select(Some(app.liked_list_index));
    }

    let items: Vec<ListItem> = app
        .liked_tracks_history
        .iter()
        .enumerate()
        .map(|(idx, line)| {
            let is_sel = idx == app.liked_list_index;
            let style = if is_sel {
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Rgb(30, 45, 60))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            ListItem::new(Span::styled(line, style))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(format!(
                    " Liked Tracks History ({}) (~/.config/reso/liked_tracks.md) ",
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

    f.render_stateful_widget(list, area, &mut app.liked_list_state);

    let visible_height = area.height.saturating_sub(2) as usize;
    if total_len > visible_height {
        let mut scrollbar_state = ScrollbarState::new(total_len).position(app.liked_list_index);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .track_symbol(Some("│"))
            .thumb_symbol("█");
        f.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

/// Renders the Help modal popup.
fn render_help_modal(f: &mut Frame, area: Rect) {
    let popup_area = Rect {
        x: area.width / 6,
        y: area.height / 6,
        width: area.width * 2 / 3,
        height: area.height * 2 / 3,
    };

    f.render_widget(Clear, popup_area);

    let help_text = r#"
=============================================================================
                          RESO KEYBOARD SHORTCUTS
=============================================================================

  Playback Controls:
    Space               Toggle Play / Pause
    Enter               Play selected station / search result
    s                   Stop playback
    n / p               Next / Previous station
    + / =               Increase hardware volume (wpctl)
    - / _               Decrease hardware volume (wpctl)
    m                   Toggle mute

  Audiophile & Actions:
    f                   Toggle favorite on selected station
    r                   Toggle lossless stream recording (~/Music/Radio/)
    l                   Like / Bookmark track (~/.config/reso/liked_tracks.md)
    c                   Copy current track title to clipboard
    /                   Filter station list / Search

  Navigation:
    1                   Switch to Stations tab
    2                   Switch to Search tab (radio-browser.info)
    3                   Switch to Audiophile Inspector tab
    4                   Switch to Liked Tracks history tab
    j / Down            Navigate down
    k / Up              Navigate up
    PgDn / PgUp         Scroll by page (5 items)
    g / Home            Jump to top
    G / End             Jump to bottom
    ?                   Toggle this Help screen
    q / Esc             Quit Reso

=============================================================================
                     Press '?' or 'Esc' to close
"#;

    let help_widget = Paragraph::new(help_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .title(" Reso Audiophile Help ")
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(help_widget, popup_area);
}
