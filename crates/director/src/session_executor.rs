//! Session executor - runs agents to complete session tasks.
//!
//! This module contains the logic that actually executes sessions,
//! spawning LLM agents to work on issues, modules, or cycles.
//!
//! ## Skills Support
//!
//! Sessions can have skills applied that specialize the agent:
//! - Base skills (coding best practices)
//! - Language skills (TypeScript, Rust, Vue.js)
//! - Project skills (Riff.CC, Flagship conventions)
//!
//! Skills are stacked in order (base → specialized).

use crate::{
    DirectorError, DirectorResult, Executor, ExecutorConfig,
    Session, SessionManager, SessionStatus, SessionStrategy, SessionTarget, SingleTarget,
    LogLevel, ZulipReporter, ZulipTool,
};
use llm_code_sdk::skills::{LocalSkill, SkillStack};
use llm_code_sdk::tools::{ToolEvent, ToolEventCallback};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Configuration for session execution.
#[derive(Debug, Clone)]
pub struct SessionExecutorConfig {
    /// LLM endpoint URL.
    pub llm_url: String,
    /// Model to use.
    pub model: String,
    /// Max tokens per request.
    pub max_tokens: u32,
    /// Plane.so workspace.
    pub workspace: String,
    /// Plane.so project.
    pub project: String,
    /// Enable Zulip reporting.
    pub zulip_enabled: bool,
    /// Zulip stream to report to.
    pub zulip_stream: String,
    /// Skills to load for this session (auto-detected or manual).
    pub skills: Vec<String>,
}

impl Default for SessionExecutorConfig {
    fn default() -> Self {
        Self {
            llm_url: "http://localhost:1234/v1".to_string(),
            model: "glm-4-plus".to_string(),
            max_tokens: 4096,
            workspace: "wings".to_string(),
            project: "PAL".to_string(),
            zulip_enabled: false,
            zulip_stream: "palace".to_string(),
            skills: Vec::new(),
        }
    }
}

/// Session executor - runs a session to completion.
pub struct SessionExecutor {
    config: SessionExecutorConfig,
    manager: Arc<SessionManager>,
    zulip: Option<ZulipReporter>,
    skill_stack: SkillStack,
}

impl SessionExecutor {
    /// Create a new session executor.
    pub fn new(config: SessionExecutorConfig, manager: Arc<SessionManager>) -> Self {
        let zulip = if config.zulip_enabled {
            ZulipReporter::from_env()
                .map(|r| r.with_stream(config.zulip_stream.clone()))
                .ok()
        } else {
            None
        };

        Self {
            config,
            manager,
            zulip,
            skill_stack: SkillStack::new(),
        }
    }

    /// Enable Zulip reporting with a custom reporter.
    pub fn with_zulip(mut self, reporter: ZulipReporter) -> Self {
        self.zulip = Some(reporter);
        self
    }

    /// Load skills for a session.
    /// Merges session skills with config skills (config skills take precedence).
    fn load_skills(&mut self, session: &Session) {
        self.skill_stack = SkillStack::new();

        // Collect all skills (session + config, deduped)
        let mut all_skills: Vec<String> = session.skills.clone();
        for skill in &self.config.skills {
            if !all_skills.contains(skill) {
                all_skills.push(skill.clone());
            }
        }

        for skill_path in &all_skills {
            // Try to load as a file path first
            let path = PathBuf::from(skill_path);
            if path.exists() {
                if let Err(e) = self.skill_stack.load(&path) {
                    tracing::warn!("Failed to load skill from {}: {}", skill_path, e);
                } else {
                    tracing::info!("Loaded skill from {}", skill_path);
                }
            } else {
                // Try to load from standard locations
                let locations = [
                    session.project_path.join(".palace/skills").join(format!("{}.md", skill_path)),
                    session.project_path.join(".claude/commands").join(format!("{}.md", skill_path)),
                    PathBuf::from(format!("{}/.claude/commands/{}.md", std::env::var("HOME").unwrap_or_default(), skill_path)),
                ];

                let mut found = false;
                for loc in &locations {
                    if loc.exists() {
                        if let Err(e) = self.skill_stack.load(loc) {
                            tracing::warn!("Failed to load skill from {:?}: {}", loc, e);
                        } else {
                            tracing::info!("Loaded skill '{}' from {:?}", skill_path, loc);
                            found = true;
                            break;
                        }
                    }
                }

                if !found {
                    // Create inline skill from name
                    tracing::info!("Creating inline skill reference: {}", skill_path);
                    self.skill_stack.push(LocalSkill::from_content(
                        skill_path,
                        format!("Apply {} specialist knowledge.", skill_path),
                    ));
                }
            }
        }

        if !self.skill_stack.is_empty() {
            tracing::info!("Loaded {} skills for session", self.skill_stack.len());
        }
    }

