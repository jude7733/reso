//! Main entry point for `reso` internet radio player and audiophile inspector.

pub mod cli;
pub mod config;
pub mod error;
pub mod events;
pub mod inspector;
pub mod integration;
pub mod mpv;
pub mod tui;
pub mod util;

use clap::Parser;
use cli::{Cli, Commands};
use config::ConfigManager;
use error::{ResoError, Result};
use events::AudioStreamParams;
use inspector::status::PipelineStatus;
use integration::api::ApiClient;
use integration::clipboard::copy_to_clipboard;
use integration::mpris::start_mpris_server;
use mpv::client::MpvClient;
use mpv::process::MpvProcessManager;
use mpv::record::StreamRecorder;
use std::process::Command;
use tokio::signal;
use tokio::sync::mpsc;
use util::{format_duration, format_sample_rate, parse_duration_str};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config_manager = ConfigManager::load_or_init()?;

    match cli.command {
        None | Some(Commands::Tui) => {
            tui::run_tui(config_manager).await?;
        }

        Some(Commands::List(args)) => {
            handle_list_command(&config_manager, args.favorites, args.json)?;
        }

        Some(Commands::Add(args)) => {
            handle_add_command(config_manager, args).await?;
        }

        Some(Commands::Edit(args)) => {
            handle_edit_command(config_manager, args)?;
        }

        Some(Commands::Delete(args)) => {
            handle_delete_command(config_manager, args.station)?;
        }

        Some(Commands::Play(args)) => {
            handle_play_command(config_manager, args.station, args.sleep, args.foreground).await?;
        }

        Some(Commands::Pause) => {
            let (client, _) = get_mpv_client(&config_manager).await?;
            client.pause().await?;
            println!("Playback paused.");
        }

        Some(Commands::Resume) => {
            let (client, _) = get_mpv_client(&config_manager).await?;
            client.resume().await?;
            println!("Playback resumed.");
        }

        Some(Commands::Toggle) => {
            let (client, _) = get_mpv_client(&config_manager).await?;
            client.toggle_pause().await?;
            println!("Toggled playback state.");
        }

        Some(Commands::Stop) => {
            let (client, _) = get_mpv_client(&config_manager).await?;
            client.stop().await?;
            println!("Playback stopped.");
        }

        Some(Commands::Next) => {
            handle_cycle_station(&config_manager, true).await?;
        }

        Some(Commands::Prev) => {
            handle_cycle_station(&config_manager, false).await?;
        }

        Some(Commands::Current(args)) => {
            handle_current_command(&config_manager, args.json).await?;
        }

        Some(Commands::Search(args)) => {
            handle_search_command(
                config_manager,
                args.query,
                args.codec,
                args.bitrate,
                args.limit,
                args.add,
            )
            .await?;
        }

        Some(Commands::Inspect(args)) => {
            handle_inspect_command(&config_manager, args.json).await?;
        }

        Some(Commands::Record(args)) => {
            handle_record_command(&config_manager, &args.action, args.output).await?;
        }

        Some(Commands::Like) => {
            let mut cm = config_manager;
            handle_like_command(&mut cm).await?;
        }

        Some(Commands::Liked(args)) => {
            handle_liked_command(config_manager, args.action, args.id, args.json)?;
        }

        Some(Commands::Copy) => {
            handle_copy_command(&config_manager).await?;
        }

        Some(Commands::Volume(args)) => {
            handle_volume_command(&args.action)?;
        }

        Some(Commands::Daemon) => {
            handle_daemon_command(config_manager).await?;
        }
    }

    Ok(())
}

/// Connects to the MPV IPC server or starts it if required.
async fn get_mpv_client(
    config_manager: &ConfigManager,
) -> Result<(
    MpvClient,
    tokio::sync::broadcast::Receiver<events::MpvEvent>,
)> {
    let mut mpv_proc = MpvProcessManager::new(config_manager.config.socket_path.clone());
    mpv_proc.ensure_running().await?;
    mpv_proc.disown();
    MpvClient::connect(config_manager.config.socket_path.clone()).await
}

