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
    #[serde(default)]
    pub metadata: MetadataConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataConfig {
    #[serde(default = "default_mal_client_id")]
    pub mal_client_id: String,
    #[serde(default)]
    pub mal_access_token: Option<String>,
    #[serde(default)]
    pub mal_refresh_token: Option<String>,
    #[serde(default)]
    pub mal_token_expires: Option<i64>,
}

fn default_mal_client_id() -> String {
    "".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    pub media_dirs: Vec<PathBuf>,
    pub player: String,
    #[serde(default)]
    pub compress_episodes: bool,
    #[serde(default = "default_compression_level")]
    pub compression_level: i32,
    #[serde(default = "default_archive_path")]
    pub archive_path: PathBuf,
    #[serde(default = "default_archive_mode")]
    pub archive_mode: String,
    #[serde(default = "default_true")]
    pub notifications: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerConfig {
    #[serde(default)]
    pub mpv: PlayerProfile,
    #[serde(default)]
    pub vlc: Option<PlayerProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerProfile {
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
    #[serde(default)]
    pub managed_daemon_command: Option<String>,
    #[serde(default)]
    pub managed_daemon_args: Option<Vec<String>>,
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

fn default_compression_level() -> i32 {
    3
}

fn default_accent_color() -> String {
    "magenta".to_string()
}

fn default_archive_path() -> PathBuf {
    PathBuf::from("~/.miru/archives")
}

fn default_archive_mode() -> String {
    "ghost".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            player: PlayerConfig::default(),
            ui: UiConfig::default(),
            torrent: TorrentConfig::default(),
            metadata: MetadataConfig::default(),
        }
    }
}

impl Default for MetadataConfig {
    fn default() -> Self {
        Self {
            mal_client_id: default_mal_client_id(),
            mal_access_token: None,
            mal_refresh_token: None,
            mal_token_expires: None,
        }
    }
}

impl Default for GeneralConfig {
    fn default() -> Self {
        let default_media_dir = data_dir()
            .map(|d| d.join("shows"))
            .unwrap_or_else(|_| PathBuf::from("shows"));

        Self {
            media_dirs: vec![default_media_dir],
            player: "mpv".to_string(),
            compress_episodes: false,
            compression_level: default_compression_level(),
            archive_path: default_archive_path(),
            archive_mode: default_archive_mode(),
            notifications: true,
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

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            mpv: PlayerProfile::default_mpv(),
            vlc: None,
        }
    }
}

impl Default for PlayerProfile {
    fn default() -> Self {
        Self::default_mpv()
    }
}

impl PlayerProfile {
    pub fn default_mpv() -> Self {
        Self {
            args: vec!["--fullscreen".to_string()],
            track_progress: true,
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
            managed_daemon_command: None,
            managed_daemon_args: None,
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

    pub fn expanded_media_dirs(&self) -> Vec<PathBuf> {
        self.general
            .media_dirs
            .iter()
            .map(|p| {
                let path_str = p.to_string_lossy();
                // Support both ~/. and ~\ for generic home directory expansion
                if path_str.starts_with("~/") || path_str.starts_with("~\\") || path_str == "~" {
                    if let Some(home) = dirs_home() {
                        if path_str == "~" {
                            return home;
                        }
                        return home.join(&path_str[2..]);
                    }
                }
                p.clone()
            })
            .collect()
    }

    pub fn expanded_archive_path(&self) -> PathBuf {
        let path_str = self.general.archive_path.to_string_lossy();
        if path_str.starts_with("~/") || path_str.starts_with("~\\") || path_str == "~" {
            if let Some(home) = dirs_home() {
                if path_str == "~" {
                    return home;
                }
                return home.join(&path_str[2..]);
            }
        }
        self.general.archive_path.clone()
    }
}

fn dirs_home() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
}
