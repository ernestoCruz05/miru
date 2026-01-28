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

pub async fn check_for_updates(library: &Library, client: &NyaaClient) -> Vec<UpdateResult> {
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
                    if let Some(show) = library.get_show(&series.id) {
                        if show.get_episode(ep_num).is_some() {
                            continue; // Already have it
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