/// Handles `reso list`.
fn handle_list_command(
    config_manager: &ConfigManager,
    favorites_only: bool,
    as_json: bool,
) -> Result<()> {
    let stations: Vec<_> = config_manager
        .catalog
        .stations
        .iter()
        .filter(|s| !favorites_only || s.favorite)
        .collect();

    if as_json {
        let json = serde_json::to_string_pretty(&stations)?;
        println!("{}", json);
        return Ok(());
    }

    println!("================================================================================");
    println!("                           RESO CONFIGURED STATIONS                             ");
    println!("================================================================================");
    println!(
        "{:<14} {:<34} {:<6} {:<10} TAGS",
        "ID", "STATION NAME", "CODEC", "RATE"
    );
    println!("--------------------------------------------------------------------------------");

    for st in stations {
        let fav = if st.favorite { "★" } else { " " };
        let rate_str = format_sample_rate(st.sample_rate);
        let tags_str = st.tags.join(", ");
        println!(
            "{} {:<12} {:<34} {:<6} {:<10} {}",
            fav, st.id, st.name, st.codec, rate_str, tags_str
        );
    }
    println!("================================================================================");

    Ok(())
}

/// Handles `reso play <station>`.
async fn handle_play_command(
    config_manager: ConfigManager,
    station_query: Option<String>,
    sleep_arg: Option<String>,
    foreground: bool,
) -> Result<()> {
    let station = match station_query {
        Some(q) => {
            if q.starts_with("http://") || q.starts_with("https://") {
                config::Station {
                    id: "custom-stream".to_string(),
                    name: "Custom Stream".to_string(),
                    url: q,
                    codec: "FLAC".to_string(),
                    sample_rate: 44100,
                    bit_depth: 16,
                    homepage: None,
                    tags: vec!["custom".to_string()],
                    favorite: false,
                    rp_channel: None,
                }
            } else if let Some(s) = config_manager.find_station(&q) {
                s.clone()
            } else {
                return Err(ResoError::Config(format!(
                    "Station '{}' not found in catalog",
                    q
                )));
            }
        }
        None => {
            let default_id = config_manager
                .config
                .default_station_id
                .as_deref()
                .unwrap_or("rp-main");
            config_manager
                .find_station(default_id)
                .cloned()
                .unwrap_or_else(|| config_manager.catalog.stations[0].clone())
        }
    };

    let (client, mut rx) = get_mpv_client(&config_manager).await?;
    client.load_file(&station.url).await?;

    println!("▶ Streaming: {} [{}]", station.name, station.codec);
    println!("  URL: {}", station.url);

    // Sleep Timer
    let sleep_duration = sleep_arg.as_deref().and_then(parse_duration_str);
    if let Some(dur) = sleep_duration {
        println!("⏳ Sleep timer set for {}", format_duration(dur.as_secs()));
        let client_clone = client.clone();
        tokio::spawn(async move {
            tokio::time::sleep(dur).await;
            println!("\n⏳ Sleep timer elapsed. Stopping playback.");
            let _ = client_clone.stop().await;
        });
    }

    if foreground {
        println!("Press Ctrl+C to stop...");
        tokio::select! {
            _ = signal::ctrl_c() => {
                println!("\nStopping playback...");
                let _ = client.stop().await;
            }
            _ = async {
                while let Ok(ev) = rx.recv().await {
                    if let events::MpvEvent::Metadata(m) = ev {
                        println!("♪ Now Playing: {}", m.display_name());
                    }
                }
            } => {}
        }
    }

    Ok(())
}

/// Cycles to next or previous station.
async fn handle_cycle_station(config_manager: &ConfigManager, forward: bool) -> Result<()> {
    let (client, _) = get_mpv_client(config_manager).await?;
    let stations = &config_manager.catalog.stations;
    if stations.is_empty() {
        return Ok(());
    }

    let target_idx = if forward { 0 } else { stations.len() - 1 };
    let st = &stations[target_idx];
    client.load_file(&st.url).await?;
    println!("▶ Switched to: {} [{}]", st.name, st.codec);
    Ok(())
}

