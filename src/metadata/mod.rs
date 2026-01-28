use serde::{Deserialize, Serialize};
use crate::error::Result;

pub mod mal;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnimeMetadata {
    pub id: u64,
    pub title: String,
    pub cover_url: Option<String>,
    pub synopsis: Option<String>,
    pub score: Option<f64>,
    pub status: String, // e.g., "Currently Airing", "Finished Airing"
    pub episodes: Option<u32>,
    pub genres: Vec<String>,
}

#[async_trait::async_trait]
pub trait MetadataProvider {
    async fn search(&self, query: &str) -> Result<Vec<AnimeMetadata>>;
    async fn get_details(&self, id: u64) -> Result<AnimeMetadata>;
}
