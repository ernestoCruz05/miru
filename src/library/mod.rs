pub mod models;
pub mod parser;
pub mod scanner;
pub mod tracking;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::info;

pub use models::{Episode, Show};
pub use scanner::scan_all_media_dirs;

use crate::config::library_path;
use crate::error::Result;

/// Persisted library state
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Library {
    #[serde(default)]
    pub shows: Vec<Show>,
    #[serde(default)]
    pub tracked_shows: Vec<models::TrackedSeries>,
}

impl Library {
    pub fn load() -> Result<Self> {
        let path = library_path()?;

        if !path.exists() {
            return Ok(Library::default());
        }

        let content = std::fs::read_to_string(&path)?;
        let library: Library = toml::from_str(&content)?;
        Ok(library)
    }

    pub fn save(&self) -> Result<()> {
        let path = library_path()?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Refresh library by scanning directories, preserving watch progress
    pub fn refresh(&mut self, media_dirs: &[impl AsRef<std::path::Path>]) -> Result<()> {
        let scanned = scan_all_media_dirs(media_dirs)?;
        info!(count = scanned.len(), "Scanned shows from media directories");

        // Build a map of existing shows for quick lookup
        let existing: HashMap<String, &Show> =
            self.shows.iter().map(|s| (s.id.clone(), s)).collect();

        // Merge scanned shows with existing watch progress
        let mut merged_shows = Vec::new();

        for mut scanned_show in scanned {
            if let Some(existing_show) = existing.get(&scanned_show.id) {
                // Preserve watch progress from existing episodes
                let existing_eps: HashMap<u32, &Episode> = existing_show
                    .episodes
                    .iter()
                    .map(|e| (e.number, e))
                    .collect();

                for ep in &mut scanned_show.episodes {
                    if let Some(existing_ep) = existing_eps.get(&ep.number) {
                        ep.watched = existing_ep.watched;
                        ep.last_position = existing_ep.last_position;
                    }
                }
            }
            merged_shows.push(scanned_show);
        }

        self.shows = merged_shows;
        Ok(())
    }

    pub fn get_show(&self, id: &str) -> Option<&Show> {
        self.shows.iter().find(|s| s.id == id)
    }

    pub fn get_show_mut(&mut self, id: &str) -> Option<&mut Show> {
        self.shows.iter_mut().find(|s| s.id == id)
    }

    /// Toggle watched status for an episode
    pub fn toggle_watched(&mut self, show_id: &str, episode_num: u32) -> bool {
        if let Some(show) = self.get_show_mut(show_id) {
            if let Some(ep) = show.get_episode_mut(episode_num) {
                ep.watched = !ep.watched;
                if ep.watched {
                    ep.last_position = 0; // Reset position when marking as watched
                }
                return true;
            }
        }
        false
    }

    /// Update playback position for an episode
    pub fn update_position(&mut self, show_id: &str, episode_num: u32, position: u64) {
        if let Some(show) = self.get_show_mut(show_id) {
            if let Some(ep) = show.get_episode_mut(episode_num) {
                ep.last_position = position;
            }
        }
    }

    /// Mark episode as watched
    pub fn mark_watched(&mut self, show_id: &str, episode_num: u32) {
        if let Some(show) = self.get_show_mut(show_id) {
            if let Some(ep) = show.get_episode_mut(episode_num) {
                ep.watched = true;
                ep.last_position = 0;
            }
        }
    }
}
