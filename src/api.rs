use crate::config::ApiConfig;

use reqwest::{RequestBuilder, StatusCode};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::fmt;
use std::time::Duration;
use uuid::Uuid;

pub struct Api {
    client: reqwest::Client,
    url: String,
    token: Option<String>,
}

#[derive(Debug)]
pub enum ApiError {
    Unauthorized,
    Unavailable(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ApiError::Unauthorized => f.write_str("unauthorized"),
            ApiError::Unavailable(reason) => write!(f, "unavailable: {}", reason),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Account {
    pub user_id: String,
    pub nickname: String,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub minecraft_uuid: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct LinkedAccount {
    pub user_id: String,
    pub nickname: Option<String>,
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoleDefinition {
    pub id: String,
    pub display_name: String,
    pub is_staff: bool,
}

impl Api {
    pub fn new(config: &ApiConfig) -> Api {
        Api {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("could not build http client"),
            url: config.url.trim_end_matches('/').to_owned(),
            token: config.token.clone(),
        }
    }

    pub async fn account(&self, access_token: &str) -> Result<Account, ApiError> {
        let request = self
            .client
            .get(format!("{}/api/v3/oauth/user", self.url))
            .bearer_auth(access_token);
        fetch(request).await?.ok_or(ApiError::Unauthorized)
    }

    /// The account whose main Minecraft account is `uuid`.
    pub async fn linked_account(&self, uuid: Uuid) -> Result<Option<LinkedAccount>, ApiError> {
        let Some(token) = &self.token else { return Ok(None) };
        let request = self
            .client
            .get(format!("{}/api/v2/user/by-minecraft/{}", self.url, uuid))
            .bearer_auth(token);
        fetch(request).await
    }

    pub async fn roles(&self) -> Result<Vec<RoleDefinition>, ApiError> {
        let Some(token) = &self.token else { return Ok(Vec::new()) };
        let request = self.client.get(format!("{}/api/v2/user/roles", self.url)).bearer_auth(token);
        Ok(fetch(request).await?.unwrap_or_default())
    }
}

/// `None` for 404.
async fn fetch<T: DeserializeOwned>(request: RequestBuilder) -> Result<Option<T>, ApiError> {
    let response = request
        .send()
        .await
        .map_err(|err| ApiError::Unavailable(err.to_string()))?;
    match response.status() {
        StatusCode::NOT_FOUND => Ok(None),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(ApiError::Unauthorized),
        status if status.is_success() => response
            .json()
            .await
            .map(Some)
            .map_err(|err| ApiError::Unavailable(err.to_string())),
        status => Err(ApiError::Unavailable(status.to_string())),
    }
}
