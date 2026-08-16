//! MPV headless process manager.
//!
//! Spawns and supervises a dedicated headless `mpv` instance with PipeWire audio output.

use crate::error::{ResoError, Result};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UnixStream;

/// Manages the headless MPV process lifecycle.
pub struct MpvProcessManager {
    socket_path: PathBuf,
    child: Option<Child>,
    owns_process: Arc<AtomicBool>,
}

impl MpvProcessManager {
    /// Creates a new manager with the given IPC socket path.
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            child: None,
            owns_process: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Checks if MPV is already running and accepting connections on the socket.
    pub async fn is_running(&self) -> bool {
        if !self.socket_path.exists() {
            return false;
        }
        UnixStream::connect(&self.socket_path).await.is_ok()
    }

    /// Ensures that MPV is running; spawns a new instance if not currently responsive.
    pub async fn ensure_running(&mut self) -> Result<()> {
        if self.is_running().await {
            return Ok(());
        }

        // Clean up stale socket file if any
        if self.socket_path.exists() {
            let _ = std::fs::remove_file(&self.socket_path);
        }

        // Ensure parent directory exists
        if let Some(parent) = self.socket_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let socket_arg = format!("--input-ipc-server={}", self.socket_path.display());

        let child = Command::new("mpv")
            .arg("--idle")
            .arg(&socket_arg)
            .arg("--ao=pipewire")
            .arg("--audio-display=no")
            .arg("--no-video")
            .arg("--volume=100")
            .arg("--gapless-audio=yes")
            .arg("--force-seekable=no")
            .arg("--terminal=no")
            .arg("--msg-level=all=no")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| ResoError::Mpv(format!("Failed to spawn headless mpv: {}", e)))?;

        self.child = Some(child);
        self.owns_process.store(true, Ordering::SeqCst);

        // Poll until socket is ready (timeout after 3 seconds)
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if self.socket_path.exists() && UnixStream::connect(&self.socket_path).await.is_ok() {
                return Ok(());
            }
        }

        Err(ResoError::Mpv(format!(
            "Timed out waiting for MPV IPC socket at {}",
            self.socket_path.display()
        )))
    }

    /// Returns a reference to the socket path.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

impl Drop for MpvProcessManager {
    fn drop(&mut self) {
        if self.owns_process.load(Ordering::SeqCst) {
            if let Some(mut child) = self.child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            if self.socket_path.exists() {
                let _ = std::fs::remove_file(&self.socket_path);
            }
        }
    }
}
