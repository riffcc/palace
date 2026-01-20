//! Output streaming and granularity control for Conductor.
//!
//! Manages the visibility and detail level of LLM outputs:
//! - L2/R2 control granularity (context depth)
//! - RB/LB switch between visible agents
//! - Real-time code formatting and highlighting

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Output granularity level (controlled by L2/R2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub enum OutputGranularity {
    /// Minimal - just key decisions.
    Minimal,
    /// Summary - main points and reasoning.
    #[default]
    Summary,
    /// Detailed - full context and explanation.
    Detailed,
    /// Verbose - everything including internal state.
    Verbose,
    /// Code - show actual code being generated with highlighting.
    Code,
}

impl OutputGranularity {
    /// Decrease granularity (L2).
    pub fn decrease(self) -> Self {
        match self {
            OutputGranularity::Code => OutputGranularity::Verbose,
            OutputGranularity::Verbose => OutputGranularity::Detailed,
            OutputGranularity::Detailed => OutputGranularity::Summary,
            OutputGranularity::Summary => OutputGranularity::Minimal,
            OutputGranularity::Minimal => OutputGranularity::Minimal,
        }
    }

    /// Increase granularity (R2).
    pub fn increase(self) -> Self {
        match self {
            OutputGranularity::Minimal => OutputGranularity::Summary,
            OutputGranularity::Summary => OutputGranularity::Detailed,
            OutputGranularity::Detailed => OutputGranularity::Verbose,
            OutputGranularity::Verbose => OutputGranularity::Code,
            OutputGranularity::Code => OutputGranularity::Code,
        }
    }

    /// Get display name.
    pub fn name(&self) -> &'static str {
        match self {
            OutputGranularity::Minimal => "Minimal",
            OutputGranularity::Summary => "Summary",
            OutputGranularity::Detailed => "Detailed",
            OutputGranularity::Verbose => "Verbose",
            OutputGranularity::Code => "Code",
        }
    }
}

/// An output from an AI agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOutput {
    /// Output type.
    pub output_type: OutputType,

    /// Content.
    pub content: String,

    /// Timestamp.
    pub timestamp: chrono::DateTime<chrono::Utc>,

    /// Token count.
    pub tokens: Option<u32>,

    /// Confidence if applicable.
    pub confidence: Option<f32>,

    /// Associated file path if code.
    pub file_path: Option<String>,

    /// Language for syntax highlighting.
    pub language: Option<String>,

    /// Metadata.
    pub metadata: HashMap<String, serde_json::Value>,
}

impl AgentOutput {
    /// Create a text output.
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            output_type: OutputType::Text,
            content: content.into(),
            timestamp: chrono::Utc::now(),
            tokens: None,
            confidence: None,
            file_path: None,
            language: None,
            metadata: HashMap::new(),
        }
    }

    /// Create a thinking/reasoning output.
    pub fn thinking(content: impl Into<String>) -> Self {
        Self {
            output_type: OutputType::Thinking,
            content: content.into(),
            timestamp: chrono::Utc::now(),
            tokens: None,
            confidence: None,
            file_path: None,
            language: None,
            metadata: HashMap::new(),
        }
    }

    /// Create a code output.
    pub fn code(content: impl Into<String>, language: impl Into<String>) -> Self {
        Self {
            output_type: OutputType::Code,
            content: content.into(),
            timestamp: chrono::Utc::now(),
            tokens: None,
            confidence: None,
            file_path: None,
            language: Some(language.into()),
            metadata: HashMap::new(),
        }
    }

    /// Create a decision output.
    pub fn decision(content: impl Into<String>, confidence: f32) -> Self {
        Self {
            output_type: OutputType::Decision,
            content: content.into(),
            timestamp: chrono::Utc::now(),
            tokens: None,
            confidence: Some(confidence),
            file_path: None,
            language: None,
            metadata: HashMap::new(),
        }
    }

    /// Set file path.
    pub fn with_file(mut self, path: impl Into<String>) -> Self {
        self.file_path = Some(path.into());
        self
    }

    /// Set token count.
    pub fn with_tokens(mut self, tokens: u32) -> Self {
        self.tokens = Some(tokens);
        self
    }

    /// Add metadata.
    pub fn with_meta(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// Check if this output should be visible at the given granularity.
    pub fn visible_at(&self, granularity: OutputGranularity) -> bool {
        match (granularity, &self.output_type) {
            // Minimal: only decisions
            (OutputGranularity::Minimal, OutputType::Decision) => true,
            (OutputGranularity::Minimal, _) => false,

            // Summary: decisions and text
            (OutputGranularity::Summary, OutputType::Decision) => true,
            (OutputGranularity::Summary, OutputType::Text) => true,
            (OutputGranularity::Summary, _) => false,

            // Detailed: decisions, text, thinking
            (OutputGranularity::Detailed, OutputType::Thinking) => true,
            (OutputGranularity::Detailed, OutputType::Decision) => true,
            (OutputGranularity::Detailed, OutputType::Text) => true,
            (OutputGranularity::Detailed, _) => false,

            // Verbose and Code: everything
            (OutputGranularity::Verbose, _) => true,
            (OutputGranularity::Code, _) => true,
        }
    }
}

