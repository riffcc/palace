//! Palace GBA - Game Boy Advance emulation for Palace.
//!
//! Provides a high-level wrapper around rustboyadvance-ng with:
//! - Real-time audio output via tinyaudio
//! - Screen capture for LLM vision
//! - Save state support for Mountain time-delay synchronization
//! - Input handling

mod emulator;
mod audio;
mod error;

pub use emulator::{GbaEmulator, GbaButton, GbaConfig};
pub use audio::AudioPlayer;
pub use error::{GbaError, GbaResult};

/// GBA screen dimensions.
pub const GBA_WIDTH: usize = 240;
pub const GBA_HEIGHT: usize = 160;
