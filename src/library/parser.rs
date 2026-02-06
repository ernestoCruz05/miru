use regex::Regex;
use std::sync::LazyLock;

static SEASON_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // S01E05, S02E03 (season+episode combo, most specific)
        Regex::new(r"(?i)\bS(\d{1,2})\s*E\d").unwrap(),
        // S01, S02 (season only, common in anime releases like "Show S02 - 05")
        Regex::new(r"(?i)\bS(\d{1,2})\b").unwrap(),
        // "Season 2", "Season 01"
        Regex::new(r"(?i)\bSeason\s*(\d{1,2})\b").unwrap(),
        // "2nd Season", "3rd Season"
        Regex::new(r"(?i)\b(\d{1,2})(?:st|nd|rd|th)\s+Season\b").unwrap(),
        // "Part 2" (some shows use Part N for seasons)
        Regex::new(r"(?i)\bPart\s*(\d{1,2})\b").unwrap(),
    ]
});

static EPISODE_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // [SubGroup] Show Name - 01 [1080p].mkv or Show Name - 01v2.mkv
        // Matches: " - " followed by episode number, optional version suffix
        Regex::new(r"- (\d{1,4})(?:v\d)?(?:\s*[\[\(]|\.|\s|$)").unwrap(),
        // S01E01 format (common for western naming)
        Regex::new(r"[Ss]\d{1,2}[Ee](\d{1,3})").unwrap(),
        // Show.Name.01.mkv or Show_Name_01.mkv
        // Matches: separator followed by bare number before extension
        Regex::new(r"[._\s](\d{1,3})[._\s]*(?:\[|$|\.)").unwrap(),
        // Bare number at start: 01.mkv, 01 - title.mkv
        Regex::new(r"^(\d{1,3})(?:\s*[-._]|\.mkv|\.mp4|\.avi)").unwrap(),
        // Episode 01 or Ep 01 or EP01
        Regex::new(r"[Ee][Pp](?:isode)?[\s._]*(\d{1,3})").unwrap(),
        // E01 format (common short form)
        Regex::new(r"(?:[-._\s]|^)[Ee](\d{1,4})(?:v\d)?(?:[._\s]|$)").unwrap(),
    ]
});

const VIDEO_EXTENSIONS: &[&str] = &["mkv", "mp4", "avi", "webm", "m4v", "mov"];

const COMPRESSED_EXTENSION: &str = ".zst";

pub fn parse_episode_number(filename: &str) -> Option<u32> {
    let filename = filename
        .strip_suffix(COMPRESSED_EXTENSION)
        .unwrap_or(filename);

    for pattern in EPISODE_PATTERNS.iter() {
        if let Some(caps) = pattern.captures(filename) {
            if let Some(num_match) = caps.get(1) {
                if let Ok(num) = num_match.as_str().parse::<u32>() {
                    // Sanity check: episode numbers are usually 1-999
                    if num > 0 && num < 1000 {
                        return Some(num);
                    }
                }
            }
        }
    }
    None
}

pub fn parse_season_number(title: &str) -> Option<u32> {
    for pattern in SEASON_PATTERNS.iter() {
        if let Some(caps) = pattern.captures(title) {
            if let Some(num_match) = caps.get(1) {
                if let Ok(num) = num_match.as_str().parse::<u32>() {
                    if num > 0 && num < 100 {
                        return Some(num);
                    }
                }
            }
        }
    }
    None
}

pub fn is_video_file(filename: &str) -> bool {
    let lower = filename.to_lowercase();

    if lower.ends_with(COMPRESSED_EXTENSION) {
        let base = &lower[..lower.len() - COMPRESSED_EXTENSION.len()];
        return VIDEO_EXTENSIONS.iter().any(|ext| base.ends_with(ext));
    }

    VIDEO_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
}

