//! Executor: Wires Director plans to actual execution via Mountain/llm-code-sdk.
//!
//! This module bridges PlanStep actions to real tool execution:
//! - Implement → SmartWrite tool
//! - Test → Bash with test commands
//! - Build → Bash with build commands
//! - Research → SmartRead for codebase exploration
//! - CreateIssue → Plane API
//! - CreatePR → GitHub API
//! - HumanReview → Conductor escalation

use crate::planner::{PlanStep, StepAction, StepResult};
use crate::state::ProjectMetrics;
use crate::{DirectorError, DirectorResult};
use llm_code_sdk::Client;
use llm_code_sdk::tools::{Tool, ToolRunner, ToolRunnerConfig, ToolEventCallback};
use llm_code_sdk::tools::smart::SmartReadTool;
use llm_code_sdk::types::{MessageCreateParams, MessageParam};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Executor configuration.
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Project root path for tool execution
    pub project_path: PathBuf,
    /// LLM endpoint URL
    pub llm_url: String,
    /// API key (required for Z.ai, optional for local)
    pub api_key: Option<String>,
    /// Model to use for code generation
    pub model: String,
    /// Max tokens per step
    pub max_tokens: u32,
    /// Dry run mode (no actual changes)
    pub dry_run: bool,
    /// Use stock tools only (no SmartRead) for A/B comparison
    pub stock_tools: bool,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            project_path: PathBuf::from("."),
            llm_url: "http://localhost:1234/v1".to_string(),
            api_key: None,
            model: "GLM-4.7".to_string(),
            max_tokens: 65536,
            dry_run: false,
            stock_tools: false,
        }
    }
}

impl ExecutorConfig {
    /// Create an LLM client based on the URL format.
    /// - Z.ai / Anthropic URLs use Anthropic format
    /// - OpenRouter uses OpenAI format with API key
    /// - Other URLs use OpenAI-compatible format (no key)
    pub fn create_client(&self) -> Result<Client, String> {
        if self.llm_url.contains("openrouter.ai") {
            // OpenRouter - OpenAI format with API key
            let api_key = self.api_key.clone()
                .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
                .ok_or_else(|| "OPENROUTER_API_KEY required for OpenRouter".to_string())?;

            Client::builder(&api_key)
                .base_url(&self.llm_url)
                .format(llm_code_sdk::client::ApiFormat::OpenAI)
                .build()
                .map_err(|e| format!("{}", e))
        } else if self.llm_url.contains("z.ai") || self.llm_url.contains("anthropic") {
            // Anthropic-compatible format (Z.ai, Anthropic)
            let api_key = self.api_key.clone()
                .or_else(|| std::env::var("ZAI_API_KEY").ok())
                .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
                .ok_or_else(|| "ZAI_API_KEY or ANTHROPIC_API_KEY required".to_string())?;

            Client::builder(&api_key)
                .base_url(&self.llm_url)
                .build()
                .map_err(|e| format!("{}", e))
        } else {
            // OpenAI-compatible (LM Studio, Ollama, etc.) - no key required
            Client::openai_compatible(&self.llm_url)
                .map_err(|e| format!("{}", e))
        }
    }
}

/// The Executor bridges Director plans to real execution.
pub struct Executor {
    config: ExecutorConfig,
    metrics: Arc<RwLock<ProjectMetrics>>,
}

impl Executor {
    /// Create a new executor.
    pub fn new(config: ExecutorConfig) -> Self {
        Self {
            config,
            metrics: Arc::new(RwLock::new(ProjectMetrics::default())),
        }
    }

