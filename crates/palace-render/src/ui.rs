//! UI state and elements for Palace.

use crate::slider::ConfidenceSliderWidget;
use conductor::{ControlFocus, OutputGranularity};
use mountain::AssuranceLevel;
use serde::{Deserialize, Serialize};

/// Overall UI state.
#[derive(Debug, Clone, Default)]
pub struct UIState {
    /// Confidence slider.
    pub slider: ConfidenceSliderWidget,

    /// Output granularity level.
    pub granularity: OutputGranularity,

    /// Current control focus.
    pub focus: ControlFocus,

    /// Visible agent index.
    pub visible_agent: usize,

    /// Total number of agents.
    pub agent_count: usize,

    /// Whether radial menus are active.
    pub radial_active: bool,

    /// LLM output lines to display.
    pub llm_output: Vec<OutputLine>,

    /// Code lines being generated.
    pub code_output: Vec<CodeLine>,

    /// Status messages.
    pub status_messages: Vec<StatusMessage>,

    /// Whether AI control is enabled.
    pub ai_enabled: bool,
}

impl UIState {
    /// Update assurance level.
    pub fn set_assurance(&mut self, level: AssuranceLevel) {
        self.slider.state.current = level;
    }

    /// Add an LLM output line.
    pub fn add_llm_output(&mut self, line: OutputLine) {
        self.llm_output.push(line);
        // Keep last 100 lines
        if self.llm_output.len() > 100 {
            self.llm_output.remove(0);
        }
    }

    /// Add a code output line.
    pub fn add_code_line(&mut self, line: CodeLine) {
        self.code_output.push(line);
        if self.code_output.len() > 500 {
            self.code_output.remove(0);
        }
    }

    /// Add a status message.
    pub fn add_status(&mut self, message: StatusMessage) {
        self.status_messages.push(message);
        // Keep last 10 messages
        if self.status_messages.len() > 10 {
            self.status_messages.remove(0);
        }
    }

    /// Clear status messages.
    pub fn clear_status(&mut self) {
        self.status_messages.clear();
    }
}

/// A line of LLM output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputLine {
    /// Agent ID that produced this.
    pub agent_id: usize,

    /// Agent name.
    pub agent_name: String,

    /// The text content.
    pub text: String,

    /// Output type.
    pub output_type: OutputType,

    /// Timestamp (ms since epoch).
    pub timestamp_ms: u64,
}

/// Type of output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputType {
    /// Thinking/reasoning.
    Thinking,
    /// Regular text.
    Text,
    /// Decision.
    Decision,
    /// Tool usage.
    Tool,
    /// Error.
    Error,
}

impl OutputType {
    /// Get color for this output type.
    pub fn color(&self) -> [f32; 4] {
        match self {
            OutputType::Thinking => [0.6, 0.6, 0.8, 1.0], // Light purple
            OutputType::Text => [1.0, 1.0, 1.0, 1.0],     // White
            OutputType::Decision => [0.3, 1.0, 0.3, 1.0], // Green
            OutputType::Tool => [1.0, 0.8, 0.3, 1.0],     // Yellow
            OutputType::Error => [1.0, 0.3, 0.3, 1.0],    // Red
        }
    }
}

/// A line of code being generated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeLine {
    /// Line number.
    pub line_number: u32,

    /// The code text.
    pub text: String,

    /// Language for syntax highlighting.
    pub language: String,

    /// File path.
    pub file_path: Option<String>,

    /// Whether this line is new (for animation).
    pub is_new: bool,
}

/// A status message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusMessage {
    /// Message text.
    pub text: String,

    /// Severity level.
    pub level: StatusLevel,

    /// Timestamp (ms since epoch).
    pub timestamp_ms: u64,
}

/// Status message severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusLevel {
    Info,
    Success,
    Warning,
    Error,
}

impl StatusLevel {
    /// Get color for this level.
    pub fn color(&self) -> [f32; 4] {
        match self {
            StatusLevel::Info => [0.7, 0.7, 0.7, 1.0],
            StatusLevel::Success => [0.3, 1.0, 0.3, 1.0],
            StatusLevel::Warning => [1.0, 0.8, 0.3, 1.0],
            StatusLevel::Error => [1.0, 0.3, 0.3, 1.0],
        }
    }
}

/// A UI element that can be rendered.
pub trait UIElement {
    /// Get the element's bounds.
    fn bounds(&self) -> ElementBounds;

    /// Whether the element is visible.
    fn is_visible(&self) -> bool;
}

/// Bounds for a UI element.
#[derive(Debug, Clone, Copy)]
pub struct ElementBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ui_state_output() {
        let mut state = UIState::default();

        state.add_llm_output(OutputLine {
            agent_id: 0,
            agent_name: "test".into(),
            text: "Hello".into(),
            output_type: OutputType::Text,
            timestamp_ms: 0,
        });

        assert_eq!(state.llm_output.len(), 1);
    }

    #[test]
    fn test_output_type_colors() {
        let thinking = OutputType::Thinking;
        let color = thinking.color();
        assert!(color[3] == 1.0); // Full alpha
    }
}
