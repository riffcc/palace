//! Error types for palace-gba.

use thiserror::Error;

/// GBA emulation error.
#[derive(Debug, Error)]
pub enum GbaError {
    #[error("Failed to load ROM: {0}")]
    RomLoad(String),

    #[error("Failed to load BIOS: {0}")]
    BiosLoad(String),

    #[error("Failed to build cartridge: {0}")]
    CartridgeBuild(String),

    #[error("Save state error: {0}")]
    SaveState(String),

    #[error("Audio error: {0}")]
    Audio(String),

    #[error("{0}")]
    Other(String),
}

/// Result type for GBA operations.
pub type GbaResult<T> = Result<T, GbaError>;
