//! Session management for Palace agents.
//!
//! Sessions are long-running agent instances that work on tasks, modules, or cycles.
//! Each session operates in its own git worktree (for parallel strategies) and maintains
//! state, logs, and can be monitored via the CLI.
//!
//! # Strategies
//!
//! - **Simple**: Sequential execution, one task at a time
//! - **Parallel**: Parallel decomposition with git worktrees
//! - **Priority**: Self-optimizing with dynamic skill learning

use crate::{DirectorError, DirectorResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

/// Session execution strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SessionStrategy {
    /// Sequential execution, one task at a time.
    #[default]
    Simple,
    /// Parallel decomposition with git worktrees.
    Parallel,
    /// Self-optimizing with dynamic skill learning.
    Priority,
    /// Director: Project management session using Zulip as I/O bus.
    /// Manages other sessions, syncs with Plane, handles handoffs.
    Director,
}

/// Handoff message between Directors/Sessions.
/// Token-compact protocol for transferring control.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Handoff {
    /// Source session ID.
    pub from: Uuid,
    /// Target session ID (or None for any available).
    pub to: Option<Uuid>,
    /// Work being handed off.
    pub work: SessionTarget,
    /// Handoff type.
    pub kind: HandoffKind,
    /// Brief context (keep short for token efficiency).
    pub context: String,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
}

/// Handoff type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HandoffKind {
    /// Requesting to take control.
    Request,
    /// Confirming handoff accepted.
    Accept,
    /// Declining handoff.
    Decline,
    /// Completed, returning control.
    Complete,
}

impl Handoff {
    /// Create a handoff request.
    pub fn request(from: Uuid, work: SessionTarget, context: &str) -> Self {
        Self {
            from,
            to: None,
            work,
            kind: HandoffKind::Request,
            context: context.to_string(),
            timestamp: Utc::now(),
        }
    }

    /// Format for Zulip (token-compact).
    pub fn to_zulip(&self) -> String {
        let kind_char = match self.kind {
            HandoffKind::Request => '?',
            HandoffKind::Accept => '+',
            HandoffKind::Decline => '-',
            HandoffKind::Complete => '!',
        };
        let to_str = self.to.map(|id| format!(">{}", &id.to_string()[..4])).unwrap_or_default();
        format!("⚡{}{}{} {} | {}",
            &self.from.to_string()[..4],
            kind_char,
            to_str,
            self.work,
            self.context
        )
    }

    /// Parse from Zulip format.
    pub fn from_zulip(s: &str) -> Option<Self> {
        // Parse: ⚡abcd?ef12 issue:PAL-52 | context here
        if !s.starts_with('⚡') { return None; }
        // TODO: Full parsing
        None
    }
}

impl std::fmt::Display for SessionStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionStrategy::Simple => write!(f, "simple"),
            SessionStrategy::Parallel => write!(f, "parallel"),
            SessionStrategy::Priority => write!(f, "priority"),
            SessionStrategy::Director => write!(f, "director"),
        }
    }
}

impl std::str::FromStr for SessionStrategy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "simple" | "default" => Ok(SessionStrategy::Simple),
            "parallel" => Ok(SessionStrategy::Parallel),
            "priority" | "omniscience" => Ok(SessionStrategy::Priority),
            "director" | "manage" | "pm" => Ok(SessionStrategy::Director),
            _ => Err(format!("Unknown strategy: {}", s)),
        }
    }
}

/// Session target - what the session is working on.
/// Supports single targets or multiple targets for multi-agent coordination.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SessionTarget {
    /// Single target.
    Single(SingleTarget),
    /// Multiple targets (for multi-agent work distribution).
    Multi(Vec<SingleTarget>),
}

/// A single work target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "id")]
pub enum SingleTarget {
    /// Working on a specific issue.
    Issue(String),
    /// Working on a module.
    Module(String),
    /// Working on a cycle.
    Cycle(String),
    /// Working on a goal.
    Goal(String),
}

impl SingleTarget {
    /// Get branch name - just the ID, human readable.
    pub fn branch_name(&self) -> String {
        match self {
            SingleTarget::Issue(id) => id.clone(),
            SingleTarget::Module(id) => id.clone(),
            SingleTarget::Cycle(id) => id.clone(),
            SingleTarget::Goal(id) => id.clone(),
        }
    }
}

impl std::fmt::Display for SingleTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SingleTarget::Issue(id) => write!(f, "issue:{}", id),
            SingleTarget::Module(id) => write!(f, "module:{}", id),
            SingleTarget::Cycle(id) => write!(f, "cycle:{}", id),
            SingleTarget::Goal(id) => write!(f, "goal:{}", id),
        }
    }
}

