//! Error types for Palace CI.

use thiserror::Error;

/// Error type for CI operations.
#[derive(Debug, Error)]
pub enum CIError {
    /// Dagger connection failed.
    #[error("Failed to connect to Dagger: {0}")]
    DaggerConnection(String),

    /// Build step failed.
    #[error("Build failed: {0}")]
    BuildFailed(String),

    /// Lint step failed.
    #[error("Lint failed with {0} warnings/errors")]
    LintFailed(usize),

    /// Test step failed.
    #[error("Tests failed: {0} failures")]
    TestsFailed(usize),

    /// Binary execution failed.
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    /// Scenario failed.
    #[error("Scenario '{0}' failed: {1}")]
    ScenarioFailed(String, String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Generic error wrapper.
    #[error("{0}")]
    Other(String),
}

/// Result type for CI operations.
pub type CIResult<T> = std::result::Result<T, CIError>;

impl From<eyre::Report> for CIError {
    fn from(e: eyre::Report) -> Self {
        CIError::Other(e.to_string())
    }
}
