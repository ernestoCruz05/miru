use scraper::{Html, Selector};
use tracing::debug;

use crate::error::{Error, Result};

const NYAA_BASE_URL: &str = "https://nyaa.si";

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

pub struct NyaaClient {
    client: reqwest::Client,
    pub category: NyaaCategory,
    pub filter: NyaaFilter,
}

impl NyaaClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("miru/0.1")
                .build()
                .expect("Failed to create HTTP client"),
            category: NyaaCategory::AnimeEnglish, // Default to English subs
            filter: NyaaFilter::NoFilter,
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

    /// Search nyaa.si for torrents matching the query
    pub async fn search(&self, query: &str) -> Result<Vec<NyaaResult>> {
        self.search_with_options(query, self.category, self.filter).await
    }

    /// Search nyaa.si with specific category and filter options
    pub async fn search_with_options(&self, query: &str, category: NyaaCategory, filter: NyaaFilter) -> Result<Vec<NyaaResult>> {
        let encoded_query = urlencoding::encode(query);
        let url = format!(
            "{}/?f={}&c={}&q={}",
            NYAA_BASE_URL,
            filter.as_query_param(),
            category.as_query_param(),
            encoded_query
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

    /// Parse the HTML search results page
    fn parse_results(&self, html: &str) -> Result<Vec<NyaaResult>> {
        let document = Html::parse_document(html);

        // Selectors for nyaa.si table structure
        let row_selector =
            Selector::parse("table.torrent-list tbody tr").expect("Invalid row selector");
        let cell_selector = Selector::parse("td").expect("Invalid cell selector");
        let link_selector = Selector::parse("a").expect("Invalid link selector");

        let mut results = Vec::new();

        for row in document.select(&row_selector) {
            let cells: Vec<_> = row.select(&cell_selector).collect();

            // Nyaa table structure:
            // 0: Category, 1: Name (with links), 2: Links (torrent/magnet),
            // 3: Size, 4: Date, 5: Seeders, 6: Leechers, 7: Downloads
            if cells.len() < 8 {
                continue;
            }

            // Extract category from first cell
            let category = cells[0]
                .select(&link_selector)
                .next()
                .map(|a| a.attr("title").unwrap_or("Unknown").to_string())
                .unwrap_or_else(|| "Unknown".to_string());

            // Extract title - second link in the name cell (first is category icon)
            let name_cell = &cells[1];
            let title = name_cell
                .select(&link_selector)
                .filter(|a| a.attr("href").is_some_and(|h| h.starts_with("/view/")))
                .next()
                .map(|a| a.text().collect::<String>().trim().to_string())
                .unwrap_or_else(|| "Unknown".to_string());

            // Extract torrent and magnet links from the links cell
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

            // Extract other fields
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