/// Handles `reso current`.
async fn handle_current_command(config_manager: &ConfigManager, as_json: bool) -> Result<()> {
    let (_client, _rx) = get_mpv_client(config_manager).await?;
    let status = PipelineStatus::evaluate(&AudioStreamParams::default(), 100.0, None);

    if as_json {
        let json = serde_json::to_string_pretty(&status)?;
        println!("{}", json);
    } else {
        println!("=================================================================");
        println!("                     RESO CURRENT PLAYBACK                       ");
        println!("=================================================================");
        println!(
            "Source Codec:      {}",
            if status.source_codec.is_empty() {
                "FLAC (Lossless)"
            } else {
                &status.source_codec
            }
        );
        println!(
            "Sample Rate:       {}",
            format_sample_rate(status.source_rate)
        );
        println!(
            "PipeWire Sink:     {}",
            status
                .pw_sink
                .as_ref()
                .map(|s| s.description.as_str())
                .unwrap_or("Default Sink")
        );
        println!(
            "Sink Clock Rate:   {}",
            format_sample_rate(status.pw_sink_rate.unwrap_or(0))
        );
        println!(
            "Hardware DAC:      {}",
            status.alsa_card_name.as_deref().unwrap_or("USB DAC")
        );
        println!(
            "Bit-Perfect Verdict: {:?}",
            status
                .verdict
                .unwrap_or(inspector::status::BitPerfectVerdict::Idle)
        );
        println!("=================================================================");
    }

    Ok(())
}

/// Handles `reso search`.
async fn handle_search_command(
    mut config_manager: ConfigManager,
    query: String,
    codec: Option<String>,
    bitrate: Option<u32>,
    limit: usize,
    add_to_catalog: bool,
) -> Result<()> {
    let api = ApiClient::new();
    println!(
        "🔍 Searching Radio-Browser for '{}' (codec: {:?})...",
        query, codec
    );
    let results = api
        .search_radio_browser(&query, codec.as_deref(), bitrate, limit)
        .await?;

    if results.is_empty() {
        println!("No matching stations found.");
        return Ok(());
    }

    println!("Found {} stations:\n", results.len());
    println!("{:<12} {:<34} {:<8} TAGS", "ID", "STATION NAME", "CODEC");
    println!("--------------------------------------------------------------------------------");

    for st in &results {
        println!(
            "{:<12} {:<34} {:<8} {}",
            st.id,
            st.name,
            st.codec,
            st.tags.join(", ")
        );
        if add_to_catalog {
            let _ = config_manager.add_station(st.clone());
        }
    }

    if add_to_catalog {
        println!(
            "\n✓ Added {} stations to ~/.config/reso/stations.toml",
            results.len()
        );
    }

    Ok(())
}

/// Handles `reso inspect`.
async fn handle_inspect_command(_config_manager: &ConfigManager, as_json: bool) -> Result<()> {
    let status = PipelineStatus::evaluate(
        &AudioStreamParams {
            codec: "FLAC".to_string(),
            sample_rate: 44100,
            channels: 2,
            bit_depth: Some(24),
            bitrate_kbps: Some(920),
        },
        100.0,
        Some(44100),
    );

    if as_json {
        let json = serde_json::to_string_pretty(&status)?;
        println!("{}", json);
    } else {
        print!("{}", status.format_cli_report());
    }

    Ok(())
}

/// Handles `reso record`.
async fn handle_record_command(
    config_manager: &ConfigManager,
    action: &str,
    output: Option<std::path::PathBuf>,
) -> Result<()> {
    let (client, _) = get_mpv_client(config_manager).await?;
    let recorder = StreamRecorder::new(client, config_manager.config.record_dir.clone());

    match action {
        "start" => {
            let path = recorder
                .start_recording("Manual_Record", None, output)
                .await?;
            println!("🔴 Recording started -> {}", path.display());
        }
        "stop" => {
            if let Some(prev) = recorder.stop_recording().await? {
                println!("⏹ Recording stopped -> {}", prev.display());
            } else {
                println!("No active recording was running.");
            }
        }
        "status" => {
            if let Some(path) = recorder.active_path().await {
                println!("🔴 Recording in progress -> {}", path.display());
            } else {
                println!("⏹ Not recording.");
            }
        }
        _ => {
            if recorder.is_recording().await {
                if let Some(prev) = recorder.stop_recording().await? {
                    println!("⏹ Recording stopped -> {}", prev.display());
                }
            } else {
                let path = recorder.start_recording("Stream", None, output).await?;
                println!("🔴 Recording started -> {}", path.display());
            }
        }
    }

    Ok(())
}