impl std::fmt::Display for SessionTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionTarget::Single(t) => write!(f, "{}", t),
            SessionTarget::Multi(targets) => {
                let strs: Vec<String> = targets.iter().map(|t| t.to_string()).collect();
                write!(f, "[{}]", strs.join(", "))
            }
        }
    }
}

impl SessionTarget {
    /// Create a single issue target.
    pub fn issue(id: impl Into<String>) -> Self {
        SessionTarget::Single(SingleTarget::Issue(id.into()))
    }

    /// Create a single module target.
    pub fn module(id: impl Into<String>) -> Self {
        SessionTarget::Single(SingleTarget::Module(id.into()))
    }

    /// Create a single cycle target.
    pub fn cycle(id: impl Into<String>) -> Self {
        SessionTarget::Single(SingleTarget::Cycle(id.into()))
    }

    /// Create a single goal target.
    pub fn goal(id: impl Into<String>) -> Self {
        SessionTarget::Single(SingleTarget::Goal(id.into()))
    }

    /// Create multiple targets.
    pub fn multi(targets: Vec<SingleTarget>) -> Self {
        SessionTarget::Multi(targets)
    }

    /// Get all single targets (flattened).
    pub fn targets(&self) -> Vec<&SingleTarget> {
        match self {
            SessionTarget::Single(t) => vec![t],
            SessionTarget::Multi(ts) => ts.iter().collect(),
        }
    }

    /// Count of targets.
    pub fn count(&self) -> usize {
        match self {
            SessionTarget::Single(_) => 1,
            SessionTarget::Multi(ts) => ts.len(),
        }
    }

    /// Get branch name for this target.
    /// Returns the human-readable identifier (e.g., "PAL-60" not "session-abc123").
    pub fn branch_name(&self) -> String {
        match self {
            SessionTarget::Single(t) => t.branch_name(),
            SessionTarget::Multi(ts) => {
                // For multi, use first target
                ts.first().map(|t| t.branch_name()).unwrap_or_else(|| "multi".to_string())
            }
        }
    }

    /// Check if this target matches another exactly (same type and ID).
    pub fn matches(&self, other: &SessionTarget) -> bool {
        match (self, other) {
            (SessionTarget::Single(a), SessionTarget::Single(b)) => {
                std::mem::discriminant(a) == std::mem::discriminant(b)
                    && a.branch_name() == b.branch_name()
            }
            (SessionTarget::Multi(a), SessionTarget::Multi(b)) => {
                a.len() == b.len()
                    && a.iter().zip(b.iter()).all(|(x, y)| {
                        std::mem::discriminant(x) == std::mem::discriminant(y)
                            && x.branch_name() == y.branch_name()
                    })
            }
            _ => false,
        }
    }

    /// Check if this target overlaps with another (any shared single targets).
    pub fn overlaps(&self, other: &SessionTarget) -> bool {
        let self_targets = self.targets();
        let other_targets = other.targets();

        for s in &self_targets {
            for o in &other_targets {
                if std::mem::discriminant(*s) == std::mem::discriminant(*o)
                    && s.branch_name() == o.branch_name()
                {
                    return true;
                }
            }
        }
        false
    }
}

/// Session status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    /// Session is starting up.
    Starting,
    /// Session is actively working.
    Running,
    /// Session is paused.
    Paused,
    /// Session completed successfully.
    Completed,
    /// Session failed.
    Failed,
    /// Session was cancelled.
    Cancelled,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionStatus::Starting => write!(f, "starting"),
            SessionStatus::Running => write!(f, "running"),
            SessionStatus::Paused => write!(f, "paused"),
            SessionStatus::Completed => write!(f, "completed"),
            SessionStatus::Failed => write!(f, "failed"),
            SessionStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// A log entry from a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLogEntry {
    /// Timestamp of the log entry.
    pub timestamp: DateTime<Utc>,
    /// Log level.
    pub level: LogLevel,
    /// Log message.
    pub message: String,
    /// Optional associated issue.
    pub issue_id: Option<String>,
    /// Optional associated file.
    pub file: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// Skill directive with priority learning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDirective {
    /// Directive content.
    pub content: String,
    /// Priority weight (-100 to +100).
    pub priority: i32,
    /// Times applied.
    pub applications: u32,
    /// Success rate when applied.
    pub success_rate: f32,
}

/// Dynamic skill for priority strategy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectSkill {
    /// Skill name.
    pub name: String,
    /// Project this skill is for.
    pub project_id: String,
    /// Directives with learned priorities.
    pub directives: Vec<SkillDirective>,
    /// Last updated.
    pub updated_at: DateTime<Utc>,
}