    /// Get the skill system prompt.
    fn skill_system_prompt(&self) -> Option<String> {
        if self.skill_stack.is_empty() {
            None
        } else {
            Some(self.skill_stack.to_system_prompt())
        }
    }

    /// Execute a session.
    pub async fn execute(&mut self, session_id: Uuid) -> DirectorResult<()> {
        let session = self.manager.get_session(session_id).await
            .ok_or_else(|| DirectorError::Other(format!("Session not found: {}", session_id)))?;

        tracing::info!("Starting session execution: {} ({})", session.name, session.short_id());

        // Load skills for this session
        self.load_skills(&session);
        if !self.skill_stack.is_empty() {
            self.manager.log(
                session_id,
                LogLevel::Info,
                &format!("Loaded {} skills: {:?}",
                    self.skill_stack.len(),
                    self.skill_stack.skills().iter().map(|s| &s.name).collect::<Vec<_>>()
                )
            ).await;
        }

        // Update status to running
        self.manager.update_status(session_id, SessionStatus::Running).await;
        self.manager.log(session_id, LogLevel::Info, "Session started").await;

        // Get working directory (worktree or project path)
        let work_dir = session.worktree_path.as_ref()
            .unwrap_or(&session.project_path)
            .clone();

        let session_name = session.name.clone();

        // Execute based on strategy
        let result = match session.strategy {
            SessionStrategy::Simple => {
                self.execute_simple(session_id, &session_name, &session.target, &work_dir).await
            }
            SessionStrategy::Parallel => {
                self.execute_parallel(session_id, &session_name, &session.target, &work_dir).await
            }
            SessionStrategy::Priority => {
                self.execute_priority(session_id, &session_name, &session.target, &work_dir).await
            }
            SessionStrategy::Director => {
                // Director sessions use Zulip event loop - they don't execute targets directly
                self.execute_director(session_id, &session_name).await
            }
        };

        // Update final status
        match &result {
            Ok(()) => {
                self.manager.update_status(session_id, SessionStatus::Completed).await;
                self.manager.log(session_id, LogLevel::Info, "Session completed successfully").await;

                // Report completion to Zulip
                if let Some(ref mut zulip) = self.zulip {
                    if let Err(e) = zulip.session_completed(session_id, &session.name, "All tasks completed successfully").await {
                        tracing::warn!("Failed to report session completion to Zulip: {}", e);
                    }
                }
            }
            Err(e) => {
                self.manager.log(session_id, LogLevel::Error, &format!("Session failed: {}", e)).await;
                // Update session with error
                if let Some(mut sess) = self.manager.get_session(session_id).await {
                    sess.fail(&e.to_string());
                }
                self.manager.update_status(session_id, SessionStatus::Failed).await;

                // Report failure to Zulip
                if let Some(ref mut zulip) = self.zulip {
                    if let Err(ze) = zulip.session_failed(session_id, &session.name, &e.to_string()).await {
                        tracing::warn!("Failed to report session failure to Zulip: {}", ze);
                    }
                }
            }
        }

        result
    }

