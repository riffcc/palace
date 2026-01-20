//! Gamepad input handling using gilrs.

use anyhow::Result;
use gilrs::{Button, Event, EventType, Gilrs, GamepadId};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Gamepad input state.
pub struct GamepadInput {
    gilrs: Gilrs,
    active_gamepad: Option<GamepadId>,
    button_states: HashMap<Button, bool>,
    left_stick: (f32, f32),
    right_stick: (f32, f32),
    left_trigger: f32,
    right_trigger: f32,
    deadzone: f32,

    // PS button double-press detection
    ps_last_press: Option<Instant>,
    ps_double_press_timeout: Duration,
}

impl GamepadInput {
    /// Create a new gamepad input handler.
    pub fn new() -> Result<Self> {
        let gilrs = Gilrs::new().map_err(|e| anyhow::anyhow!("Failed to init gilrs: {}", e))?;

        // Find first connected gamepad
        let active_gamepad = gilrs.gamepads().next().map(|(id, _)| id);

        if let Some(id) = active_gamepad {
            let gamepad = gilrs.gamepad(id);
            info!("Found gamepad: {} ({:?})", gamepad.name(), gamepad.uuid());
        } else {
            warn!("No gamepad connected");
        }

        Ok(Self {
            gilrs,
            active_gamepad,
            button_states: HashMap::new(),
            left_stick: (0.0, 0.0),
            right_stick: (0.0, 0.0),
            left_trigger: 0.0,
            right_trigger: 0.0,
            deadzone: 0.15,
            ps_last_press: None,
            ps_double_press_timeout: Duration::from_millis(300),
        })
    }

    /// Set stick deadzone.
    pub fn set_deadzone(&mut self, deadzone: f32) {
        self.deadzone = deadzone;
    }

    /// Poll for gamepad events. Returns any special events that occurred.
    pub fn poll(&mut self) -> Vec<GamepadEvent> {
        let mut events = Vec::new();

        while let Some(Event { id, event, .. }) = self.gilrs.next_event() {
            // Track active gamepad
            if self.active_gamepad.is_none() {
                self.active_gamepad = Some(id);
                let gamepad = self.gilrs.gamepad(id);
                info!("Gamepad connected: {}", gamepad.name());
            }

            match event {
                EventType::ButtonPressed(button, _) => {
                    self.button_states.insert(button, true);
                    debug!("Button pressed: {:?}", button);

                    // Check for special buttons
                    if let Some(ev) = self.handle_button_press(button) {
                        events.push(ev);
                    }
                }

                EventType::ButtonReleased(button, _) => {
                    self.button_states.insert(button, false);
                    debug!("Button released: {:?}", button);

                    if let Some(ev) = self.handle_button_release(button) {
                        events.push(ev);
                    }
                }

                EventType::ButtonChanged(button, value, _) => {
                    // Handle analog trigger values
                    match button {
                        Button::LeftTrigger2 => {
                            self.left_trigger = value;
                        }
                        Button::RightTrigger2 => {
                            self.right_trigger = value;
                        }
                        _ => {}
                    }
                }

                EventType::AxisChanged(axis, value, _) => {
                    use gilrs::Axis;

                    match axis {
                        Axis::LeftStickX => self.left_stick.0 = self.apply_deadzone(value),
                        Axis::LeftStickY => self.left_stick.1 = self.apply_deadzone(value),
                        Axis::RightStickX => self.right_stick.0 = self.apply_deadzone(value),
                        Axis::RightStickY => self.right_stick.1 = self.apply_deadzone(value),
                        _ => {}
                    }
                }

                EventType::Connected => {
                    let gamepad = self.gilrs.gamepad(id);
                    info!("Gamepad connected: {}", gamepad.name());
                    events.push(GamepadEvent::Connected(id));
                }

                EventType::Disconnected => {
                    warn!("Gamepad disconnected");
                    if self.active_gamepad == Some(id) {
                        self.active_gamepad = None;
                    }
                    events.push(GamepadEvent::Disconnected(id));
                }

                _ => {}
            }
        }

        events
    }

    /// Apply deadzone to axis value.
    fn apply_deadzone(&self, value: f32) -> f32 {
        if value.abs() < self.deadzone {
            0.0
        } else {
            // Rescale to 0..1 range after deadzone
            let sign = value.signum();
            let magnitude = (value.abs() - self.deadzone) / (1.0 - self.deadzone);
            sign * magnitude
        }
    }

