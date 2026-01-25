use serde::Deserialize;
use tracing::debug;

use super::{TorrentClient, TorrentState, TorrentStatus};
use crate::error::{Error, Result};

/// qBittorrent WebUI API client
#[derive(Clone)]
pub struct QBittorrentClient {
    client: reqwest::Client,
    base_url: String,
}

impl QBittorrentClient {
    pub fn new(host: &str, port: u16, username: Option<&str>, password: Option<&str>) -> Self {
        let client = reqwest::Client::builder()
            .cookie_store(true)
            .build()
            .expect("Failed to create HTTP client");

        let qb = Self {
            client,
            base_url: format!("http://{}:{}", host, port),
        };

        // Store credentials for later login if provided
        if let (Some(_user), Some(_pass)) = (username, password) {
            // Login will happen on first API call
        }

        qb
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<()> {
        let url = format!("{}/api/v2/auth/login", self.base_url);

        let response = self
            .client
            .post(&url)
            .form(&[("username", username), ("password", password)])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(Error::TorrentClient(
                "qBittorrent login failed".to_string(),
            ));
        }

        let text = response.text().await?;
        if text != "Ok." {
            return Err(Error::TorrentClient(format!(
                "qBittorrent login failed: {}",
                text
            )));
        }

        debug!("Logged in to qBittorrent");
        Ok(())
    }
}

#[derive(Deserialize)]
struct QBTorrent {
    hash: String,
    name: String,
    progress: f64,
    dlspeed: u64,
    upspeed: u64,
    size: u64,
    downloaded: u64,
    num_seeds: u32,
    state: String,
    save_path: String,
    content_path: String,
}

impl TorrentClient for QBittorrentClient {
    async fn add_magnet(&self, magnet: &str) -> Result<String> {
        let url = format!("{}/api/v2/torrents/add", self.base_url);

        let response = self
            .client
            .post(&url)
            .form(&[("urls", magnet)])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(Error::TorrentClient(format!(
                "qBittorrent add failed: {}",
                response.status()
            )));
        }

        // qBittorrent doesn't return the hash directly, extract from magnet
        // Magnet format: magnet:?xt=urn:btih:<hash>&...
        let hash = magnet
            .split("btih:")
            .nth(1)
            .and_then(|s| s.split('&').next())
            .unwrap_or("")
            .to_lowercase();

        debug!(hash = %hash, "Added magnet to qBittorrent");
        Ok(hash)
    }

    async fn list_torrents(&self) -> Result<Vec<TorrentStatus>> {
        let url = format!("{}/api/v2/torrents/info", self.base_url);

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(Error::TorrentClient(format!(
                "qBittorrent list failed: {}",
                response.status()
            )));
        }

        let torrents: Vec<QBTorrent> = response.json().await?;

        let statuses = torrents
            .into_iter()
            .map(|t| TorrentStatus {
                hash: t.hash,
                name: t.name,
                progress: t.progress,
                download_rate: t.dlspeed,
                upload_rate: t.upspeed,
                size: t.size,
                downloaded: t.downloaded,
                seeders: t.num_seeds,
                state: parse_qb_state(&t.state),
                save_path: t.save_path,
                content_path: t.content_path,
            })
            .collect();

        Ok(statuses)
    }

    async fn pause(&self, hash: &str) -> Result<()> {
        let url = format!("{}/api/v2/torrents/pause", self.base_url);

        self.client
            .post(&url)
            .form(&[("hashes", hash)])
            .send()
            .await?;

        Ok(())
    }

    async fn resume(&self, hash: &str) -> Result<()> {
        let url = format!("{}/api/v2/torrents/resume", self.base_url);

        self.client
            .post(&url)
            .form(&[("hashes", hash)])
            .send()
            .await?;

        Ok(())
    }

    async fn remove(&self, hash: &str, delete_data: bool) -> Result<()> {
        let url = format!("{}/api/v2/torrents/delete", self.base_url);

        self.client
            .post(&url)
            .form(&[
                ("hashes", hash),
                ("deleteFiles", if delete_data { "true" } else { "false" }),
            ])
            .send()
            .await?;

        Ok(())
    }
}

fn parse_qb_state(state: &str) -> TorrentState {
    match state {
        "downloading" | "forcedDL" | "metaDL" | "stalledDL" => TorrentState::Downloading,
        "uploading" | "forcedUP" | "stalledUP" => TorrentState::Seeding,
        "pausedDL" | "pausedUP" => TorrentState::Paused,
        "queuedDL" | "queuedUP" => TorrentState::Queued,
        "checkingDL" | "checkingUP" | "checkingResumeData" => TorrentState::Checking,
        "error" | "missingFiles" => TorrentState::Error,
        _ => TorrentState::Unknown,
    }
}