    /// Execute a single plan step.
    pub async fn execute(&self, step: &PlanStep) -> DirectorResult<StepResult> {
        let start = std::time::Instant::now();

        let result = match &step.action {
            StepAction::Implement { files, description } => {
                self.execute_implement(files, description).await
            }
            StepAction::Test { test_pattern } => {
                self.execute_test(test_pattern.as_deref()).await
            }
            StepAction::Build { release } => {
                self.execute_build(*release).await
            }
            StepAction::Research { topic } => {
                self.execute_research(topic).await
            }
            StepAction::CreateIssue { issue } => {
                self.execute_create_issue(issue).await
            }
            StepAction::CreatePR { title, branch } => {
                self.execute_create_pr(title, branch).await
            }
            StepAction::HumanReview { question } => {
                self.execute_human_review(question).await
            }
            StepAction::Deploy { environment } => {
                self.execute_deploy(environment).await
            }
            StepAction::Custom { action_type, params } => {
                self.execute_custom(action_type, params).await
            }
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(r) => {
                // Update metrics
                let mut metrics = self.metrics.write().await;
                if let Some(tokens) = r.outputs.get("tokens_used").and_then(|v| v.as_u64()) {
                    metrics.llm_tokens_used += tokens;
                }
                Ok(r.with_duration(duration_ms))
            }
            Err(e) => Err(e),
        }
    }

    /// Execute an implementation step using SmartWrite.
    async fn execute_implement(&self, files: &[String], description: &str) -> DirectorResult<StepResult> {
        if self.config.dry_run {
            return Ok(StepResult::success(format!("[DRY RUN] Would implement: {} in {:?}", description, files)));
        }

        // Create LLM client
        let client = self.config.create_client()
            .map_err(DirectorError::Execution)?;

        // Get editing tools
        let tools = llm_code_sdk::create_editing_tools(&self.config.project_path);

        // Create tool runner
        let mut runner = ToolRunner::new(client, tools);

        // Create implementation prompt
        let prompt = format!(
            "Implement the following in the codebase:\n\n{}\n\nFiles to modify: {:?}\n\nUse the available tools to read, analyze, and write the necessary changes.",
            description, files
        );

        // Run the tool loop
        let result = runner.run(MessageCreateParams {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            messages: vec![MessageParam::user(&prompt)],
            ..Default::default()
        }).await.map_err(|e| DirectorError::Execution(e.to_string()))?;

        let output = result.text().unwrap_or_default();
        Ok(StepResult::success(format!("Implementation complete: {}", output)))
    }

    /// Execute a test step.
    async fn execute_test(&self, test_pattern: Option<&str>) -> DirectorResult<StepResult> {
        let cmd = match test_pattern {
            Some(p) if !p.is_empty() => format!("cargo test {}", p),
            _ => "cargo test".to_string(),
        };

        self.run_bash(&cmd).await
    }

    /// Execute a build step.
    async fn execute_build(&self, release: bool) -> DirectorResult<StepResult> {
        let cmd = if release {
            "cargo build --release"
        } else {
            "cargo build"
        };

        self.run_bash(cmd).await
    }

    /// Execute a research step using SmartRead.
    async fn execute_research(&self, topic: &str) -> DirectorResult<StepResult> {
        // Create LLM client
        let client = self.config.create_client()
            .map_err(DirectorError::Execution)?;

        // Get exploration tools
        let tools = llm_code_sdk::create_exploration_tools(&self.config.project_path);

        // Create tool runner
        let mut runner = ToolRunner::new(client, tools);

        let prompt = format!(
            "Research the following topic in the codebase:\n\n{}\n\nUse the available tools to explore, read files, and gather information. Provide a summary of your findings.",
            topic
        );

        let result = runner.run(MessageCreateParams {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            messages: vec![MessageParam::user(&prompt)],
            ..Default::default()
        }).await.map_err(|e| DirectorError::Execution(e.to_string()))?;

        let output = result.text().unwrap_or_default();
        Ok(StepResult::success(format!("Research complete: {}", output)))
    }

    /// Execute issue creation via Plane API.
    async fn execute_create_issue(&self, issue: &crate::issues::Issue) -> DirectorResult<StepResult> {
        // Use pal call plane to create issue
        let cmd = format!(
            r#"./target/debug/pal call plane --input '{{"verb": "create", "type": "issue", "project": "PAL", "name": "{}", "priority": "{}"}}'"#,
            issue.title.replace('\'', "\\'"),
            format!("{:?}", issue.priority).to_lowercase()
        );

        self.run_bash(&cmd).await
    }