pub fn make_show_id(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn make_show_title(name: &str) -> String {
    name.replace('_', " ")
        .replace('.', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn parse_release_group(filename: &str) -> Option<String> {
    Regex::new(r"^\[([^\]]+)\]")
        .unwrap()
        .captures(filename)
        .map(|c| c.get(1).unwrap().as_str().to_string())
}

pub fn parse_quality(filename: &str) -> Option<String> {
    let re = Regex::new(r"((?:360|480|720|1080|2160)[pP]|4[kK])").unwrap();
    re.captures(filename)
        .map(|c| c.get(1).unwrap().as_str().to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subgroup_format() {
        assert_eq!(
            parse_episode_number("[SubGroup] Show Name - 01 [1080p].mkv"),
            Some(1)
        );
        assert_eq!(
            parse_episode_number("[HorribleSubs] Monster - 74 [720p].mkv"),
            Some(74)
        );
    }

    #[test]
    fn test_version_suffix() {
        assert_eq!(parse_episode_number("Show Name - 01v2.mkv"), Some(1));
        assert_eq!(parse_episode_number("Show - 05v3 [1080p].mkv"), Some(5));
    }

    #[test]
    fn test_season_episode_format() {
        assert_eq!(parse_episode_number("Show.Name.S01E01.mkv"), Some(1));
        assert_eq!(parse_episode_number("Show Name S02E15.mp4"), Some(15));
    }

    #[test]
    fn test_bare_number() {
        assert_eq!(parse_episode_number("01.mkv"), Some(1));
        assert_eq!(parse_episode_number("12 - Episode Title.mkv"), Some(12));
    }

    #[test]
    fn test_episode_prefix() {
        assert_eq!(parse_episode_number("Show Episode 05.mkv"), Some(5));
        assert_eq!(parse_episode_number("Show Ep01.mkv"), Some(1));
        assert_eq!(parse_episode_number("Fate Strange Fake - E01.mkv"), Some(1));
        assert_eq!(parse_episode_number("Show - E02.mkv"), Some(2));
    }

    #[test]
    fn test_is_video() {
        assert!(is_video_file("test.mkv"));
        assert!(is_video_file("Test.MKV"));
        assert!(is_video_file("video.mp4"));
        assert!(!is_video_file("subtitle.srt"));
        assert!(!is_video_file("readme.txt"));
    }

    #[test]
    fn test_make_show_id() {
        assert_eq!(make_show_id("Monster"), "monster");
        assert_eq!(make_show_id("Attack on Titan"), "attack-on-titan");
        assert_eq!(make_show_id("Steins;Gate"), "steins-gate");
    }

    #[test]
    fn test_season_s_prefix() {
        // S01E05 format
        assert_eq!(
            parse_season_number("[SubsPlease] Oshi no Ko S02 - 05 [1080p].mkv"),
            Some(2)
        );
        assert_eq!(
            parse_season_number("Show.Name.S01E01.mkv"),
            Some(1)
        );
        assert_eq!(
            parse_season_number("[Judas] Attack on Titan S03E12.mkv"),
            Some(3)
        );
    }

    #[test]
    fn test_season_text_pattern() {
        assert_eq!(
            parse_season_number("Oshi no Ko Season 2 - 05 [1080p].mkv"),
            Some(2)
        );
        assert_eq!(
            parse_season_number("Show Season 1 Episode 5.mkv"),
            Some(1)
        );
    }

    #[test]
    fn test_season_ordinal_pattern() {
        assert_eq!(
            parse_season_number("Oshi no Ko 2nd Season - 05 [1080p].mkv"),
            Some(2)
        );
        assert_eq!(
            parse_season_number("Attack on Titan 3rd Season E12.mkv"),
            Some(3)
        );
    }

    #[test]
    fn test_season_part_pattern() {
        assert_eq!(
            parse_season_number("Show Name Part 2 - 05 [1080p].mkv"),
            Some(2)
        );
    }

    #[test]
    fn test_season_none_when_absent() {
        assert_eq!(
            parse_season_number("[SubsPlease] Frieren - 09 [1080p].mkv"),
            None
        );
        assert_eq!(
            parse_season_number("Monster - 74 [720p].mkv"),
            None
        );
    }

    #[test]
    fn test_season_cour_pattern() {
        assert_eq!(parse_season_number("Show Cour 2"), Some(2));
        assert_eq!(parse_season_number("Show Cour 02"), Some(2));
    }

    #[test]
    fn test_season_ordinal_cour_pattern() {
        assert_eq!(parse_season_number("Show 2nd Cour"), Some(2));
        assert_eq!(parse_season_number("Show 3rd Cour"), Some(3));
    }

    #[test]
    fn test_season_roman_numeral() {
        assert_eq!(parse_season_number("Show III"), Some(3));
        assert_eq!(parse_season_number("Show II"), Some(2));
        assert_eq!(parse_season_number("Show IV"), Some(4));
        assert_eq!(parse_season_number("Show X"), Some(10));
        // Bare "I" excluded -- too ambiguous, season 1 is default
        assert_eq!(parse_season_number("Show I"), None);
    }

    #[test]
    fn test_season_japanese_ki_marker() {
        assert_eq!(parse_season_number("Show 2期"), Some(2));
        assert_eq!(parse_season_number("Show 3期"), Some(3));
    }

    #[test]
    fn test_season_ordinal_word() {
        assert_eq!(parse_season_number("Second Season"), Some(2));
        assert_eq!(parse_season_number("Third Cour"), Some(3));
        assert_eq!(parse_season_number("Fifth Season"), Some(5));
        assert_eq!(parse_season_number("Tenth Season"), Some(10));
        assert_eq!(parse_season_number("First Season"), Some(1));
    }
}
