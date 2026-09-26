use std::sync::Arc;

use reqwest::{Client, Method, RequestBuilder};
use serde::de::DeserializeOwned;

use crate::api::error::{ApiError, ApiResult};

#[derive(Clone)]
pub struct ApiClient {
    http_client: Client,
    base_url: String,
    pub api_key: Option<Arc<String>>,
}

impl Default for ApiClient {
    fn default() -> Self {
        ApiClient {
            http_client: reqwest::Client::new(),
            base_url: "https://pixeldrain.com".into(),
            api_key: None,
        }
    }
}

impl ApiClient {
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(Arc::new(api_key.into()));
        self
    }

    pub fn request(&self, method: Method, endpoint: &str) -> RequestBuilder {
        let url = format!("{}{}", self.base_url, endpoint);
        let mut builder = self.http_client.request(method, &url);

        if let Some(ref key) = self.api_key {
            builder = builder.basic_auth("", Some(key.as_str()));
        }
        builder
    }

    pub async fn send<T: DeserializeOwned>(&self, builder: RequestBuilder) -> ApiResult<T> {
        let response = builder.send().await?;
        let status = response.status();

        if !status.is_success() {
            let message = response.text().await.unwrap_or_default();
            if status == reqwest::StatusCode::UNAUTHORIZED {
                return Err(ApiError::Unauthorized(message));
            }
            return Err(ApiError::HttpStatusError { status, message });
        }
        let data = response.json::<T>().await?;
        Ok(data)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}