    /// Execute PR creation via GitHub CLI.
    async fn execute_create_pr(&self, title: &str, branch: &str) -> DirectorResult<StepResult> {
        let cmd = format!(
            "git checkout -b {} && git add . && git commit -m '{}' && gh pr create --title '{}' --body 'Auto-generated by Palace Director'",
            branch, title, title
        );

        self.run_bash(&cmd).await
    }

    /// Execute human review escalation.
    async fn execute_human_review(&self, question: &str) -> DirectorResult<StepResult> {
        // For now, just log and return - in future, integrate with Conductor
        tracing::warn!("Human review required: {}", question);

        Ok(StepResult::failure(format!("HUMAN REVIEW REQUIRED: {}", question)))
    }

    /// Execute deployment.
    async fn execute_deploy(&self, environment: &str) -> DirectorResult<StepResult> {
        tracing::warn!("Deploy to {} not implemented", environment);

        Ok(StepResult::failure(format!("Deploy to {} not implemented", environment)))
    }

    /// Execute custom action.
    async fn execute_custom(&self, action_type: &str, params: &serde_json::Value) -> DirectorResult<StepResult> {
        tracing::info!("Custom action: {} with params: {:?}", action_type, params);

        Ok(StepResult::success(format!("Custom action {} executed", action_type)))
    }