    /// Simple strategy: Sequential execution.
    async fn execute_simple(
        &mut self,
        session_id: Uuid,
        session_name: &str,
        target: &SessionTarget,
        work_dir: &PathBuf,
    ) -> DirectorResult<()> {
        self.manager.log(session_id, LogLevel::Info, "Using simple (sequential) strategy").await;

        // Create executor for this session
        let executor_config = ExecutorConfig {
            project_path: work_dir.clone(),
            llm_url: self.config.llm_url.clone(),
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            dry_run: false,
        };

        let executor = Executor::new(executor_config);

        // Get all targets and execute sequentially
        let targets = target.targets();
        let total = targets.len();

        self.manager.log(session_id, LogLevel::Info, &format!("Executing {} target(s) sequentially", total)).await;

        // Report to Zulip
        if let Some(ref mut zulip) = self.zulip {
            let _ = zulip.session_progress(session_id, session_name, 0, total as u32, "Starting execution").await;
        }

        for (i, t) in targets.iter().enumerate() {
            self.manager.log(session_id, LogLevel::Info, &format!("Target {}/{}: {}", i + 1, total, t)).await;

            // Report progress to Zulip
            if let Some(ref mut zulip) = self.zulip {
                let _ = zulip.session_progress(session_id, session_name, i as u32, total as u32, &format!("{}", t)).await;
            }

            match t {
                SingleTarget::Issue(issue_id) => {
                    self.execute_issue(&executor, session_id, session_name, issue_id).await?;
                }
                SingleTarget::Module(module_id) => {
                    self.execute_module(&executor, session_id, session_name, module_id).await?;
                }
                SingleTarget::Cycle(cycle_id) => {
                    self.execute_cycle(&executor, session_id, session_name, cycle_id).await?;
                }
                SingleTarget::Goal(goal_id) => {
                    self.execute_goal(&executor, session_id, session_name, goal_id).await?;
                }
            }
        }

        Ok(())
    }

    /// Parallel strategy: Decompose and run concurrently.
    async fn execute_parallel(
        &mut self,
        session_id: Uuid,
        session_name: &str,
        target: &SessionTarget,
        work_dir: &PathBuf,
    ) -> DirectorResult<()> {
        self.manager.log(session_id, LogLevel::Info, "Using parallel strategy").await;
        self.manager.log(session_id, LogLevel::Warn, "Parallel strategy not yet implemented, falling back to simple").await;

        // TODO: Implement parallel decomposition
        // For now, fall back to simple
        self.execute_simple(session_id, session_name, target, work_dir).await
    }

    /// Priority strategy: Self-optimizing execution.
    async fn execute_priority(
        &mut self,
        session_id: Uuid,
        session_name: &str,
        target: &SessionTarget,
        work_dir: &PathBuf,
    ) -> DirectorResult<()> {
        self.manager.log(session_id, LogLevel::Info, "Using priority (Omniscience) strategy").await;
        self.manager.log(session_id, LogLevel::Warn, "Priority strategy not yet implemented, falling back to simple").await;

        // TODO: Implement priority/Omniscience strategy
        // For now, fall back to simple
        self.execute_simple(session_id, session_name, target, work_dir).await
    }

    /// Director strategy: Event-driven project management via Zulip.
    async fn execute_director(
        &mut self,
        session_id: Uuid,
        session_name: &str,
    ) -> DirectorResult<()> {
        self.manager.log(session_id, LogLevel::Info, "Using Director strategy (Zulip event-driven)").await;

        // Report to Zulip
        if let Some(ref mut zulip) = self.zulip {
            let _ = zulip.message(session_id, session_name,
                "📡 Director session active. Monitoring Zulip for @Director mentions."
            ).await;
        }

        // Director sessions run as event loops - they don't complete until stopped
        // The actual event handling is done by the ZulipReactor in the daemon
        self.manager.log(session_id, LogLevel::Info, "Director session started - running event loop").await;

        // For now, just mark as running and return
        // The daemon's ZulipReactor handles the actual event processing
        Ok(())
    }

