//! Task types and generation.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use llm_code_sdk::client::Client;
use llm_code_sdk::tools::{ToolEventCallback, ToolRunner, ToolRunnerConfig};
use llm_code_sdk::types::{MessageCreateParams, MessageParam, SystemPrompt};
use tracing::{info, warn};

use crate::exploration::create_exploration_tools;
use crate::suggestions::parse_suggestions_from_text;

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

/// Relation type between tasks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationType {
    /// This task depends on another (must be done first)
    DependsOn,
    /// This task blocks another (must be done before it)
    Blocks,
    /// This task is related to another (informational)
    RelatedTo,
}

/// A relation to another task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRelation {
    /// Type of relation
    pub relation_type: RelationType,
    /// Index of the target task
    pub target_index: usize,
    /// Optional explanation
    #[serde(default)]
    pub reason: Option<String>,
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

    // === Enhanced fields (populated by progressive enhancement) ===

    /// Detailed plan steps to accomplish this task.
    #[serde(default)]
    pub plan: Option<Vec<String>>,

    /// Subtasks (smaller work items).
    #[serde(default)]
    pub subtasks: Option<Vec<String>>,

    /// Relations to other tasks in the list.
    #[serde(default)]
    pub relations: Option<Vec<TaskRelation>>,
}

pub fn default_timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Generate task suggestions by analyzing the codebase using agentic exploration.
///
/// The LLM is given tools to explore the codebase (read_file, list_directory, glob, grep)
/// and will autonomously investigate before generating suggestions as JSON.
///
/// Uses Z.ai API with glm-4.7 by default (requires ZAI_API_KEY env var).
/// Pass `lm_studio_url` to use LM Studio instead.
pub async fn generate_suggestions(project_path: &Path) -> Result<Vec<PendingTask>> {
    generate_suggestions_with_options(project_path, None, None, None).await
}

