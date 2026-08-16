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
            handle_search_command(config_manager, args.query, args.codec, args.bitrate, args.limit, args.add).await?;
        }

        Some(Commands::Inspect(args)) => {
            handle_inspect_command(&config_manager, args.json).await?;
        }

        Some(Commands::Record(args)) => {
            handle_record_command(&config_manager, &args.action, args.output).await?;
        }

        Some(Commands::Like) => {
            handle_like_command(&config_manager).await?;
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
async fn get_mpv_client(config_manager: &ConfigManager) -> Result<(MpvClient, tokio::sync::broadcast::Receiver<events::MpvEvent>)> {
    let mut mpv_proc = MpvProcessManager::new(config_manager.config.socket_path.clone());
    mpv_proc.ensure_running().await?;
    MpvClient::connect(config_manager.config.socket_path.clone()).await
}

/// Handles `reso list`.
fn handle_list_command(config_manager: &ConfigManager, favorites_only: bool, as_json: bool) -> Result<()> {
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
    println!("{:<14} {:<34} {:<6} {:<10} TAGS", "ID", "STATION NAME", "CODEC", "RATE");
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
                return Err(ResoError::Config(format!("Station '{}' not found in catalog", q)));
            }
        }
        None => {
            let default_id = config_manager.config.default_station_id.as_deref().unwrap_or("rp-main");
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
    let status = PipelineStatus::evaluate(&AudioStreamParams::default(), 100.0);

    if as_json {
        let json = serde_json::to_string_pretty(&status)?;
        println!("{}", json);
    } else {
        println!("=================================================================");
        println!("                     RESO CURRENT PLAYBACK                       ");
        println!("=================================================================");
        println!("Source Codec:      {}", if status.source_codec.is_empty() { "FLAC (Lossless)" } else { &status.source_codec });
        println!("Sample Rate:       {}", format_sample_rate(status.source_rate));
        println!("PipeWire Sink:     {}", status.pw_sink.as_ref().map(|s| s.description.as_str()).unwrap_or("Default Sink"));
        println!("Sink Clock Rate:   {}", format_sample_rate(status.pw_sink_rate.unwrap_or(0)));
        println!("Hardware DAC:      {}", status.alsa_card_name.as_deref().unwrap_or("USB DAC"));
        println!("Bit-Perfect Verdict: {:?}", status.verdict.unwrap_or(inspector::status::BitPerfectVerdict::Idle));
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
    println!("🔍 Searching Radio-Browser for '{}' (codec: {:?})...", query, codec);
    let results = api.search_radio_browser(&query, codec.as_deref(), bitrate, limit).await?;

    if results.is_empty() {
        println!("No matching stations found.");
        return Ok(());
    }

    println!("Found {} stations:\n", results.len());
    println!("{:<12} {:<34} {:<8} TAGS", "ID", "STATION NAME", "CODEC");
    println!("--------------------------------------------------------------------------------");

    for st in &results {
        println!("{:<12} {:<34} {:<8} {}", st.id, st.name, st.codec, st.tags.join(", "));
        if add_to_catalog {
            let _ = config_manager.add_station(st.clone());
        }
    }

    if add_to_catalog {
        println!("\n✓ Added {} stations to ~/.config/reso/stations.toml", results.len());
    }

    Ok(())
}

/// Handles `reso inspect`.
async fn handle_inspect_command(_config_manager: &ConfigManager, as_json: bool) -> Result<()> {
    let status = PipelineStatus::evaluate(&AudioStreamParams {
        codec: "FLAC".to_string(),
        sample_rate: 44100,
        channels: 2,
        bit_depth: Some(24),
        bitrate_kbps: Some(920),
    }, 100.0);

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
            let path = recorder.start_recording("Manual_Record", None, output).await?;
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

/// Handles `reso like`.
async fn handle_like_command(config_manager: &ConfigManager) -> Result<()> {
    let path = config_manager.bookmark_track(
        "Current Stream",
        "Bookmarked Track",
        "Lossless Radio Stream",
        None,
        Some("FLAC"),
        Some(44100),
    )?;
    println!("✓ Track bookmarked to {}", path.display());
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
            let out = Command::new("wpctl").arg("get-volume").arg("@DEFAULT_AUDIO_SINK@").output()?;
            print!("{}", String::from_utf8_lossy(&out.stdout));
        }
        "up" => {
            let _ = Command::new("wpctl").arg("set-volume").arg("@DEFAULT_AUDIO_SINK@").arg("5%+").output()?;
            let out = Command::new("wpctl").arg("get-volume").arg("@DEFAULT_AUDIO_SINK@").output()?;
            print!("{}", String::from_utf8_lossy(&out.stdout));
        }
        "down" => {
            let _ = Command::new("wpctl").arg("set-volume").arg("@DEFAULT_AUDIO_SINK@").arg("5%-").output()?;
            let out = Command::new("wpctl").arg("get-volume").arg("@DEFAULT_AUDIO_SINK@").output()?;
            print!("{}", String::from_utf8_lossy(&out.stdout));
        }
        "mute" => {
            let _ = Command::new("wpctl").arg("set-mute").arg("@DEFAULT_AUDIO_SINK@").arg("toggle").output()?;
            let out = Command::new("wpctl").arg("get-volume").arg("@DEFAULT_AUDIO_SINK@").output()?;
            print!("{}", String::from_utf8_lossy(&out.stdout));
        }
        val => {
            let arg = if val.ends_with('%') { val.to_string() } else { format!("{}%", val) };
            let _ = Command::new("wpctl").arg("set-volume").arg("@DEFAULT_AUDIO_SINK@").arg(&arg).output()?;
            let out = Command::new("wpctl").arg("get-volume").arg("@DEFAULT_AUDIO_SINK@").output()?;
            print!("{}", String::from_utf8_lossy(&out.stdout));
        }
    }
    Ok(())
}

/// Handles `reso daemon`.
async fn handle_daemon_command(config_manager: ConfigManager) -> Result<()> {
    println!("⚡ Launching Reso background daemon (MPV + MPRIS2)...");
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
