//! GBA emulator wrapper implementing Mountain's Controllable trait.

use anyhow::Result;
use mountain::{
    Controllable, ControlDecision, MountainError, MountainResult, ProgramState, StateSnapshot,
};
use palace_gba::{GbaButton, GbaEmulator};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Wrapper around GbaEmulator that implements Controllable.
pub struct GbaControllable {
    emulator: Arc<Mutex<GbaEmulator>>,
    frame_count: u64,
    last_screen_hash: u64,
}

impl GbaControllable {
    /// Create a new GBA controllable wrapper.
    pub fn new(emulator: Arc<Mutex<GbaEmulator>>) -> Self {
        Self {
            emulator,
            frame_count: 0,
            last_screen_hash: 0,
        }
    }

    /// Hash screen pixels for change detection.
    fn hash_screen(pixels: &[u32]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        // Sample every 100th pixel for speed
        for (i, pixel) in pixels.iter().enumerate() {
            if i % 100 == 0 {
                pixel.hash(&mut hasher);
            }
        }
        hasher.finish()
    }

    /// Convert button name to GbaButton.
    fn parse_button(name: &str) -> Option<GbaButton> {
        match name.to_lowercase().as_str() {
            "a" => Some(GbaButton::A),
            "b" => Some(GbaButton::B),
            "start" => Some(GbaButton::Start),
            "select" => Some(GbaButton::Select),
            "up" => Some(GbaButton::Up),
            "down" => Some(GbaButton::Down),
            "left" => Some(GbaButton::Left),
            "right" => Some(GbaButton::Right),
            "l" | "l1" => Some(GbaButton::L),
            "r" | "r1" => Some(GbaButton::R),
            _ => None,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Controllable for GbaControllable {
    async fn capture_state(&self) -> MountainResult<StateSnapshot> {
        let emu = self.emulator.lock().await;
        let screen = emu.screen_rgba();
        let screen_hash = Self::hash_screen(screen);

        // Create state snapshot with screen data
        let mut snapshot = StateSnapshot::new()
            .with_variable("frame", serde_json::json!(self.frame_count))
            .with_variable("screen_hash", serde_json::json!(screen_hash))
            .with_variable("screen_changed", serde_json::json!(screen_hash != self.last_screen_hash));

        // Encode screen as base64 for the LLM (downsampled for efficiency)
        // We'll send a 60x40 thumbnail
        let mut thumbnail = Vec::with_capacity(60 * 40 * 3);
        for y in (0..160).step_by(4) {
            for x in (0..240).step_by(4) {
                let idx = y * 240 + x;
                let pixel = screen[idx];
                thumbnail.push(((pixel >> 16) & 0xFF) as u8); // R
                thumbnail.push(((pixel >> 8) & 0xFF) as u8);  // G
                thumbnail.push((pixel & 0xFF) as u8);         // B
            }
        }

        // Base64 encode the thumbnail
        use base64::Engine;
        let thumbnail_b64 = base64::engine::general_purpose::STANDARD.encode(&thumbnail);
        snapshot = snapshot.with_variable("screen_thumbnail_b64", serde_json::json!(thumbnail_b64));
        snapshot = snapshot.with_variable("screen_width", serde_json::json!(60));
        snapshot = snapshot.with_variable("screen_height", serde_json::json!(40));

        Ok(snapshot)
    }

    async fn apply_decision(&mut self, decision: &ControlDecision) -> MountainResult<()> {
        let mut emu = self.emulator.lock().await;

        // Get the button from params if present
        let button_name = decision.params
            .get("button")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let button = Self::parse_button(button_name);

        // Apply the action
        match decision.action.as_str() {
            "press" => {
                if let Some(btn) = button {
                    emu.press_button(btn);
                    tracing::debug!("Pressing button: {:?}", btn);
                }
            }
            "release" => {
                if let Some(btn) = button {
                    emu.release_button(btn);
                    tracing::debug!("Releasing button: {:?}", btn);
                }
            }
            "tap" => {
                // Press and release in sequence (will be released next frame)
                if let Some(btn) = button {
                    emu.press_button(btn);
                    tracing::debug!("Tapping button: {:?}", btn);
                    // Release after a short delay is handled by the game loop
                }
            }
            "continue" | "wait" => {
                // No input this frame
            }
            _ => {
                tracing::warn!("Unknown action type: {}", decision.action);
            }
        }

        self.frame_count += 1;
        Ok(())
    }

    async fn pause(&mut self) -> MountainResult<()> {
        // GBA emulator doesn't need explicit pause - just stop calling run_frame
        Ok(())
    }

    async fn resume(&mut self) -> MountainResult<()> {
        Ok(())
    }

    async fn save_state(&self, slot: u32) -> MountainResult<Vec<u8>> {
        let emu = self.emulator.lock().await;
        emu.save_state()
            .map_err(|e| MountainError::State(format!("Save failed: {}", e)))
    }

    async fn load_state(&mut self, _slot: u32, data: &[u8]) -> MountainResult<()> {
        let mut emu = self.emulator.lock().await;
        emu.load_state(data)
            .map_err(|e| MountainError::State(format!("Load failed: {}", e)))
    }

    fn is_running(&self) -> bool {
        true // GBA emulator is always "running" when we have it
    }

    fn program_info(&self) -> ProgramState {
        ProgramState::new("Pokemon Emerald")
            .with_metadata("platform", "GBA")
            .with_metadata("resolution", "240x160")
    }
}
