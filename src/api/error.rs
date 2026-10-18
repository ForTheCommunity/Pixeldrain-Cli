use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("Network Connection Error : {0}")]
    Network(#[from] reqwest::Error),

    // Fix: Added fields to the error message display string
    #[error("API request failed with Status Code {status}: {message}")]
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