use std::time::Duration;

use ratatui::widgets::ListState;
use regex::Regex;
use serde::Deserialize;

use crate::error::{Error, Result};
use crate::metadata::AnimeMetadata;

#[derive(Debug, Clone)]
pub enum FileType {
    Video,
    Subtitle,
    Other,
}

#[derive(Debug, Clone)]
pub struct TorrentFileEntry {
    pub path: String,
    pub size: u64,
    pub file_type: FileType,
}

pub enum PreviewSection<T> {
    Loading,
    Loaded(T),
    Error(String),
}

pub struct PreviewState {
    pub torrent_title: String,
    pub torrent_files: PreviewSection<Vec<TorrentFileEntry>>,
    pub mal_info: PreviewSection<AnimeMetadata>,
    pub is_magnet_only: bool,
    pub scroll_state: ListState,
}

// Bencode deserialization structs (private)

#[derive(Deserialize)]
struct TorrentMeta {
    info: TorrentInfo,
}

#[derive(Deserialize)]
struct TorrentInfo {
    name: String,
    #[serde(default)]
    length: Option<i64>,
    #[serde(default)]
    files: Option<Vec<TorrentFile>>,
}

#[derive(Deserialize)]
struct TorrentFile {
    path: Vec<String>,
    length: i64,
}

pub fn classify_file(path: &str) -> FileType {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "mkv" | "mp4" | "avi" | "webm" | "m4v" | "mov" | "wmv" => FileType::Video,
        "ass" | "ssa" | "srt" | "sub" | "idx" | "vtt" => FileType::Subtitle,
        _ => FileType::Other,
    }
}

pub fn parse_torrent_files(bytes: &[u8]) -> Result<Vec<TorrentFileEntry>> {
    let meta: TorrentMeta = serde_bencode::from_bytes(bytes)
        .map_err(|e| Error::TorrentClient(format!("Failed to parse torrent: {}", e)))?;

    let entries = if let Some(files) = meta.info.files {
        files
            .into_iter()
            .map(|f| {
                let path = f.path.join("/");
                let file_type = classify_file(&path);
                TorrentFileEntry {
                    path,
                    size: f.length as u64,
                    file_type,
                }
            })
            .collect()
    } else {
        let file_type = classify_file(&meta.info.name);
        vec![TorrentFileEntry {
            path: meta.info.name,
            size: meta.info.length.unwrap_or(0) as u64,
            file_type,
        }]
    };

    Ok(entries)
}

pub async fn fetch_torrent_files(
    client: &reqwest::Client,
    torrent_url: &str,
) -> Result<Vec<TorrentFileEntry>> {
    let bytes = tokio::time::timeout(Duration::from_secs(10), async {
        let resp = client.get(torrent_url).send().await?;
        resp.bytes().await.map_err(Error::from)
    })
    .await
    .map_err(|_| Error::TorrentClient("Torrent fetch timed out".to_string()))??;

    parse_torrent_files(&bytes)
}

pub fn extract_anime_title(torrent_name: &str) -> String {
    // Strip [bracketed] content (subgroup, hash, quality)
    let re_brackets = Regex::new(r"\[.*?\]").unwrap();
    let clean = re_brackets.replace_all(torrent_name, "");

    // Strip (parenthesized) content
    let re_parens = Regex::new(r"\(.*?\)").unwrap();
    let clean = re_parens.replace_all(&clean, "");

    // Take everything before episode indicators
    let re_episode =
        Regex::new(r"(?i)\s*-\s*\d{1,3}\b|\bS\d{1,2}E\d{1,3}|\bEp?\s*\d{1,3}\b").unwrap();
    let title = re_episode.split(&clean).next().unwrap_or(&clean);

    title.trim().to_string()
}
