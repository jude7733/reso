# reso

**Lossless Audiophile Internet Radio Player & Bit-Perfect PipeWire Inspector for Linux**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Language](https://img.shields.io/badge/Language-Rust-orange.svg)](https://www.rust-lang.org/)
[![Audio Server](https://img.shields.io/badge/Audio-PipeWire%20%2F%20ALSA-green.svg)](https://pipewire.org/)
[![UI](https://img.shields.io/badge/TUI-Ratatui-purple.svg)](https://ratatui.rs/)

`reso` is a high-performance terminal internet radio player and audio stream inspector written in **Rust**. Built for Linux audiophiles, Hi-Fi enthusiasts, and power users, it guarantees bit-perfect lossless playback across PipeWire and ALSA audio pipelines, dynamically matching native stream sample rates to your hardware DAC without software resampling, bit truncation, or DSP degradation.

```text
┌────────────────────────┐      ┌─────────────────────────┐      ┌───────────────────────┐      ┌──────────────────────┐
│  Stream (FLAC / Hi-Res)│ ───► │  MPV Decoder (Lossless) │ ───► │ PipeWire Dynamic Graph│ ───► │ ALSA / Hardware DAC  │
│  44.1 / 48 / 96 / 192k │      │  Bit-Exact Unity Volume │      │ Native Clock Matching │      │ Exact Hardware Rate  │
└────────────────────────┘      └─────────────────────────┘      └───────────────────────┘      └──────────────────────┘
```

---

## Key Features

- **Bit-Perfect Audio Pipeline**:
  - Unaltered 16/24/32-bit PCM delivery from lossless streams (FLAC, WAV, Hi-Res).
  - Decoder volume is locked to 100% unity gain to prevent dynamic range compression and bit-depth quantization errors, delegating volume adjustments directly to hardware via PipeWire (`wpctl`).
- **3-Stage Audiophile Inspector (`reso inspect`)**:
  1. **Source / Decoder Stage**: Real-time inspection of active decoder codec, sample rate, bit depth, channel configuration, and bitrate via Unix domain socket JSON-IPC.
  2. **PipeWire Graph Stage**: Parses `pw-dump` to determine active sink clock rates, stream graph configurations, and verify bit-exact rate matching.
  3. **ALSA / Hardware Stage**: Reads `/proc/asound/card<X>/pcm0p/sub0/hw_params` and `/proc/asound/card<X>/stream0` to verify physical USB DAC clock rates and capability profiles.
- **Resampling Culprit Detection**:
  - Automatically identifies background applications or streams locking the audio graph at a conflicting clock rate (e.g., `"Resampling active: Firefox (PID 84229) is locking graph at 48.0 kHz"`).
- **DSP Filter Guard**:
  - Detects active software filters, equalizers, or effect chains (such as EasyEffects or LSP) and reports whether the audio path is bit-exact.
- **Rich Terminal User Interface (`ratatui`)**:
  - Full-screen dashboard featuring station catalogs, favorites, search, stream diagnostics, and liked tracks history.
  - Album cover art rendering via Kitty graphics and Sixel protocols (`ratatui-image`) with graceful stylized fallback.
  - Real-time animated Unicode spectrum visualizer.
- **Lossless Stream Recording (`reso record` / `r`)**:
  - Uses direct stream dumping to record lossless FLAC radio streams into `~/Music/Radio/` tagged with live ICY and station metadata.
- **Desktop & MPRIS2 Integration**:
  - Full D-Bus MPRIS2 server (`org.mpris.MediaPlayer2.reso`) for native Waybar, Polybar, KDE, GNOME, Sway, Hyprland, and media key control (`playerctl`).
- **Station Discovery & Rich Metadata**:
  - Live station discovery via `radio-browser.info` (`reso search --codec FLAC "jazz"`).
  - Rich metadata polling for services like Radio Paradise (high-res cover art, upcoming tracks, time remaining).
- **Scrobbling & Track Bookmarking**:
  - Integrated Last.fm and ListenBrainz scrobbler adhering to the standard 50% / 4-minute playback threshold rule.
  - Instant track bookmarking to `~/.config/reso/liked_tracks.md` (`l` key) and clipboard copy (`c` key via Wayland `wl-copy` / X11).

---

## Architecture & Subsystems

```text
┌────────────────────────────────────────────────────────────────────────┐
│                          CLI & TUI Layer                               │
│ (Clap CLI, Ratatui Dashboard, Kitty/Sixel Art, Spectrum Visualizer)    │
└──────┬────────────────────┬──────────────────────┬──────────────┬──────┘
       │                    │                      │              │
┌──────▼──────┐     ┌───────▼───────┐     ┌────────▼───────┐ ┌────▼────┐
│   Station   │     │    MPV IPC    │     │ Audio Inspector│ │ Desktop │
│   Manager   │     │ (Play, Record)│     │(ALSA, PipeWire)│ │(MPRIS2) │
└──────┬──────┘     └───────┬───────┘     └────────┬───────┘ └─────────┘
       │                    │                      │
┌──────▼────────────────────▼──────────────────────▼───────────────────┐
│     Ecosystem Integrations (Radio-Browser API, RP JSON, Scrobbler)   │
└──────────────────────────────────────────────────────────────────────┘
```

---

## Installation & Requirements

### System Dependencies

`reso` requires `mpv` (compiled with IPC support), `pipewire`, `wireplumber`, and ALSA development libraries.

#### Arch Linux
```bash
sudo pacman -S --needed base-devel rust cargo mpv pipewire wireplumber
```

#### Debian / Ubuntu
```bash
sudo apt update
sudo apt install -y build-essential cargo mpv pipewire wireplumber libasound2-dev libdbus-1-dev
```

#### Fedora
```bash
sudo dnf install -y @development-tools cargo mpv pipewire wireplumber alsa-lib-devel dbus-devel
```

### Build from Source

```bash
# Clone the repository
git clone https://github.com/your-username/reso.git
cd reso

# Build release binary
cargo build --release

# Install to system PATH
sudo install -Dm755 target/release/reso /usr/local/bin/reso
```

---

## PipeWire Multi-Rate Audiophile Configuration

To allow PipeWire to dynamically switch hardware sample rates to match native 44.1 kHz, 48 kHz, 88.2 kHz, 96 kHz, and 192 kHz lossless streams without software resampling:

1. Create or edit `~/.config/pipewire/pipewire.conf.d/10-rates.conf`:
```conf
context.properties = {
    default.clock.rate          = 48000
    default.clock.allowed-rates = [ 44100 48000 88200 96000 192000 ]
}
```

2. Restart the PipeWire user services:
```bash
systemctl --user restart pipewire wireplumber
```

---

## CLI Commands Reference

| Command | Description | Example |
| :--- | :--- | :--- |
| `reso` | Launch interactive full-screen TUI dashboard (default) | `reso` |
| `reso list` | List all configured stations | `reso list --favorites` |
| `reso play <id/name/url>` | Play a station with optional sleep timer | `reso play rp-main --sleep 45m` |
| `reso pause` / `resume` / `toggle` | Control active playback state | `reso toggle` |
| `reso stop` | Stop active playback | `reso stop` |
| `reso next` / `prev` | Cycle stations in catalog | `reso next` |
| `reso current` | Display current track metadata and audio specifications | `reso current --json` |
| `reso search <query>` | Discover lossless stations from Radio-Browser | `reso search classical --codec FLAC --add` |
| `reso inspect` | Run 3-stage Audiophile Pipeline Inspector | `reso inspect` |
| `reso record [action]` | Manage lossless stream recording | `reso record start` / `stop` / `status` |
| `reso like` | Bookmark current track to `liked_tracks.md` | `reso like` |
| `reso copy` | Copy `Artist - Title` to system clipboard | `reso copy` |
| `reso volume [action]` | Hardware volume control via `wpctl` | `reso volume up` / `reso volume 40%` |
| `reso daemon` | Run headless background daemon with MPRIS2 | `reso daemon` |

---

## TUI Interactive Keybindings

When running the interactive TUI (`reso`):

| Key | Action |
| :--- | :--- |
| `Space` | Toggle Play / Pause |
| `Enter` | Play selected station or search result |
| `s` | Stop playback |
| `n` / `p` | Skip to Next / Previous station |
| `f` | Toggle Favorite status on selected station |
| `r` | Toggle Lossless Stream Recording (`~/Music/Radio/`) |
| `l` | Bookmark track to `~/.config/reso/liked_tracks.md` |
| `c` | Copy `Artist - Title` to system clipboard |
| `+` / `-` | Increase / Decrease Hardware Volume (`wpctl`) |
| `m` | Toggle Hardware Mute (`wpctl`) |
| `/` | Focus search / filter input |
| `1` | Switch to **Stations Catalog** view |
| `2` | Switch to **Radio-Browser Search** view |
| `3` | Switch to **Audiophile Inspector & DAC Matrix** view |
| `4` | Switch to **Liked Tracks History** view |
| `?` | Toggle Help & Shortcuts overlay |
| `q` / `Esc` | Quit `reso` |

---

## Audiophile Pipeline Diagnostics Example

Run `reso inspect` at any time to verify bit-perfect signal flow:

```text
=================================================================
               RESO AUDIOPHILE PIPELINE INSPECTOR                
=================================================================

1. STREAM SOURCE (MPV DECODER)
   Codec:        FLAC
   Sample Rate:  44.1 kHz
   Bit Depth:    24-bit
   Channels:     2
   MPV Volume:   100.0% (Bit-perfect unity gain)

2. PIPEWIRE AUDIO GRAPH
   Sink Node:    High-Resolution Audio DAC [alsa_output.usb-Audio_DAC.analog-stereo]
   Sink Rate:    44.1 kHz
   Sink Format:  S24LE
   Allowed Rates: [44100, 48000, 88200, 96000, 192000]
   DSP Filters:  None (Bit-exact path)

3. HARDWARE DAC / ALSA
   DAC Device:   USB-Audio - High-Resolution Audio DAC (usb-0000:00:14.0-1)
   ALSA Status:  Active (Running)
   HW Rate:      44.1 kHz
   HW Format:    S24_3LE
   Buffer Size:  32768 periods
   DAC Supported Rates:  [44100, 48000, 88200, 96000, 192000]
   DAC Supported Depths: [16, 24, 32] bits

=================================================================
 STATUS: [ PASS ] ✓ BIT-PERFECT DIRECT PLAYBACK
 Native source rate matches PipeWire sink and hardware DAC clocks.
=================================================================
```

---

## Desktop & Status Bar Integration

`reso` exposes a full MPRIS2 interface on D-Bus (`org.mpris.MediaPlayer2.reso`).

### Control via `playerctl`
```bash
playerctl --player=reso play-pause
playerctl --player=reso next
playerctl --player=reso metadata --format "{{ artist }} - {{ title }} [{{ album }}]"
```

### Waybar Module Example
Add the following to `~/.config/waybar/config`:
```json
"custom/reso": {
    "format": "📻 {}",
    "exec": "playerctl --player=reso metadata --format '{{ artist }} - {{ title }}' --follow 2>/dev/null",
    "on-click": "playerctl --player=reso play-pause",
    "on-click-right": "reso toggle",
    "on-scroll-up": "wpctl set-volume @DEFAULT_AUDIO_SINK@ 2%+",
    "on-scroll-down": "wpctl set-volume @DEFAULT_AUDIO_SINK@ 2%-"
}
```

---

## Configuration & File Locations

All user configuration, catalogs, and logs reside in standard XDG paths:

- `~/.config/reso/config.toml`: General settings, IPC socket configuration, UI theme, and Last.fm/ListenBrainz credentials.
- `~/.config/reso/stations.toml`: Station catalog and custom user stream definitions.
- `~/.config/reso/liked_tracks.md`: Appended log of bookmarked tracks with timestamps, station metadata, and audio formats.
- `~/Music/Radio/`: Default destination directory for lossless stream recordings (`.flac`).

---

## License

This project is licensed under the [MIT License](LICENSE).
