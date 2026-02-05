use crate::library::{Library, parser};
use crate::nyaa::{NyaaCategory, NyaaClient, NyaaFilter, NyaaSort};
use std::collections::HashMap;
use tracing::{debug, info};

pub struct UpdateResult {
    pub series_title: String,
    pub episode_number: u32,
    pub magnet: String,
    pub title: String,
}

pub struct ExistingTorrent {
    pub hash: String,
    pub name: String,
}

pub async fn check_for_updates(
    library: &Library,
    client: &NyaaClient,
    existing_torrents: &[ExistingTorrent],
) -> Vec<UpdateResult> {
    let mut updates = Vec::new();

    let tracked = library.tracked_shows.clone();

    for series in tracked {
        info!(series = %series.title, "Checking for updates");

        // Search Nyaa
        match client
            .search(
                &series.query,
                NyaaCategory::AnimeEnglish,
                NyaaFilter::TrustedOnly,
                NyaaSort::Seeders,
            )
            .await
        {
            Ok(results) => {
                let mut best_candidates: HashMap<u32, (i32, String, String)> = HashMap::new();
                // Map: EpisodeNum -> (Score, Magnet, Title)

                for result in results {
                    let title = &result.title;

                    let ep_num = match parser::parse_episode_number(title) {
                        Some(n) => n,
                        None => continue,
                    };

                    if ep_num < series.min_episode {
                        continue;
                    }

                    let existing_show = library.get_show(&series.id).or_else(|| {
                        library.shows.iter().find(|s| {
                            let s_title = s.title.to_lowercase();
                            let q_title = series.title.to_lowercase(); // series.title is the query
                            s_title.contains(&q_title) || q_title.contains(&s_title)
                        })
                    });

                    if let Some(show) = existing_show {
                        if show.get_episode(ep_num).is_some() {
                            continue;
                        }

                        // Check if we are currently downloading it (fuzzy match on title/name)
                        let is_downloading = existing_torrents.iter().any(|t| {
                            let t_name = t.name.to_lowercase();
                            // Check if torrent name contains series title AND episode number
                            if t_name == title.to_lowercase() {
                                return true;
                            }
                            // TODO: fix (maybe?), isn't causing issues yet
                            false
                        });

                        if is_downloading {
                            debug!(
                                "Skipping {} - Episode {} (already downloading)",
                                series.title, ep_num
                            );
                            continue;
                        }
                    }

                    if let Some(ref group) = series.filter_group {
                        if let Some(parsed_group) = parser::parse_release_group(title) {
                            if !parsed_group.contains(group) {
                                // Loose matching?
                                continue;
                            }
                        } else {
                            continue;
                        }
                    }

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
                    // Tune values later if needed (you, the user, i'm too lazy to make it a config value)

                    let mut score: i32 = 0;
                    if let Some(q) = parser::parse_quality(title) {
                        if q == "1080p" {
                            score += 10;
                        } else if q == "720p" {
                            score += 5;
                        }
                    }

                    let current_best =
                        best_candidates
                            .entry(ep_num)
                            .or_insert((-1, String::new(), String::new()));
                    if score > current_best.0 {
                        *current_best = (score, result.magnet_link.clone(), result.title.clone());
                    }
                }

                for (ep_num, (_, magnet, title)) in best_candidates {
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
