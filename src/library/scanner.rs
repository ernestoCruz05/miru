use std::fs;
use std::path::Path;

use tracing::debug;

use super::models::{Episode, Show};
use super::parser::{is_video_file, make_show_id, make_show_title, parse_episode_number};
use crate::error::Result;

/// Scan a single directory that contains a show's episodes
pub fn scan_show_dir(path: &Path) -> Option<Show> {
    if !path.is_dir() {
        return None;
    }

    let dir_name = path.file_name()?.to_string_lossy();
    let id = make_show_id(&dir_name);
    let title = make_show_title(&dir_name);

    let mut show = Show::new(id, title, path.to_path_buf());

    // Scan for video files
    if let Ok(entries) = fs::read_dir(path) {
        let mut episodes: Vec<Episode> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .filter_map(|e| {
                let filename = e.file_name().to_string_lossy().to_string();
                if !is_video_file(&filename) {
                    return None;
                }

                let ep_num = parse_episode_number(&filename).unwrap_or_else(|| {
                    debug!(filename = %filename, "Could not parse episode number, using 0");
                    0
                });

                Some(Episode::new(ep_num, filename))
            })
            .collect();

        // Sort by episode number
        episodes.sort_by_key(|e| e.number);
        show.episodes = episodes;
    }

    if show.episodes.is_empty() {
        return None;
    }

    show.total_episodes = Some(show.episodes.len() as u32);
    Some(show)
}

/// Scan a media directory for show subdirectories
pub fn scan_media_dir(path: &Path) -> Result<Vec<Show>> {
    let mut shows = Vec::new();

    if !path.exists() {
        debug!(path = %path.display(), "Media directory does not exist, skipping");
        return Ok(shows);
    }

    let entries = fs::read_dir(path)?;

    // Collect loose video files for treating as individual shows
    let mut loose_files: Vec<String> = Vec::new();

    for entry in entries.filter_map(|e| e.ok()) {
        let entry_path = entry.path();
        if entry_path.is_dir() {
            // Standard case: subdirectory containing episodes
            if let Some(show) = scan_show_dir(&entry_path) {
                debug!(show = %show.title, episodes = %show.episodes.len(), "Found show");
                shows.push(show);
            }
        } else if entry_path.is_file() {
            // Loose video file directly in media dir
            let filename = entry.file_name().to_string_lossy().to_string();
            if is_video_file(&filename) {
                loose_files.push(filename);
            }
        }
    }

    // Create individual shows for each loose video file
    for filename in loose_files {
        let file_path = path.join(&filename);
        
        // Derive show title from filename (strip extension and clean up)
        let title_base = filename
            .rsplit_once('.')
            .map(|(name, _)| name)
            .unwrap_or(&filename);
        
        let id = make_show_id(title_base);
        let title = make_show_title(title_base);
        
        let ep_num = parse_episode_number(&filename).unwrap_or(1);
        let episode = Episode::new(ep_num, &filename);
        
        let mut show = Show::new(&id, &title, file_path.parent().unwrap_or(path).to_path_buf());
        show.episodes.push(episode);
        show.total_episodes = Some(1);
        
        // Use file path as show path so playback works
        // But store parent dir since that's where the file lives
        show.path = path.to_path_buf();
        
        debug!(show = %show.title, "Found loose video file as show");
        shows.push(show);
    }

    // Sort shows by title
    shows.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    Ok(shows)
}

/// Scan all configured media directories
pub fn scan_all_media_dirs(dirs: &[impl AsRef<Path>]) -> Result<Vec<Show>> {
    let mut all_shows = Vec::new();

    for dir in dirs {
        let shows = scan_media_dir(dir.as_ref())?;
        all_shows.extend(shows);
    }

    // Remove duplicates by ID, preferring the first occurrence
    let mut seen_ids = std::collections::HashSet::new();
    all_shows.retain(|show| seen_ids.insert(show.id.clone()));

    // Final sort
    all_shows.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    Ok(all_shows)
}