    /// Run a bash command.
    async fn run_bash(&self, cmd: &str) -> DirectorResult<StepResult> {
        if self.config.dry_run {
            return Ok(StepResult::success(format!("[DRY RUN] Would run: {}", cmd)));
        }

        let output = tokio::process::Command::new("bash")
            .arg("-c")
            .arg(cmd)
            .current_dir(&self.config.project_path)
            .output()
            .await
            .map_err(|e| DirectorError::Execution(e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(StepResult::success(stdout.to_string()))
        } else {
            Ok(StepResult::failure(format!("FAILED:\n{}\n{}", stdout, stderr)))
        }
    }

    /// Get current metrics.
    pub async fn metrics(&self) -> ProjectMetrics {
        self.metrics.read().await.clone()
    }

    /// Run an arbitrary task using the LLM agent.
    ///
    /// This is the main entry point for session execution - it takes a task
    /// description and uses the LLM to complete it using available tools.
    pub async fn run_task(&self, task: &str) -> DirectorResult<String> {
        if self.config.dry_run {
            return Ok(format!("[DRY RUN] Would execute task: {}", task));
        }

        tracing::info!("Running task: {}", &task[..task.len().min(100)]);

        // Create LLM client
        let client = self.config.create_client()
            .map_err(DirectorError::Execution)?;

        // Get all tools (exploration + editing + smart)
        let mut tools = llm_code_sdk::create_exploration_tools(&self.config.project_path);
        tools.extend(llm_code_sdk::create_editing_tools(&self.config.project_path));
        tools.push(Arc::new(SmartReadTool::new(&self.config.project_path)) as Arc<dyn Tool>);

        // Create tool runner
        let runner = ToolRunner::new(client, tools);

        // Create the task prompt - emphasize multi-layer smart_read
        let system = r#"You are a software development agent. Work FAST and MINIMAL.

CRITICAL: Use smart_read with MULTIPLE layers in ONE call:
  smart_read(path="file.py", layers=["ast", "call_graph"])

Layers: ast (structure), call_graph (relationships), dfg (data flow), raw (source)

Rules:
- ONE smart_read with multiple layers beats multiple calls
- edit_file for changes (not write_file)
- Do NOT run tests, pip install, or execute code
- Make MINIMAL changes only"#;

        let prompt = format!("{}\n\nTask:\n{}", system, task);

        // Run the tool loop
        let result = runner.run(MessageCreateParams {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            messages: vec![MessageParam::user(&prompt)],
            ..Default::default()
        }).await.map_err(|e| DirectorError::Execution(e.to_string()))?;

        // Extract the final response
        let output = result.text().unwrap_or_default().to_string();

        // Update metrics
        {
            let mut metrics = self.metrics.write().await;
            // Estimate tokens used (rough approximation)
            metrics.llm_tokens_used += (prompt.len() / 4 + output.len() / 4) as u64;
        }

        Ok(output)
    }

    /// Run a task with a callback for tool events.
    ///
    /// This allows real-time streaming of tool calls and results.
    pub async fn run_task_with_callback(
        &self,
        task: &str,
        callback: ToolEventCallback,
    ) -> DirectorResult<String> {
        if self.config.dry_run {
            return Ok(format!("[DRY RUN] Would execute task: {}", task));
        }

        tracing::info!("Running task with callback: {}", &task[..task.len().min(100)]);

        // Create LLM client
        let client = self.config.create_client()
            .map_err(DirectorError::Execution)?;

        // Get all tools (exploration + editing)
        let mut tools = llm_code_sdk::create_exploration_tools(&self.config.project_path);
        tools.extend(llm_code_sdk::create_editing_tools(&self.config.project_path));

        // Add SmartRead unless stock_tools mode
        if !self.config.stock_tools {
            tools.push(Arc::new(SmartReadTool::new(&self.config.project_path)) as Arc<dyn Tool>);
        }

        // Create tool runner with callback
        let config = ToolRunnerConfig {
            on_event: Some(callback),
            ..Default::default()
        };
        let runner = ToolRunner::with_config(client, tools, config);

        // Create the task prompt with system context
        let system = if self.config.stock_tools {
            // Stock tools prompt - basic SWE-agent style
            r#"You are a software development agent working on a codebase.

Available tools: read_file, edit_file, grep, glob, bash, list_directory

Workflow:
1. Explore the codebase to find relevant files
2. Read files to understand the code
3. Make targeted edits using edit_file
4. Verify changes

Work methodically. When complete, summarize what you did."#
        } else {
            // SmartRead-enhanced prompt
            r#"You are a software development agent working on a codebase.

IMPORTANT: You have access to smart_read, a powerful code analysis tool. USE IT:
- smart_read with layer="ast" shows code structure with signatures
- smart_read with query="..." searches git history for relevant commits - USE THIS instead of manual git log/show commands!
  Example: smart_read {"path": "migrations/file.py", "query": "permission fix"} finds commits about permission fixes

For understanding how something was fixed before, or finding related changes:
  smart_read {"path": "the/file.py", "query": "the issue or feature"}
This returns matching commits with their descriptions and changes - no need for manual git archaeology.

Workflow:
1. Read the relevant file to understand current state
2. If you need to understand history/prior fixes, use smart_read with query=
3. Make targeted edits using edit_file
4. Verify changes

Work methodically. When complete, summarize what you did."#
        };

        let prompt = format!("{}\n\nTask:\n{}", system, task);

        // Run the tool loop
        let result = runner.run(MessageCreateParams {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            messages: vec![MessageParam::user(&prompt)],
            ..Default::default()
        }).await.map_err(|e| DirectorError::Execution(e.to_string()))?;

        // Extract the final response
        let output = result.text().unwrap_or_default().to_string();

        // Update metrics
        {
            let mut metrics = self.metrics.write().await;
            metrics.llm_tokens_used += (prompt.len() / 4 + output.len() / 4) as u64;
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_executor_dry_run() {
        let config = ExecutorConfig {
            dry_run: true,
            ..Default::default()
        };
        let executor = Executor::new(config);

        let step = PlanStep::new("Test step", StepAction::Build { release: false });

        let result = executor.execute(&step).await.unwrap();
        assert!(result.success);
        assert!(result.message.contains("DRY RUN"));
    }
}
