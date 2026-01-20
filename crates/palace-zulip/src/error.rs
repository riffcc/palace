//! Error types for palace-zulip.

use thiserror::Error;

/// Error type for Zulip operations.
#[derive(Debug, Error)]
pub enum ZulipError {
    /// Connection error.
    #[error("Connection error: {0}")]
    Connection(String),

    /// Authentication error.
    #[error("Authentication error: {0}")]
    Auth(String),

    /// API error.
    #[error("Zulip API error: {0}")]
    Api(String),

    /// Stream not found.
    #[error("Stream not found: {0}")]
    StreamNotFound(String),

    /// Message send failed.
    #[error("Failed to send message: {0}")]
    SendFailed(String),

    /// No project set.
    #[error("No project set for this agent")]
    NoProject,

    /// HTTP error.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// URL error.
    #[error("URL error: {0}")]
    Url(#[from] url::ParseError),

    /// Generic error.
    #[error("{0}")]
    Other(String),
}

/// Result type for Zulip operations.
pub type ZulipResult<T> = std::result::Result<T, ZulipError>;
