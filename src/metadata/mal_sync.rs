//! MAL Sync - Import "Currently Watching" anime from MyAnimeList

use crate::error::Result;
use crate::library::models::TrackedSeries;
use crate::metadata::mal::{MalClient, UserAnimeEntry};

/// Import user's "Currently Watching" anime list from MAL into TrackedSeries
///
/// For each anime:
/// - Creates a TrackedSeries with title as the search query
/// - Sets min_episode to num_watched + 1 (skip already watched)
/// - Skips anime already in existing_tracked
pub fn import_watching_list(
    entries: Vec<UserAnimeEntry>,
    existing_tracked: &[TrackedSeries],
) -> Vec<TrackedSeries> {
    entries
        .into_iter()
        .filter(|entry| {
            // Skip if already tracked (by MAL ID or title match)
            !existing_tracked.iter().any(|t| {
                t.metadata_id == Some(entry.mal_id)
                    || t.title.to_lowercase() == entry.title.to_lowercase()
            })
        })
        .map(|entry| {
            let id = format!("mal-{}", entry.mal_id);
            TrackedSeries {
                id,
                title: entry.title.clone(),
                query: entry.title, // Use MAL title as Nyaa search query
                filter_group: None,
                filter_quality: None,
                min_episode: entry.num_watched + 1, // Start from next unwatched
                season: 1,
                metadata_id: Some(entry.mal_id),
                cached_metadata: None,
            }
        })
        .collect()
}

/// Perform full MAL sync: fetch watching list and convert to TrackedSeries
pub async fn sync_from_mal(
    client: &MalClient,
    existing_tracked: &[TrackedSeries],
) -> Result<Vec<TrackedSeries>> {
    let watching = client.get_user_animelist("watching").await?;
    let new_tracked = import_watching_list(watching, existing_tracked);
    Ok(new_tracked)
}
