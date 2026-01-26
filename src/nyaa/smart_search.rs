use regex::Regex;
use std::sync::LazyLock;

// ============================================================================
// TYPES
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedQuery {
    pub show_name: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub is_batch_request: bool,
    pub raw_query: String,
}

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub primary: String,
    /// Alternative queries tried in order if primary fails
    pub alternatives: Vec<String>,
    pub parsed: ParsedQuery,
}

// ============================================================================
// REGEX PATTERNS
// ============================================================================

static SEASON_EPISODE_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // Matches: S01E09, s01e09, S1E9
        Regex::new(r"(?i)\bS(\d{1,2})\s*E(\d{1,3})\b").unwrap(),
        // Matches: S01 E09 (with space)
        Regex::new(r"(?i)\bS(\d{1,2})\s+E(\d{1,3})\b").unwrap(),
        // Matches: 1x09, 01x09
        Regex::new(r"(?i)\b(\d{1,2})x(\d{1,3})\b").unwrap(),
        // Matches: Season 1 Episode 9
        Regex::new(r"(?i)\bSeason\s*(\d{1,2})\s*Episode\s*(\d{1,3})\b").unwrap(),
        // Matches: S01 Ep09, S1 Episode 9
        Regex::new(r"(?i)\bS(\d{1,2})\s*(?:Ep|Episode|E)\s*(\d{1,3})\b").unwrap(),
        // Matches: Ep 09, Episode 09 (no season, implies S1)
        Regex::new(r"(?i)\b(?:Ep|Episode)\s*(\d{1,3})\b").unwrap(),
    ]
});

static SEASON_ONLY_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // Matches: S01, S1
        Regex::new(r"(?i)\bS(\d{1,2})\b").unwrap(),
        // Matches: Season 1, Season 01
        Regex::new(r"(?i)\bSeason\s*(\d{1,2})\b").unwrap(),
    ]
});

static BATCH_INDICATORS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(batch|complete|full|all\s*episodes?|1-\d+|\d+-\d+)\b").unwrap()
});

static TITLE_CLEANUP: LazyLock<Regex> = LazyLock::new(|| {
    // Remove common noise words that might interfere with search
    Regex::new(r"(?i)\b(the|a|an)\b").unwrap()
});

// ============================================================================
// CORE PARSING LOGIC
// ============================================================================

pub fn parse_query(query: &str) -> ParsedQuery {
    let query = query.trim();
    let mut show_name = query.to_string();
    let mut season: Option<u32> = None;
    let mut episode: Option<u32> = None;
    let is_batch = BATCH_INDICATORS.is_match(query);

    // Try to match season + episode patterns first
    for pattern in SEASON_EPISODE_PATTERNS.iter() {
        if let Some(caps) = pattern.captures(query) {
            // Everything before the match is the show name
            let match_start = caps.get(0).unwrap().start();
            show_name = query[..match_start].trim().to_string();

            if caps.len() == 2 {
                // Handle episode-only pattern (implies Season 1)
                season = Some(1);
                episode = caps.get(1).and_then(|m| m.as_str().parse().ok());
            } else {
                season = caps.get(1).and_then(|m| m.as_str().parse().ok());
                episode = caps.get(2).and_then(|m| m.as_str().parse().ok());
            }
            break;
        }
    }

    // Fallback: try season-only patterns if no episode found
    if episode.is_none() {
        for pattern in SEASON_ONLY_PATTERNS.iter() {
            if let Some(caps) = pattern.captures(query) {
                let match_start = caps.get(0).unwrap().start();
                show_name = query[..match_start].trim().to_string();
                season = caps.get(1).and_then(|m| m.as_str().parse().ok());
                break;
            }
        }
    }

    show_name = normalize_show_name(&show_name);

    ParsedQuery {
        show_name,
        season,
        episode,
        is_batch_request: is_batch || (season.is_some() && episode.is_none()),
        raw_query: query.to_string(),
    }
}

fn normalize_show_name(name: &str) -> String {
    let name = name.trim();
    // Remove trailing punctuation (e.g. from "Show Name -")
    let name = name.trim_end_matches(|c: char| c == '-' || c == ':' || c.is_whitespace());
    
    let parts: Vec<&str> = name.split_whitespace().collect();
    parts.join(" ")
}

fn format_episode(ep: u32) -> String {
    if ep < 10 {
        format!("{:02}", ep)
    } else {
        ep.to_string()
    }
}

