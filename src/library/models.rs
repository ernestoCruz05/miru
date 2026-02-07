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
    #[serde(default)]
    pub seasons: Vec<Season>,
    #[serde(default)]
    pub specials: Vec<Episode>,
    #[serde(default)]
    pub metadata: Option<crate::metadata::AnimeMetadata>,
    #[serde(default)]
    pub cover_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub number: u32,
    pub filename: String,
    #[serde(default)]
    pub watched: bool,
    #[serde(default)]
    pub last_position: u64,
    #[serde(default)]
    pub relative_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Season {
    pub number: u32,
    pub folder_name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub episodes: Vec<Episode>,
}

fn default_season() -> u32 {
    1
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
    #[serde(default = "default_season")]
    pub season: u32,
    #[serde(default)]
    pub metadata_id: Option<u64>,
    #[serde(default)]
    pub cached_metadata: Option<crate::metadata::AnimeMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ArchiveMode {
    #[default]
    Ghost,
    Compressed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedShow {
    pub id: String,
    pub title: String,
    pub archived_at: String,
    pub mode: ArchiveMode,
    pub archive_file: Option<PathBuf>,
    pub watch_history: Vec<ArchivedEpisode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedEpisode {
    pub number: u32,
    pub watched: bool,
    pub last_position: u64,
}

impl Show {
    pub fn new(id: impl Into<String>, title: impl Into<String>, path: PathBuf) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            path,
            total_episodes: None,
            episodes: Vec::new(),
            seasons: Vec::new(),
            specials: Vec::new(),
            metadata: None,
            cover_path: None,
        }
    }

    pub fn is_seasonal(&self) -> bool {
        !self.seasons.is_empty()
    }

    pub fn watched_count(&self) -> usize {
        let flat_watched = self.episodes.iter().filter(|e| e.watched).count();
        let season_watched: usize = self
            .seasons
            .iter()
            .flat_map(|s| s.episodes.iter())
            .filter(|e| e.watched)
            .count();
        let special_watched = self.specials.iter().filter(|e| e.watched).count();
        flat_watched + season_watched + special_watched
    }

    pub fn episode_count(&self) -> usize {
        let flat_count = self.episodes.len();
        let season_count: usize = self.seasons.iter().map(|s| s.episodes.len()).sum();
        let special_count = self.specials.len();
        flat_count + season_count + special_count
    }

    pub fn next_unwatched(&self) -> Option<&Episode> {
        if let Some(ep) = self.episodes.iter().find(|e| !e.watched) {
            return Some(ep);
        }
        for season in &self.seasons {
            if let Some(ep) = season.episodes.iter().find(|e| !e.watched) {
                return Some(ep);
            }
        }
        None
    }

    pub fn get_episode(&self, number: u32) -> Option<&Episode> {
        self.episodes.iter().find(|e| e.number == number)
    }

    pub fn get_episode_mut(&mut self, number: u32) -> Option<&mut Episode> {
        self.episodes.iter_mut().find(|e| e.number == number)
    }

    pub fn get_season_episode(&self, season_num: u32, episode_num: u32) -> Option<&Episode> {
        self.seasons
            .iter()
            .find(|s| s.number == season_num)
            .and_then(|s| s.episodes.iter().find(|e| e.number == episode_num))
    }

    pub fn get_season_episode_mut(
        &mut self,
        season_num: u32,
        episode_num: u32,
    ) -> Option<&mut Episode> {
        self.seasons
            .iter_mut()
            .find(|s| s.number == season_num)
            .and_then(|s| s.episodes.iter_mut().find(|e| e.number == episode_num))
    }

    pub fn all_episodes(&self) -> impl Iterator<Item = &Episode> {
        self.episodes
            .iter()
            .chain(self.seasons.iter().flat_map(|s| s.episodes.iter()))
            .chain(self.specials.iter())
    }
}

impl Episode {
    pub fn new(number: u32, filename: impl Into<String>) -> Self {
        Self {
            number,
            filename: filename.into(),
            watched: false,
            last_position: 0,
            relative_path: None,
        }
    }

    pub fn with_relative_path(
        number: u32,
        filename: impl Into<String>,
        relative_path: impl Into<String>,
    ) -> Self {
        Self {
            number,
            filename: filename.into(),
            watched: false,
            last_position: 0,
            relative_path: Some(relative_path.into()),
        }
    }

    pub fn full_path(&self, show_path: &PathBuf) -> PathBuf {
        if let Some(ref rel_path) = self.relative_path {
            show_path.join(rel_path).join(&self.filename)
        } else {
            show_path.join(&self.filename)
        }
    }
}
