//! Ratatui rendering engine for `reso` TUI dashboard.

use crate::events::PlaybackState;
use crate::inspector::status::BitPerfectVerdict;
use crate::tui::app::{ActiveTab, App};
use crate::tui::visualizer::VisualizerWidget;
use crate::util::{format_duration, format_sample_rate};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap};
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
        Span::styled("⚡ RESO ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
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
            vec![
                Span::styled(format!(" {} ", name), style),
                Span::raw(" "),
            ]
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
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
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
            Style::default().fg(Color::White).bg(pulse).add_modifier(Modifier::BOLD),
        ));
    }

    // Bit-Perfect Badge
    match &app.pipeline_status.verdict {
        Some(BitPerfectVerdict::NativeBitPerfect) => {
            let rate_str = format_sample_rate(app.pipeline_status.source_rate);
            badge_spans.push(Span::styled(
                format!(" ✓ BIT-PERFECT ({}) ", rate_str),
                Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD),
            ));
        }
        Some(BitPerfectVerdict::Resampled { source_rate, sink_rate, .. }) => {
            badge_spans.push(Span::styled(
                format!(" ⚠ RESAMPLED ({}→{}) ", format_sample_rate(*source_rate), format_sample_rate(*sink_rate)),
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
        }
        Some(BitPerfectVerdict::VolumeDegraded { .. }) => {
            badge_spans.push(Span::styled(
                " ⚠ VOL < 100% ",
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
        }
        Some(BitPerfectVerdict::DspFilterActive { .. }) => {
            badge_spans.push(Span::styled(
                " ⚠ DSP ACTIVE ",
                Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD),
            ));
        }
        _ => {
            badge_spans.push(Span::styled(
                " STANDBY ",
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
fn render_stations_list(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.active_tab == ActiveTab::Stations && !app.search_input_active;

    let items: Vec<ListItem> = app
        .filtered_stations()
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

            let rate_badge = format!("[{} {}]", station.codec, format_sample_rate(station.sample_rate));

            let base_style = if is_selected {
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Rgb(30, 45, 60))
                    .add_modifier(Modifier::BOLD)
            } else if is_playing {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            let content = Line::from(vec![
                Span::styled(play_icon, if is_playing { Style::default().fg(Color::Green) } else { base_style }),
                Span::styled(fav_icon, Style::default().fg(Color::Yellow)),
                Span::styled(format!("{:<28}", station.name), base_style),
                Span::styled(rate_badge, Style::default().fg(Color::DarkGray)),
            ]);

            ListItem::new(content)
        })
        .collect();

    let border_color = if is_focused { Color::Cyan } else { Color::DarkGray };
    let title = format!(" Stations ({}) ", app.catalog.stations.len());

    let list_widget = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(title)
            .border_style(Style::default().fg(border_color)),
    );

    f.render_widget(list_widget, area);
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

    let track_title = app
        .track_metadata
        .title
        .clone()
        .unwrap_or_else(|| app.track_metadata.raw_title.clone().unwrap_or_else(|| "Idle".to_string()));

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
        if app.stream_params.codec.is_empty() { "FLAC" } else { &app.stream_params.codec },
        format_sample_rate(app.stream_params.sample_rate),
        app.stream_params.bit_depth.map(|b| format!("{}-bit", b)).unwrap_or_else(|| "16/24-bit".to_string()),
        crate::util::format_bitrate(app.stream_params.bitrate_kbps.unwrap_or(0)),
    );

    let text = vec![
        Line::from(vec![
            Span::styled("Station: ", Style::default().fg(Color::DarkGray)),
            Span::styled(station_name, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Track:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(track_title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
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
    let dac_rate = format_sample_rate(status.alsa_hw_params.as_ref().and_then(|h| h.rate).unwrap_or(0));
    let dac_fmt = status.alsa_hw_params.as_ref().and_then(|h| h.format.as_deref()).unwrap_or("S24_3LE");

    let diagram_line = Line::from(vec![
        Span::styled(format!(" [1] Source: {} ", src_rate), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(" ──▶ ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!(" [2] PipeWire Sink: {} ", pw_rate), Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
        Span::styled(" ──▶ ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!(" [3] USB DAC: {} ({}) ", dac_rate, dac_fmt), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
    ]);

    let dac_name = status.alsa_card_name.as_deref().unwrap_or("USB DAC / ALSA Hardware");

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
            Span::styled("⚠ Resampling Culprit: ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled(&culprit.description, Style::default().fg(Color::Yellow)),
        ]));
    }

    // Hardware Volume Bar (wpctl)
    let vol = app.hardware_volume;
    let vol_bars = (vol * 20.0).round() as usize;
    let bar_str: String = "█".repeat(vol_bars.min(20)) + &"░".repeat(20usize.saturating_sub(vol_bars));
    lines.push(Line::from(vec![
        Span::styled("Hardware Vol (wpctl): ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:.0}% [{}]", vol * 100.0, bar_str), Style::default().fg(Color::Cyan)),
        Span::styled(if app.is_muted { " [MUTED]" } else { "" }, Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
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
                Span::styled(format!("[{}]", k), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{} ", v), Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
            ]
        })
        .collect();

    let footer_widget = Paragraph::new(Line::from(spans)).alignment(Alignment::Center);
    f.render_widget(footer_widget, area);
}

/// Renders the Search tab targeting Radio-Browser API.
fn render_search_tab(f: &mut Frame, app: &App, area: Rect) {
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

    let items: Vec<ListItem> = app
        .search_results
        .iter()
        .enumerate()
        .map(|(idx, s)| {
            let is_sel = idx == app.search_results_index;
            let style = if is_sel {
                Style::default().fg(Color::White).bg(Color::Rgb(30, 45, 60)).add_modifier(Modifier::BOLD)
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

    let results_widget = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(format!(" Search Results ({}) - [Enter: Play | 'a': Add to Catalog] ", app.search_results.len()))
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(results_widget, chunks[1]);
}

/// Renders the detailed Audiophile Inspector tab.
fn render_inspector_tab(f: &mut Frame, app: &App, area: Rect) {
    let report_text = app.pipeline_status.format_cli_report();
    let p = Paragraph::new(report_text)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" Full Hardware & PipeWire Graph Inspection Report ")
                .border_style(Style::default().fg(Color::Cyan)),
        );
    f.render_widget(p, area);
}

/// Renders the Liked Tracks history tab.
fn render_liked_tab(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .liked_tracks_history
        .iter()
        .map(|line| ListItem::new(Span::styled(line, Style::default().fg(Color::Gray))))
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Liked Tracks History (~/.config/reso/liked_tracks.md) ")
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(list, area);
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