    /// Execute a single issue.
    async fn execute_issue(
        &mut self,
        executor: &Executor,
        session_id: Uuid,
        session_name: &str,
        issue_id: &str,
    ) -> DirectorResult<()> {
        self.manager.log(session_id, LogLevel::Info, &format!("Working on issue: {}", issue_id)).await;
        self.manager.update_progress(session_id, 0, 1, &format!("Analyzing {}", issue_id)).await;

        // Fetch issue details from Plane.so
        let issue_details = self.fetch_issue_details(issue_id).await?;

        self.manager.log(session_id, LogLevel::Info, &format!("Issue: {}", issue_details.name)).await;

        // Report to Zulip
        if let Some(ref mut zulip) = self.zulip {
            let _ = zulip.message(
                session_id,
                session_name,
                &format!("📋 **{}**\n\n{}", issue_details.name, issue_details.description.as_deref().unwrap_or("_No description_")),
            ).await;
        }

        // Create a task description for the executor
        let skill_context = self.skill_system_prompt()
            .map(|s| format!("{}\n\n---\n\n", s))
            .unwrap_or_default();

        let task = format!(
            "{}Complete the following issue:\n\nTitle: {}\n\nDescription:\n{}\n\n\
            Work in the current directory. Make necessary code changes, run tests, and ensure the task is complete.",
            skill_context,
            issue_details.name,
            issue_details.description.unwrap_or_default()
        );

        self.manager.update_progress(session_id, 0, 1, &format!("Executing {}", issue_id)).await;

        // Create channel for tool events → Zulip streaming
        let (tx, mut rx) = mpsc::unbounded_channel::<ToolEvent>();

        // Spawn task to forward events to Zulip
        let stream = self.config.zulip_stream.clone();
        let topic = session_name.replace(":", "/");
        tokio::spawn(async move {
            let zulip = match ZulipTool::from_env_palace() {
                Ok(z) => z,
                Err(e) => {
                    tracing::warn!("Failed to create Zulip tool for streaming: {}", e);
                    return;
                }
            };

            // Track last tool call message ID for editing
            let mut last_tool_msg: Option<(String, u64, String)> = None; // (name, msg_id, params)

            while let Some(event) = rx.recv().await {
                match event {
                    ToolEvent::Text { text } => {
                        // Check if this is a thinking block
                        if text.contains("<think>") || text.contains("</think>") {
                            // Extract thinking content, strip tags and all whitespace
                            let thinking: String = text
                                .replace("<think>", "")
                                .replace("</think>", "")
                                .chars()
                                .filter(|c| !c.is_whitespace())
                                .collect();

                            // Only emit if there's actual content (not just whitespace/tags)
                            if !thinking.is_empty() {
                                // Use original with tags stripped but whitespace preserved for readability
                                let display = text
                                    .replace("<think>", "")
                                    .replace("</think>", "");
                                let display = display.trim();
                                let msg = format!("```spoiler 💭 Thinking\n{}\n```", display);
                                let _ = zulip.send(&stream, &topic, &msg).await;
                            }
                        } else {
                            // Normal text - emit as-is if non-empty
                            let text = text.trim();
                            if !text.is_empty() {
                                let _ = zulip.send(&stream, &topic, text).await;
                            }
                        }
                    }
                    ToolEvent::ToolCall { name, input } => {
                        // Send tool call with nice formatting
                        let (emoji, display_name) = format_tool_name(&name);
                        let params = format_tool_params(&name, &input);
                        let msg = format!("{} {}: {}", emoji, display_name, params);
                        if let Ok(msg_id) = zulip.send(&stream, &topic, &msg).await {
                            last_tool_msg = Some((name, msg_id, params));
                        }
                    }
                    ToolEvent::ToolResult { name, success, .. } => {
                        // Edit the tool call message to append result icon
                        if let Some((ref last_name, msg_id, ref params)) = last_tool_msg {
                            if &name == last_name {
                                let (emoji, display_name) = format_tool_name(&name);
                                let result = if success { "✅" } else { "❌" };
                                let updated = format!("{} {}: {} {}", emoji, display_name, params, result);
                                let _ = zulip.update_message(msg_id, &updated).await;
                            }
                        }
                        last_tool_msg = None;
                    }
                }
            }
        });

        // Create callback that sends to channel
        let callback: ToolEventCallback = Arc::new(move |event| {
            let _ = tx.send(event);
        });

        // Run the executor with callback
        let result = executor.run_task_with_callback(&task, callback).await;

        match result {
            Ok(output) => {
                self.manager.log(session_id, LogLevel::Info, &format!("Issue completed: {}", output)).await;
                self.manager.update_progress(session_id, 1, 1, "Complete").await;

                // Report completion to Zulip
                if let Some(ref mut zulip) = self.zulip {
                    let _ = zulip.message(session_id, session_name, &format!(
                        "✅ **Completed**\n\n{}",
                        output
                    )).await;
                }
                Ok(())
            }
            Err(e) => {
                self.manager.log(session_id, LogLevel::Error, &format!("Issue execution failed: {}", e)).await;

                // Report blocker to Zulip
                if let Some(ref mut zulip) = self.zulip {
                    let _ = zulip.blocker(
                        session_id,
                        session_name,
                        &format!("Execution failed: {}", e),
                        &["Retry with different approach", "Skip this issue", "Pause session for manual intervention"],
                    ).await;
                }
                Err(e)
            }
        }
    }

