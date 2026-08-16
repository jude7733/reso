//! Clipboard integration supporting Wayland and X11 via `arboard` with shell fallback.

use crate::error::{ResoError, Result};
use std::io::Write;
use std::process::{Command, Stdio};

/// Copies text to the system clipboard (Wayland / X11).
pub fn copy_to_clipboard(text: &str) -> Result<()> {
    // 1. Try arboard first
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if clipboard.set_text(text.to_string()).is_ok() {
            return Ok(());
        }
    }

    // 2. Try wl-copy (Wayland)
    if let Ok(mut child) = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        if child.wait().is_ok() {
            return Ok(());
        }
    }

    // 3. Try xclip (X11)
    if let Ok(mut child) = Command::new("xclip")
        .arg("-selection")
        .arg("clipboard")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        if child.wait().is_ok() {
            return Ok(());
        }
    }

    Err(ResoError::Clipboard(
        "Failed to copy to clipboard (arboard, wl-copy, and xclip unavailable)".to_string(),
    ))
}
