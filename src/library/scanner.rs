use std::fs;
use std::path::Path;

use tracing::debug;

use super::batch::{categorize_folder, FolderCategory};
use super::models::{Episode, Season, Show};
use super::parser::{is_video_file, make_show_id, make_show_title, parse_episode_number};
use crate::error::Result;

/// Collect video files from a directory (non-recursive) and create episodes
fn collect_episodes_from_dir(path: &Path, relative_path: Option<&str>) -> Vec<Episode> {
    let Ok(entries) = fs::read_dir(path) else {
        return Vec::new();
    };

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

            if let Some(rel) = relative_path {
                Some(Episode::with_relative_path(ep_num, filename, rel))
            } else {
                Some(Episode::new(ep_num, filename))
            }
        })
        .collect();

    // Sort by episode number
    episodes.sort_by_key(|e| e.number);
    episodes
}

/// Scan a single directory that contains a show's episodes (with recursive season detection)
pub fn scan_show_dir(path: &Path) -> Option<Show> {
    if !path.is_dir() {
        return None;
    }

    let dir_name = path.file_name()?.to_string_lossy();
    let id = make_show_id(&dir_name);
    let title = make_show_title(&dir_name);

    let mut show = Show::new(id, title, path.to_path_buf());

    // Collect loose video files in the root
    show.episodes = collect_episodes_from_dir(path, None);

    // Scan subdirectories for seasons/specials
    let Ok(entries) = fs::read_dir(path) else {
        return finalize_show(show);
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }

        let folder_name = entry.file_name().to_string_lossy().to_string();
        let category = categorize_folder(&folder_name);

        match category {
            FolderCategory::Season(num) => {
                let episodes = collect_episodes_from_dir(&entry_path, Some(&folder_name));
                if !episodes.is_empty() {
                    debug!(season = num, folder = %folder_name, episodes = episodes.len(), "Found season");
                    show.seasons.push(Season {
                        number: num,
                        folder_name: folder_name.clone(),
                        path: entry_path,
                        episodes,
                    });
                }
            }
            FolderCategory::Ova | FolderCategory::Special | FolderCategory::Movie => {
                let episodes = collect_episodes_from_dir(&entry_path, Some(&folder_name));
                debug!(category = ?category, folder = %folder_name, count = episodes.len(), "Found specials");
                show.specials.extend(episodes);
            }
            FolderCategory::Extra => {
                // Extras are usually NCOP/NCED, we can include them as specials or skip
                let episodes = collect_episodes_from_dir(&entry_path, Some(&folder_name));
                debug!(folder = %folder_name, count = episodes.len(), "Found extras (adding as specials)");
                show.specials.extend(episodes);
            }
            FolderCategory::Unknown => {
                // Unknown folder - check if it contains video files directly
                // This handles nested structures like "Show/Subfolder/episodes"
                let nested_episodes = collect_episodes_from_dir(&entry_path, Some(&folder_name));
                if !nested_episodes.is_empty() {
                    debug!(folder = %folder_name, count = nested_episodes.len(), "Found episodes in unknown subfolder");
                    // Add to loose episodes with relative path set
                    show.episodes.extend(nested_episodes);
                }
            }
        }
    }

    // Sort seasons by number
    show.seasons.sort_by_key(|s| s.number);

    finalize_show(show)
}

/// Finalize a show (set total episodes, check if empty)
fn finalize_show(mut show: Show) -> Option<Show> {
    let total = show.episode_count();
    if total == 0 {
        return None;
    }

    show.total_episodes = Some(total as u32);
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
                let season_info = if show.is_seasonal() {
                    format!(" ({} seasons)", show.seasons.len())
                } else {
                    String::new()
                };
                debug!(
                    show = %show.title, 
                    episodes = %show.episode_count(),
                    seasonal = %show.is_seasonal(),
                    "Found show{}", season_info
                );
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
        // Derive show title from filename (strip extension and clean up)
        let title_base = filename
            .rsplit_once('.')
            .map(|(name, _)| name)
            .unwrap_or(&filename);
        
        let id = make_show_id(title_base);
        let title = make_show_title(title_base);
        
        let ep_num = parse_episode_number(&filename).unwrap_or(1);
        let episode = Episode::new(ep_num, &filename);
        
        let mut show = Show::new(&id, &title, path.to_path_buf());
        show.episodes.push(episode);
        show.total_episodes = Some(1);
        
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

