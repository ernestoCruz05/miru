mod qbittorrent;
pub mod preview;
mod transmission;

pub use qbittorrent::QBittorrentClient;
pub use transmission::TransmissionClient;

use crate::error::Result;

#[derive(Debug, Clone)]
pub struct TorrentStatus {
    pub name: String,
    pub hash: String,
    pub progress: f64,
    pub download_rate: u64,
    pub upload_rate: u64,
    pub size: u64,
    pub downloaded: u64,
    pub seeders: u32,
    pub state: TorrentState,
    pub save_path: String,
    pub content_path: String,
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

pub trait TorrentClient {
    fn add_magnet(&self, magnet: &str) -> impl std::future::Future<Output = Result<String>> + Send;

    fn list_torrents(&self)
    -> impl std::future::Future<Output = Result<Vec<TorrentStatus>>> + Send;

    fn pause(&self, hash: &str) -> impl std::future::Future<Output = Result<()>> + Send;

    fn resume(&self, hash: &str) -> impl std::future::Future<Output = Result<()>> + Send;

    fn remove(
        &self,
        hash: &str,
        delete_data: bool,
    ) -> impl std::future::Future<Output = Result<()>> + Send;
}

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
