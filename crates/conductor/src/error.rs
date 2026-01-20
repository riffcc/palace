//! Error types for Conductor.

use thiserror::Error;

/// Error type for Conductor operations.
#[derive(Debug, Error)]
pub enum ConductorError {
    /// Gamepad error.
    #[error("Gamepad error: {0}")]
    Gamepad(String),

    /// Interview error.
    #[error("Interview error: {0}")]
    Interview(String),

    /// Question generation error.
    #[error("Question generation error: {0}")]
    QuestionGeneration(String),

    /// No gamepad connected.
    #[error("No gamepad connected")]
    NoGamepad,

    /// Invalid answer.
    #[error("Invalid answer: {0}")]
    InvalidAnswer(String),

    /// LLM error.
    #[error("LLM error: {0}")]
    Llm(#[from] llm_code_sdk::Error),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Generic error.
    #[error("{0}")]
    Other(String),
}

/// Result type for Conductor operations.
pub type ConductorResult<T> = std::result::Result<T, ConductorError>;
