use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Show {
    pub id: String,
    pub title: String,
    pub path: PathBuf,
    #[serde(default)]
    pub total_episodes: Option<u32>,
    #[serde(default)]
    pub episodes: Vec<Episode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub number: u32,
    pub filename: String,
    #[serde(default)]
    pub watched: bool,
    /// Last playback position in seconds (for resume)
    #[serde(default)]
    pub last_position: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrackedSeries {
    pub id: String,
    pub title: String,
    pub query: String,
    pub filter_group: Option<String>,
    pub filter_quality: Option<String>,
    #[serde(default)]
    pub min_episode: u32,
}

impl Show {
    pub fn new(id: impl Into<String>, title: impl Into<String>, path: PathBuf) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            path,
            total_episodes: None,
            episodes: Vec::new(),
        }
    }

    pub fn watched_count(&self) -> usize {
        self.episodes.iter().filter(|e| e.watched).count()
    }

    pub fn episode_count(&self) -> usize {
        self.episodes.len()
    }

    /// Find the next unwatched episode
    pub fn next_unwatched(&self) -> Option<&Episode> {
        self.episodes.iter().find(|e| !e.watched)
    }

    pub fn get_episode(&self, number: u32) -> Option<&Episode> {
        self.episodes.iter().find(|e| e.number == number)
    }

    pub fn get_episode_mut(&mut self, number: u32) -> Option<&mut Episode> {
        self.episodes.iter_mut().find(|e| e.number == number)
    }
}

impl Episode {
    pub fn new(number: u32, filename: impl Into<String>) -> Self {
        Self {
            number,
            filename: filename.into(),
            watched: false,
            last_position: 0,
        }
    }

    pub fn full_path(&self, show_path: &PathBuf) -> PathBuf {
        show_path.join(&self.filename)
    }
}
