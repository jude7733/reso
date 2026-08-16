# ⚡ `reso` — Lossless Audiophile Internet Radio Player & PipeWire Inspector

`reso` is a production-grade, highly-featured CLI and TUI internet radio player written in **Rust**, specifically tailored for **Arch Linux**, **PipeWire**, and audiophile USB DACs (such as the **Moondrop CHU II DSP** and UAC2 devices).

It is built from the ground up to stream lossless/Hi-Res audio (e.g., Radio Paradise FLAC mixes, Czech Radio D-Dur, JB Radio-2, Mother Earth Radio) and strictly preserve native source sample rates (44.1 kHz, 48 kHz, 88.2 kHz, 96 kHz, 16/24-bit) throughout the entire playback chain:
$$\text{MPV (Lossless Decoder)} \longrightarrow \text{PipeWire Graph} \longrightarrow \text{ALSA / Hardware USB DAC}$$

without forced software resampling, sample rate degradation, or software DSP tampering.

---

## 🌟 Key Features

- **Bit-Perfect Audio Pipeline**: MPV volume is locked to 100% to preserve full 24-bit dynamic range, delegating hardware volume to PipeWire (`wpctl`).
- **3-Stage Audiophile Inspector (`reso inspect`)**:
  1. **Source / MPV Stage**: Real-time inspection of active decoder sample rate, bit depth, codec, and bitrate via Unix domain socket JSON-IPC.
  2. **PipeWire Graph Stage**: Parses `pw-dump` to determine active sink clock rate, stream sample rates, and verify if PipeWire graph rate matches stream rate.
  3. **ALSA / Hardware Stage**: Reads `/proc/asound/card<X>/pcm0p/sub0/hw_params` and `/proc/asound/card<X>/stream0` to verify physical USB DAC clock rates and capability matrices.
- **Resampling Culprit Detector**: Automatically scans all active audio streams in PipeWire. If graph resampling is detected (e.g., sink forced to 48 kHz when playing a 44.1 kHz stream), `reso` identifies the offending application (e.g., `"Resampling active: Firefox (PID 84229) is locking graph at 48.0 kHz"`).
- **DSP Filter Guard**: Detects active software filters, equalizers, or effect chains (such as EasyEffects or LSP) and warns if bit-exactness is altered.
- **Rich Terminal UI (`ratatui`)**:
  - Full-screen dashboard featuring station catalogs, favorites, search, and diagnostics.
  - Terminal album cover art rendering via Kitty graphics and Sixel protocols (`ratatui-image`) with stylized Vinyl Hi-Res fallback.
  - Real-time animated Unicode/Braille spectrum visualizer.
- **Lossless Stream Recording (`reso record` / `r`)**: Uses MPV direct stream dumping to record raw lossless FLAC streams into `~/Music/Radio/` tagged with active ICY and station metadata.
- **Desktop & MPRIS2 Integration**: Full D-Bus MPRIS2 server (`org.mpris.MediaPlayer2.reso`) for native Waybar, Polybar, KDE, GNOME, Sway, Hyprland, and media key control (`playerctl`).
- **Radio-Browser & Radio Paradise APIs**:
  - Live station discovery via `radio-browser.info` (`reso search --codec FLAC "jazz"`).
  - Rich metadata polling for Radio Paradise (high-res cover art, next tracks, time remaining).
- **Scrobbling & Track Bookmarking**:
  - Integrated Last.fm and ListenBrainz scrobbler adhering to the 50% or 4-minute playback threshold rule.
  - Track bookmarking to `~/.config/reso/liked_tracks.md` (`l` key) and instant clipboard copy (`c` key via Wayland `wl-copy` / X11 `arboard`).

---

## 🏗 Architecture & Subsystems