    /// Execute a module (all issues in the module).
    async fn execute_module(
        &mut self,
        executor: &Executor,
        session_id: Uuid,
        session_name: &str,
        module_id: &str,
    ) -> DirectorResult<()> {
        self.manager.log(session_id, LogLevel::Info, &format!("Working on module: {}", module_id)).await;

        // Fetch issues in this module
        let issues = self.fetch_module_issues(module_id).await?;
        let total = issues.len() as u32;

        self.manager.log(session_id, LogLevel::Info, &format!("Found {} issues in module", total)).await;

        // Report to Zulip
        if let Some(ref mut zulip) = self.zulip {
            let _ = zulip.message(session_id, session_name, &format!(
                "📦 Starting module `{}` with {} issues", module_id, total
            )).await;
        }

        for (i, issue) in issues.iter().enumerate() {
            self.manager.update_progress(session_id, i as u32, total, &format!("Issue {}/{}", i + 1, total)).await;
            self.execute_issue(executor, session_id, session_name, &issue.id).await?;
        }

        self.manager.update_progress(session_id, total, total, "Module complete").await;
        Ok(())
    }

    /// Execute a cycle (all issues in the cycle).
    async fn execute_cycle(
        &mut self,
        executor: &Executor,
        session_id: Uuid,
        session_name: &str,
        cycle_id: &str,
    ) -> DirectorResult<()> {
        self.manager.log(session_id, LogLevel::Info, &format!("Working on cycle: {}", cycle_id)).await;

        // Fetch issues in this cycle
        let issues = self.fetch_cycle_issues(cycle_id).await?;
        let total = issues.len() as u32;

        self.manager.log(session_id, LogLevel::Info, &format!("Found {} issues in cycle", total)).await;

        // Report to Zulip
        if let Some(ref mut zulip) = self.zulip {
            let _ = zulip.message(session_id, session_name, &format!(
                "🔄 Starting cycle `{}` with {} issues", cycle_id, total
            )).await;
        }

        for (i, issue) in issues.iter().enumerate() {
            self.manager.update_progress(session_id, i as u32, total, &format!("Issue {}/{}", i + 1, total)).await;
            self.execute_issue(executor, session_id, session_name, &issue.id).await?;
        }

        self.manager.update_progress(session_id, total, total, "Cycle complete").await;
        Ok(())
    }

    /// Execute a goal.
    async fn execute_goal(
        &mut self,
        _executor: &Executor,
        session_id: Uuid,
        session_name: &str,
        goal_id: &str,
    ) -> DirectorResult<()> {
        self.manager.log(session_id, LogLevel::Info, &format!("Working on goal: {}", goal_id)).await;
        self.manager.log(session_id, LogLevel::Warn, "Goal execution not yet implemented").await;

        // Report to Zulip
        if let Some(ref mut zulip) = self.zulip {
            let _ = zulip.blocker(
                session_id,
                session_name,
                &format!("Goal `{}` execution requires planning - not yet implemented", goal_id),
                &["Decompose goal into issues", "Skip goal", "Manual planning"],
            ).await;
        }

        // TODO: Implement goal execution
        // Goals require planning and decomposition first
        Err(DirectorError::Other("Goal execution not yet implemented".to_string()))
    }

    /// Fetch issue details from Plane.so.
    async fn fetch_issue_details(&self, issue_id: &str) -> DirectorResult<IssueInfo> {
        // Parse issue ID (e.g., "PAL-37" -> sequence 37)
        let sequence: u32 = if issue_id.contains('-') {
            issue_id.split('-').last()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0)
        } else {
            issue_id.parse().unwrap_or(0)
        };

        // Call Plane API
        let api_key = std::env::var("PLANE_API_KEY")
            .map_err(|_| DirectorError::PlaneApi("PLANE_API_KEY not set".to_string()))?;
        let api_url = std::env::var("PLANE_API_URL")
            .unwrap_or_else(|_| "https://api.plane.so/api/v1".to_string());

        let client = reqwest::Client::new();

        // First get project ID
        let projects_url = format!("{}/workspaces/{}/projects/", api_url, self.config.workspace);
        let projects_resp: serde_json::Value = client.get(&projects_url)
            .header("X-API-Key", &api_key)
            .send().await
            .map_err(|e| DirectorError::PlaneApi(e.to_string()))?
            .json().await
            .map_err(|e| DirectorError::PlaneApi(e.to_string()))?;

