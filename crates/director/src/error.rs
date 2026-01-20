//! Error types for Director.

use thiserror::Error;

/// Error type for Director operations.
#[derive(Debug, Error)]
pub enum DirectorError {
    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Planning error.
    #[error("Planning error: {0}")]
    Planning(String),

    /// Execution error.
    #[error("Execution error: {0}")]
    Execution(String),

    /// Goal error.
    #[error("Goal error: {0}")]
    Goal(String),

    /// Issue tracking error.
    #[error("Issue tracking error: {0}")]
    IssueTracking(String),

    /// Plane.so API error.
    #[error("Plane.so API error: {0}")]
    PlaneApi(String),

    /// GitHub API error.
    #[error("GitHub API error: {0}")]
    GitHubApi(String),

    /// Human escalation required.
    #[error("Human decision required: {0}")]
    HumanRequired(String),

    /// LLM SDK error.
    #[error("LLM error: {0}")]
    Llm(#[from] llm_code_sdk::Error),

    /// Mountain error.
    #[error("Mountain error: {0}")]
    Mountain(#[from] mountain::MountainError),

    /// Zulip error.
    #[error("Zulip error: {0}")]
    Zulip(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Generic error.
    #[error("{0}")]
    Other(String),
}

/// Result type for Director operations.
pub type DirectorResult<T> = std::result::Result<T, DirectorError>;
