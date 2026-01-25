//! Confidence slider widget for the UI.

use mountain::{AssuranceLevel, ConfidenceSliderState};
use serde::{Deserialize, Serialize};

/// Visual configuration for the confidence slider.
#[derive(Debug, Clone)]
pub struct SliderStyle {
    /// Background color (RGBA).
    pub background: [f32; 4],
    /// Text color for current level.
    pub text_color: [f32; 4],
    /// Text color for dimmed adjacent levels.
    pub dimmed_color: [f32; 4],
    /// Highlight color when OC-3 pending.
    pub pending_color: [f32; 4],
    /// Font size.
    pub font_size: f32,
    /// Padding in pixels.
    pub padding: f32,
}

impl Default for SliderStyle {
    fn default() -> Self {
        Self {
            background: [0.0, 0.0, 0.0, 0.7],
            text_color: [1.0, 1.0, 1.0, 1.0],
            dimmed_color: [0.6, 0.6, 0.6, 0.5],
            pending_color: [1.0, 0.8, 0.0, 1.0],
            font_size: 14.0,
            padding: 8.0,
        }
    }
}

/// The confidence slider widget.
#[derive(Debug, Clone)]
pub struct ConfidenceSliderWidget {
    /// Current state.
    pub state: ConfidenceSliderState,
    /// Visual style.
    pub style: SliderStyle,
    /// Position from bottom-right corner.
    pub margin_right: f32,
    pub margin_bottom: f32,
}

impl ConfidenceSliderWidget {
    /// Create a new slider widget.
    pub fn new() -> Self {
        Self {
            state: ConfidenceSliderState::default(),
            style: SliderStyle::default(),
            margin_right: 16.0,
            margin_bottom: 16.0,
        }
    }

    /// Get the display strings for rendering.
    pub fn get_display(&self) -> SliderDisplay {
        let (prev, current, next) = self.state.display_levels();

        SliderDisplay {
            previous: prev.map(|l| l.short_name()),
            current: current.short_name(),
            current_description: current.description(),
            next: next.map(|l| l.short_name()),
            is_pending: self.state.high_oc_pending,
        }
    }

    /// Calculate the widget bounds.
    pub fn calculate_bounds(&self, window_width: f32, window_height: f32) -> WidgetBounds {
        // Estimate text width based on content
        let char_width = self.style.font_size * 0.6;
        let display = self.get_display();

        let prev_width = display.previous.map(|s| s.len() as f32 * char_width).unwrap_or(0.0);
        let curr_width = display.current.len() as f32 * char_width;
        let next_width = display.next.map(|s| s.len() as f32 * char_width).unwrap_or(0.0);

        let spacing = self.style.padding * 2.0;
        let total_width = prev_width + curr_width + next_width + spacing * 3.0;
        let height = self.style.font_size + self.style.padding * 2.0;

        let x = window_width - total_width - self.margin_right;
        let y = window_height - height - self.margin_bottom;

        WidgetBounds {
            x,
            y,
            width: total_width,
            height,
            prev_x: x + self.style.padding,
            prev_width,
            curr_x: x + self.style.padding + prev_width + spacing,
            curr_width,
            next_x: x + self.style.padding + prev_width + curr_width + spacing * 2.0,
            next_width,
        }
    }
}

impl Default for ConfidenceSliderWidget {
    fn default() -> Self {
        Self::new()
    }
}

/// Display strings for the slider.
#[derive(Debug, Clone)]
pub struct SliderDisplay {
    /// Previous level (dimmed, left).
    pub previous: Option<&'static str>,
    /// Current level (bright, center).
    pub current: &'static str,
    /// Current level description.
    pub current_description: &'static str,
    /// Next level (dimmed, right).
    pub next: Option<&'static str>,
    /// Whether OC-3 activation is pending.
    pub is_pending: bool,
}

/// Calculated bounds for the widget.
#[derive(Debug, Clone)]
pub struct WidgetBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub prev_x: f32,
    pub prev_width: f32,
    pub curr_x: f32,
    pub curr_width: f32,
    pub next_x: f32,
    pub next_width: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slider_display() {
        let widget = ConfidenceSliderWidget::new();
        let display = widget.get_display();

        // Default is Flash
        assert_eq!(display.current, "FLASH");
        assert!(display.previous.is_some());
        assert!(display.next.is_some());
    }

    #[test]
    fn test_widget_bounds() {
        let widget = ConfidenceSliderWidget::new();
        let bounds = widget.calculate_bounds(1920.0, 1080.0);

        assert!(bounds.x > 0.0);
        assert!(bounds.y > 0.0);
        assert!(bounds.x + bounds.width < 1920.0);
    }
}
