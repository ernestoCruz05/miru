use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use std::sync::LazyLock;
use tracing::debug;

use super::parser::is_video_file;

static SEASON_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // "Season 1", "Season 02", "Season 1 - Arc Name"
        Regex::new(r"(?i)^Season\s*(\d+)").unwrap(),
        // "S01", "S1", "S01 - Name"
        Regex::new(r"(?i)^S(\d{1,2})(?:\s|$|-)").unwrap(),
        // "Part 1", "Part 2"
        Regex::new(r"(?i)^Part\s*(\d+)").unwrap(),
        // "Cour 1", "Cour 2"
        Regex::new(r"(?i)^Cour\s*(\d+)").unwrap(),
    ]
});

static SPECIAL_PATTERNS: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    vec![
        ("ova", Regex::new(r"(?i)^(OVA|OAV|OAD)s?$").unwrap()),
        ("special", Regex::new(r"(?i)^Specials?$").unwrap()),
        ("movie", Regex::new(r"(?i)^Movies?$").unwrap()),
        ("extra", Regex::new(r"(?i)^Extras?$").unwrap()),
        ("extra", Regex::new(r"(?i)^Bonus$").unwrap()),
        ("extra", Regex::new(r"(?i)^NC[OE][PD]s?$").unwrap()), // NCOP/NCED
    ]
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderCategory {
    /// A season folder (e.g., "Season 1", "S02")
    Season(u32),
    /// OVA content
    Ova,
    /// Specials
    Special,
    /// Movies
    Movie,
    /// Extra content (NCOP, NCED, bonus)
    Extra,
    /// Unknown/uncategorized folder
    Unknown,
}

#[derive(Debug, Clone)]
pub struct SeasonInfo {
    pub number: u32,
    pub folder_name: String,
    pub path: PathBuf,
    pub episodes: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct SpecialsInfo {
    pub ovas: Vec<PathBuf>,
    pub movies: Vec<PathBuf>,
    pub specials: Vec<PathBuf>,
    pub extras: Vec<PathBuf>,
}

impl SpecialsInfo {
    pub fn is_empty(&self) -> bool {
        self.ovas.is_empty()
            && self.movies.is_empty()
            && self.specials.is_empty()
            && self.extras.is_empty()
    }

    pub fn total_count(&self) -> usize {
        self.ovas.len() + self.movies.len() + self.specials.len() + self.extras.len()
    }
}

#[derive(Debug, Clone)]
pub struct BatchAnalysis {
    pub is_batch: bool,
    pub total_videos: usize,
    pub seasons: Vec<SeasonInfo>,
    pub specials: SpecialsInfo,
    pub loose_episodes: Vec<PathBuf>,
}

impl BatchAnalysis {
    pub fn empty() -> Self {
        Self {
            is_batch: false,
            total_videos: 0,
            seasons: Vec::new(),
            specials: SpecialsInfo::default(),
            loose_episodes: Vec::new(),
        }
    }

    pub fn summary(&self) -> String {
        let mut parts = Vec::new();

        if !self.seasons.is_empty() {
            parts.push(format!("{} season(s)", self.seasons.len()));
        }
        if !self.specials.ovas.is_empty() {
            parts.push(format!("{} OVA(s)", self.specials.ovas.len()));
        }
        if !self.specials.movies.is_empty() {
            parts.push(format!("{} movie(s)", self.specials.movies.len()));
        }
        if !self.specials.specials.is_empty() {
            parts.push(format!("{} special(s)", self.specials.specials.len()));
        }
        if !self.loose_episodes.is_empty() {
            parts.push(format!("{} episode(s)", self.loose_episodes.len()));
        }

        if parts.is_empty() {
            "Empty".to_string()
        } else {
            parts.join(", ")
        }
    }
}

pub fn categorize_folder(name: &str) -> FolderCategory {
    for pattern in SEASON_PATTERNS.iter() {
        if let Some(caps) = pattern.captures(name) {
            if let Some(num_match) = caps.get(1) {
                if let Ok(num) = num_match.as_str().parse::<u32>() {
                    return FolderCategory::Season(num);
                }
            }
        }
    }

    for (category, pattern) in SPECIAL_PATTERNS.iter() {
        if pattern.is_match(name) {
            return match *category {
                "ova" => FolderCategory::Ova,
                "special" => FolderCategory::Special,
                "movie" => FolderCategory::Movie,
                "extra" => FolderCategory::Extra,
                _ => FolderCategory::Unknown,
            };
        }
    }

    FolderCategory::Unknown
}

pub fn is_batch_folder(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }

    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };

    let mut video_count = 0;
    let mut subdir_count = 0;

    for entry in entries.filter_map(|e| e.ok()) {
        let entry_path = entry.path();
        if entry_path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if categorize_folder(&name) != FolderCategory::Unknown {
                return true;
            }
            subdir_count += 1;
        } else if entry_path.is_file() {
            let name = entry.file_name().to_string_lossy().to_string();
            if is_video_file(&name) {
                video_count += 1;
            }
        }
    }

    video_count >= 4 || subdir_count > 0
}

