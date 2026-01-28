use serde::Deserialize;
use serde_json::json;
use tracing::debug;

use super::{TorrentClient, TorrentState, TorrentStatus};
use crate::error::{Error, Result};

/// Transmission RPC client
#[derive(Clone)]
pub struct TransmissionClient {
    client: reqwest::Client,
    url: String,
    session_id: std::sync::Arc<tokio::sync::RwLock<Option<String>>>,
}

impl TransmissionClient {
    pub fn new(host: &str, port: u16, username: Option<&str>, password: Option<&str>) -> Self {
        let mut builder = reqwest::Client::builder();

        if let (Some(user), Some(pass)) = (username, password) {
            builder = builder.default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                let auth = base64::engine::general_purpose::STANDARD
                    .encode(format!("{}:{}", user, pass));
                headers.insert(
                    reqwest::header::AUTHORIZATION,
                    format!("Basic {}", auth).parse().unwrap(),
                );
                headers
            });
        }

        Self {
            client: builder.build().expect("Failed to create HTTP client"),
            url: format!("http://{}:{}/transmission/rpc", host, port),
            session_id: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    async fn rpc_call(&self, method: &str, arguments: serde_json::Value) -> Result<serde_json::Value> {
        let body = json!({
            "method": method,
            "arguments": arguments
        });

        let mut request = self.client.post(&self.url).json(&body);

        // Add session ID header if we have one
        if let Some(session_id) = self.session_id.read().await.as_ref() {
            request = request.header("X-Transmission-Session-Id", session_id);
        }

        let response = request.send().await?;

        // Handle 409 Conflict (need to update session ID)
        if response.status() == reqwest::StatusCode::CONFLICT {
            if let Some(new_session_id) = response.headers().get("X-Transmission-Session-Id") {
                let session_id_str = new_session_id.to_str().unwrap_or("").to_string();
                debug!(session_id = %session_id_str, "Updated Transmission session ID");
                *self.session_id.write().await = Some(session_id_str.clone());

                // Retry with new session ID
                let retry_response = self
                    .client
                    .post(&self.url)
                    .header("X-Transmission-Session-Id", session_id_str)
                    .json(&body)
                    .send()
                    .await?;

                let result: TransmissionResponse = retry_response.json().await?;
                return Ok(result.arguments);
            }
        }

        if !response.status().is_success() {
            return Err(Error::TorrentClient(format!(
                "Transmission RPC error: {}",
                response.status()
            )));
        }

        let result: TransmissionResponse = response.json().await?;
        Ok(result.arguments)
    }
}

#[derive(Deserialize)]
struct TransmissionResponse {
    #[allow(dead_code)]
    result: String,
    #[serde(default)]
    arguments: serde_json::Value,
}

impl TorrentClient for TransmissionClient {
    async fn add_magnet(&self, magnet: &str) -> Result<String> {
        let args = json!({
            "filename": magnet
        });

        let result = self.rpc_call("torrent-add", args).await?;

        // Extract hash from response
        let hash = result
            .get("torrent-added")
            .or_else(|| result.get("torrent-duplicate"))
            .and_then(|t| t.get("hashString"))
            .and_then(|h| h.as_str())
            .unwrap_or("")
            .to_string();

        debug!(hash = %hash, "Added magnet to Transmission");
        Ok(hash)
    }

    async fn list_torrents(&self) -> Result<Vec<TorrentStatus>> {
        let args = json!({
            "fields": [
                "hashString", "name", "percentDone", "rateDownload", "rateUpload",
                "totalSize", "downloadedEver", "status", "peersSendingToUs",
                "downloadDir", "files"
            ]
        });

        let result = self.rpc_call("torrent-get", args).await?;

        let torrents = result
            .get("torrents")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| {
                        let name = t.get("name")?.as_str()?.to_string();
                        let download_dir = t.get("downloadDir")?.as_str()?.to_string();
                        
                        // Determine content_path from files
                        let content_path = if let Some(files) = t.get("files").and_then(|f| f.as_array()) {
                            if !files.is_empty() {
                                if let Some(first_file) = files[0].get("name").and_then(|n| n.as_str()) {
                                    // Check if multi-file (folder) or single file
                                    let first_component = first_file.split('/').next().unwrap_or(first_file);
                                    // If multiple files, usually they share a folder. If they don't, it's flat in download_dir.
                                    // Assuming if multi-file, first component is the folder name IF it matches others?
                                    // For simplicity: if it has directory separators, use the top level dir.
                                    // If strictly single file torrent, `files` len is 1.
                                    if files.len() > 1 && first_file.contains('/') {
                                        format!("{}/{}", download_dir, first_component)
                                    } else if files.len() == 1 {
                                        format!("{}/{}", download_dir, first_file)
                                    } else {
                                        // Multiple files but no common root? Unlikely for torrents but possible. 
                                        // Default to download_dir? Or download_dir/name?
                                        // Let's fallback to name usage if structure is unclear, but usually name == root folder.
                                        format!("{}/{}", download_dir, name)
                                    }
                                } else {
                                     format!("{}/{}", download_dir, name)
                                }
                            } else {
                                format!("{}/{}", download_dir, name)
                            }
                        } else {
                            format!("{}/{}", download_dir, name)
                        };

                        Some(TorrentStatus {
                            hash: t.get("hashString")?.as_str()?.to_string(),
                            name,
                            progress: t.get("percentDone")?.as_f64()?,
                            download_rate: t.get("rateDownload")?.as_u64()?,
                            upload_rate: t.get("rateUpload")?.as_u64()?,
                            size: t.get("totalSize")?.as_u64()?,
                            downloaded: t.get("downloadedEver")?.as_u64()?,
                            seeders: t.get("peersSendingToUs")?.as_u64()? as u32,
                            state: parse_transmission_status(t.get("status")?.as_i64()?),
                            save_path: download_dir,
                            content_path,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(torrents)
    }

    async fn pause(&self, hash: &str) -> Result<()> {
        let args = json!({
            "ids": [hash]
        });
        self.rpc_call("torrent-stop", args).await?;
        Ok(())
    }

    async fn resume(&self, hash: &str) -> Result<()> {
        let args = json!({
            "ids": [hash]
        });
        self.rpc_call("torrent-start", args).await?;
        Ok(())
    }

    async fn remove(&self, hash: &str, delete_data: bool) -> Result<()> {
        let args = json!({
            "ids": [hash],
            "delete-local-data": delete_data
        });
        self.rpc_call("torrent-remove", args).await?;
        Ok(())
    }
}

fn parse_transmission_status(status: i64) -> TorrentState {
    // Transmission status codes:
    // 0: Stopped, 1: Check waiting, 2: Checking, 3: Download waiting,
    // 4: Downloading, 5: Seed waiting, 6: Seeding
    match status {
        0 => TorrentState::Paused,
        1 | 2 => TorrentState::Checking,
        3 => TorrentState::Queued,
        4 => TorrentState::Downloading,
        5 => TorrentState::Queued,
        6 => TorrentState::Seeding,
        _ => TorrentState::Unknown,
    }
}

// Need base64 for auth
use base64::Engine;
