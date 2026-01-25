mod transmission;
mod qbittorrent;

pub use transmission::TransmissionClient;
pub use qbittorrent::QBittorrentClient;

use crate::error::Result;

/// Status of a torrent download
#[derive(Debug, Clone)]
pub struct TorrentStatus {
    pub name: String,
    pub hash: String,
    pub progress: f64,       // 0.0 to 1.0
    pub download_rate: u64,  // bytes/sec
    pub upload_rate: u64,    // bytes/sec
    pub size: u64,           // total bytes
    pub downloaded: u64,     // bytes downloaded
    pub seeders: u32,
    pub state: TorrentState,
    pub save_path: String,   // directory where torrent is saved
    pub content_path: String, // full path to content (file or folder)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TorrentState {
    Downloading,
    Seeding,
    Paused,
    Queued,
    Checking,
    Error,
    Unknown,
}

impl TorrentState {
    pub fn as_str(&self) -> &'static str {
        match self {
            TorrentState::Downloading => "Downloading",
            TorrentState::Seeding => "Seeding",
            TorrentState::Paused => "Paused",
            TorrentState::Queued => "Queued",
            TorrentState::Checking => "Checking",
            TorrentState::Error => "Error",
            TorrentState::Unknown => "Unknown",
        }
    }
}

/// Common interface for torrent clients
pub trait TorrentClient {
    /// Add a torrent via magnet link
    fn add_magnet(&self, magnet: &str) -> impl std::future::Future<Output = Result<String>> + Send;

    /// Get status of all torrents
    fn list_torrents(&self) -> impl std::future::Future<Output = Result<Vec<TorrentStatus>>> + Send;

    /// Pause a torrent
    fn pause(&self, hash: &str) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Resume a torrent
    fn resume(&self, hash: &str) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Remove a torrent (optionally with data)
    fn remove(&self, hash: &str, delete_data: bool) -> impl std::future::Future<Output = Result<()>> + Send;
}

/// Enum to hold any supported torrent client
#[derive(Clone)]
pub enum AnyTorrentClient {
    Transmission(TransmissionClient),
    QBittorrent(QBittorrentClient),
}

impl AnyTorrentClient {
    pub async fn add_magnet(&self, magnet: &str) -> Result<String> {
        match self {
            AnyTorrentClient::Transmission(c) => c.add_magnet(magnet).await,
            AnyTorrentClient::QBittorrent(c) => c.add_magnet(magnet).await,
        }
    }

    pub async fn list_torrents(&self) -> Result<Vec<TorrentStatus>> {
        match self {
            AnyTorrentClient::Transmission(c) => c.list_torrents().await,
            AnyTorrentClient::QBittorrent(c) => c.list_torrents().await,
        }
    }

    pub async fn pause(&self, hash: &str) -> Result<()> {
        match self {
            AnyTorrentClient::Transmission(c) => c.pause(hash).await,
            AnyTorrentClient::QBittorrent(c) => c.pause(hash).await,
        }
    }

    pub async fn resume(&self, hash: &str) -> Result<()> {
        match self {
            AnyTorrentClient::Transmission(c) => c.resume(hash).await,
            AnyTorrentClient::QBittorrent(c) => c.resume(hash).await,
        }
    }

    pub async fn remove(&self, hash: &str, delete_data: bool) -> Result<()> {
        match self {
            AnyTorrentClient::Transmission(c) => c.remove(hash, delete_data).await,
            AnyTorrentClient::QBittorrent(c) => c.remove(hash, delete_data).await,
        }
    }
}