fn collect_videos_in_dir(path: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(path) else {
        return Vec::new();
    };

    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter(|e| is_video_file(&e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .collect()
}

pub fn analyze_batch(path: &Path) -> BatchAnalysis {
    if !path.is_dir() {
        if path.is_file() && is_video_file(&path.file_name().unwrap_or_default().to_string_lossy())
        {
            return BatchAnalysis {
                is_batch: false,
                total_videos: 1,
                seasons: Vec::new(),
                specials: SpecialsInfo::default(),
                loose_episodes: vec![path.to_path_buf()],
            };
        }
        return BatchAnalysis::empty();
    }

    let mut analysis = BatchAnalysis {
        is_batch: false,
        total_videos: 0,
        seasons: Vec::new(),
        specials: SpecialsInfo::default(),
        loose_episodes: Vec::new(),
    };

    analysis.loose_episodes = collect_videos_in_dir(path);

    let Ok(entries) = fs::read_dir(path) else {
        return analysis;
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }

        let folder_name = entry.file_name().to_string_lossy().to_string();
        let category = categorize_folder(&folder_name);
        let videos = collect_videos_in_dir(&entry_path);

        debug!(folder = %folder_name, category = ?category, videos = videos.len(), "Categorized folder");

        match category {
            FolderCategory::Season(num) => {
                analysis.seasons.push(SeasonInfo {
                    number: num,
                    folder_name,
                    path: entry_path,
                    episodes: videos,
                });
            }
            FolderCategory::Ova => {
                analysis.specials.ovas.extend(videos);
            }
            FolderCategory::Special => {
                analysis.specials.specials.extend(videos);
            }
            FolderCategory::Movie => {
                analysis.specials.movies.extend(videos);
            }
            FolderCategory::Extra => {
                analysis.specials.extras.extend(videos);
            }
            FolderCategory::Unknown => {
                let sub_analysis = analyze_batch(&entry_path);
                if !sub_analysis.seasons.is_empty() {
                    analysis.seasons.extend(sub_analysis.seasons);
                    analysis.specials.ovas.extend(sub_analysis.specials.ovas);
                    analysis
                        .specials
                        .movies
                        .extend(sub_analysis.specials.movies);
                    analysis
                        .specials
                        .specials
                        .extend(sub_analysis.specials.specials);
                    analysis
                        .specials
                        .extras
                        .extend(sub_analysis.specials.extras);
                } else {
                    analysis.loose_episodes.extend(videos);
                }
            }
        }
    }

    analysis.seasons.sort_by_key(|s| s.number);

    analysis.total_videos = analysis.loose_episodes.len()
        + analysis
            .seasons
            .iter()
            .map(|s| s.episodes.len())
            .sum::<usize>()
        + analysis.specials.total_count();

    analysis.is_batch =
        !analysis.seasons.is_empty() || analysis.total_videos >= 4 || !analysis.specials.is_empty();

    analysis
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_categorize_season_folders() {
        assert_eq!(categorize_folder("Season 1"), FolderCategory::Season(1));
        assert_eq!(categorize_folder("Season 02"), FolderCategory::Season(2));
        assert_eq!(categorize_folder("S01"), FolderCategory::Season(1));
        assert_eq!(categorize_folder("S1"), FolderCategory::Season(1));
        assert_eq!(categorize_folder("Part 1"), FolderCategory::Season(1));
        assert_eq!(categorize_folder("Cour 2"), FolderCategory::Season(2));
    }

    #[test]
    fn test_categorize_special_folders() {
        assert_eq!(categorize_folder("OVA"), FolderCategory::Ova);
        assert_eq!(categorize_folder("OVAs"), FolderCategory::Ova);
        assert_eq!(categorize_folder("Specials"), FolderCategory::Special);
        assert_eq!(categorize_folder("Movie"), FolderCategory::Movie);
        assert_eq!(categorize_folder("Movies"), FolderCategory::Movie);
        assert_eq!(categorize_folder("Extras"), FolderCategory::Extra);
        assert_eq!(categorize_folder("NCOP"), FolderCategory::Extra);
        assert_eq!(categorize_folder("NCED"), FolderCategory::Extra);
    }

    #[test]
    fn test_categorize_unknown_folders() {
        assert_eq!(categorize_folder("Random Folder"), FolderCategory::Unknown);
        assert_eq!(categorize_folder("Subs"), FolderCategory::Unknown);
        assert_eq!(categorize_folder("Fonts"), FolderCategory::Unknown);
    }
}
