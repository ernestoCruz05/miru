use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub player: PlayerConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub torrent: TorrentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    pub media_dirs: Vec<PathBuf>,
    pub player: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerConfig {
    #[serde(default)]
    pub mpv: MpvConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpvConfig {
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_true")]
    pub track_progress: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_accent_color")]
    pub accent_color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentConfig {
    #[serde(default = "default_torrent_client")]
    pub client: String,
    #[serde(default = "default_torrent_host")]
    pub host: String,
    #[serde(default = "default_torrent_port")]
    pub port: u16,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

fn default_torrent_client() -> String {
    "transmission".to_string()
}

fn default_torrent_host() -> String {
    "localhost".to_string()
}

fn default_torrent_port() -> u16 {
    9091 // Transmission default
}

fn default_true() -> bool {
    true
}

fn default_accent_color() -> String {
    "magenta".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            player: PlayerConfig::default(),
            ui: UiConfig::default(),
            torrent: TorrentConfig::default(),
        }
    }
}

impl Default for GeneralConfig {
    fn default() -> Self {
        let default_media_dir = data_dir()
            .map(|d| d.join("shows"))
            .unwrap_or_else(|_| PathBuf::from("~/.local/share/miru/shows"));

        Self {
            media_dirs: vec![default_media_dir],
            player: "mpv".to_string(),
        }
    }
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            mpv: MpvConfig::default(),
        }
    }
}

impl Default for MpvConfig {
    fn default() -> Self {
        Self {
            args: vec!["--fullscreen".to_string()],
            track_progress: true,
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            accent_color: default_accent_color(),
        }
    }
}

impl Default for TorrentConfig {
    fn default() -> Self {
        Self {
            client: default_torrent_client(),
            host: default_torrent_host(),
            port: default_torrent_port(),
            username: None,
            password: None,
        }
    }
}

fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("", "", "miru").ok_or(Error::NoConfigDir)
}

pub fn config_dir() -> Result<PathBuf> {
    Ok(project_dirs()?.config_dir().to_path_buf())
}

pub fn data_dir() -> Result<PathBuf> {
    Ok(project_dirs()?.data_dir().to_path_buf())
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

pub fn library_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("library.toml"))
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;

        if !path.exists() {
            let config = Config::default();
            config.save()?;
            return Ok(config);
        }

        let content = std::fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path()?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Expand ~ to home directory in media paths
    pub fn expanded_media_dirs(&self) -> Vec<PathBuf> {
        self.general
            .media_dirs
            .iter()
            .map(|p| {
                let path_str = p.to_string_lossy();
                if path_str.starts_with("~/") {
                    if let Some(home) = dirs_home() {
                        return home.join(&path_str[2..]);
                    }
                }
                p.clone()
            })
            .collect()
    }
}

fn dirs_home() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
}