/// Handles `reso add <url>`.
async fn handle_add_command(mut config_manager: ConfigManager, args: cli::AddArgs) -> Result<()> {
    let clean_url = args.url.trim().to_string();
    if clean_url.is_empty() {
        return Err(ResoError::Config("Stream URL cannot be empty".to_string()));
    }

    let (codec, sample_rate, bit_depth, auto_name, auto_tags, is_lossless) = if !args.no_test {
        println!("⏳ Testing radio stream endpoint: {}", clean_url);
        match integration::prober::StreamProber::probe_endpoint(&clean_url).await {
            Ok(probe) => {
                println!("✓ Stream verified successfully!");
                println!(
                    "  • Codec:       {} ({})",
                    probe.codec,
                    if probe.is_lossless {
                        "Lossless"
                    } else {
                        "Lossy"
                    }
                );
                println!("  • Sample Rate: {}", format_sample_rate(probe.sample_rate));
                println!("  • Bit Depth:   {}-bit", probe.bit_depth);
                if let Some(br) = probe.bitrate_kbps {
                    println!("  • Bitrate:     {} kbps", br);
                }
                (
                    probe.codec,
                    probe.sample_rate,
                    probe.bit_depth,
                    probe.suggested_name,
                    probe.tags,
                    probe.is_lossless,
                )
            }
            Err(e) => {
                eprintln!("✗ Stream probe failed: {}", e);
                return Err(ResoError::Config(format!(
                    "Failed to connect to stream: {}",
                    e
                )));
            }
        }
    } else {
        (
            "FLAC".to_string(),
            44100,
            16,
            integration::prober::infer_name_from_url(&clean_url),
            vec!["custom".to_string()],
            true,
        )
    };

    let name = args.name.unwrap_or(auto_name);
    let mut tags: Vec<String> = if let Some(t_str) = args.tags {
        t_str
            .split([';', ',', '/'])
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty())
            .collect()
    } else {
        auto_tags
    };

    if is_lossless && !tags.contains(&"lossless".to_string()) {
        tags.push("lossless".to_string());
    }

    let slug_id = util::sanitize_filename(&name)
        .to_lowercase()
        .replace(' ', "-");

    let station = config::Station {
        id: slug_id,
        name: name.clone(),
        url: clean_url,
        codec,
        sample_rate,
        bit_depth,
        homepage: None,
        tags,
        favorite: false,
        rp_channel: None,
    };

    config_manager.add_station(station)?;
    println!(
        "✓ Station '{}' added to catalog (~/.config/reso/stations.toml).",
        name
    );
    Ok(())
}

/// Handles `reso edit <station> [flags]`.
fn handle_edit_command(mut config_manager: ConfigManager, args: cli::EditArgs) -> Result<()> {
    let target = config_manager.find_station(&args.station).cloned();
    let existing = match target {
        Some(st) => st,
        None => {
            return Err(ResoError::Config(format!(
                "Station '{}' not found in catalog",
                args.station
            )))
        }
    };

    let name = args.name.unwrap_or(existing.name);
    let url = args.url.unwrap_or(existing.url);
    let codec = args.codec.unwrap_or(existing.codec);
    let sample_rate = args.sample_rate.unwrap_or(existing.sample_rate);
    let bit_depth = args.bit_depth.unwrap_or(existing.bit_depth);
    let tags = if let Some(t_str) = args.tags {
        t_str
            .split([';', ',', '/'])
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty())
            .collect()
    } else {
        existing.tags
    };

    let updated = config::Station {
        id: existing.id.clone(),
        name: name.clone(),
        url,
        codec,
        sample_rate: if sample_rate == 0 { 44100 } else { sample_rate },
        bit_depth: if bit_depth == 0 { 16 } else { bit_depth },
        homepage: existing.homepage,
        tags,
        favorite: existing.favorite,
        rp_channel: existing.rp_channel,
    };

    config_manager.edit_station(&existing.id, updated)?;
    println!(
        "✓ Updated station '{}' [{}] in stations.toml.",
        name, existing.id
    );
    Ok(())
}

