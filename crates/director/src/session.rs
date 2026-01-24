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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Path to the session transcript file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<PathBuf>,
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
            transcript_path: None,
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
    /// Sessions state file.
    state_file: PathBuf,
}

impl SessionManager {
    /// Create a new session manager.
    pub fn new(project_path: PathBuf) -> Self {
        let state_file = project_path.join(".palace/sessions.json");
        let (events_tx, _) = broadcast::channel(1000);

        // Load existing state synchronously before creating the manager
        let sessions = if let Ok(state) = std::fs::read_to_string(&state_file) {
            serde_json::from_str::<HashMap<Uuid, Session>>(&state).unwrap_or_default()
        } else {
            HashMap::new()
        };

        Self {
            sessions: Arc::new(RwLock::new(sessions)),
            logs: Arc::new(RwLock::new(HashMap::new())),
            skills: Arc::new(RwLock::new(HashMap::new())),
            events_tx,
            project_path,
            state_file,
        }
    }

    /// Subscribe to session events.
    pub fn subscribe(&self) -> broadcast::Receiver<SessionEvent> {
        self.events_tx.subscribe()
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
        let session = Session::new(target, strategy, self.project_path.clone())
            .with_skills(skills);

        // Set up transcript path for recording
        let transcript_dir = self.project_path.join(".palace/transcripts");
        std::fs::create_dir_all(&transcript_dir).ok();
        let mut session = session;
        session.transcript_path = Some(transcript_dir.join(format!("{}.jsonl", session.short_id())));

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

        // Save state
        self.save_state().await?;

        Ok(session)
    }

    /// Set up git worktree for parallel execution.
    async fn setup_worktree(&self, mut session: Session) -> DirectorResult<Session> {
        let worktree_name = format!("session-{}", session.short_id());
        let worktree_path = self.project_path.join(".palace/worktrees").join(&worktree_name);
        let branch_name = format!("session/{}", session.short_id());

        // Create worktree directory
        std::fs::create_dir_all(worktree_path.parent().unwrap())
            .map_err(|e| DirectorError::Io(e))?;

        // Create git worktree
        let output = tokio::process::Command::new("git")
            .args(["worktree", "add", "-b", &branch_name, worktree_path.to_str().unwrap()])
            .current_dir(&self.project_path)
            .output()
            .await
            .map_err(|e| DirectorError::Io(e))?;

        if !output.status.success() {
            tracing::warn!(
                "Failed to create worktree: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            // Continue without worktree
        } else {
            session.worktree_path = Some(worktree_path);
            session.branch = Some(branch_name);
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

        let _ = self.save_state().await;
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

        let _ = self.save_state().await;
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
    pub async fn cancel_session(&self, session_id: Uuid) -> DirectorResult<()> {
        self.update_status(session_id, SessionStatus::Cancelled).await;

        // Clean up worktree if exists
        let session = self.get_session(session_id).await;
        if let Some(session) = session {
            if let Some(worktree_path) = session.worktree_path {
                let _ = tokio::process::Command::new("git")
                    .args(["worktree", "remove", worktree_path.to_str().unwrap(), "--force"])
                    .current_dir(&self.project_path)
                    .output()
                    .await;
            }
        }

        Ok(())
    }

    /// Save session state to disk.
    async fn save_state(&self) -> DirectorResult<()> {
        let sessions = self.sessions.read().await;

        // Ensure directory exists
        if let Some(parent) = self.state_file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| DirectorError::Io(e))?;
        }

        let json = serde_json::to_string_pretty(&*sessions)
            .map_err(|e| DirectorError::Other(e.to_string()))?;

        std::fs::write(&self.state_file, json)
            .map_err(|e| DirectorError::Io(e))?;

        Ok(())
    }
}