    /// Handle special button presses.
    fn handle_button_press(&mut self, button: Button) -> Option<GamepadEvent> {
        match button {
            // PS/Home button - check for double press
            Button::Mode => {
                let now = Instant::now();
                if let Some(last) = self.ps_last_press {
                    if now.duration_since(last) < self.ps_double_press_timeout {
                        self.ps_last_press = None;
                        return Some(GamepadEvent::PSDoublePress);
                    }
                }
                self.ps_last_press = Some(now);
                Some(GamepadEvent::PSPress)
            }

            // Start/Select for confidence adjustment (when conductor focused)
            Button::Start => Some(GamepadEvent::ConfidenceIncrease),
            Button::Select => Some(GamepadEvent::ConfidenceDecrease),

            // L2/R2 triggers
            Button::LeftTrigger2 => Some(GamepadEvent::L2Press),
            Button::RightTrigger2 => Some(GamepadEvent::R2Press),

            _ => None,
        }
    }

    /// Handle special button releases.
    fn handle_button_release(&mut self, button: Button) -> Option<GamepadEvent> {
        match button {
            Button::LeftTrigger2 => Some(GamepadEvent::L2Release),
            Button::RightTrigger2 => Some(GamepadEvent::R2Release),
            _ => None,
        }
    }

    /// Get start/select button states.
    pub fn start_select(&self) -> (bool, bool) {
        (
            self.is_pressed(Button::Start),
            self.is_pressed(Button::Select),
        )
    }

    /// Get shoulder button states (L1/R1).
    pub fn shoulders(&self) -> (bool, bool) {
        (
            self.is_pressed(Button::LeftTrigger),
            self.is_pressed(Button::RightTrigger),
        )
    }

    /// Get trigger values (L2/R2) as analog 0.0-1.0.
    pub fn triggers_analog(&self) -> (f32, f32) {
        (self.left_trigger, self.right_trigger)
    }

    /// Get trigger states as digital (threshold 0.5).
    pub fn triggers(&self) -> (bool, bool) {
        (self.left_trigger > 0.5, self.right_trigger > 0.5)
    }

    /// Check if a button is pressed.
    pub fn is_pressed(&self, button: Button) -> bool {
        *self.button_states.get(&button).unwrap_or(&false)
    }

    /// Get left stick position.
    pub fn left_stick(&self) -> (f32, f32) {
        self.left_stick
    }

    /// Get right stick position.
    pub fn right_stick(&self) -> (f32, f32) {
        self.right_stick
    }

    /// Get D-pad as digital directions.
    pub fn dpad(&self) -> (bool, bool, bool, bool) {
        (
            self.is_pressed(Button::DPadUp),
            self.is_pressed(Button::DPadDown),
            self.is_pressed(Button::DPadLeft),
            self.is_pressed(Button::DPadRight),
        )
    }

    /// Get face buttons (A/B/X/Y or Cross/Circle/Square/Triangle).
    pub fn face_buttons(&self) -> (bool, bool, bool, bool) {
        (
            self.is_pressed(Button::South), // A / Cross
            self.is_pressed(Button::East),  // B / Circle
            self.is_pressed(Button::West),  // X / Square
            self.is_pressed(Button::North), // Y / Triangle
        )
    }

    /// Check if any gamepad is connected.
    pub fn is_connected(&self) -> bool {
        self.active_gamepad.is_some()
    }
}

/// Special gamepad events.
#[derive(Debug, Clone)]
pub enum GamepadEvent {
    /// PS/Home button pressed once (toggle focus).
    PSPress,
    /// PS/Home button double-pressed (move execution).
    PSDoublePress,
    /// D-pad up (increase confidence).
    ConfidenceIncrease,
    /// D-pad down (decrease confidence).
    ConfidenceDecrease,
    /// L2 pressed.
    L2Press,
    /// L2 released.
    L2Release,
    /// R2 pressed.
    R2Press,
    /// R2 released.
    R2Release,
    /// Gamepad connected.
    Connected(GamepadId),
    /// Gamepad disconnected.
    Disconnected(GamepadId),
}

/// List connected gamepads.
pub fn list_gamepads() -> Result<()> {
    let gilrs = Gilrs::new().map_err(|e| anyhow::anyhow!("Failed to init gilrs: {}", e))?;

    println!("Connected gamepads:");
    println!();

    let mut count = 0;
    for (id, gamepad) in gilrs.gamepads() {
        println!("  [{:?}] {} ", id, gamepad.name());
        println!("       UUID: {:?}", gamepad.uuid());
        println!("       Power: {:?}", gamepad.power_info());

        // List buttons
        print!("       Buttons: ");
        for button in [
            Button::South, Button::East, Button::North, Button::West,
            Button::LeftTrigger, Button::RightTrigger,
            Button::LeftTrigger2, Button::RightTrigger2,
            Button::Select, Button::Start, Button::Mode,
            Button::LeftThumb, Button::RightThumb,
            Button::DPadUp, Button::DPadDown, Button::DPadLeft, Button::DPadRight,
        ] {
            if gamepad.is_pressed(button) {
                print!("{:?} ", button);
            }
        }
        println!();
        println!();
        count += 1;
    }

    if count == 0 {
        println!("  (none)");
        println!();
        println!("Connect a gamepad and try again.");
    } else {
        println!("Found {} gamepad(s)", count);
    }

    Ok(())
}