/// Handles `reso delete <station>`.
fn handle_delete_command(mut config_manager: ConfigManager, station_query: String) -> Result<()> {
    let target_station = config_manager.find_station(&station_query).cloned();
    match target_station {
        Some(st) => {
            config_manager.remove_station(&st.id)?;
            println!("✓ Removed station '{}' [{}] from catalog.", st.name, st.id);
            Ok(())
        }
        None => Err(ResoError::Config(format!(
            "Station '{}' not found in catalog",
            station_query
        ))),
    }
}

/// Handles `reso like`.
async fn handle_like_command(config_manager: &mut ConfigManager) -> Result<()> {
    let (artist, title, album, station, codec, sample_rate, bit_depth) =
        if let Ok((_client, mut rx)) = get_mpv_client(config_manager).await {
            let mut meta = events::TrackMetadata::default();
            let mut params = events::AudioStreamParams::default();
            let timeout = tokio::time::sleep(tokio::time::Duration::from_millis(60));
            tokio::pin!(timeout);
            loop {
                tokio::select! {
                    _ = &mut timeout => break,
                    Ok(ev) = rx.recv() => {
                        match ev {
                            events::MpvEvent::Metadata(m) => meta = m,
                            events::MpvEvent::AudioParams(p) => params = p,
                            _ => {}
                        }
                    }
                }
            }
            let art = meta.artist.unwrap_or_else(|| "Unknown Artist".to_string());
            let tit = meta
                .title
                .or(meta.raw_title)
                .unwrap_or_else(|| "Lossless Stream".to_string());
            let alb = meta.album;
            let cdc = if params.codec.is_empty() {
                None
            } else {
                Some(params.codec)
            };
            let sr = if params.sample_rate > 0 {
                Some(params.sample_rate)
            } else {
                None
            };
            let bd = params.bit_depth;
            (art, tit, alb, "Reso Stream".to_string(), cdc, sr, bd)
        } else {
            (
                "Unknown Artist".to_string(),
                "Bookmarked Track".to_string(),
                None,
                "Reso".to_string(),
                Some("FLAC".to_string()),
                Some(44100),
                Some(16),
            )
        };

    let track = config_manager.bookmark_track(
        &station,
        &artist,
        &title,
        album.as_deref(),
        codec.as_deref(),
        sample_rate,
        bit_depth,
    )?;

    println!("✓ Liked track saved to ~/.config/reso/liked_tracks.toml:");
    println!("  • {}", track.display_summary());
    Ok(())
}

/// Handles `reso liked`.
fn handle_liked_command(
    mut config_manager: ConfigManager,
    action: String,
    id: Option<String>,
    as_json: bool,
) -> Result<()> {
    match action.as_str() {
        "delete" | "remove" | "rm" => {
            if let Some(track_id) = id {
                let removed = config_manager.remove_liked_track(&track_id)?;
                if removed {
                    println!(
                        "✓ Removed track '{}' from ~/.config/reso/liked_tracks.toml.",
                        track_id
                    );
                } else {
                    println!("✗ Track with ID '{}' not found.", track_id);
                }
            } else {
                return Err(ResoError::Config(
                    "Please specify --id <track_id> to delete".to_string(),
                ));
            }
        }
        _ => {
            let tracks = &config_manager.liked_catalog.tracks;
            if as_json {
                let json = serde_json::to_string_pretty(tracks)?;
                println!("{}", json);
                return Ok(());
            }

            println!(
                "================================================================================"
            );
            println!(
                "                           RESO LIKED TRACKS HISTORY                            "
            );
            println!(
                "================================================================================"
            );
            println!(
                "{:<20} {:<24} {:<24} {:<10}",
                "TIMESTAMP", "ARTIST", "TITLE", "STATION"
            );
            println!(
                "--------------------------------------------------------------------------------"
            );
            for t in tracks {
                println!(
                    "{:<20} {:<24} {:<24} {:<10}",
                    t.timestamp,
                    util::truncate_str(&t.artist, 22),
                    util::truncate_str(&t.title, 22),
                    util::truncate_str(&t.station, 10),
                );
            }
            println!(
                "================================================================================"
            );
            println!(
                "Total: {} tracks bookmarked (~/.config/reso/liked_tracks.toml)",
                tracks.len()
            );
        }
    }
    Ok(())
}