/// Generate task suggestions with optional callback, request count, and endpoint override.
///
/// - `on_event`: Optional callback for exploration events
/// - `request_up_to`: Optional guidance for how many tasks needed (not a constraint)
/// - `lm_studio_url`: Optional LM Studio URL (uses Z.ai API if None)
pub async fn generate_suggestions_with_options(
    project_path: &Path,
    on_event: Option<ToolEventCallback>,
    request_up_to: Option<usize>,
    lm_studio_url: Option<&str>,
) -> Result<Vec<PendingTask>> {
    // Create LLM client - Z.ai by default, LM Studio if URL provided
    let client = if let Some(url) = lm_studio_url {
        Client::openai_compatible(url)
            .context("Failed to create LM Studio client")?
    } else {
        let creds = crate::config::Credentials::load()
            .context("Failed to load credentials")?;
        let api_key = creds.zai_api_key()
            .context("Z.ai API key not found. Set ZAI_API_KEY env var or add to ~/.palace/credentials.json")?;
        Client::zai(&api_key)
            .context("Failed to create Z.ai client")?
    };

    // Create exploration tools (no suggest tool - model outputs JSON directly)
    let tools = create_exploration_tools(project_path);

    // Tool runner config - no max iterations, model decides when done
    let config = ToolRunnerConfig {
        max_iterations: None,
        verbose: false,
        on_event,
    };

    let project_root = project_path.display();

    let request_guidance = request_up_to
        .map(|n| format!("\n\nThe user has requested up to {} suggestions if you can find that many.", n))
        .unwrap_or_default();

    // Load JECJIT context if project is configured
    let jecjit_context = match crate::config::ProjectConfig::load(project_path) {
        Ok(config) => {
            let ctx = crate::jecjit::JecjitContext::new(config);

            // Load specs and fetch issues (best effort)
            ctx.load_specs(project_path);
            let _ = ctx.refresh().await;

            // Get spec gaps and all issues for context
            let gaps = ctx.spec_gaps();
            let all_issues = ctx.search(""); // Get all issues

            let mut context = String::new();

            if !all_issues.is_empty() {
                context.push_str("\n## Existing Issues (already tracked in Plane)\n");
                for issue in all_issues.iter().take(15) {
                    context.push_str(&format!("- [{}] {} ({})\n", issue.id, issue.name, issue.state));
                }
                if all_issues.len() > 15 {
                    context.push_str(&format!("  ... and {} more\n", all_issues.len() - 15));
                }
                context.push_str("\nDon't suggest work that's already tracked above.\n");
            }

            if !gaps.is_empty() {
                context.push_str("\n## Spec Gaps (from roadmap/spec files, NOT yet tracked)\n");
                for gap in gaps.iter().take(10) {
                    context.push_str(&format!("- {} (from {})\n", gap.description, gap.source));
                }
                if gaps.len() > 10 {
                    context.push_str(&format!("  ... and {} more\n", gaps.len() - 10));
                }
                context.push_str("\nConsider suggesting these gaps as high-priority work.\n");
            }

            context
        }
        Err(_) => String::new(),
    };

    let system_prompt = format!(r#"You are a code analyst. Explore this project and suggest what to work on next.

Project root: {project_root}
{jecjit_context}
## Instructions

1. Use your tools to explore the codebase:
   - Start with list_directory(".") to see the structure
   - Read config files (Cargo.toml, package.json, CLAUDE.md, README.md)
   - Read key source files to understand the architecture
   - Look for TODOs, FIXMEs, issues, or missing functionality

2. When you've gathered enough context, output your suggestions as a JSON array.

## Output Format

When ready, output ONLY a JSON array of suggestions (no other text):

```json
[
  {{"title": "Short actionable title", "description": "What to do and why"}},
  {{"title": "Another task", "description": "Details..."}}
]
```

Optional fields: "priority" (1-5), "effort" ("S"/"M"/"L"/"XL"), "related_files" (array), "tags" (array){request_guidance}"#);

    let user_prompt = "Explore this codebase and suggest what to work on next.";

    let params = MessageCreateParams {
        model: "glm-4.7".to_string(),
        max_tokens: 65536,
        system: Some(SystemPrompt::Text(system_prompt)),
        messages: vec![MessageParam::user(user_prompt)],
        ..Default::default()
    };

    let runner = ToolRunner::with_config(client, tools, config);

    // Run the agentic loop - model explores then outputs JSON
    let final_message = runner.run(params).await
        .context("Agentic exploration failed")?;

    // Parse suggestions from the model's final text output
    let final_text = final_message.text().unwrap_or_default();

    let tasks = match parse_suggestions_from_text(&final_text) {
        Ok(tasks) => tasks,
        Err(e) => {
            warn!("Failed to parse suggestions from output: {}", e);
            warn!("Final text was: {}", &final_text[..final_text.len().min(500)]);
            Vec::new()
        }
    };

    if tasks.is_empty() {
        warn!("No suggestions parsed from model output");
    } else {
        info!("Generated {} suggestions", tasks.len());
    }

    Ok(tasks)
}

/// Callback for enhancement events.
pub type EnhancementCallback = std::sync::Arc<dyn Fn(usize, &PendingTask) + Send + Sync>;

/// Enhance a single task with plan, subtasks, and relations.
///
/// Takes the full list of suggestions for context so the model can identify relations.
pub async fn enhance_suggestion(
    task_index: usize,
    all_suggestions: &[crate::suggestions::StoredSuggestion],
    lm_studio_url: Option<&str>,
) -> Result<PendingTask> {
    let suggestion = all_suggestions.iter()
        .find(|s| s.index == task_index)
        .context("Task not found")?;

    // Build context showing all suggestions
    let context = all_suggestions.iter()
        .map(|s| format!("{}. {} - {}",
            s.index,
            s.task.title,
            s.task.description.as_deref().unwrap_or("")))
        .collect::<Vec<_>>()
        .join("\n");

    // Create LLM client
    let client = if let Some(url) = lm_studio_url {
        Client::openai_compatible(url)
            .context("Failed to create LM Studio client")?
    } else {
        let creds = crate::config::Credentials::load()
            .context("Failed to load credentials")?;
        let api_key = creds.zai_api_key()
            .context("Z.ai API key not found")?;
        Client::zai(&api_key)
            .context("Failed to create Z.ai client")?
    };

    let system_prompt = format!(r#"You are a project planner. Enhance a task with implementation details.

## All Tasks in This Project
{context}

## Your Job
For task #{task_index}, provide:
1. A step-by-step plan (3-7 concrete steps)
2. Any subtasks if the work should be broken down further
3. Relations to other tasks (dependencies, blocks, related)

## Output Format
Return ONLY valid JSON:
```json
{{
  "plan": ["Step 1...", "Step 2...", ...],
  "subtasks": ["Subtask 1...", ...],
  "relations": [
    {{"relation_type": "depends_on", "target_index": N, "reason": "why"}},
    ...
  ]
}}
```

relation_type can be: depends_on, blocks, related_to
Only include relations that genuinely exist. Empty arrays are fine."#);

    let user_prompt = format!(
        "Enhance task #{}: {}\n\nDescription: {}",
        task_index,
        suggestion.task.title,
        suggestion.task.description.as_deref().unwrap_or("(none)")
    );

    let params = MessageCreateParams {
        model: "glm-4.7".to_string(),
        max_tokens: 2048,
        system: Some(SystemPrompt::Text(system_prompt)),
        messages: vec![MessageParam::user(&user_prompt)],
        response_format: Some(llm_code_sdk::types::ResponseFormat::json_object()),
        ..Default::default()
    };

    let response = client.messages().create(params).await
        .context("Enhancement request failed")?;

    let response_text = response.text().unwrap_or_default();

    // Parse the enhancement response
    #[derive(Deserialize)]
    struct EnhancementResponse {
        #[serde(default)]
        plan: Vec<String>,
        #[serde(default)]
        subtasks: Vec<String>,
        #[serde(default)]
        relations: Vec<TaskRelation>,
    }

    // Try to extract JSON from the response (might be wrapped in markdown)
    let json_str = if let Some(start) = response_text.find('{') {
        if let Some(end) = response_text.rfind('}') {
            &response_text[start..=end]
        } else {
            &response_text
        }
    } else {
        &response_text
    };

    let enhancement: EnhancementResponse = serde_json::from_str(json_str)
        .with_context(|| format!("Failed to parse enhancement response: {}", &json_str[..json_str.len().min(200)]))?;

    // Clone the original task and add enhancements
    let mut enhanced = suggestion.task.clone();
    enhanced.plan = if enhancement.plan.is_empty() { None } else { Some(enhancement.plan) };
    enhanced.subtasks = if enhancement.subtasks.is_empty() { None } else { Some(enhancement.subtasks) };
    enhanced.relations = if enhancement.relations.is_empty() { None } else { Some(enhancement.relations) };

    info!("Enhanced task #{}: {} plan steps, {} subtasks, {} relations",
        task_index,
        enhanced.plan.as_ref().map(|p| p.len()).unwrap_or(0),
        enhanced.subtasks.as_ref().map(|s| s.len()).unwrap_or(0),
        enhanced.relations.as_ref().map(|r| r.len()).unwrap_or(0));

    Ok(enhanced)
}

/// Progressively enhance all suggestions.
///
/// Calls the callback after each enhancement so callers can stream to Zulip.
pub async fn enhance_all_suggestions(
    project_path: &Path,
    on_enhanced: Option<EnhancementCallback>,
    lm_studio_url: Option<&str>,
) -> Result<Vec<crate::suggestions::StoredSuggestion>> {
    let suggestions = crate::suggestions::load_suggestions(project_path)?;

    if suggestions.is_empty() {
        return Ok(vec![]);
    }

    let mut enhanced_suggestions = Vec::new();

    for suggestion in &suggestions {
        match enhance_suggestion(suggestion.index, &suggestions, lm_studio_url).await {
            Ok(enhanced_task) => {
                let enhanced = crate::suggestions::StoredSuggestion {
                    index: suggestion.index,
                    task: enhanced_task.clone(),
                };

                // Call the callback so caller can stream to Zulip
                if let Some(ref callback) = on_enhanced {
                    callback(suggestion.index, &enhanced_task);
                }

                enhanced_suggestions.push(enhanced);
            }
            Err(e) => {
                warn!("Failed to enhance task #{}: {}", suggestion.index, e);
                // Keep the original unenhanced
                enhanced_suggestions.push(suggestion.clone());
            }
        }
    }

    // Update suggestions.json with enhanced versions
    let palace_dir = project_path.join(".palace");
    let suggestions_file = palace_dir.join("suggestions.json");
    let json = serde_json::to_string_pretty(&enhanced_suggestions)?;
    let temp_file = palace_dir.join(".suggestions.json.tmp");
    std::fs::write(&temp_file, &json)?;
    std::fs::rename(&temp_file, &suggestions_file)?;

    Ok(enhanced_suggestions)
}
