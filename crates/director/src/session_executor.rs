//! Session executor - runs agents to complete session tasks.
//!
//! This module contains the logic that actually executes sessions,
//! spawning LLM agents to work on issues, modules, or cycles.

use crate::{
    DirectorError, DirectorResult, Executor, ExecutorConfig,
    Session, SessionManager, SessionStatus, SessionStrategy, SessionTarget, SingleTarget,
    LogLevel, ZulipReporter,
};
use std::path::PathBuf;
use std::sync::Arc;
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
        }
    }
}

/// Session executor - runs a session to completion.
pub struct SessionExecutor {
    config: SessionExecutorConfig,
    manager: Arc<SessionManager>,
    zulip: Option<ZulipReporter>,
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

        Self { config, manager, zulip }
    }

    /// Enable Zulip reporting with a custom reporter.
    pub fn with_zulip(mut self, reporter: ZulipReporter) -> Self {
        self.zulip = Some(reporter);
        self
    }

    /// Execute a session.
    pub async fn execute(&mut self, session_id: Uuid) -> DirectorResult<()> {
        let session = self.manager.get_session(session_id).await
            .ok_or_else(|| DirectorError::Other(format!("Session not found: {}", session_id)))?;

        tracing::info!("Starting session execution: {} ({})", session.name, session.short_id());

        // Update status to running
        self.manager.update_status(session_id, SessionStatus::Running).await;
        self.manager.log(session_id, LogLevel::Info, "Session started").await;

        // Report to Zulip
        if let Some(ref mut zulip) = self.zulip {
            let target_str = format!("{:?}", session.target);
            if let Err(e) = zulip.session_started(session_id, &session.name, &target_str).await {
                tracing::warn!("Failed to report session start to Zulip: {}", e);
            }
        }

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
            let _ = zulip.tool_call(
                session_id,
                session_name,
                "fetch_issue",
                &format!("Fetched issue details for `{}`", issue_id),
            ).await;
        }

        // Create a task description for the executor
        let task = format!(
            "Complete the following issue:\n\nTitle: {}\n\nDescription:\n{}\n\n\
            Work in the current directory. Make necessary code changes, run tests, and ensure the task is complete.",
            issue_details.name,
            issue_details.description.unwrap_or_default()
        );

        self.manager.update_progress(session_id, 0, 1, &format!("Executing {}", issue_id)).await;

        // Report progress to Zulip
        if let Some(ref mut zulip) = self.zulip {
            let _ = zulip.session_progress(session_id, session_name, 0, 1, &format!("Executing {}", issue_id)).await;
        }

        // Run the executor
        let result = executor.run_task(&task).await;

        match result {
            Ok(output) => {
                self.manager.log(session_id, LogLevel::Info, &format!("Issue completed: {}", output)).await;
                self.manager.update_progress(session_id, 1, 1, "Complete").await;

                // Report completion to Zulip
                if let Some(ref mut zulip) = self.zulip {
                    let _ = zulip.message(session_id, session_name, &format!(
                        "✅ Issue `{}` completed\n\n{}",
                        issue_id,
                        if output.len() > 200 { format!("{}...", &output[..200]) } else { output }
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
                        &format!("Issue `{}` execution failed: {}", issue_id, e),
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
                p["identifier"].as_str() == Some(&self.config.project) ||
                p["name"].as_str().map(|n| n.to_lowercase()) == Some(self.config.project.to_lowercase())
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
            description: issue["description_html"].as_str().map(|s| {
                // Strip HTML tags for plain text
                s.replace("<p>", "").replace("</p>", "\n").replace("<br>", "\n")
            }),
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
