use crate::error::{Error, Result};
use crate::metadata::{AnimeMetadata, MetadataProvider};
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};

const MAL_API_BASE: &str = "https://api.myanimelist.net/v2";
const MAL_OAUTH_BASE: &str = "https://myanimelist.net/v1/oauth2";

pub struct MalClient {
    client: Client,
    client_id: String,
    access_token: Option<String>,
}

impl MalClient {
    pub fn new(client_id: String) -> Self {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            "X-MAL-CLIENT-ID",
            header::HeaderValue::from_str(&client_id).unwrap(),
        );

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .unwrap_or_default();

        Self {
            client,
            client_id,
            access_token: None,
        }
    }

    pub fn with_access_token(mut self, token: String) -> Self {
        self.access_token = Some(token);
        self
    }

    pub fn set_access_token(&mut self, token: String) {
        self.access_token = Some(token);
    }

    pub fn generate_pkce_pair() -> (String, String) {
        use rand::Rng;
        let code_verifier: String = rand::rng()
            .sample_iter(&rand::distr::Alphanumeric)
            .take(128)
            .map(char::from)
            .collect();

        // MAL uses plain PKCE where code_challenge = code_verifier
        let code_challenge = code_verifier.clone();

        (code_verifier, code_challenge)
    }

    pub fn build_auth_url(&self, code_challenge: &str) -> String {
        format!(
            "{}/authorize?response_type=code&client_id={}&code_challenge={}",
            MAL_OAUTH_BASE, self.client_id, code_challenge
        )
    }

    pub async fn exchange_code(&self, code: &str, code_verifier: &str) -> Result<TokenResponse> {
        let response = self
            .client
            .post(&format!("{}/token", MAL_OAUTH_BASE))
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("grant_type", "authorization_code"),
                ("code", code),
                ("code_verifier", code_verifier),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::Metadata(format!(
                "Token exchange failed: {} - {}",
                status, body
            )));
        }

        let token: TokenResponse = response.json().await?;
        Ok(token)
    }

    pub async fn refresh_access_token(&self, refresh_token: &str) -> Result<TokenResponse> {
        let response = self
            .client
            .post(&format!("{}/token", MAL_OAUTH_BASE))
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::Metadata(format!(
                "Token refresh failed: {} - {}",
                status, body
            )));
        }

        let token: TokenResponse = response.json().await?;
        Ok(token)
    }

    pub async fn get_user_animelist(&self, status: &str) -> Result<Vec<UserAnimeEntry>> {
        let access_token = self
            .access_token
            .as_ref()
            .ok_or_else(|| Error::Metadata("No access token set".to_string()))?;

        let url = format!("{}/users/@me/animelist", MAL_API_BASE);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .query(&[
                ("status", status),
                ("limit", "100"),
                ("fields", "list_status{num_episodes_watched},num_episodes"),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status_code = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::Metadata(format!(
                "Animelist fetch failed: {} - {}",
                status_code, body
            )));
        }

        let resp: UserAnimeListResponse = response.json().await?;
        Ok(resp
            .data
            .into_iter()
            .map(|node| UserAnimeEntry {
                mal_id: node.node.id,
                title: node.node.title,
                num_episodes: node.node.num_episodes,
                num_watched: node
                    .list_status
                    .map(|s| s.num_episodes_watched)
                    .unwrap_or(0),
            })
            .collect())
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub token_type: String,
}

#[derive(Debug)]
pub struct UserAnimeEntry {
    pub mal_id: u64,
    pub title: String,
    pub num_episodes: Option<u32>,
    pub num_watched: u32,
}

#[derive(Deserialize)]
struct UserAnimeListResponse {
    data: Vec<UserAnimeNode>,
}

#[derive(Deserialize)]
struct UserAnimeNode {
    node: UserAnimeData,
    list_status: Option<ListStatus>,
}

#[derive(Deserialize)]
struct UserAnimeData {
    id: u64,
    title: String,
    num_episodes: Option<u32>,
}

#[derive(Deserialize)]
struct ListStatus {
    num_episodes_watched: u32,
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
