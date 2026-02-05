use crate::error::{Error, Result};
use crate::metadata::{AnimeMetadata, MetadataProvider};
use reqwest::{Client, header};
use serde::Deserialize;

const MAL_API_BASE: &str = "https://api.myanimelist.net/v2";

pub struct MalClient {
    client: Client,
    client_id: String,
}

impl MalClient {
    pub fn new(client_id: String) -> Self {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            "X-MAL-CLIENT-ID",
            header::HeaderValue::from_str(&client_id).unwrap(),
        );

        // If we have an access token for user context, we add it too (later for sync)

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .unwrap_or_default();

        Self { client, client_id }
    }
}

#[derive(Deserialize)]
struct MalNode {
    node: MalAnimeData,
}

#[derive(Deserialize)]
struct MalAnimeData {
    id: u64,
    title: String,
    main_picture: Option<MalPicture>,
    synopsis: Option<String>,
    mean: Option<f64>,
    status: Option<String>,
    num_episodes: Option<u32>,
    genres: Option<Vec<MalGenre>>,
}

#[derive(Deserialize)]
struct MalPicture {
    #[serde(default)]
    medium: Option<String>,
    #[serde(default)]
    large: Option<String>,
}

#[derive(Deserialize)]
struct MalGenre {
    name: String,
}

#[derive(Deserialize)]
struct MalSearchResponse {
    data: Vec<MalNode>,
}

#[async_trait::async_trait]
impl MetadataProvider for MalClient {
    async fn search(&self, query: &str) -> Result<Vec<AnimeMetadata>> {
        let url = format!("{}/anime", MAL_API_BASE);

        let response = self
            .client
            .get(&url)
            .query(&[
                ("q", query),
                ("limit", "5"),
                (
                    "fields",
                    "start_date,end_date,mean,status,num_episodes,synopsis,main_picture,genres",
                ),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(Error::Metadata(format!(
                "MAL API Error: {}",
                response.status()
            )));
        }

        let resp_json: MalSearchResponse = response.json().await?;

        let results = resp_json
            .data
            .into_iter()
            .map(|node| {
                let a = node.node;
                AnimeMetadata {
                    id: a.id,
                    title: a.title,
                    cover_url: a.main_picture.and_then(|p| p.large.or(p.medium)),
                    synopsis: a.synopsis,
                    score: a.mean,
                    status: a.status.unwrap_or_else(|| "Unknown".to_string()),
                    episodes: a.num_episodes,
                    genres: a
                        .genres
                        .map(|g| g.into_iter().map(|ge| ge.name).collect())
                        .unwrap_or_default(),
                }
            })
            .collect();

        Ok(results)
    }

    async fn get_details(&self, id: u64) -> Result<AnimeMetadata> {
        let url = format!("{}/anime/{}", MAL_API_BASE, id);
        let response = self
            .client
            .get(&url)
            .query(&[(
                "fields",
                "start_date,end_date,mean,status,num_episodes,synopsis,main_picture,genres",
            )])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(Error::Metadata(format!(
                "MAL API Error: {}",
                response.status()
            )));
        }

        let a: MalAnimeData = response.json().await?;
        Ok(AnimeMetadata {
            id: a.id,
            title: a.title,
            cover_url: a.main_picture.and_then(|p| p.large.or(p.medium)),
            synopsis: a.synopsis,
            score: a.mean,
            status: a.status.unwrap_or_else(|| "Unknown".to_string()),
            episodes: a.num_episodes,
            genres: a
                .genres
                .map(|g| g.into_iter().map(|ge| ge.name).collect())
                .unwrap_or_default(),
        })
    }
}
