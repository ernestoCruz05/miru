use std::path::Path;
use std::process::{Child, Command, Stdio};

use tracing::{debug, info, warn};

use crate::error::{Error, Result};

pub struct ExternalPlayer {
    command: String,
    args: Vec<String>,
    child: Option<Child>,
}

impl ExternalPlayer {
    pub fn new(command: String, args: Vec<String>) -> Self {
        Self {
            command,
            args,
            child: None,
        }
    }

    pub fn play(&mut self, path: &Path, start_position: Option<u64>) -> Result<()> {
        let command = resolve_executable(&self.command);
        let mut cmd = Command::new(&command);

        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());

        for arg in &self.args {
            cmd.arg(arg);
        }

        if let Some(pos) = start_position {
            if pos > 0 {
                if self.command.contains("mpv") {
                    cmd.arg(format!("--start={}", pos));
                } else if self.command.contains("vlc") {
                    cmd.arg(format!("--start-time={}", pos));
                } else {
                    warn!(
                        "Unknown player '{}', cannot set start position",
                        self.command
                    );
                }

                info!(position = pos, "Resuming playback");
            }
        }

        cmd.arg(path);

        debug!(command = %self.command, path = %path.display(), "Launching player");

        let child = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::PlayerNotFound(self.command.clone())
            } else {
                Error::PlayerLaunch(format!("{}: {}", self.command, e))
            }
        })?;

        self.child = Some(child);
        Ok(())
    }

    pub fn wait(&mut self) -> Result<bool> {
        if let Some(ref mut child) = self.child {
            let status = child.wait()?;
            self.child = None;
            Ok(status.success())
        } else {
            Ok(true)
        }
    }

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

fn resolve_executable(name: &str) -> String {
    if Path::new(name).is_absolute() {
        return name.to_string();
    }

    #[cfg(target_os = "windows")]
    {
        let common_paths = [
            r"C:\Program Files\VideoLAN\VLC\vlc.exe",
            r"C:\Program Files (x86)\VideoLAN\VLC\vlc.exe",
            r"C:\Program Files\mpv\mpv.exe",
            r"C:\Program Files (x86)\mpv\mpv.exe",
            r"%LOCALAPPDATA%\Programs\mpv\mpv.exe",
        ];

        let lower_name = name.to_lowercase();

        // If looking for vlc and it's not in path (we can't easily check path existence without trying to spawn,
        // but we can check if these files exist and prioritize them if the name matches)
        if lower_name.contains("vlc") {
            for path in common_paths
                .iter()
                .filter(|p| p.to_lowercase().contains("vlc"))
            {
                let p = Path::new(path);
                if p.exists() {
                    debug!("Found VLC at {:?}", p);
                    return path.to_string();
                }
            }
        }

        if lower_name.contains("mpv") {
            for path in common_paths
                .iter()
                .filter(|p| p.to_lowercase().contains("mpv"))
            {
                let p = Path::new(path);
                if p.exists() {
                    debug!("Found MPV at {:?}", p);
                    return path.to_string();
                }
            }
        }
    }

    name.to_string()
}
