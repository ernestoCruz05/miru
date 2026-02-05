use regex::Regex;
use scraper::{Html, Selector};
use std::sync::OnceLock;
use tracing::debug;

use crate::error::{Error, Result};

mod smart_search;
pub use smart_search::{rank_results, smart_search};

const NYAA_BASE_URL: &str = "https://nyaa.si";

static BATCH_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();

fn get_batch_patterns() -> &'static Vec<Regex> {
    BATCH_PATTERNS.get_or_init(|| {
        vec![
            Regex::new(r"(?i)\[batch\]").unwrap(),
            Regex::new(r"(?i)\bcomplete\b").unwrap(),
            Regex::new(r"(?i)\bseason\s+\d+\b").unwrap(),
            Regex::new(r"\bS\d{2}\b").unwrap(),
            Regex::new(r"(?i)\d+-\d+\s*(?:END|FINAL)").unwrap(),
        ]
    })
}

fn parse_size_mb(size_str: &str) -> f64 {
    let parts: Vec<&str> = size_str.split_whitespace().collect();
    if parts.len() != 2 {
        return 0.0;
    }

    let value: f64 = parts[0].parse().unwrap_or(0.0);
    match parts[1].to_uppercase().as_str() {
        "KIB" => value / 1024.0,
        "MIB" => value,
        "GIB" => value * 1024.0,
        "TIB" => value * 1024.0 * 1024.0,
        _ => 0.0,
    }
}

#[derive(Debug, Clone)]
pub struct NyaaResult {
    pub title: String,
    pub category: String,
    pub size: String,
    pub seeders: u32,
    pub leechers: u32,
    pub downloads: u32,
    pub torrent_url: String,
    pub magnet_link: String,
    pub date: String,
    pub is_trusted: bool,
    pub is_batch: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NyaaCategory {
    AllAnime,
    AnimeEnglish,
    AnimeRaw,
    AnimeNonEnglish,
}

impl NyaaCategory {
    fn as_query_param(&self) -> &'static str {
        match self {
            NyaaCategory::AllAnime => "1_0",
            NyaaCategory::AnimeEnglish => "1_2",
            NyaaCategory::AnimeRaw => "1_4",
            NyaaCategory::AnimeNonEnglish => "1_3",
        }
    }

    pub fn as_display(&self) -> &'static str {
        match self {
            NyaaCategory::AllAnime => "All Anime",
            NyaaCategory::AnimeEnglish => "English-translated",
            NyaaCategory::AnimeRaw => "Raw",
            NyaaCategory::AnimeNonEnglish => "Non-English",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            NyaaCategory::AllAnime => NyaaCategory::AnimeEnglish,
            NyaaCategory::AnimeEnglish => NyaaCategory::AnimeRaw,
            NyaaCategory::AnimeRaw => NyaaCategory::AnimeNonEnglish,
            NyaaCategory::AnimeNonEnglish => NyaaCategory::AllAnime,
        }
    }
}

impl Default for NyaaCategory {
    fn default() -> Self {
        NyaaCategory::AllAnime
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NyaaFilter {
    NoFilter,
    TrustedOnly,
    NoRemakes,
}

impl NyaaFilter {
    fn as_query_param(&self) -> &'static str {
        match self {
            NyaaFilter::NoFilter => "0",
            NyaaFilter::TrustedOnly => "2",
            NyaaFilter::NoRemakes => "1",
        }
    }

    pub fn as_display(&self) -> &'static str {
        match self {
            NyaaFilter::NoFilter => "No Filter",
            NyaaFilter::TrustedOnly => "Trusted Only",
            NyaaFilter::NoRemakes => "No Remakes",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            NyaaFilter::NoFilter => NyaaFilter::TrustedOnly,
            NyaaFilter::TrustedOnly => NyaaFilter::NoRemakes,
            NyaaFilter::NoRemakes => NyaaFilter::NoFilter,
        }
    }
}

impl Default for NyaaFilter {
    fn default() -> Self {
        NyaaFilter::NoFilter
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NyaaSort {
    Date,
    Seeders,
    Size,
    Downloads,
}

impl NyaaSort {
    fn as_query_param(&self) -> &'static str {
        match self {
            NyaaSort::Date => "id",
            NyaaSort::Seeders => "seeders",
            NyaaSort::Size => "size",
            NyaaSort::Downloads => "downloads",
        }
    }

    pub fn as_display(&self) -> &'static str {
        match self {
            NyaaSort::Date => "Date",
            NyaaSort::Seeders => "Seeders",
            NyaaSort::Size => "Size",
            NyaaSort::Downloads => "Downloads",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            NyaaSort::Date => NyaaSort::Seeders,
            NyaaSort::Seeders => NyaaSort::Size,
            NyaaSort::Size => NyaaSort::Downloads,
            NyaaSort::Downloads => NyaaSort::Date,
        }
    }
}

impl Default for NyaaSort {
    fn default() -> Self {
        NyaaSort::Seeders
    }
}

pub struct NyaaClient {
    client: reqwest::Client,
    pub category: NyaaCategory,
    pub filter: NyaaFilter,
    pub sort: NyaaSort,
}

