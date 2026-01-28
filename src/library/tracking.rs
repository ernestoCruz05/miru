use std::collections::HashMap;
use tracing::{info, debug};
use crate::nyaa::{NyaaClient, NyaaCategory, NyaaFilter, NyaaSort};
use crate::library::{Library, parser};

/// Result of a check, containing magnet link and metadata
pub struct UpdateResult {
    pub series_title: String,
    pub episode_number: u32,
    pub magnet: String,
    pub title: String,
}

// Basic info about existing torrents to avoid re-adding
pub struct ExistingTorrent {
    pub hash: String,
    pub name: String,
}

pub async fn check_for_updates(library: &Library, client: &NyaaClient, existing_torrents: &[ExistingTorrent]) -> Vec<UpdateResult> {
    let mut updates = Vec::new();

    // Iterate over tracked shows
    // We clone the list to avoid borrowing issues while mutating library later (though we only read here ideally)
    // Actually we only read library to check for existing episodes.
    let tracked = library.tracked_shows.clone();

    for series in tracked {
        info!(series = %series.title, "Checking for updates");

        // Search Nyaa
        // Use generic filters (Anime - Trusted only)
        match client.search(
            &series.query,
            NyaaCategory::AnimeEnglish, // Safe default? or All?
            NyaaFilter::TrustedOnly, // Prioritize trusted
            NyaaSort::Seeders // Sort by seeders to get healthy torrents first
        ).await {
            Ok(results) => {
                let mut best_candidates: HashMap<u32, (i32, String, String)> = HashMap::new(); 
                // Map: EpisodeNum -> (Score, Magnet, Title)
                // Score: higher is better

                for result in results {
                    let title = &result.title;
                    
                    // Parse metadata
                    let ep_num = match parser::parse_episode_number(title) {
                        Some(n) => n,
                        None => continue, // Skip if can't parse episode
                    };

                    if ep_num < series.min_episode {
                        continue;
                    }

                    // Check if we already have this episode
                    // Try ID match first, then fallback to title match
                    let existing_show = library.get_show(&series.id).or_else(|| {
                        library.shows.iter().find(|s| {
                            // Simple case-insensitive containment check
                            // Check if library show title contains query, or vice-versa
                            let s_title = s.title.to_lowercase();
                            let q_title = series.title.to_lowercase(); // series.title is the query
                            s_title.contains(&q_title) || q_title.contains(&s_title)
                        })
                    });

                    if let Some(show) = existing_show {
                        if show.get_episode(ep_num).is_some() {
                            continue; // Already have in library
                        }
                        
                        // Also check if we are currently downloading it (fuzzy match on title/name)
                        // We check if any existing torrent looks like this episode
                        // This is a heuristic.
                        let is_downloading = existing_torrents.iter().any(|t| {
                            let t_name = t.name.to_lowercase();
                            // Check if torrent name contains series title AND episode number
                            // Or matches the result title roughly
                            if t_name == title.to_lowercase() {
                                return true;
                            }
                            // Heuristic: torrent name contains "Show Name" and "02" or "E02"
                            // This is tricky. 
                            // Easier: check against resolved "UpdateResult" later? 
                            // No, we want to filter early.
                            // Let's rely on exact title match (often works if Nyaa title is used as name)
                            // OR if client uses magnet name.
                            false
                        });
                        
                        if is_downloading {
                            debug!("Skipping {} - Episode {} (already downloading)", series.title, ep_num);
                            continue;
                        }
                    }

                    // Filter by Group
                    if let Some(ref group) = series.filter_group {
                        if let Some(parsed_group) = parser::parse_release_group(title) {
                            if !parsed_group.contains(group) { // Loose matching?
                                continue;
                            }
                        } else {
                            // If we require a group but can't find one, skip (safe) or allow?
                            // Safest is skip logic: strict matching.
                            continue;
                        }
                    }

                    // Filter by Quality (strict or partial?)
                    if let Some(ref quality) = series.filter_quality {
                        if let Some(parsed_qual) = parser::parse_quality(title) {
                             if parsed_qual != quality.to_lowercase() {
                                 continue;
                             }
                        } else {
                            continue;
                        }
                    }

                    // Calculate score for selection
                    // Base score = 10
                    // Bonus for 1080p = +5 (unless filtered)
                    // Bonus for matching preferred group (already filtered)
                    // Tie breaker = seeders (results come sorted by seeders, so first one usually wins if we don't overwrite)
                    
                    // Actually, since we sort by seeders, the first valid match is usually the best one unless we want to prioritize quality specifically.
                    // If user set quality filter, we only see that quality.
                    // If user left quality blank, we might see 720p and 1080p.
                    // We prefer 1080p.
                    
                    let mut score: i32 = 0;
                    if let Some(q) = parser::parse_quality(title) {
                        if q == "1080p" { score += 10; }
                        else if q == "720p" { score += 5; }
                    }

                    // If we haven't picked this episode yet, or this one is better score
                    // Note: Since results are sorted by seeders, later processing might have fewer seeders.
                    // If score is equal, keep existing (higher seeders).
                    // If score is higher, take new one.
                    
                    let current_best = best_candidates.entry(ep_num).or_insert((-1, String::new(), String::new()));
                    if score > current_best.0 {
                        *current_best = (score, result.magnet_link.clone(), result.title.clone());
                    }
                }

                // Collect results
                for (ep_num, (_, magnet, title)) in best_candidates {
                    // One last check to ensure logic is sound (we score initialized to -1 so if no valid found it stays -1? No, we insert valid ones)
                    // Actually logic above inserts with score 0 minimum if matched.
                    
                    updates.push(UpdateResult {
                        series_title: series.title.clone(),
                        episode_number: ep_num,
                        magnet,
                        title,
                    });
                }
            }
            Err(e) => {
                debug!("Failed to check updates for {}: {}", series.title, e);
            }
        }
    }

    updates
}
