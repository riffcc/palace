//! Gamepad input handling for Conductor.

use crate::error::ConductorResult;
use crate::touchpad::TouchpadGesture;
use serde::{Deserialize, Serialize};

/// Raw gamepad input state.
#[derive(Debug, Clone, Default)]
pub struct GamepadInput {
    // Face buttons
    pub a_pressed: bool,
    pub b_pressed: bool,
    pub x_held: bool,
    pub y_pressed: bool,

    // Bumpers
    pub lb_pressed: bool,
    pub rb_pressed: bool,

    // Triggers (0.0 - 1.0)
    pub l2: f32,
    pub r2: f32,

    // Stick buttons
    pub l3_pressed: bool,
    pub r3_pressed: bool,

    // Left stick (-1.0 to 1.0)
    pub left_stick_x: f32,
    pub left_stick_y: f32,

    // Right stick (-1.0 to 1.0)
    pub right_stick_x: f32,
    pub right_stick_y: f32,

    // D-pad
    pub dpad_up: bool,
    pub dpad_down: bool,
    pub dpad_left: bool,
    pub dpad_right: bool,

    // Start/Options and Back/Share
    pub start_pressed: bool,
    pub back_pressed: bool,

    // PS5 specific - touchpad
    pub touchpad_gesture: Option<TouchpadGesture>,
    pub touchpad_x: f32,
    pub touchpad_y: f32,
    pub touchpad_pressed: bool,
}

impl GamepadInput {
    /// Get left stick angle in radians (0 = right, PI/2 = up).
    pub fn left_stick_angle(&self) -> f32 {
        self.left_stick_y.atan2(self.left_stick_x)
    }

    /// Get left stick magnitude (0.0 - 1.0).
    pub fn left_stick_magnitude(&self) -> f32 {
        (self.left_stick_x.powi(2) + self.left_stick_y.powi(2))
            .sqrt()
            .min(1.0)
    }

    /// Get right stick angle in radians.
    pub fn right_stick_angle(&self) -> f32 {
        self.right_stick_y.atan2(self.right_stick_x)
    }

    /// Get right stick magnitude.
    pub fn right_stick_magnitude(&self) -> f32 {
        (self.right_stick_x.powi(2) + self.right_stick_y.powi(2))
            .sqrt()
            .min(1.0)
    }
}

/// High-level gamepad state.
#[derive(Debug, Clone, Default)]
pub struct GamepadState {
    /// Whether a gamepad is connected.
    pub connected: bool,

    /// Gamepad name.
    pub name: String,

    /// Whether it's a PS5 controller.
    pub is_ps5: bool,

    /// Battery level (0-100) if available.
    pub battery: Option<u8>,
}

/// PS5 DualSense controller wrapper.
pub struct PS5Controller {
    // In a real implementation, this would use gilrs
    // gilrs: gilrs::Gilrs,
    // gamepad_id: Option<gilrs::GamepadId>,
    state: GamepadState,
    last_input: GamepadInput,
}

impl PS5Controller {
    /// Create a new PS5 controller handler.
    pub fn new() -> ConductorResult<Self> {
        // In a real implementation:
        // let gilrs = gilrs::Gilrs::new().map_err(|e| ConductorError::Gamepad(e.to_string()))?;

        Ok(Self {
            state: GamepadState {
                connected: false,
                name: "PS5 DualSense".into(),
                is_ps5: true,
                battery: None,
            },
            last_input: GamepadInput::default(),
        })
    }

    /// Poll for input.
    pub fn poll(&self) -> ConductorResult<GamepadInput> {
        // In a real implementation, this would read from gilrs
        // For now, return the last input
        Ok(self.last_input.clone())
    }

    /// Get controller state.
    pub fn state(&self) -> &GamepadState {
        &self.state
    }

    /// Check if connected.
    pub fn is_connected(&self) -> bool {
        self.state.connected
    }

    /// Simulate input for testing.
    #[cfg(test)]
    pub fn simulate_input(&mut self, input: GamepadInput) {
        self.last_input = input;
    }

    /// Send haptic feedback.
    pub fn send_haptic(&self, pattern: HapticFeedback) {
        // In a real implementation, this would use the DualSense haptics API
        tracing::debug!("Haptic feedback: {:?}", pattern);
    }
}

/// Haptic feedback patterns for the controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HapticFeedback {
    /// Single click.
    Click,
    /// Double click.
    DoubleClick,
    /// Light tap (for pending state).
    Tap,
    /// Bump (hit limit).
    Bump,
    /// De-bump (timeout/deactivation).
    Debump,
    /// Role swap notification - distinct pattern so both players know.
    RoleSwap,
    /// Focus changed between Game and Orchestrator.
    FocusChange,
    /// Execution moved between devices.
    ExecutionMove,
}

impl HapticFeedback {
    /// Get the intensity for this pattern (0.0 - 1.0).
    pub fn intensity(&self) -> f32 {
        match self {
            HapticFeedback::Click => 0.5,
            HapticFeedback::DoubleClick => 0.6,
            HapticFeedback::Tap => 0.3,
            HapticFeedback::Bump => 0.8,
            HapticFeedback::Debump => 0.4,
            HapticFeedback::RoleSwap => 0.7,
            HapticFeedback::FocusChange => 0.4,
            HapticFeedback::ExecutionMove => 0.6,
        }
    }

    /// Get the duration in milliseconds.
    pub fn duration_ms(&self) -> u32 {
        match self {
            HapticFeedback::Click => 50,
            HapticFeedback::DoubleClick => 100,
            HapticFeedback::Tap => 30,
            HapticFeedback::Bump => 80,
            HapticFeedback::Debump => 60,
            HapticFeedback::RoleSwap => 200,
            HapticFeedback::FocusChange => 40,
            HapticFeedback::ExecutionMove => 150,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stick_angle() {
        let mut input = GamepadInput::default();

        // Right = 0
        input.left_stick_x = 1.0;
        input.left_stick_y = 0.0;
        assert!((input.left_stick_angle() - 0.0).abs() < 0.01);

        // Up = PI/2
        input.left_stick_x = 0.0;
        input.left_stick_y = 1.0;
        assert!((input.left_stick_angle() - std::f32::consts::FRAC_PI_2).abs() < 0.01);
    }

    #[test]
    fn test_stick_magnitude() {
        let mut input = GamepadInput::default();

        input.left_stick_x = 0.6;
        input.left_stick_y = 0.8;
        assert!((input.left_stick_magnitude() - 1.0).abs() < 0.01);

        input.left_stick_x = 0.3;
        input.left_stick_y = 0.4;
        assert!((input.left_stick_magnitude() - 0.5).abs() < 0.01);
    }
}