fn format_season(season: u32) -> String {
    if season < 10 {
        format!("{:02}", season)
    } else {
        season.to_string()
    }
}

// ============================================================================
// SEARCH QUERY GENERATION
// ============================================================================

/// Generate optimized search queries from user input
pub fn build_search_query(input: &str) -> SearchQuery {
    let parsed = parse_query(input);

    // If we couldn't parse anything meaningful, just return the raw query
    if parsed.show_name.is_empty() {
        return SearchQuery {
            primary: parsed.raw_query.clone(),
            alternatives: vec![],
            parsed,
        };
    }

    let queries = if parsed.is_batch_request {
        generate_batch_queries(&parsed)
    } else if let Some(ep) = parsed.episode {
        generate_episode_queries(&parsed, ep)
    } else {
        // Just a show name, no season/episode
        generate_show_queries(&parsed)
    };

    SearchQuery {
        primary: queries.first().cloned().unwrap_or(parsed.raw_query.clone()),
        alternatives: queries.into_iter().skip(1).collect(),
        parsed,
    }
}

/// Generate queries for batch/complete season downloads
fn generate_batch_queries(parsed: &ParsedQuery) -> Vec<String> {
    let show = &parsed.show_name;
    let mut queries = Vec::new();

    match parsed.season {
        Some(1) => {
            // Season 1 batch - various naming conventions
            queries.push(format!("{} batch", show));
            queries.push(format!("{} complete", show));
            queries.push(format!("{} 1080p batch", show));
            queries.push(format!("{} Season 1", show));
            queries.push(format!("{} S01", show));
        }
        Some(s) => {
            // Other seasons
            let s_fmt = format_season(s);
            queries.push(format!("{} S{} batch", show, s_fmt));
            queries.push(format!("{} Season {} batch", show, s));
            queries.push(format!("{} S{}", show, s_fmt));
            queries.push(format!("{} Season {}", show, s));
            queries.push(format!("{} {}nd Season", show, s)); // 2nd Season, etc.
        }
        None => {
            // No season specified, generic batch
            queries.push(format!("{} batch", show));
            queries.push(format!("{} complete", show));
            queries.push(show.clone());
        }
    }

    queries
}

/// Generate queries for specific episode searches
fn generate_episode_queries(parsed: &ParsedQuery, episode: u32) -> Vec<String> {
    let show = &parsed.show_name;
    let ep = format_episode(episode);
    let mut queries = Vec::new();

    match parsed.season {
        Some(1) | None => {
            // Season 1 (or unspecified, assume S1)
            // Most anime uses absolute episode numbers for S1

            // Primary: "Show Name 09" - most common format
            queries.push(format!("{} {}", show, ep));

            // "Show Name - 09" - common subgroup format
            queries.push(format!("{} - {}", show, ep));

            // "Show Name E09"
            queries.push(format!("{} E{}", show, ep));

            // "Show Name Episode 09"
            queries.push(format!("{} Episode {}", show, ep));

            // With quality tags (often helps narrow down)
            queries.push(format!("{} {} 1080p", show, ep));

            // Absolute number without padding (for ep > 9)
            if episode >= 10 {
                queries.push(format!("{} {}", show, episode));
            }
        }
        Some(s) => {
            // Season 2+ - more complex naming
            let s_fmt = format_season(s);

            // "Show S02 - 05" - common for multi-season shows
            queries.push(format!("{} S{} - {}", show, s_fmt, ep));

            // "Show S02 05"
            queries.push(format!("{} S{} {}", show, s_fmt, ep));

            // "Show Season 2 - 05"
            queries.push(format!("{} Season {} - {}", show, s, ep));

            // "Show 2nd Season 05"
            queries.push(format!("{} {} {}", show, ordinal(s), ep));

            // Some shows use "Part 2" instead of "Season 2"
            queries.push(format!("{} Part {} {}", show, s, ep));

            // Try absolute numbering (S2E5 might be episode 17 absolute)
            // This is a rough estimate - 12 eps per season is common
            let absolute_estimate = (s - 1) * 12 + episode;
            queries.push(format!("{} {}", show, format_episode(absolute_estimate)));
        }
    }

    queries
}

/// Generate queries for show-only searches (no specific episode)
fn generate_show_queries(parsed: &ParsedQuery) -> Vec<String> {
    let show = &parsed.show_name;

    vec![
        show.clone(),
        format!("{} 1080p", show),
        format!("{} complete", show),
    ]
}

