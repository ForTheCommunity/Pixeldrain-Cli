use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("Network Connection Error : {0}")]
    Network(#[from] reqwest::Error),

    #[error("API request failed with Status Code : ")]
    HttpStatusError {
        status: reqwest::StatusCode,
        message: String,
    },

    #[error("Failed to parse response : {0}")]
    Deserialization(#[from] serde_json::Error),

    #[error("Authentication Error : {0}")]
    Unauthorized(String),
}

pub type ApiResult<T> = Result<T, ApiError>;