/// Type of output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputType {
    /// Plain text.
    Text,
    /// Internal reasoning/thinking.
    Thinking,
    /// Code being generated.
    Code,
    /// A decision.
    Decision,
    /// An error.
    Error,
    /// Tool usage.
    Tool,
    /// Streaming delta (partial).
    Delta,
}

/// An AI agent in the stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    /// Agent ID.
    pub id: usize,

    /// Agent name.
    pub name: String,

    /// Model being used.
    pub model: String,

    /// Whether local or cloud.
    pub is_local: bool,

    /// Current status.
    pub status: AgentStatus,

    /// Color for UI.
    pub color: String,
}

/// Status of an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    Idle,
    Thinking,
    Generating,
    Complete,
    Error,
}

/// The output stream manages all agent outputs.
#[derive(Debug)]
pub struct OutputStream {
    /// All agents.
    pub agents: Vec<Agent>,

    /// Outputs by agent ID.
    outputs: HashMap<usize, Vec<AgentOutput>>,

    /// Currently visible agent.
    visible_agent: usize,

    /// Current granularity.
    granularity: OutputGranularity,

    /// Maximum outputs to keep per agent.
    max_outputs: usize,
}

impl OutputStream {
    /// Create a new output stream.
    pub fn new() -> Self {
        Self {
            agents: vec![],
            outputs: HashMap::new(),
            visible_agent: 0,
            granularity: OutputGranularity::default(),
            max_outputs: 1000,
        }
    }

    /// Add an agent.
    pub fn add_agent(&mut self, agent: Agent) {
        let id = agent.id;
        self.agents.push(agent);
        self.outputs.insert(id, vec![]);
    }

    /// Add an output for an agent.
    pub fn add_output(&mut self, agent_id: usize, output: AgentOutput) {
        if let Some(outputs) = self.outputs.get_mut(&agent_id) {
            outputs.push(output);

            // Trim if over limit
            while outputs.len() > self.max_outputs {
                outputs.remove(0);
            }
        }
    }

    /// Get visible outputs at current granularity.
    pub fn visible_outputs(&self) -> Vec<&AgentOutput> {
        self.outputs
            .get(&self.visible_agent)
            .map(|outputs| {
                outputs
                    .iter()
                    .filter(|o| o.visible_at(self.granularity))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all outputs for an agent.
    pub fn outputs_for(&self, agent_id: usize) -> Option<&Vec<AgentOutput>> {
        self.outputs.get(&agent_id)
    }

    /// Switch visible agent (RB/LB).
    pub fn switch_agent(&mut self, agent_id: usize) {
        if agent_id < self.agents.len() {
            self.visible_agent = agent_id;
        }
    }

    /// Set granularity (L2/R2).
    pub fn set_granularity(&mut self, granularity: OutputGranularity) {
        self.granularity = granularity;
    }

    /// Get current granularity.
    pub fn granularity(&self) -> OutputGranularity {
        self.granularity
    }

    /// Get visible agent.
    pub fn visible_agent(&self) -> usize {
        self.visible_agent
    }

    /// Get visible agent info.
    pub fn visible_agent_info(&self) -> Option<&Agent> {
        self.agents.get(self.visible_agent)
    }

    /// Clear all outputs.
    pub fn clear(&mut self) {
        for outputs in self.outputs.values_mut() {
            outputs.clear();
        }
    }
}

impl Default for OutputStream {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_granularity_adjustment() {
        let mut level = OutputGranularity::Summary;

        level = level.increase();
        assert_eq!(level, OutputGranularity::Detailed);

        level = level.decrease();
        assert_eq!(level, OutputGranularity::Summary);

        level = level.decrease();
        assert_eq!(level, OutputGranularity::Minimal);

        // Can't go below minimal
        level = level.decrease();
        assert_eq!(level, OutputGranularity::Minimal);
    }

    #[test]
    fn test_output_visibility() {
        let decision = AgentOutput::decision("Choose A", 0.9);
        let thinking = AgentOutput::thinking("Considering options...");
        let code = AgentOutput::code("fn main() {}", "rust");

        // Minimal: only decisions
        assert!(decision.visible_at(OutputGranularity::Minimal));
        assert!(!thinking.visible_at(OutputGranularity::Minimal));
        assert!(!code.visible_at(OutputGranularity::Minimal));

        // Code: everything
        assert!(decision.visible_at(OutputGranularity::Code));
        assert!(thinking.visible_at(OutputGranularity::Code));
        assert!(code.visible_at(OutputGranularity::Code));
    }

    #[test]
    fn test_output_stream() {
        let mut stream = OutputStream::new();

        stream.add_agent(Agent {
            id: 0,
            name: "Agent 1".into(),
            model: "test".into(),
            is_local: true,
            status: AgentStatus::Idle,
            color: "#FF0000".into(),
        });

        stream.add_output(0, AgentOutput::text("Hello"));
        stream.add_output(0, AgentOutput::decision("Choose A", 0.9));

        let visible = stream.visible_outputs();
        assert_eq!(visible.len(), 2); // At Summary level
    }
}