/// Convert number to ordinal suffix (2 -> "2nd Season")
fn ordinal(n: u32) -> String {
    let suffix = match n % 10 {
        1 if n % 100 != 11 => "st",
        2 if n % 100 != 12 => "nd",
        3 if n % 100 != 13 => "rd",
        _ => "th",
    };
    format!("{}{} Season", n, suffix)
}

// ============================================================================
// RESULT FILTERING (Post-search refinement)
// ============================================================================

/// Score a search result based on how well it matches the parsed query
/// Higher score = better match
pub fn score_result(result_title: &str, parsed: &ParsedQuery) -> i32 {
    let title_lower = result_title.to_lowercase();
    let show_lower = parsed.show_name.to_lowercase();
    let mut score = 0;

    // Check if show name is in title
    if title_lower.contains(&show_lower) {
        score += 100;
    } else {
        // Try matching individual words
        let show_words: Vec<&str> = show_lower.split_whitespace().collect();
        let matched_words = show_words
            .iter()
            .filter(|w| title_lower.contains(*w))
            .count();
        score += (matched_words * 20) as i32;
    }

    // Check episode number
    if let Some(ep) = parsed.episode {
        let ep_padded = format_episode(ep);
        let ep_patterns = [
            format!(" {} ", ep_padded),
            format!(" {}", ep_padded),
            format!("- {}", ep_padded),
            format!("-{}", ep_padded),
            format!("E{}", ep_padded),
            format!("e{}", ep_padded),
            format!(" {} ", ep),
            format!("E{} ", ep),
        ];

        if ep_patterns.iter().any(|p| title_lower.contains(p)) {
            score += 50;
        }
    }

    // Check season
    if let Some(s) = parsed.season {
        if s > 1 {
            let s_padded = format_season(s);
            let season_patterns = [
                format!("s{}", s_padded),
                format!("s{}", s),
                format!("season {}", s),
                format!("{}nd season", s),
                format!("{}rd season", s),
                format!("{}th season", s),
                format!("part {}", s),
            ];

            if season_patterns
                .iter()
                .any(|p| title_lower.contains(&p.to_lowercase()))
            {
                score += 30;
            }
        }
    }

    // Prefer 1080p
    if title_lower.contains("1080p") {
        score += 10;
    }

    // Prefer known good subgroups (examples)
    let good_subgroups = ["subsplease", "erai-raws", "judas", "horriblesubs"];
    if good_subgroups.iter().any(|g| title_lower.contains(g)) {
        score += 15;
    }

    // Penalize batch results when looking for specific episode
    if parsed.episode.is_some() && !parsed.is_batch_request {
        let batch_indicators = ["batch", "complete", "1-", "01-"];
        if batch_indicators.iter().any(|b| title_lower.contains(b)) {
            score -= 50;
        }
    }

    // Penalize very old/low quality
    if title_lower.contains("480p") || title_lower.contains("360p") {
        score -= 20;
    }

    score
}

/// Filter and sort search results based on relevance to the query
pub fn rank_results<T, F>(results: &mut [T], parsed: &ParsedQuery, get_title: F)
where
    F: Fn(&T) -> &str,
{
    results.sort_by(|a, b| {
        let score_a = score_result(get_title(a), parsed);
        let score_b = score_result(get_title(b), parsed);
        score_b.cmp(&score_a) // Descending order
    });
}

// ============================================================================
// HIGH-LEVEL API
// ============================================================================