impl NyaaClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("miru/0.1")
                .build()
                .expect("Failed to create HTTP client"),
            category: NyaaCategory::AnimeEnglish,
            filter: NyaaFilter::NoFilter,
            sort: NyaaSort::default(),
        }
    }

    pub fn set_category(&mut self, category: NyaaCategory) {
        self.category = category;
    }

    pub fn set_filter(&mut self, filter: NyaaFilter) {
        self.filter = filter;
    }

    pub fn with_category(mut self, category: NyaaCategory) -> Self {
        self.category = category;
        self
    }

    pub fn with_filter(mut self, filter: NyaaFilter) -> Self {
        self.filter = filter;
        self
    }

    pub fn with_sort(mut self, sort: NyaaSort) -> Self {
        self.sort = sort;
        self
    }

    pub async fn search(
        &self,
        query: &str,
        category: NyaaCategory,
        filter: NyaaFilter,
        sort: NyaaSort,
    ) -> Result<Vec<NyaaResult>> {
        let search_query = smart_search(query);
        let mut all_results = Vec::new();
        let mut seen_magnets = std::collections::HashSet::new();

        let queries =
            std::iter::once(&search_query.primary).chain(search_query.alternatives.iter());

        for query_str in queries {
            debug!(query = %query_str, "Trying search query");

            match self
                .search_with_options(query_str, category, filter, sort)
                .await
            {
                Ok(results) => {
                    let mut count = 0;
                    for result in results {
                        if seen_magnets.insert(result.magnet_link.clone()) {
                            all_results.push(result);
                            count += 1;
                        }
                    }

                    if (count > 0 && all_results.len() >= 15) || all_results.len() >= 30 {
                        break;
                    }
                }
                Err(e) => {
                    debug!(error = %e, query = %query_str, "Search query failed");
                    continue;
                }
            }
        }

        rank_results(&mut all_results, &search_query.parsed, |r| &r.title);

        Ok(all_results)
    }

    pub async fn search_with_options(
        &self,
        query: &str,
        category: NyaaCategory,
        filter: NyaaFilter,
        sort: NyaaSort,
    ) -> Result<Vec<NyaaResult>> {
        let encoded_query = urlencoding::encode(query);
        let url = format!(
            "{}/?f={}&c={}&q={}&s={}&o=desc",
            NYAA_BASE_URL,
            filter.as_query_param(),
            category.as_query_param(),
            encoded_query,
            sort.as_query_param()
        );

        debug!(url = %url, "Searching nyaa.si");

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(Error::NyaaSearch(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let html = response.text().await?;
        self.parse_results(&html)
    }

    fn parse_results(&self, html: &str) -> Result<Vec<NyaaResult>> {
        let document = Html::parse_document(html);

        let row_selector =
            Selector::parse("table.torrent-list tbody tr").expect("Invalid row selector");
        let cell_selector = Selector::parse("td").expect("Invalid cell selector");
        let link_selector = Selector::parse("a").expect("Invalid link selector");

        let mut results = Vec::new();

        for row in document.select(&row_selector) {
            let row_class = row.value().attr("class").unwrap_or("default");
            let is_trusted = row_class.contains("success");

            let cells: Vec<_> = row.select(&cell_selector).collect();

            if cells.len() < 8 {
                continue;
            }

            let category = cells[0]
                .select(&link_selector)
                .next()
                .map(|a| a.attr("title").unwrap_or("Unknown").to_string())
                .unwrap_or_else(|| "Unknown".to_string());

            let name_cell = &cells[1];
            let title = name_cell
                .select(&link_selector)
                .filter(|a| {
                    a.attr("href").is_some_and(|h| {
                        h.starts_with("/view/") && !h.contains('?') && !h.contains('#')
                    })
                })
                .next()
                .map(|a| a.text().collect::<String>().trim().to_string())
                .unwrap_or_else(|| "Unknown".to_string());

            let links_cell = &cells[2];
            let mut torrent_url = String::new();
            let mut magnet_link = String::new();

            for link in links_cell.select(&link_selector) {
                if let Some(href) = link.attr("href") {
                    if href.ends_with(".torrent") {
                        torrent_url = format!("{}{}", NYAA_BASE_URL, href);
                    } else if href.starts_with("magnet:") {
                        magnet_link = href.to_string();
                    }
                }
            }

            let size = cells[3].text().collect::<String>().trim().to_string();
            let date = cells[4].text().collect::<String>().trim().to_string();
            let seeders = cells[5]
                .text()
                .collect::<String>()
                .trim()
                .parse()
                .unwrap_or(0);
            let leechers = cells[6]
                .text()
                .collect::<String>()
                .trim()
                .parse()
                .unwrap_or(0);
            let downloads = cells[7]
                .text()
                .collect::<String>()
                .trim()
                .parse()
                .unwrap_or(0);

            let is_batch = get_batch_patterns().iter().any(|re| re.is_match(&title))
                || parse_size_mb(&size) > 5120.0;

            results.push(NyaaResult {
                title,
                category,
                size,
                seeders,
                leechers,
                downloads,
                torrent_url,
                magnet_link,
                date,
                is_trusted,
                is_batch,
            });
        }

        debug!(count = results.len(), "Parsed nyaa search results");
        Ok(results)
    }
}

impl Default for NyaaClient {
    fn default() -> Self {
        Self::new()
    }
}