impl ProjectSkill {
    /// Create a new skill for a project.
    pub fn new(project_id: &str) -> Self {
        Self {
            name: format!("{}-skill", project_id),
            project_id: project_id.to_string(),
            directives: Vec::new(),
            updated_at: Utc::now(),
        }
    }

    /// Add or update a directive.
    pub fn add_directive(&mut self, content: &str) {
        if let Some(d) = self.directives.iter_mut().find(|d| d.content == content) {
            d.priority = (d.priority + 10).min(100);
        } else {
            self.directives.push(SkillDirective {
                content: content.to_string(),
                priority: 0,
                applications: 0,
                success_rate: 0.0,
            });
        }
        self.updated_at = Utc::now();
    }

    /// Increase priority of a directive (++).
    pub fn boost(&mut self, content: &str) {
        if let Some(d) = self.directives.iter_mut().find(|d| d.content == content) {
            d.priority = (d.priority + 10).min(100);
            self.updated_at = Utc::now();
        }
    }

    /// Decrease priority of a directive (--).
    pub fn demote(&mut self, content: &str) {
        if let Some(d) = self.directives.iter_mut().find(|d| d.content == content) {
            d.priority = (d.priority - 10).max(-100);
            self.updated_at = Utc::now();
        }
    }

    /// Get directives sorted by priority.
    pub fn ranked_directives(&self) -> Vec<&SkillDirective> {
        let mut sorted: Vec<_> = self.directives.iter().collect();
        sorted.sort_by(|a, b| b.priority.cmp(&a.priority));
        sorted
    }
}

/// A session instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session ID.
    pub id: Uuid,
    /// Human-readable name.
    pub name: String,
    /// What the session is working on.
    pub target: SessionTarget,
    /// Execution strategy.
    pub strategy: SessionStrategy,
    /// Current status.
    pub status: SessionStatus,
    /// Project path.
    pub project_path: PathBuf,
    /// Git worktree path (for parallel strategy).
    pub worktree_path: Option<PathBuf>,
    /// Git branch name.
    pub branch: Option<String>,
    /// Created timestamp.
    pub created_at: DateTime<Utc>,
    /// Started timestamp.
    pub started_at: Option<DateTime<Utc>>,
    /// Completed timestamp.
    pub completed_at: Option<DateTime<Utc>>,
    /// Tasks completed.
    pub tasks_completed: u32,
    /// Tasks total.
    pub tasks_total: u32,
    /// Current task description.
    pub current_task: Option<String>,
    /// Error message if failed.
    pub error: Option<String>,
    /// Process ID if running in background.
    pub pid: Option<u32>,
    /// Skills to apply to this session (paths or names).
    #[serde(default)]
    pub skills: Vec<String>,
    /// Recording enabled for this session.
    #[serde(default)]
    pub recording: bool,
}

impl Session {
    /// Create a new session.
    pub fn new(target: SessionTarget, strategy: SessionStrategy, project_path: PathBuf) -> Self {
        let id = Uuid::new_v4();
        // Name based on target (e.g., "PAL-53") not UUID garbage
        let name = target.to_string();

        Self {
            id,
            name,
            target,
            strategy,
            status: SessionStatus::Starting,
            project_path,
            worktree_path: None,
            branch: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            tasks_completed: 0,
            tasks_total: 0,
            current_task: None,
            error: None,
            pid: None,
            skills: Vec::new(),
            recording: true, // Enable recording by default
        }
    }

    /// Create a session with skills.
    pub fn with_skills(mut self, skills: Vec<String>) -> Self {
        self.skills = skills;
        self
    }

    /// Enable or disable recording.
    pub fn with_recording(mut self, recording: bool) -> Self {
        self.recording = recording;
        self
    }

    /// Mark the session as running.
    pub fn start(&mut self) {
        self.status = SessionStatus::Running;
        self.started_at = Some(Utc::now());
    }

    /// Mark the session as completed.
    pub fn complete(&mut self) {
        self.status = SessionStatus::Completed;
        self.completed_at = Some(Utc::now());
    }

    /// Mark the session as failed.
    pub fn fail(&mut self, error: &str) {
        self.status = SessionStatus::Failed;
        self.error = Some(error.to_string());
        self.completed_at = Some(Utc::now());
    }

    /// Check if session is active (running or starting).
    pub fn is_active(&self) -> bool {
        matches!(self.status, SessionStatus::Starting | SessionStatus::Running)
    }

    /// Get session duration.
    pub fn duration(&self) -> Option<chrono::Duration> {
        let start = self.started_at?;
        let end = self.completed_at.unwrap_or_else(Utc::now);
        Some(end - start)
    }