/// Main entry point: convert user input to search queries
///
/// # Examples
/// ```
/// let result = smart_search("Frieren S01E09");
/// assert_eq!(result.primary, "Frieren 09");
///
/// let result = smart_search("One Piece S02");
/// assert!(result.parsed.is_batch_request);
/// ```
pub fn smart_search(input: &str) -> SearchQuery {
    build_search_query(input)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_s01_ep09() {
        let parsed = parse_query("Frieren S01 Ep09");
        assert_eq!(parsed.show_name, "Frieren");
        assert_eq!(parsed.season, Some(1));
        assert_eq!(parsed.episode, Some(9));
    }

    #[test]
    fn test_parse_s01e09() {
        let parsed = parse_query("Frieren S01E09");
        assert_eq!(parsed.show_name, "Frieren");
        assert_eq!(parsed.season, Some(1));
        assert_eq!(parsed.episode, Some(9));
        assert!(!parsed.is_batch_request);
    }

    #[test]
    fn test_parse_lowercase() {
        let parsed = parse_query("frieren s1e5");
        assert_eq!(parsed.show_name, "frieren");
        assert_eq!(parsed.season, Some(1));
        assert_eq!(parsed.episode, Some(5));
    }

    #[test]
    fn test_parse_with_spaces() {
        let parsed = parse_query("Sousou no Frieren S01 E09");
        assert_eq!(parsed.show_name, "Sousou no Frieren");
        assert_eq!(parsed.season, Some(1));
        assert_eq!(parsed.episode, Some(9));
    }

    #[test]
    fn test_parse_1x09_format() {
        let parsed = parse_query("Breaking Bad 2x05");
        assert_eq!(parsed.show_name, "Breaking Bad");
        assert_eq!(parsed.season, Some(2));
        assert_eq!(parsed.episode, Some(5));
    }

    #[test]
    fn test_parse_season_only() {
        let parsed = parse_query("One Piece S02");
        assert_eq!(parsed.show_name, "One Piece");
        assert_eq!(parsed.season, Some(2));
        assert_eq!(parsed.episode, None);
        assert!(parsed.is_batch_request);
    }

    #[test]
    fn test_parse_episode_only() {
        let parsed = parse_query("Frieren Episode 9");
        assert_eq!(parsed.show_name, "Frieren");
        assert_eq!(parsed.season, Some(1)); // Implied
        assert_eq!(parsed.episode, Some(9));
    }

    #[test]
    fn test_parse_batch_keyword() {
        let parsed = parse_query("Frieren batch");
        assert!(parsed.is_batch_request);
    }

    #[test]
    fn test_generate_s01_episode_queries() {
        let query = smart_search("Frieren S01E09");
        assert_eq!(query.primary, "Frieren 09");
        assert!(query.alternatives.contains(&"Frieren - 09".to_string()));
    }

    #[test]
    fn test_generate_s02_episode_queries() {
        let query = smart_search("Attack on Titan S02E05");
        // Should include season indicator for S2+
        assert!(query.primary.contains("S02") || query.primary.contains("Season 2"));
    }

    #[test]
    fn test_generate_batch_queries() {
        let query = smart_search("Frieren S01");
        assert!(query.parsed.is_batch_request);
        assert!(query.primary.contains("batch") || query.alternatives.iter().any(|q| q.contains("batch")));
    }

    #[test]
    fn test_score_exact_match() {
        let parsed = parse_query("Frieren S01E09");
        let score1 = score_result("[SubsPlease] Sousou no Frieren - 09 [1080p].mkv", &parsed);
        let score2 = score_result("[SubsPlease] Random Anime - 09 [1080p].mkv", &parsed);
        assert!(score1 > score2);
    }

    #[test]
    fn test_score_penalizes_batch_for_episode_search() {
        let parsed = parse_query("Frieren S01E09");
        let score_single = score_result("[SubsPlease] Frieren - 09 [1080p].mkv", &parsed);
        let score_batch = score_result("[SubsPlease] Frieren - Batch (01-12) [1080p].mkv", &parsed);
        assert!(score_single > score_batch);
    }

    #[test]
    fn test_ordinal() {
        assert_eq!(ordinal(1), "1st Season");
        assert_eq!(ordinal(2), "2nd Season");
        assert_eq!(ordinal(3), "3rd Season");
        assert_eq!(ordinal(4), "4th Season");
        assert_eq!(ordinal(11), "11th Season");
        assert_eq!(ordinal(12), "12th Season");
        assert_eq!(ordinal(13), "13th Season");
        assert_eq!(ordinal(21), "21st Season");
        assert_eq!(ordinal(22), "22nd Season");
    }

    #[test]
    fn test_format_episode() {
        assert_eq!(format_episode(5), "05");
        assert_eq!(format_episode(9), "09");
        assert_eq!(format_episode(10), "10");
        assert_eq!(format_episode(100), "100");
    }

    #[test]
    fn test_complex_show_names() {
        let parsed = parse_query("Kaguya-sama: Love is War S02E03");
        assert_eq!(parsed.show_name, "Kaguya-sama: Love is War");
        assert_eq!(parsed.season, Some(2));
        assert_eq!(parsed.episode, Some(3));
    }

    #[test]
    fn test_just_show_name() {
        let query = smart_search("Frieren");
        assert_eq!(query.primary, "Frieren");
        assert!(!query.parsed.is_batch_request);
    }
}
