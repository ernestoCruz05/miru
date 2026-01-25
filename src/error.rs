use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse config: {0}")]
    ConfigParse(#[from] toml::de::Error),

    #[error("Failed to serialize config: {0}")]
    ConfigSerialize(#[from] toml::ser::Error),

    #[error("Config directory not found")]
    NoConfigDir,

    #[error("Data directory not found")]
    NoDataDir,

    #[error("Media directory not found: {0}")]
    MediaDirNotFound(PathBuf),

    #[error("Player not found: {0}")]
    PlayerNotFound(String),

    #[error("Failed to launch player: {0}")]
    PlayerLaunch(String),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Nyaa search failed: {0}")]
    NyaaSearch(String),

    #[error("Torrent client error: {0}")]
    TorrentClient(String),
}

pub type Result<T> = std::result::Result<T, Error>;