/// Handles `reso copy`.
async fn handle_copy_command(_config_manager: &ConfigManager) -> Result<()> {
    copy_to_clipboard("Reso Lossless Radio")?;
    println!("✓ Copied track info to system clipboard.");
    Ok(())
}

/// Handles `reso volume`.
fn handle_volume_command(action: &str) -> Result<()> {
    match action {
        "get" => {
            let out = Command::new("wpctl")
                .arg("get-volume")
                .arg("@DEFAULT_AUDIO_SINK@")
                .output()?;
            print!("{}", String::from_utf8_lossy(&out.stdout));
        }
        "up" => {
            let _ = Command::new("wpctl")
                .arg("set-volume")
                .arg("@DEFAULT_AUDIO_SINK@")
                .arg("5%+")
                .output()?;
            let out = Command::new("wpctl")
                .arg("get-volume")
                .arg("@DEFAULT_AUDIO_SINK@")
                .output()?;
            print!("{}", String::from_utf8_lossy(&out.stdout));
        }
        "down" => {
            let _ = Command::new("wpctl")
                .arg("set-volume")
                .arg("@DEFAULT_AUDIO_SINK@")
                .arg("5%-")
                .output()?;
            let out = Command::new("wpctl")
                .arg("get-volume")
                .arg("@DEFAULT_AUDIO_SINK@")
                .output()?;
            print!("{}", String::from_utf8_lossy(&out.stdout));
        }
        "mute" => {
            let _ = Command::new("wpctl")
                .arg("set-mute")
                .arg("@DEFAULT_AUDIO_SINK@")
                .arg("toggle")
                .output()?;
            let out = Command::new("wpctl")
                .arg("get-volume")
                .arg("@DEFAULT_AUDIO_SINK@")
                .output()?;
            print!("{}", String::from_utf8_lossy(&out.stdout));
        }
        val => {
            let arg = if val.ends_with('%') {
                val.to_string()
            } else {
                format!("{}%", val)
            };
            let _ = Command::new("wpctl")
                .arg("set-volume")
                .arg("@DEFAULT_AUDIO_SINK@")
                .arg(&arg)
                .output()?;
            let out = Command::new("wpctl")
                .arg("get-volume")
                .arg("@DEFAULT_AUDIO_SINK@")
                .output()?;
            print!("{}", String::from_utf8_lossy(&out.stdout));
        }
    }
    Ok(())
}

/// Handles `reso daemon`.
async fn handle_daemon_command(config_manager: ConfigManager) -> Result<()> {
    println!("Launching Reso background daemon (MPV + MPRIS2)...");
    let mut mpv_proc = MpvProcessManager::new(config_manager.config.socket_path.clone());
    mpv_proc.ensure_running().await?;

    let (client, _) = get_mpv_client(&config_manager).await?;
    let (action_tx, mut action_rx) = mpsc::channel::<events::AppAction>(64);
    let _mpris = start_mpris_server(action_tx).await?;

    println!("✓ Reso daemon active. MPRIS2 interface exposed on D-Bus.");
    println!("Press Ctrl+C to terminate daemon.");

    let client_clone = client.clone();
    tokio::select! {
        _ = signal::ctrl_c() => {
            println!("\nDaemon stopping.");
        }
        _ = async {
            while let Some(action) = action_rx.recv().await {
                match action {
                    events::AppAction::Play(st) => { let _ = client_clone.load_file(&st.url).await; }
                    events::AppAction::Pause => { let _ = client_clone.pause().await; }
                    events::AppAction::Resume => { let _ = client_clone.resume().await; }
                    events::AppAction::TogglePause => { let _ = client_clone.toggle_pause().await; }
                    events::AppAction::Stop => { let _ = client_clone.stop().await; }
                    events::AppAction::Quit => { break; }
                    _ => {}
                }
            }
        } => {}
    }

    Ok(())
}