```text
┌────────────────────────────────────────────────────────────────────────┐
│                          CLI & TUI Layer                               │
│ (Clap commands, Ratatui Dashboard, Kitty/Sixel Art, FFT Visualizer)    │
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

## 🚀 Installation & Build on Arch Linux

### Prerequisites
Install system dependencies:
```bash
sudo pacman -S --needed base-devel rust cargo mpv pipewire wireplumber
```

### Build from Source
```bash
git clone https://github.com/your-username/reso.git
cd reso
cargo build --release
sudo install -Dm755 target/release/reso /usr/local/bin/reso
```

---

## ⚙ PipeWire & WirePlumber Audiophile Configuration

To allow PipeWire to switch rates dynamically to match native 44.1 kHz, 48 kHz, 88.2 kHz, and 96 kHz lossless streams without resampling, configure `default.clock.allowed-rates` in WirePlumber / PipeWire:

1. Create or edit `~/.config/pipewire/pipewire.conf.d/10-rates.conf`:
```conf
context.properties = {
    default.clock.rate          = 48000
    default.clock.allowed-rates = [ 44100 48000 88200 96000 192000 ]
}
```

2. Restart PipeWire:
```bash
systemctl --user restart pipewire wireplumber
```

---

## 💻 CLI Commands Reference

| Command | Description | Example |
| :--- | :--- | :--- |
| `reso` | Launch full-screen interactive TUI | `reso` |
| `reso list` | List all configured stations | `reso list --favorites` |
| `reso play <id/name/url>` | Stream a station with optional sleep timer | `reso play rp-main --sleep 45m` |
| `reso pause` / `resume` / `toggle` | Control active playback state | `reso toggle` |
| `reso stop` | Stop playback | `reso stop` |
| `reso next` / `prev` | Cycle stations in catalog | `reso next` |
| `reso current` | Print currently playing track and audio specs | `reso current --json` |
| `reso search <query>` | Discover lossless stations from Radio-Browser | `reso search classical --codec FLAC --add` |
| `reso inspect` | Run 3-stage Audiophile Pipeline Inspector | `reso inspect` |
| `reso record [action]` | Manage lossless stream recording | `reso record start` / `stop` / `status` |
| `reso like` | Bookmark current track to `liked_tracks.md` | `reso like` |
| `reso copy` | Copy `Artist - Title` to system clipboard | `reso copy` |
| `reso volume [action]` | Hardware volume control via `wpctl` | `reso volume up` / `reso volume 40%` |
| `reso daemon` | Run headless background daemon with MPRIS2 | `reso daemon` |

---

## ⌨ TUI Interactive Keybindings

When in interactive TUI mode (`reso`):

| Key | Action |
| :--- | :--- |
| `Space` | Toggle Play / Pause |
| `Enter` | Play selected station or search result |
| `s` | Stop playback |
| `n` / `p` | Skip to Next / Previous station |
| `f` | Toggle Favorite on station |
| `r` | Toggle Lossless Stream Recording (`~/Music/Radio/`) |
| `l` | Like / Bookmark track to `~/.config/reso/liked_tracks.md` |
| `c` | Copy track `Artist - Title` to system clipboard |
| `+` / `-` | Increase / Decrease Hardware Volume (`wpctl`) |
| `m` | Toggle Hardware Mute (`wpctl`) |
| `/` | Focus search / filter bar |
| `1` | Switch to **Stations Catalog** view |
| `2` | Switch to **Radio-Browser Search** view |
| `3` | Switch to **Audiophile Inspector & DAC Matrix** view |
| `4` | Switch to **Liked Tracks History** view |
| `?` | Toggle Help & Shortcuts modal |
| `q` / `Esc` | Quit Reso |

---

## 🎛 Audiophile Pipeline Diagnostics Example

Run `reso inspect` to verify your bit-perfect chain at any time:

```text
=================================================================
               RESO AUDIOPHILE PIPELINE INSPECTOR                
=================================================================

1. STREAM SOURCE (MPV DECODER)
   Codec:        FLAC
   Sample Rate:  44.1 kHz
   Bit Depth:    24-bit
   Channels:     2
   MPV Volume:   100.0% (Bit-perfect requires 100%)

2. PIPEWIRE AUDIO GRAPH
   Sink Node:    Chu2 DSP Analog Stereo [alsa_output.usb-Chu2_DSP.analog-stereo]
   Sink Rate:    44.1 kHz
   Sink Format:  S24LE
   Allowed Rates: [44100, 48000, 88200, 96000]
   DSP Filters:  None (Bit-exact path)

3. HARDWARE DAC / ALSA
   DAC Device:   USB-Audio - Chu2 DSP (KTMicro Chu2 DSP at usb-0000:00:14.0-9)
   ALSA Status:  Active (Running)
   HW Rate:      44.1 kHz
   HW Format:    S24_3LE
   Buffer Size:  32768 periods
   DAC Supported Rates:  [44100, 48000, 88200, 96000]
   DAC Supported Depths: [16, 24] bits

=================================================================
 STATUS: [ PASS ] ✓ BIT-PERFECT DIRECT PLAYBACK
 Native source rate matches PipeWire sink and hardware DAC clocks.
=================================================================
```

---

## 🖥 Desktop & Waybar Integration

`reso` exposes a full MPRIS2 interface on D-Bus (`org.mpris.MediaPlayer2.reso`).

### Control with `playerctl`:
```bash
playerctl --player=reso play-pause
playerctl --player=reso next
playerctl --player=reso metadata --format "{{ artist }} - {{ title }} [{{ album }}]"
```

### Waybar Module Configuration:
In `~/.config/waybar/config`:
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

## 📁 Configuration & File Locations

All user configuration and logs reside in `~/.config/reso/`:
- `~/.config/reso/config.toml`: General settings, socket path, UI theme, Last.fm credentials.
- `~/.config/reso/stations.toml`: Station catalog.
- `~/.config/reso/liked_tracks.md`: Appended log of liked tracks with timestamps, station, and stream formats.
- `~/Music/Radio/`: Default storage directory for lossless stream recordings (`.flac`).

---

## 📜 License

Licensed under the MIT License.
