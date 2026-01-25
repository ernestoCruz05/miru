use regex::Regex;
use std::sync::LazyLock;

/// Patterns for extracting episode numbers from anime filenames.
/// Tried in order; first match wins.
static EPISODE_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // [SubGroup] Show Name - 01 [1080p].mkv or Show Name - 01v2.mkv
        // Matches: " - " followed by episode number, optional version suffix
        Regex::new(r"- (\d{2,4})(?:v\d)?(?:\s*[\[\(]|\.|\s|$)").unwrap(),
        // S01E01 format (common for western naming)
        Regex::new(r"[Ss]\d{1,2}[Ee](\d{1,3})").unwrap(),
        // Show.Name.01.mkv or Show_Name_01.mkv
        // Matches: separator followed by bare number before extension
        Regex::new(r"[._\s](\d{2,3})[._\s]*(?:\[|$|\.)").unwrap(),
        // Bare number at start: 01.mkv, 01 - title.mkv
        Regex::new(r"^(\d{2,3})(?:\s*[-._]|\.mkv|\.mp4|\.avi)").unwrap(),
        // Episode 01 or Ep 01 or EP01
        Regex::new(r"[Ee][Pp](?:isode)?[\s._]*(\d{1,3})").unwrap(),
    ]
});

/// Video file extensions we recognize
const VIDEO_EXTENSIONS: &[&str] = &["mkv", "mp4", "avi", "webm", "m4v", "mov"];

/// Extract episode number from a filename
pub fn parse_episode_number(filename: &str) -> Option<u32> {
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

/// Check if a filename is a video file
pub fn is_video_file(filename: &str) -> bool {
    let lower = filename.to_lowercase();
    VIDEO_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
}

/// Generate a show ID from a directory name (lowercase, hyphenated)
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

/// Clean up a directory name into a display title
pub fn make_show_title(name: &str) -> String {
    // Replace common separators with spaces and clean up
    name.replace('_', " ")
        .replace('.', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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
}
