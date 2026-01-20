//! Task types and generation.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

use llm_code_sdk::client::Client;
use llm_code_sdk::tools::{ToolEvent, ToolEventCallback, ToolRunner, ToolRunnerConfig};
use llm_code_sdk::types::{MessageCreateParams, MessageParam, SystemPrompt};
use tracing::{info, warn};

use crate::exploration::create_exploration_tools;

pub use llm_code_sdk::tools::ToolEvent as ExplorationEvent;

/// Task priority levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskPriority {
    Urgent = 1,
    High = 2,
    Medium = 3,
    Low = 4,
    None = 5,
}

impl Default for TaskPriority {
    fn default() -> Self {
        Self::Medium
    }
}

impl TaskPriority {
    pub fn from_u8(n: u8) -> Self {
        match n {
            1 => Self::Urgent,
            2 => Self::High,
            3 => Self::Medium,
            4 => Self::Low,
            _ => Self::None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Urgent => "urgent",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::None => "none",
        }
    }
}

/// A pending task suggestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingTask {
    /// Task title (short, actionable).
    pub title: String,

    /// Detailed description.
    #[serde(default)]
    pub description: Option<String>,

    /// Priority level.
    #[serde(default)]
    pub priority: TaskPriority,

    /// Estimated effort (S/M/L/XL).
    #[serde(default)]
    pub effort: Option<String>,

    /// Related files in the codebase.
    #[serde(default)]
    pub related_files: Vec<String>,

    /// Tags/labels.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Plane.so issue ID (set after approval).
    #[serde(default)]
    pub plane_issue_id: Option<String>,

    /// When this was generated.
    #[serde(default = "default_timestamp")]
    pub created_at: String,
}

fn default_timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[derive(Debug, Deserialize)]
struct Suggestion {
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    priority: Option<u8>,
    #[serde(default)]
    effort: Option<String>,
    #[serde(default)]
    related_files: Option<Vec<String>>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

/// Generate task suggestions by analyzing the codebase using agentic exploration.
///
/// The LLM is given tools to explore the codebase (read_file, list_directory, glob, grep)
/// and will autonomously investigate before generating suggestions.
pub async fn generate_suggestions(project_path: &Path, lm_studio_url: &str) -> Result<Vec<PendingTask>> {
    generate_suggestions_with_callback(project_path, lm_studio_url, None).await
}

/// Generate task suggestions with an optional callback for exploration events.
pub async fn generate_suggestions_with_callback(
    project_path: &Path,
    lm_studio_url: &str,
    on_event: Option<ToolEventCallback>,
) -> Result<Vec<PendingTask>> {
    // Create LLM client pointing to LM Studio (OpenAI-compatible)
    let client = Client::openai_compatible(lm_studio_url)
        .context("Failed to create LLM client")?;

    // Create exploration tools
    let tools = create_exploration_tools(project_path);

    // Tool runner config
    let config = ToolRunnerConfig {
        max_iterations: Some(20),
        verbose: false,
        on_event,
    };

    let system_prompt = r#"You are Palace, an AI that explores software projects to understand them deeply before suggesting actions.

IMPORTANT: First use your tools to explore the project structure and read key files. Then provide your suggestions.

When exploring:
1. Read important config files (Cargo.toml, package.json, etc.)
2. Look at key source files to understand the architecture
3. Check for TODO comments or issues

After exploring, call the `suggest` tool with your suggestions."#;

    let user_prompt = "Explore this codebase and suggest what to work on next. \
Use your tools to understand the project before making suggestions.";

    let params = MessageCreateParams {
        model: "glm-4.7".to_string(),
        max_tokens: 8192,
        system: Some(SystemPrompt::Text(system_prompt.to_string())),
        messages: vec![MessageParam::user(user_prompt)],
        ..Default::default()
    };

    // Create a channel to capture suggestions when the suggest tool is called
    let suggestions_captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let suggestions_clone = suggestions_captured.clone();

    // Wrap the suggest tool to capture its input
    let original_tools = tools;
    let mut wrapped_tools: Vec<std::sync::Arc<dyn llm_code_sdk::tools::Tool>> = Vec::new();

    for tool in original_tools {
        if tool.name() == "suggest" {
            wrapped_tools.push(std::sync::Arc::new(SuggestCaptureTool {
                inner: tool,
                captured: suggestions_clone.clone(),
            }));
        } else {
            wrapped_tools.push(tool);
        }
    }

    let runner = ToolRunner::with_config(client, wrapped_tools, config);

    // Run the agentic loop
    let _final_message = runner.run(params).await
        .context("Agentic exploration failed")?;

    // Extract captured suggestions
    let captured = suggestions_captured.lock().unwrap();
    let mut tasks: Vec<PendingTask> = captured.iter().map(|s| PendingTask {
        title: s.title.clone(),
        description: s.description.clone(),
        priority: TaskPriority::from_u8(s.priority.unwrap_or(3)),
        effort: s.effort.clone(),
        related_files: s.related_files.clone().unwrap_or_default(),
        tags: s.tags.clone().unwrap_or_default(),
        plane_issue_id: None,
        created_at: default_timestamp(),
    }).collect();

    if tasks.is_empty() {
        warn!("No suggestions captured from suggest tool");
    }

    info!("Generated {} suggestions", tasks.len());
    Ok(tasks)
}

/// Wrapper tool that captures suggest inputs.
struct SuggestCaptureTool {
    inner: std::sync::Arc<dyn llm_code_sdk::tools::Tool>,
    captured: std::sync::Arc<std::sync::Mutex<Vec<Suggestion>>>,
}

#[async_trait::async_trait]
impl llm_code_sdk::tools::Tool for SuggestCaptureTool {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn to_param(&self) -> llm_code_sdk::types::ToolParam {
        self.inner.to_param()
    }

    async fn call(&self, input: std::collections::HashMap<String, serde_json::Value>) -> llm_code_sdk::tools::ToolResult {
        // Capture suggestions from input
        if let Some(suggestions) = input.get("suggestions") {
            if let Ok(parsed) = serde_json::from_value::<Vec<Suggestion>>(suggestions.clone()) {
                info!("Captured {} suggestions from suggest tool", parsed.len());
                self.captured.lock().unwrap().extend(parsed);
            }
        }
        self.inner.call(input).await
    }
}

/// Fallback: try to parse suggestions from text output.
fn parse_suggestions_from_text(text: &str) -> Result<Vec<PendingTask>> {
    // Try JSON first
    if let Some(start) = text.find('[') {
        if let Some(end) = text.rfind(']') {
            let json_str = &text[start..=end];
            if let Ok(suggestions) = serde_json::from_str::<Vec<Suggestion>>(json_str) {
                return Ok(suggestions.into_iter().map(|s| PendingTask {
                    title: s.title,
                    description: s.description,
                    priority: TaskPriority::from_u8(s.priority.unwrap_or(3)),
                    effort: s.effort,
                    related_files: s.related_files.unwrap_or_default(),
                    tags: s.tags.unwrap_or_default(),
                    plane_issue_id: None,
                    created_at: default_timestamp(),
                }).collect());
            }
        }
    }

    anyhow::bail!("Could not parse suggestions from text")
}
