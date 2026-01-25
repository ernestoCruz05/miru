use std::path::Path;
use std::process::{Child, Command, Stdio};

use tracing::{debug, info};

use crate::error::{Error, Result};

pub struct MpvPlayer {
    args: Vec<String>,
    child: Option<Child>,
}

impl MpvPlayer {
    pub fn new(args: Vec<String>) -> Self {
        Self { args, child: None }
    }

    /// Launch mpv with the given video file
    pub fn play(&mut self, path: &Path, start_position: Option<u64>) -> Result<()> {
        let mut cmd = Command::new("mpv");

        // Suppress mpv output to avoid polluting TUI
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());

        // Add configured args
        for arg in &self.args {
            cmd.arg(arg);
        }

        // Add start position if resuming
        if let Some(pos) = start_position {
            if pos > 0 {
                cmd.arg(format!("--start={}", pos));
                info!(position = pos, "Resuming playback");
            }
        }

        // Add the file path
        cmd.arg(path);

        debug!(path = %path.display(), "Launching mpv");

        let child = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::PlayerNotFound("mpv".to_string())
            } else {
                Error::PlayerLaunch(e.to_string())
            }
        })?;

        self.child = Some(child);
        Ok(())
    }

    /// Wait for mpv to exit and return the exit status
    pub fn wait(&mut self) -> Result<bool> {
        if let Some(ref mut child) = self.child {
            let status = child.wait()?;
            self.child = None;
            Ok(status.success())
        } else {
            Ok(true)
        }
    }

    /// Check if mpv is still running
    pub fn is_running(&mut self) -> bool {
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.child = None;
                    false
                }
                Ok(None) => true,
                Err(_) => false,
            }
        } else {
            false
        }
    }
}

impl Default for MpvPlayer {
    fn default() -> Self {
        Self::new(vec!["--fullscreen".to_string()])
    }
}
