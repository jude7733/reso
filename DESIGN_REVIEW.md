# Reso Design Review and Recommended Updates

This review looks at Reso as a Linux terminal radio player, its current Rust/TUI architecture, and the desktop integrations that would make it feel at home outside the terminal. Recommendations are ordered by user impact and by how much they strengthen the existing design.

## Design Review

Reso has a clear identity: a keyboard-driven radio catalog paired with a now-playing view and audio pipeline diagnostics. Ratatui fits the target audience, and the station, playback, inspector, and liked-track views give the TUI a useful information hierarchy. The use of MPV over IPC keeps stream decoding separate from the interface, while the event/action types provide a starting point for making integrations share playback behavior.

The design currently treats the TUI as the most complete application. It owns station selection, recording, rich metadata polling, scrobbling, and most state updates. The CLI can control an MPV process, and `reso daemon` advertises a desktop service, but the daemon currently handles only a subset of MPRIS actions and does not consume MPV events to keep player state and metadata current. MPRIS startup errors are also silently ignored in the TUI. This makes desktop behavior less complete than the feature list implies.

The product copy promises “bit-perfect” playback as a guarantee. The inspector is a useful differentiator, but some inputs are estimates or unavailable on some systems: station catalog rates may be nominal, PipeWire/ALSA inspection can fail, and source format can differ from output format. Make the UI report what was observed and what could not be verified, and reserve a definitive pass for cases where all relevant stages were measured.

The interface is feature-dense. The dashboard, inspector, settings, station editing, discovery, recording, bookmarks, themes, and diagnostics are all exposed through one terminal workflow. Keep the keyboard-first model, but make the common path—find station, play, see track, control playback—visually dominant and move less frequent configuration and diagnostics behind secondary views or progressively disclosed detail.

## Recommended Updates

### P0 — Make playback a shared service — Status: In progress

Create one playback/session layer that owns the MPV process, IPC client, current station, playback state, metadata, volume, recording, and event stream. Have the TUI, CLI, and daemon call this layer instead of each owning partial playback behavior. Keep presentation and integration code as clients of the same state and action interface.

This is the foundation for reliable MPRIS, desktop notifications, tray controls, and headless use. It also gives the daemon the same features as the TUI and avoids divergent behavior across entry points.

### P0 — Complete and harden MPRIS2 — Status: In progress

MPRIS2 support already exists in `src/integration/mpris.rs`; adding it from scratch is unnecessary. Finish it as a dependable desktop integration:

- Feed MPV events into daemon state so `PlaybackStatus`, station/track metadata, and volume reflect actual playback.
- Handle the complete supported action set in daemon mode, including next/previous, volume, and stop/quit behavior; define sensible behavior for actions that radio streams cannot support, such as seeking.
- Ensure clients receive property change notifications when playback state or metadata changes, and test with `playerctl` in both TUI and daemon modes.
- Surface session bus connection and name ownership failures instead of swallowing them; provide a clear status or log message when MPRIS is unavailable.
- Publish stable track IDs and useful artwork URLs, and represent radio station identity consistently when track metadata is missing.
- Consider whether `reso daemon` should be the sole owner of MPRIS, or whether launching the TUI should expose it too. Prevent two Reso processes from racing to own the same bus name or MPV session.

### P1 — Add optional Linux desktop notifications — Status: Not started

Add notifications for track changes, playback/connection failures, recording start/finish, and sleep timer completion. Track changes are especially useful when Reso is running headless or minimized in another terminal.

Use the session notification service (`org.freedesktop.Notifications`) through a small integration module. Make notification categories configurable, defaulting to track changes and errors. Avoid invoking `notify-send` through a shell; a D-Bus client provides structured fields, handles missing services, and avoids command quoting issues. Include the station in the summary and artist/title in the body; attach cover art only when a local image or supported URI is available. Rate-limit repeated reconnect and metadata events.

### P1 — Improve stream reliability and observability — Status: Not started

- Add configurable reconnect policy with backoff, timeout, and a visible connection state (connecting, buffering, playing, reconnecting, failed).
- Preserve the last useful error and expose it in the TUI and CLI instead of dropping MPV errors.
- Separate station metadata from live stream observations. Clearly label nominal directory/catalog values versus values read from the active decoder and hardware.
- Make inspection results tri-state: verified, mismatch detected, or unknown/unavailable. Do not report a clean bit-perfect pass when a required stage could not be inspected.
- Add station health checks and report stale or unreachable URLs without blocking the UI.

### P1 — Refine the information hierarchy — Status: Not started

- Keep the station list and now-playing card as the primary dashboard; make play state, buffering, and current station easy to scan at a glance.
- Let users collapse the inspector and visualizer independently, and adapt layout to small terminal sizes.
- Provide a command palette or searchable action menu for discoverability while retaining direct keyboard shortcuts.
- Make keyboard focus explicit across tabs, search, lists, and modals; show focused controls and avoid conflicting shortcuts between views.
- Offer a compact mode for small terminals and a reduced-motion option for the visualizer.

### P2 — Make station management more robust — Status: Not started

- Add station grouping, tags, and sorting/filter presets; persist favorites and user-defined collections consistently.
- Validate URLs and identifiers when importing or editing stations, and show probe results separately from manually configured codec/rate claims.
- Add import/export for common playlist formats such as PLS and M3U, while preserving Reso-specific fields in its own format.
- Add an explicit station refresh workflow for directory results and a way to hide or remove unavailable bundled stations without losing user edits.

### P2 — Add desktop quality-of-life options — Status: Not started

- Add a single-instance/session lock so CLI commands, TUI, and daemon reliably target one playback session.
- Add optional system tray controls only if there is demand for a non-terminal desktop surface; the MPRIS interface already gives desktop environments and status bars standard playback controls.
- Add desktop media key guidance and documented Waybar examples after behavior is verified across desktop environments.
- Support autostart through a user systemd unit, with clear ownership and shutdown behavior for the MPV child process.

### P2 — Strengthen accessibility and configuration — Status: Not started

- Offer a high-contrast theme and avoid relying on color alone for playback, quality, and warning states.
- Ensure every icon has a text fallback and make terminal image protocols fully optional.
- Document config keys in one reference, validate values on load, and provide a `reso config` command to show effective settings and paths.
- Move scrobbling credentials to an OS keyring when available, with a documented file-based fallback and restrictive permissions.

## Suggested Delivery Order

1. Refactor playback ownership into a shared session layer and close the daemon/MPRIS state gaps.
2. Add integration-level MPRIS checks for TUI and daemon playback, metadata, volume, and error paths.
3. Add opt-in desktop notifications using the session bus.
4. Improve reconnect/error reporting and make inspector confidence explicit.
5. Polish the TUI hierarchy and small-terminal behavior.
6. Add station import/export, systemd autostart, and optional tray support based on user demand.

## Acceptance Checks

- `playerctl` sees accurate status and metadata, and play/pause/stop/next/previous work in both supported launch modes.
- A missing session bus or notification service does not prevent playback and produces a useful diagnostic.
- Track notifications are configurable, do not repeat on every polling tick, and include the correct station/track after metadata changes.
- The inspector distinguishes measured facts from catalog assumptions and reports unknown when a stage cannot be queried.
- The primary TUI workflow remains usable in a narrow terminal without clipped controls or inaccessible actions.
