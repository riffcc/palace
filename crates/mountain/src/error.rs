//! Error types for Mountain.

use thiserror::Error;

/// Error type for Mountain operations.
#[derive(Debug, Error)]
pub enum MountainError {
    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Model inference error.
    #[error("Model inference failed for '{model}': {error}")]
    Inference { model: String, error: String },

    /// Cascade timeout.
    #[error("Cascade timed out after {0}ms")]
    CascadeTimeout(u64),

    /// State capture error.
    #[error("Failed to capture state: {0}")]
    StateCapture(String),

    /// State save/load error.
    #[error("State error: {0}")]
    State(String),

    /// Process control error.
    #[error("Process control error: {0}")]
    ProcessControl(String),

    /// Decision merge conflict.
    #[error("Decision merge conflict between models: {0}")]
    MergeConflict(String),

    /// Delay buffer underrun.
    #[error("Delay buffer underrun - execution caught up to realtime")]
    BufferUnderrun,

    /// WASM isolation error.
    #[error("WASM isolation error: {0}")]
    WasmIsolation(String),

    /// LLM SDK error.
    #[error("LLM SDK error: {0}")]
    LlmSdk(#[from] llm_code_sdk::Error),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Generic error.
    #[error("{0}")]
    Other(String),
}

/// Result type for Mountain operations.
pub type MountainResult<T> = std::result::Result<T, MountainError>;
