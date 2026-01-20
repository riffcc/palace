//! Dual-screen management for Palace.

use serde::{Deserialize, Serialize};

/// Which side of the split screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenSide {
    /// Left side - typically the game/program.
    Left,
    /// Right side - typically the orchestrator.
    Right,
}

/// View mode - split or unitasking (single screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ViewMode {
    /// Split screen showing both game and conductor.
    #[default]
    Split,
    /// Unitasking - game only.
    GameOnly,
    /// Unitasking - conductor only.
    ConductorOnly,
}

/// Manages the dual-screen split view.
#[derive(Debug, Clone)]
pub struct DualScreen {
    /// Split ratio (0.0 - 1.0, percentage for left side).
    pub split_ratio: f32,

    /// Whether the divider is visible.
    pub show_divider: bool,

    /// Divider width in pixels.
    pub divider_width: u32,

    /// Currently focused side.
    pub focused: ScreenSide,

    /// Left side content type.
    pub left_content: ContentType,

    /// Right side content type.
    pub right_content: ContentType,

    /// Current view mode.
    pub view_mode: ViewMode,
}

impl DualScreen {
    /// Create a new dual screen with the given split ratio.
    pub fn new(split_ratio: f32) -> Self {
        Self {
            split_ratio: split_ratio.clamp(0.2, 0.8),
            show_divider: true,
            divider_width: 2,
            focused: ScreenSide::Left,
            left_content: ContentType::Game,
            right_content: ContentType::Orchestrator,
            view_mode: ViewMode::Split,
        }
    }

    /// Toggle focus between sides.
    pub fn toggle_focus(&mut self) {
        self.focused = match self.focused {
            ScreenSide::Left => ScreenSide::Right,
            ScreenSide::Right => ScreenSide::Left,
        };
    }

    /// Cycle through view modes: Split -> GameOnly -> ConductorOnly -> Split.
    pub fn cycle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Split => ViewMode::GameOnly,
            ViewMode::GameOnly => ViewMode::ConductorOnly,
            ViewMode::ConductorOnly => ViewMode::Split,
        };
    }

    /// Handle guide button single press - ALWAYS toggles focus.
    /// - In Split: toggles focus between Game and Conductor sides
    /// - In Unitasking: toggles focus AND switches display
    pub fn handle_guide_single_press(&mut self) {
        match self.view_mode {
            ViewMode::Split => {
                // Just toggle focus in split mode
                self.toggle_focus();
            }
            ViewMode::GameOnly => {
                // Switch to conductor (focus + display)
                self.view_mode = ViewMode::ConductorOnly;
                self.focused = ScreenSide::Right;
            }
            ViewMode::ConductorOnly => {
                // Switch to game (focus + display)
                self.view_mode = ViewMode::GameOnly;
                self.focused = ScreenSide::Left;
            }
        }
    }

    /// Handle guide button double-tap - toggles between Split and Unitasking.
    /// - In Split: go to unitasking (keeps current focus)
    /// - In Unitasking: go back to Split
    pub fn handle_guide_double_tap(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Split => {
                // Go to unitasking, keeping current focus
                if self.focused == ScreenSide::Left {
                    ViewMode::GameOnly
                } else {
                    ViewMode::ConductorOnly
                }
            }
            ViewMode::GameOnly | ViewMode::ConductorOnly => ViewMode::Split,
        };
    }

    /// Check if game is focused (for input routing).
    pub fn game_focused(&self) -> bool {
        match self.view_mode {
            ViewMode::GameOnly => true,
            ViewMode::ConductorOnly => false,
            ViewMode::Split => self.focused == ScreenSide::Left,
        }
    }

    /// Check if in split view mode.
    pub fn is_split(&self) -> bool {
        self.view_mode == ViewMode::Split
    }

    /// Check if game should be visible.
    pub fn show_game(&self) -> bool {
        matches!(self.view_mode, ViewMode::Split | ViewMode::GameOnly)
    }

    /// Check if conductor should be visible.
    pub fn show_conductor(&self) -> bool {
        matches!(self.view_mode, ViewMode::Split | ViewMode::ConductorOnly)
    }

    /// Get the pixel bounds for a side given the window dimensions.
    pub fn get_bounds(&self, side: ScreenSide, window_width: u32, window_height: u32) -> ScreenBounds {
        let split_x = (window_width as f32 * self.split_ratio) as u32;

        match side {
            ScreenSide::Left => ScreenBounds {
                x: 0,
                y: 0,
                width: split_x.saturating_sub(self.divider_width / 2),
                height: window_height,
            },
            ScreenSide::Right => ScreenBounds {
                x: split_x + self.divider_width / 2,
                y: 0,
                width: window_width.saturating_sub(split_x + self.divider_width / 2),
                height: window_height,
            },
        }
    }

    /// Adjust split ratio.
    pub fn adjust_split(&mut self, delta: f32) {
        self.split_ratio = (self.split_ratio + delta).clamp(0.2, 0.8);
    }
}

impl Default for DualScreen {
    fn default() -> Self {
        Self::new(0.6)
    }
}

/// Pixel bounds for a screen region.
#[derive(Debug, Clone, Copy)]
pub struct ScreenBounds {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl ScreenBounds {
    /// Get aspect ratio.
    pub fn aspect_ratio(&self) -> f32 {
        self.width as f32 / self.height as f32
    }

    /// Check if a point is within bounds.
    pub fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// Type of content displayed on a screen side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentType {
    /// Game or program being controlled.
    Game,
    /// Orchestrator/Conductor interface.
    Orchestrator,
    /// LLM output stream.
    LLMStream,
    /// Code editor view.
    CodeView,
    /// Browser via Playwright.
    Browser,
    /// Terminal output.
    Terminal,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dual_screen_bounds() {
        let screen = DualScreen::new(0.5);

        let left = screen.get_bounds(ScreenSide::Left, 1920, 1080);
        let right = screen.get_bounds(ScreenSide::Right, 1920, 1080);

        assert!(left.width > 0);
        assert!(right.width > 0);
        assert!(left.width + right.width + screen.divider_width <= 1920);
    }

    #[test]
    fn test_focus_toggle() {
        let mut screen = DualScreen::default();
        assert_eq!(screen.focused, ScreenSide::Left);

        screen.toggle_focus();
        assert_eq!(screen.focused, ScreenSide::Right);

        screen.toggle_focus();
        assert_eq!(screen.focused, ScreenSide::Left);
    }
}