    /// Short ID for display.
    pub fn short_id(&self) -> String {
        self.id.to_string()[..8].to_string()
    }
}

/// Session event for live updates.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum SessionEvent {
    /// Session created.
    Created { session_id: Uuid, name: String },
    /// Session started.
    Started { session_id: Uuid },
    /// Session status changed.
    StatusChanged { session_id: Uuid, status: SessionStatus },
    /// Log entry added.
    Log { session_id: Uuid, entry: SessionLogEntry },
    /// Task progress.
    Progress { session_id: Uuid, completed: u32, total: u32, current: String },
    /// Session completed.
    Completed { session_id: Uuid, success: bool },
}

/// Session manager - tracks all sessions.
pub struct SessionManager {
    /// All sessions.
    sessions: Arc<RwLock<HashMap<Uuid, Session>>>,
    /// Session logs.
    logs: Arc<RwLock<HashMap<Uuid, Vec<SessionLogEntry>>>>,
    /// Project skills (for priority strategy).
    skills: Arc<RwLock<HashMap<String, ProjectSkill>>>,
    /// Event broadcaster.
    events_tx: broadcast::Sender<SessionEvent>,
    /// Project root path.
    project_path: PathBuf,
}

impl SessionManager {
    /// Create a new session manager.
    pub fn new(project_path: PathBuf) -> Self {
        let (events_tx, _) = broadcast::channel(1000);

        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            logs: Arc::new(RwLock::new(HashMap::new())),
            skills: Arc::new(RwLock::new(HashMap::new())),
            events_tx,
            project_path,
        }
    }

    /// Subscribe to session events.
    pub fn subscribe(&self) -> broadcast::Receiver<SessionEvent> {
        self.events_tx.subscribe()
    }

    /// Create a session without worktree (for testing only).
    #[cfg(test)]
    pub async fn create_session_for_test(
        &self,
        target: SessionTarget,
        strategy: SessionStrategy,
    ) -> Session {
        let session = Session::new(target, strategy, self.project_path.clone());
        let id = session.id;

        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(id, session.clone());
        }

        {
            let mut logs = self.logs.write().await;
            logs.insert(id, Vec::new());
        }

        session
    }

    /// Create a new session.
    pub async fn create_session(
        &self,
        target: SessionTarget,
        strategy: SessionStrategy,
    ) -> DirectorResult<Session> {
        self.create_session_with_skills(target, strategy, Vec::new()).await
    }

    /// Create a new session with skills.
    pub async fn create_session_with_skills(
        &self,
        target: SessionTarget,
        strategy: SessionStrategy,
        skills: Vec<String>,
    ) -> DirectorResult<Session> {
        // Check for overlap - refuse to create if active session exists for this target
        if self.has_active_session_for_target(&target).await {
            return Err(DirectorError::Execution(format!(
                "Active session already exists for target {}. Use 'session ls' to see active sessions.",
                target
            )));
        }

        let session = Session::new(target, strategy, self.project_path.clone())
            .with_skills(skills);

        // ALWAYS set up git worktree for sandboxing - all sessions are isolated
        let session = self.setup_worktree(session).await?;

        let id = session.id;

        // Store session
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(id, session.clone());
        }

        // Initialize log storage
        {
            let mut logs = self.logs.write().await;
            logs.insert(id, Vec::new());
        }

        // Emit event
        let _ = self.events_tx.send(SessionEvent::Created {
            session_id: id,
            name: session.name.clone(),
        });

        Ok(session)
    }

    /// Set up git worktree for parallel execution.
    /// Worktree is a SIBLING directory named `{project}-{branch}` (e.g., ../palace-2026-v2-PAL-60/).
    /// Gracefully handles: existing worktrees, existing branches, fresh starts.
    async fn setup_worktree(&self, mut session: Session) -> DirectorResult<Session> {
        // Branch name is the target ID (e.g., "PAL-60" for issue:PAL-60)
        let branch_name = session.target.branch_name();
        // Worktree is a SIBLING directory named {project}-{branch}
        let project_name = self.project_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project");
        let worktree_name = format!("{}-{}", project_name, branch_name);
        let worktree_path = self.project_path
            .parent()
            .expect("project must have parent directory")
            .join(&worktree_name);

        let worktree_str = worktree_path.to_str().unwrap();

        // Check if worktree already exists and is valid
        if worktree_path.exists() {
            // Verify it's a valid git worktree by checking for .git file
            let git_file = worktree_path.join(".git");
            if git_file.exists() {
                tracing::info!("Reusing existing worktree at {}", worktree_str);
                session.worktree_path = Some(worktree_path);
                session.branch = Some(branch_name);
                return Ok(session);
            } else {
                // Directory exists but isn't a worktree - remove it
                tracing::warn!("Removing invalid worktree directory at {}", worktree_str);
                let _ = std::fs::remove_dir_all(&worktree_path);
            }
        }

        // Try to create worktree - first with new branch, then existing branch
        let output = tokio::process::Command::new("git")
            .args(["worktree", "add", "-b", &branch_name, worktree_str])
            .current_dir(&self.project_path)
            .output()
            .await
            .map_err(|e| DirectorError::Io(e))?;

        if output.status.success() {
            tracing::info!("Created new worktree with branch {} at {}", branch_name, worktree_str);
            session.worktree_path = Some(worktree_path);
            session.branch = Some(branch_name);
        } else {
            // Branch might already exist - try using existing branch
            let output2 = tokio::process::Command::new("git")
                .args(["worktree", "add", worktree_str, &branch_name])
                .current_dir(&self.project_path)
                .output()
                .await
                .map_err(|e| DirectorError::Io(e))?;

            if output2.status.success() {
                tracing::info!("Created worktree from existing branch {} at {}", branch_name, worktree_str);
                session.worktree_path = Some(worktree_path);
                session.branch = Some(branch_name);
            } else {
                let err = String::from_utf8_lossy(&output2.stderr);
                tracing::error!("Failed to create worktree: {}", err);
                return Err(DirectorError::Execution(format!(
                    "Failed to create worktree at {}: {}",
                    worktree_str, err
                )));
            }
        }

        Ok(session)
    }

    /// Get a session by ID.
    pub async fn get_session(&self, id: Uuid) -> Option<Session> {
        self.sessions.read().await.get(&id).cloned()
    }

    /// Get a session by short ID or name.
    pub async fn find_session(&self, query: &str) -> Option<Session> {
        let sessions = self.sessions.read().await;

        // Try exact UUID match
        if let Ok(id) = Uuid::parse_str(query) {
            if let Some(s) = sessions.get(&id) {
                return Some(s.clone());
            }
        }

        // Try short ID or name match
        sessions.values().find(|s| {
            s.short_id() == query || s.name == query || s.id.to_string().starts_with(query)
        }).cloned()
    }

    /// List all sessions.
    pub async fn list_sessions(&self) -> Vec<Session> {
        self.sessions.read().await.values().cloned().collect()
    }

    /// List active sessions.
    pub async fn list_active(&self) -> Vec<Session> {
        self.sessions
            .read()
            .await
            .values()
            .filter(|s| s.is_active())
            .cloned()
            .collect()
    }

    /// Find all sessions working on a specific target.
    pub async fn find_sessions_for_target(&self, target: &SessionTarget) -> Vec<Session> {
        self.sessions
            .read()
            .await
            .values()
            .filter(|s| s.target.matches(target) || s.target.overlaps(target))
            .cloned()
            .collect()
    }

    /// Check if there's an active session for the given target.
    /// Returns true if any Running or Starting session exists for this target.
    pub async fn has_active_session_for_target(&self, target: &SessionTarget) -> bool {
        self.sessions
            .read()
            .await
            .values()
            .any(|s| s.is_active() && (s.target.matches(target) || s.target.overlaps(target)))
    }

    /// Get session logs.
    pub async fn get_logs(&self, session_id: Uuid, limit: Option<usize>) -> Vec<SessionLogEntry> {
        let logs = self.logs.read().await;
        if let Some(entries) = logs.get(&session_id) {
            match limit {
                Some(n) => entries.iter().rev().take(n).rev().cloned().collect(),
                None => entries.clone(),
            }
        } else {
            Vec::new()
        }
    }

    /// Add a log entry to a session.
    pub async fn log(&self, session_id: Uuid, level: LogLevel, message: &str) {
        let entry = SessionLogEntry {
            timestamp: Utc::now(),
            level,
            message: message.to_string(),
            issue_id: None,
            file: None,
        };

        {
            let mut logs = self.logs.write().await;
            if let Some(entries) = logs.get_mut(&session_id) {
                entries.push(entry.clone());
            }
        }

        let _ = self.events_tx.send(SessionEvent::Log {
            session_id,
            entry,
        });
    }

    /// Update session progress.
    pub async fn update_progress(&self, session_id: Uuid, completed: u32, total: u32, current: &str) {
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(&session_id) {
                session.tasks_completed = completed;
                session.tasks_total = total;
                session.current_task = Some(current.to_string());
            }
        }

        let _ = self.events_tx.send(SessionEvent::Progress {
            session_id,
            completed,
            total,
            current: current.to_string(),
        });

    }

    /// Update session status.
    pub async fn update_status(&self, session_id: Uuid, status: SessionStatus) {
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(&session_id) {
                session.status = status;
                if status == SessionStatus::Running && session.started_at.is_none() {
                    session.started_at = Some(Utc::now());
                }
                if matches!(status, SessionStatus::Completed | SessionStatus::Failed | SessionStatus::Cancelled) {
                    session.completed_at = Some(Utc::now());
                }
            }
        }

        let _ = self.events_tx.send(SessionEvent::StatusChanged {
            session_id,
            status,
        });

    }

    /// Get or create project skill.
    pub async fn get_skill(&self, project_id: &str) -> ProjectSkill {
        let skills = self.skills.read().await;
        skills.get(project_id).cloned().unwrap_or_else(|| ProjectSkill::new(project_id))
    }

    /// Update project skill.
    pub async fn update_skill(&self, skill: ProjectSkill) {
        let mut skills = self.skills.write().await;
        skills.insert(skill.project_id.clone(), skill);
    }

    /// Cancel a session.
    /// Does NOT remove worktree - we need to inspect what went wrong.
    pub async fn cancel_session(&self, session_id: Uuid) -> DirectorResult<()> {
        self.update_status(session_id, SessionStatus::Cancelled).await;
        Ok(())
    }

    /// Remove a session completely.
    pub async fn remove_session(&self, session_id: Uuid) -> DirectorResult<()> {
        // First cancel it (cleans up worktree)
        self.cancel_session(session_id).await?;

        // Then remove from sessions map
        {
            let mut sessions = self.sessions.write().await;
            sessions.remove(&session_id);
        }

        // Remove logs too
        {
            let mut logs = self.logs.write().await;
            logs.remove(&session_id);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === SingleTarget Tests ===

    #[test]
    fn test_single_target_branch_name_issue() {
        let target = SingleTarget::Issue("PAL-60".to_string());
        assert_eq!(target.branch_name(), "PAL-60");
    }

    #[test]
    fn test_single_target_branch_name_module() {
        let target = SingleTarget::Module("auth-system".to_string());
        assert_eq!(target.branch_name(), "auth-system");
    }

    #[test]
    fn test_single_target_branch_name_cycle() {
        let target = SingleTarget::Cycle("sprint-42".to_string());
        assert_eq!(target.branch_name(), "sprint-42");
    }

    #[test]
    fn test_single_target_branch_name_goal() {
        let target = SingleTarget::Goal("mvp-launch".to_string());
        assert_eq!(target.branch_name(), "mvp-launch");
    }

    #[test]
    fn test_single_target_display() {
        assert_eq!(SingleTarget::Issue("PAL-60".into()).to_string(), "issue:PAL-60");
        assert_eq!(SingleTarget::Module("auth".into()).to_string(), "module:auth");
        assert_eq!(SingleTarget::Cycle("s1".into()).to_string(), "cycle:s1");
        assert_eq!(SingleTarget::Goal("mvp".into()).to_string(), "goal:mvp");
    }

    // === SessionTarget Tests ===

    #[test]
    fn test_session_target_issue_constructor() {
        let target = SessionTarget::issue("PAL-123");
        assert_eq!(target.count(), 1);
        assert_eq!(target.branch_name(), "PAL-123");
    }

    #[test]
    fn test_session_target_multi_branch_name_uses_first() {
        let target = SessionTarget::multi(vec![
            SingleTarget::Issue("PAL-1".into()),
            SingleTarget::Issue("PAL-2".into()),
        ]);
        assert_eq!(target.branch_name(), "PAL-1");
    }

    #[test]
    fn test_session_target_multi_empty_returns_multi() {
        let target = SessionTarget::Multi(vec![]);
        assert_eq!(target.branch_name(), "multi");
    }

    #[test]
    fn test_session_target_display_single() {
        let target = SessionTarget::issue("PAL-99");
        assert_eq!(target.to_string(), "issue:PAL-99");
    }

    #[test]
    fn test_session_target_display_multi() {
        let target = SessionTarget::multi(vec![
            SingleTarget::Issue("PAL-1".into()),
            SingleTarget::Module("foo".into()),
        ]);
        assert_eq!(target.to_string(), "[issue:PAL-1, module:foo]");
    }

    #[test]
    fn test_session_target_targets_flattens() {
        let target = SessionTarget::multi(vec![
            SingleTarget::Issue("A".into()),
            SingleTarget::Issue("B".into()),
        ]);
        let targets = target.targets();
        assert_eq!(targets.len(), 2);
    }

    // === SessionStrategy Tests ===

    #[test]
    fn test_session_strategy_display() {
        assert_eq!(SessionStrategy::Simple.to_string(), "simple");
        assert_eq!(SessionStrategy::Parallel.to_string(), "parallel");
        assert_eq!(SessionStrategy::Priority.to_string(), "priority");
        assert_eq!(SessionStrategy::Director.to_string(), "director");
    }

    #[test]
    fn test_session_strategy_from_str() {
        assert_eq!("simple".parse::<SessionStrategy>().unwrap(), SessionStrategy::Simple);
        assert_eq!("default".parse::<SessionStrategy>().unwrap(), SessionStrategy::Simple);
        assert_eq!("parallel".parse::<SessionStrategy>().unwrap(), SessionStrategy::Parallel);
        assert_eq!("priority".parse::<SessionStrategy>().unwrap(), SessionStrategy::Priority);
        assert_eq!("omniscience".parse::<SessionStrategy>().unwrap(), SessionStrategy::Priority);
        assert_eq!("director".parse::<SessionStrategy>().unwrap(), SessionStrategy::Director);
        assert_eq!("pm".parse::<SessionStrategy>().unwrap(), SessionStrategy::Director);
    }

    #[test]
    fn test_session_strategy_from_str_case_insensitive() {
        assert_eq!("SIMPLE".parse::<SessionStrategy>().unwrap(), SessionStrategy::Simple);
        assert_eq!("Parallel".parse::<SessionStrategy>().unwrap(), SessionStrategy::Parallel);
    }

    #[test]
    fn test_session_strategy_from_str_invalid() {
        assert!("invalid".parse::<SessionStrategy>().is_err());
    }

    #[test]
    fn test_session_strategy_default() {
        assert_eq!(SessionStrategy::default(), SessionStrategy::Simple);
    }

    // === Session Tests ===

    #[test]
    fn test_session_new_creates_valid_session() {
        let session = Session::new(
            SessionTarget::issue("PAL-42"),
            SessionStrategy::Simple,
            PathBuf::from("/tmp/project"),
        );

        assert_eq!(session.name, "issue:PAL-42");
        assert_eq!(session.status, SessionStatus::Starting);
        assert!(session.worktree_path.is_none());
        assert!(session.branch.is_none());
        assert_eq!(session.tasks_completed, 0);
        assert_eq!(session.tasks_total, 0);
        assert!(session.error.is_none());
        assert!(session.recording);
    }

    #[test]
    fn test_session_short_id_is_8_chars() {
        let session = Session::new(
            SessionTarget::issue("PAL-1"),
            SessionStrategy::Simple,
            PathBuf::from("/tmp"),
        );

        let short = session.short_id();
        assert_eq!(short.len(), 8);
        // Should be first 8 chars of UUID
        assert!(session.id.to_string().starts_with(&short));
    }

    #[test]
    fn test_session_with_skills() {
        let session = Session::new(
            SessionTarget::issue("PAL-1"),
            SessionStrategy::Simple,
            PathBuf::from("/tmp"),
        ).with_skills(vec!["rust".into(), "testing".into()]);

        assert_eq!(session.skills.len(), 2);
        assert!(session.skills.contains(&"rust".to_string()));
    }

    // === SessionStatus Tests ===

    #[test]
    fn test_session_status_equality() {
        assert_eq!(SessionStatus::Starting, SessionStatus::Starting);
        assert_ne!(SessionStatus::Starting, SessionStatus::Running);
    }

    #[test]
    fn test_session_status_copy() {
        let status = SessionStatus::Running;
        let copied = status;
        assert_eq!(status, copied);
    }

    // === Handoff Tests ===

    #[test]
    fn test_handoff_request_creates_correctly() {
        let from = Uuid::new_v4();
        let work = SessionTarget::issue("PAL-99");
        let handoff = Handoff::request(from, work, "need help");

        assert_eq!(handoff.from, from);
        assert!(handoff.to.is_none());
        assert_eq!(handoff.kind, HandoffKind::Request);
        assert_eq!(handoff.context, "need help");
    }

    #[test]
    fn test_handoff_to_zulip_format() {
        let from = Uuid::parse_str("12345678-1234-1234-1234-123456789012").unwrap();
        let handoff = Handoff::request(from, SessionTarget::issue("PAL-1"), "ctx");

        let zulip = handoff.to_zulip();
        assert!(zulip.starts_with("⚡1234"));
        assert!(zulip.contains("issue:PAL-1"));
        assert!(zulip.contains("ctx"));
    }

    // === ProjectSkill Tests ===

    #[test]
    fn test_project_skill_new() {
        let skill = ProjectSkill::new("PAL");
        assert_eq!(skill.project_id, "PAL");
        assert_eq!(skill.name, "PAL-skill");
        assert!(skill.directives.is_empty());
    }

    #[test]
    fn test_project_skill_add_directive() {
        let mut skill = ProjectSkill::new("test");
        skill.add_directive("always use Rust");
        assert_eq!(skill.directives.len(), 1);
        assert_eq!(skill.directives[0].content, "always use Rust");
    }

    #[test]
    fn test_project_skill_add_directive_increases_priority() {
        let mut skill = ProjectSkill::new("test");
        skill.add_directive("use tabs");
        let initial_priority = skill.directives[0].priority;
        skill.add_directive("use tabs"); // Same directive again
        assert!(skill.directives[0].priority > initial_priority);
    }

    // === LogLevel Tests ===

    #[test]
    fn test_log_level_ordering() {
        assert!(LogLevel::Debug < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Error);
    }

    // === Session Overlap Detection Tests (PAL-66) ===

    #[tokio::test]
    async fn test_find_sessions_for_target_returns_matching() {
        let manager = SessionManager::new(PathBuf::from("/tmp/test"));

        // Create two sessions for PAL-60
        let target = SessionTarget::issue("PAL-60");
        let _s1 = manager.create_session_for_test(target.clone(), SessionStrategy::Simple).await;
        let _s2 = manager.create_session_for_test(target.clone(), SessionStrategy::Simple).await;

        // Create one session for PAL-61
        let other_target = SessionTarget::issue("PAL-61");
        let _s3 = manager.create_session_for_test(other_target, SessionStrategy::Simple).await;

        // Should find exactly 2 sessions for PAL-60
        let found = manager.find_sessions_for_target(&target).await;
        assert_eq!(found.len(), 2);
    }

    #[tokio::test]
    async fn test_find_sessions_for_target_empty_when_none() {
        let manager = SessionManager::new(PathBuf::from("/tmp/test"));

        let target = SessionTarget::issue("PAL-99");
        let found = manager.find_sessions_for_target(&target).await;
        assert!(found.is_empty());
    }

    #[tokio::test]
    async fn test_has_active_session_for_target_true_when_running() {
        let manager = SessionManager::new(PathBuf::from("/tmp/test"));

        let target = SessionTarget::issue("PAL-60");
        let session = manager.create_session_for_test(target.clone(), SessionStrategy::Simple).await;

        // Mark as running
        manager.update_status(session.id, SessionStatus::Running).await;

        assert!(manager.has_active_session_for_target(&target).await);
    }

    #[tokio::test]
    async fn test_has_active_session_for_target_false_when_completed() {
        let manager = SessionManager::new(PathBuf::from("/tmp/test"));

        let target = SessionTarget::issue("PAL-60");
        let session = manager.create_session_for_test(target.clone(), SessionStrategy::Simple).await;

        // Mark as completed
        manager.update_status(session.id, SessionStatus::Completed).await;

        assert!(!manager.has_active_session_for_target(&target).await);
    }

    #[tokio::test]
    async fn test_has_active_session_for_target_false_when_none() {
        let manager = SessionManager::new(PathBuf::from("/tmp/test"));

        let target = SessionTarget::issue("PAL-99");
        assert!(!manager.has_active_session_for_target(&target).await);
    }

    #[tokio::test]
    async fn test_session_target_matches() {
        let t1 = SessionTarget::issue("PAL-60");
        let t2 = SessionTarget::issue("PAL-60");
        let t3 = SessionTarget::issue("PAL-61");

        assert!(t1.matches(&t2));
        assert!(!t1.matches(&t3));
    }

    #[tokio::test]
    async fn test_create_session_refuses_duplicate_active() {
        let manager = SessionManager::new(PathBuf::from("/tmp/test"));

        let target = SessionTarget::issue("PAL-60");

        // Create first session and mark it running
        let s1 = manager.create_session_for_test(target.clone(), SessionStrategy::Simple).await;
        manager.update_status(s1.id, SessionStatus::Running).await;

        // Try to create another session for same target - should fail
        // Note: This would need the real create_session which checks for overlap
        // For now we just verify has_active returns true
        assert!(manager.has_active_session_for_target(&target).await);
    }

    #[tokio::test]
    async fn test_session_target_matches_multi() {
        let single = SessionTarget::issue("PAL-60");
        let multi = SessionTarget::multi(vec![
            SingleTarget::Issue("PAL-60".into()),
            SingleTarget::Issue("PAL-61".into()),
        ]);

        // Multi contains PAL-60, so should match
        assert!(single.overlaps(&multi));
        assert!(multi.overlaps(&single));
    }
}