        let project_id = projects_resp["results"].as_array()
            .and_then(|arr| arr.iter().find(|p| {
                // Case-insensitive comparison for both identifier and name
                p["identifier"].as_str().map(|i| i.eq_ignore_ascii_case(&self.config.project)).unwrap_or(false) ||
                p["name"].as_str().map(|n| n.eq_ignore_ascii_case(&self.config.project)).unwrap_or(false)
            }))
            .and_then(|p| p["id"].as_str())
            .ok_or_else(|| DirectorError::PlaneApi(format!("Project {} not found", self.config.project)))?;

        // Fetch issues to find by sequence
        let issues_url = format!("{}/workspaces/{}/projects/{}/issues/", api_url, self.config.workspace, project_id);
        let issues_resp: serde_json::Value = client.get(&issues_url)
            .header("X-API-Key", &api_key)
            .send().await
            .map_err(|e| DirectorError::PlaneApi(e.to_string()))?
            .json().await
            .map_err(|e| DirectorError::PlaneApi(e.to_string()))?;

        let issue = issues_resp["results"].as_array()
            .and_then(|arr| arr.iter().find(|i| i["sequence_id"].as_u64() == Some(sequence as u64)))
            .ok_or_else(|| DirectorError::PlaneApi(format!("Issue {} not found", issue_id)))?;

        Ok(IssueInfo {
            id: issue["id"].as_str().unwrap_or("").to_string(),
            sequence_id: sequence,
            name: issue["name"].as_str().unwrap_or("").to_string(),
            description: issue["description_html"].as_str().map(|s| strip_html(s)),
        })
    }

    /// Fetch issues in a module.
    async fn fetch_module_issues(&self, _module_id: &str) -> DirectorResult<Vec<IssueInfo>> {
        // TODO: Implement module issue fetching
        Ok(Vec::new())
    }

    /// Fetch issues in a cycle.
    async fn fetch_cycle_issues(&self, _cycle_id: &str) -> DirectorResult<Vec<IssueInfo>> {
        // TODO: Implement cycle issue fetching
        Ok(Vec::new())
    }
}

/// Basic issue info for execution.
#[derive(Debug, Clone)]
struct IssueInfo {
    id: String,
    #[allow(dead_code)]
    sequence_id: u32,
    name: String,
    description: Option<String>,
}

/// Format tool name for human display with emoji.
fn format_tool_name(name: &str) -> (&'static str, String) {
    // Returns (emoji, display_name)
    match name {
        "read_file" => ("📖", "Read".to_string()),
        "write_file" => ("✏️", "Write".to_string()),
        "edit_file" => ("📝", "Edit".to_string()),
        "list_directory" => ("📁", "List".to_string()),
        "glob" => ("🔍", "Glob".to_string()),
        "grep" => ("🔎", "Grep".to_string()),
        "bash" => ("💻", "Bash".to_string()),
        "smart_read" => ("📚", "SmartRead".to_string()),
        "smart_write" => ("✍️", "SmartWrite".to_string()),
        "create_file" => ("📄", "Create".to_string()),
        "delete_file" => ("🗑️", "Delete".to_string()),
        "move_file" => ("📦", "Move".to_string()),
        "copy_file" => ("📋", "Copy".to_string()),
        "web_search" => ("🌐", "Search".to_string()),
        "web_fetch" => ("🌍", "Fetch".to_string()),
        _ => ("🔧", name.to_string()),
    }
}

