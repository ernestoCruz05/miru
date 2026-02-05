use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::{debug, error, warn};

#[derive(Debug, Serialize)]
struct IpcCommand {
    command: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct IpcResponse {
    data: Option<serde_json::Value>,
    error: String,
}

pub struct MpvIpc {
    socket_path: PathBuf,
}

impl MpvIpc {
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn get_time_pos(&self) -> Option<u64> {
        self.get_property_f64("time-pos").map(|t| t as u64)
    }

    pub fn get_duration(&self) -> Option<u64> {
        self.get_property_f64("duration").map(|d| d as u64)
    }

    fn get_property_f64(&self, property: &str) -> Option<f64> {
        let cmd = IpcCommand {
            command: vec![
                serde_json::Value::String("get_property".to_string()),
                serde_json::Value::String(property.to_string()),
            ],
        };

        match self.send_command(&cmd) {
            Ok(resp) => {
                if resp.error == "success" {
                    resp.data.and_then(|v| v.as_f64())
                } else {
                    debug!("IPC error getting {}: {}", property, resp.error);
                    None
                }
            }
            Err(e) => {
                debug!("Failed to query {} from mpv: {}", property, e);
                None
            }
        }
    }

    fn send_command(&self, cmd: &IpcCommand) -> std::io::Result<IpcResponse> {
        #[cfg(unix)]
        {
            use std::os::unix::net::UnixStream;

            let mut stream = UnixStream::connect(&self.socket_path)?;
            stream.set_read_timeout(Some(Duration::from_millis(500)))?;
            stream.set_write_timeout(Some(Duration::from_millis(500)))?;

            let mut json = serde_json::to_string(cmd)?;
            json.push('\n');
            stream.write_all(json.as_bytes())?;
            stream.flush()?;

            let mut reader = BufReader::new(stream);
            let mut response = String::new();
            reader.read_line(&mut response)?;

            let parsed: IpcResponse = serde_json::from_str(&response)?;
            Ok(parsed)
        }

        #[cfg(windows)]
        {
            use std::fs::OpenOptions;

            let pipe = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.socket_path)?;

            let mut stream = pipe;
            let mut json = serde_json::to_string(cmd)?;
            json.push('\n');
            stream.write_all(json.as_bytes())?;
            stream.flush()?;

            let mut reader = BufReader::new(stream);
            let mut response = String::new();
            reader.read_line(&mut response)?;

            let parsed: IpcResponse = serde_json::from_str(&response)?;
            Ok(parsed)
        }
    }

    pub fn cleanup(&self) {
        #[cfg(unix)]
        {
            if self.socket_path.exists() {
                if let Err(e) = std::fs::remove_file(&self.socket_path) {
                    warn!("Failed to cleanup mpv socket: {}", e);
                }
            }
        }
    }
}

impl Drop for MpvIpc {
    fn drop(&mut self) {
        self.cleanup();
    }
}

pub fn generate_socket_path() -> PathBuf {
    let pid = std::process::id();

    #[cfg(unix)]
    {
        PathBuf::from(format!("/tmp/miru-mpv-{}.sock", pid))
    }

    #[cfg(windows)]
    {
        PathBuf::from(format!(r"\\.\pipe\miru-mpv-{}", pid))
    }
}
