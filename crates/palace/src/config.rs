//! Configuration for Palace.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Palace configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PalaceConfig {
    /// LM Studio endpoint.
    pub lm_studio_url: String,

    /// Window configuration.
    pub window: WindowConfig,

    /// Gamepad configuration.
    pub gamepad: GamepadConfig,

    /// Inference configuration.
    pub inference: InferenceConfig,
}

impl Default for PalaceConfig {
    fn default() -> Self {
        Self {
            lm_studio_url: "http://localhost:1234/v1".into(),
            window: WindowConfig::default(),
            gamepad: GamepadConfig::default(),
            inference: InferenceConfig::default(),
        }
    }
}

/// Window configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    /// Window width.
    pub width: u32,
    /// Window height.
    pub height: u32,
    /// Split ratio (left side).
    pub split_ratio: f32,
    /// VSync enabled.
    pub vsync: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            split_ratio: 0.6,
            vsync: true,
        }
    }
}

/// Gamepad configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamepadConfig {
    /// Deadzone for analog sticks.
    pub stick_deadzone: f32,
    /// Whether to enable haptic feedback.
    pub haptics_enabled: bool,
    /// PS button double-press timeout (ms).
    pub ps_double_press_ms: u64,
}

impl Default for GamepadConfig {
    fn default() -> Self {
        Self {
            stick_deadzone: 0.15,
            haptics_enabled: true,
            ps_double_press_ms: 300,
        }
    }
}

/// Inference configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceConfig {
    /// Local model for fast decisions.
    pub local_model: String,
    /// Cloud model for complex decisions.
    pub cloud_model: Option<String>,
    /// Maximum tokens for responses.
    pub max_tokens: u32,
    /// Temperature for sampling.
    pub temperature: f32,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            local_model: "local-model".into(),
            cloud_model: None,
            max_tokens: 1024,
            temperature: 0.7,
        }
    }
}