/// Format tool params in a compact human-readable way.
fn format_tool_params(tool_name: &str, input: &std::collections::HashMap<String, serde_json::Value>) -> String {
    // Extract the most relevant param for each tool type
    match tool_name {
        "list_directory" | "read_file" | "write_file" | "glob" => {
            input.get("path").and_then(|v| v.as_str()).unwrap_or(".").to_string()
        }
        "grep" => {
            let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
            let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            format!("`{}` in {}", pattern, path)
        }
        "edit_file" => {
            let path = input.get("path").and_then(|v| v.as_str()).unwrap_or("");
            format!("{}", path)
        }
        "bash" => {
            input.get("command").and_then(|v| v.as_str()).unwrap_or("").to_string()
        }
        _ => {
            // Fallback: show all params compactly
            input.iter()
                .map(|(k, v)| {
                    let val = match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    format!("{}={}", k, val)
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    }
}

/// Strip HTML tags from text.
fn strip_html(html: &str) -> String {
    // Simple regex-free HTML stripping
    let mut result = String::new();
    let mut in_tag = false;

    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }

    // Clean up whitespace
    result
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // === format_tool_name Tests ===

    #[test]
    fn test_format_tool_name_read() {
        let (emoji, name) = format_tool_name("read_file");
        assert_eq!(emoji, "📖");
        assert_eq!(name, "Read");
    }

    #[test]
    fn test_format_tool_name_write() {
        let (emoji, name) = format_tool_name("write_file");
        assert_eq!(emoji, "✏️");
        assert_eq!(name, "Write");
    }

    #[test]
    fn test_format_tool_name_edit() {
        let (emoji, name) = format_tool_name("edit_file");
        assert_eq!(emoji, "📝");
        assert_eq!(name, "Edit");
    }

    #[test]
    fn test_format_tool_name_list() {
        let (emoji, name) = format_tool_name("list_directory");
        assert_eq!(emoji, "📁");
        assert_eq!(name, "List");
    }

    #[test]
    fn test_format_tool_name_glob() {
        let (emoji, name) = format_tool_name("glob");
        assert_eq!(emoji, "🔍");
        assert_eq!(name, "Glob");
    }

    #[test]
    fn test_format_tool_name_grep() {
        let (emoji, name) = format_tool_name("grep");
        assert_eq!(emoji, "🔎");
        assert_eq!(name, "Grep");
    }

    #[test]
    fn test_format_tool_name_bash() {
        let (emoji, name) = format_tool_name("bash");
        assert_eq!(emoji, "💻");
        assert_eq!(name, "Bash");
    }

    #[test]
    fn test_format_tool_name_unknown_returns_original() {
        let (emoji, name) = format_tool_name("some_random_tool");
        assert_eq!(emoji, "🔧");
        assert_eq!(name, "some_random_tool");
    }

    // === format_tool_params Tests ===

    #[test]
    fn test_format_tool_params_read_file() {
        let mut input = HashMap::new();
        input.insert("path".to_string(), serde_json::json!("/foo/bar.rs"));

        let result = format_tool_params("read_file", &input);
        assert_eq!(result, "/foo/bar.rs");
    }

    #[test]
    fn test_format_tool_params_grep() {
        let mut input = HashMap::new();
        input.insert("pattern".to_string(), serde_json::json!("TODO"));
        input.insert("path".to_string(), serde_json::json!("src/"));

        let result = format_tool_params("grep", &input);
        assert_eq!(result, "`TODO` in src/");
    }

    #[test]
    fn test_format_tool_params_bash() {
        let mut input = HashMap::new();
        input.insert("command".to_string(), serde_json::json!("cargo test"));

        let result = format_tool_params("bash", &input);
        assert_eq!(result, "cargo test");
    }

    #[test]
    fn test_format_tool_params_unknown_shows_all() {
        let mut input = HashMap::new();
        input.insert("foo".to_string(), serde_json::json!("bar"));

        let result = format_tool_params("unknown_tool", &input);
        assert!(result.contains("foo=bar"));
    }

    // === strip_html Tests ===

    #[test]
    fn test_strip_html_removes_tags() {
        let html = "<p>Hello</p>";
        assert_eq!(strip_html(html), "Hello");
    }

    #[test]
    fn test_strip_html_handles_nested_tags() {
        let html = "<div><p><strong>Bold</strong> text</p></div>";
        assert_eq!(strip_html(html), "Bold text");
    }

    #[test]
    fn test_strip_html_decodes_entities() {
        let html = "&amp; &lt; &gt; &quot; &nbsp;";
        let result = strip_html(html);
        assert!(result.contains("&"));
        assert!(result.contains("<"));
        assert!(result.contains(">"));
        assert!(result.contains("\""));
    }

    #[test]
    fn test_strip_html_removes_empty_lines() {
        let html = "<p>First</p>\n\n\n<p>Second</p>";
        let result = strip_html(html);
        assert_eq!(result, "First\nSecond");
    }

    #[test]
    fn test_strip_html_trims_whitespace() {
        let html = "  <p>  Hello  </p>  ";
        assert_eq!(strip_html(html), "Hello");
    }

    #[test]
    fn test_strip_html_plane_paragraph() {
        let html = r#"<p class="editor-paragraph-block">This is a task description.</p>"#;
        assert_eq!(strip_html(html), "This is a task description.");
    }

    #[test]
    fn test_strip_html_empty_input() {
        assert_eq!(strip_html(""), "");
    }

    #[test]
    fn test_strip_html_no_tags() {
        assert_eq!(strip_html("Just plain text"), "Just plain text");
    }
}
