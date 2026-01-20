//! Status board using topic/channel descriptions.
//!
//! Channel and topic descriptions can be used as a real-time status board
//! that shows current TODOs, goals, and state. Users can mutate state by
//! messaging intent or using commands directly.

use crate::{ZulipError, ZulipResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A status board entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusEntry {
    /// Entry type.
    pub kind: StatusKind,
    /// Entry content.
    pub content: String,
    /// Priority (higher = more important).
    pub priority: i32,
    /// Whether this is completed.
    pub done: bool,
    /// Associated session/agent ID.
    pub owner: Option<String>,
}

/// Types of status board entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusKind {
    /// A goal to achieve.
    Goal,
    /// A task to complete.
    Task,
    /// A blocker or issue.
    Blocker,
    /// A note or comment.
    Note,
    /// Current work in progress.
    Wip,
}

impl StatusEntry {
    /// Create a new goal entry.
    pub fn goal(content: impl Into<String>) -> Self {
        Self {
            kind: StatusKind::Goal,
            content: content.into(),
            priority: 0,
            done: false,
            owner: None,
        }
    }

    /// Create a new task entry.
    pub fn task(content: impl Into<String>) -> Self {
        Self {
            kind: StatusKind::Task,
            content: content.into(),
            priority: 0,
            done: false,
            owner: None,
        }
    }

    /// Create a new blocker entry.
    pub fn blocker(content: impl Into<String>) -> Self {
        Self {
            kind: StatusKind::Blocker,
            content: content.into(),
            priority: 100, // Blockers are high priority
            done: false,
            owner: None,
        }
    }

    /// Create a new WIP entry.
    pub fn wip(content: impl Into<String>, owner: impl Into<String>) -> Self {
        Self {
            kind: StatusKind::Wip,
            content: content.into(),
            priority: 50,
            done: false,
            owner: Some(owner.into()),
        }
    }

    /// Set owner.
    pub fn with_owner(mut self, owner: impl Into<String>) -> Self {
        self.owner = Some(owner.into());
        self
    }

    /// Set priority.
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Mark as done.
    pub fn mark_done(&mut self) {
        self.done = true;
    }

    /// Format as markdown line.
    pub fn to_markdown(&self) -> String {
        let prefix = match self.kind {
            StatusKind::Goal => "🎯",
            StatusKind::Task => if self.done { "✅" } else { "⬜" },
            StatusKind::Blocker => "🚫",
            StatusKind::Note => "📝",
            StatusKind::Wip => "🔄",
        };

        let owner = self.owner.as_ref()
            .map(|o| format!(" @{o}"))
            .unwrap_or_default();

        format!("{prefix} {}{owner}", self.content)
    }
}

/// Status board for a channel or topic.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatusBoard {
    /// Board title.
    pub title: String,
    /// Status entries.
    pub entries: Vec<StatusEntry>,
    /// Last updated timestamp.
    pub updated: Option<chrono::DateTime<chrono::Utc>>,
}

impl StatusBoard {
    /// Create a new status board.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            entries: Vec::new(),
            updated: Some(chrono::Utc::now()),
        }
    }

    /// Add an entry.
    pub fn add(&mut self, entry: StatusEntry) {
        self.entries.push(entry);
        self.updated = Some(chrono::Utc::now());
    }

    /// Remove entries by content match.
    pub fn remove(&mut self, content: &str) {
        self.entries.retain(|e| !e.content.contains(content));
        self.updated = Some(chrono::Utc::now());
    }

    /// Mark an entry as done.
    pub fn complete(&mut self, content: &str) -> bool {
        for entry in &mut self.entries {
            if entry.content.contains(content) {
                entry.mark_done();
                self.updated = Some(chrono::Utc::now());
                return true;
            }
        }
        false
    }

    /// Get active (not done) entries.
    pub fn active(&self) -> Vec<&StatusEntry> {
        self.entries.iter().filter(|e| !e.done).collect()
    }

    /// Get completed entries.
    pub fn completed(&self) -> Vec<&StatusEntry> {
        self.entries.iter().filter(|e| e.done).collect()
    }

    /// Get blockers.
    pub fn blockers(&self) -> Vec<&StatusEntry> {
        self.entries.iter()
            .filter(|e| e.kind == StatusKind::Blocker && !e.done)
            .collect()
    }

    /// Format as topic description.
    pub fn to_description(&self) -> String {
        let mut desc = format!("**{}**\n\n", self.title);

        // Show blockers first
        let blockers = self.blockers();
        if !blockers.is_empty() {
            desc.push_str("**Blockers:**\n");
            for entry in blockers {
                desc.push_str(&format!("- {}\n", entry.to_markdown()));
            }
            desc.push('\n');
        }

        // Show active items by priority
        let mut active: Vec<_> = self.active().into_iter()
            .filter(|e| e.kind != StatusKind::Blocker)
            .collect();
        active.sort_by(|a, b| b.priority.cmp(&a.priority));

        if !active.is_empty() {
            desc.push_str("**Active:**\n");
            for entry in active {
                desc.push_str(&format!("- {}\n", entry.to_markdown()));
            }
            desc.push('\n');
        }

        // Show recent completed (last 3)
        let completed: Vec<_> = self.completed();
        if !completed.is_empty() {
            desc.push_str("**Done:**\n");
            for entry in completed.iter().rev().take(3) {
                desc.push_str(&format!("- {}\n", entry.to_markdown()));
            }
        }

        if let Some(updated) = self.updated {
            desc.push_str(&format!("\n_Updated: {}_", updated.format("%H:%M UTC")));
        }

        desc
    }

    /// Parse from description text (reverse of to_description).
    pub fn from_description(text: &str) -> Self {
        let mut board = StatusBoard::default();

        let mut in_blockers = false;
        let mut in_active = false;
        let mut in_done = false;

        for line in text.lines() {
            let line = line.trim();

            if line.starts_with("**") && line.ends_with("**") {
                let section = &line[2..line.len()-2];
                match section.to_lowercase().as_str() {
                    s if s.starts_with("blocker") => {
                        in_blockers = true;
                        in_active = false;
                        in_done = false;
                    }
                    s if s.starts_with("active") => {
                        in_blockers = false;
                        in_active = true;
                        in_done = false;
                    }
                    s if s.starts_with("done") => {
                        in_blockers = false;
                        in_active = false;
                        in_done = true;
                    }
                    _ => {
                        // Title line
                        board.title = section.to_string();
                    }
                }
                continue;
            }

            if !line.starts_with("- ") && !line.starts_with("* ") {
                continue;
            }

            let content = line[2..].trim();
            // Strip emoji prefix
            let content = content
                .strip_prefix("🎯 ")
                .or_else(|| content.strip_prefix("✅ "))
                .or_else(|| content.strip_prefix("⬜ "))
                .or_else(|| content.strip_prefix("🚫 "))
                .or_else(|| content.strip_prefix("📝 "))
                .or_else(|| content.strip_prefix("🔄 "))
                .unwrap_or(content);

            if content.is_empty() {
                continue;
            }

            let entry = if in_blockers {
                StatusEntry::blocker(content)
            } else if in_done {
                let mut e = StatusEntry::task(content);
                e.done = true;
                e
            } else {
                StatusEntry::task(content)
            };

            board.entries.push(entry);
        }

        board
    }
}

/// Manager for multiple status boards across channels/topics.
#[derive(Debug, Default)]
pub struct StatusBoardManager {
    /// Boards keyed by "stream:topic".
    boards: HashMap<String, StatusBoard>,
}

impl StatusBoardManager {
    /// Create a new manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create board for a location.
    pub fn get_or_create(&mut self, stream: &str, topic: &str) -> &mut StatusBoard {
        let key = format!("{}:{}", stream, topic);
        self.boards.entry(key).or_insert_with(|| {
            StatusBoard::new(format!("{} / {}", stream, topic))
        })
    }

    /// Get board for a location.
    pub fn get(&self, stream: &str, topic: &str) -> Option<&StatusBoard> {
        let key = format!("{}:{}", stream, topic);
        self.boards.get(&key)
    }

    /// Update board from description text.
    pub fn update_from_description(&mut self, stream: &str, topic: &str, description: &str) {
        let key = format!("{}:{}", stream, topic);
        let board = StatusBoard::from_description(description);
        self.boards.insert(key, board);
    }
}

/// Parse user intent from a message.
pub fn parse_intent(content: &str) -> Option<Intent> {
    let content = content.trim().to_lowercase();

    // Goal setting
    if content.starts_with("goal:") || content.starts_with("let's") || content.starts_with("we need to") {
        let desc = content
            .strip_prefix("goal:")
            .or_else(|| content.strip_prefix("let's"))
            .or_else(|| content.strip_prefix("we need to"))
            .map(|s| s.trim())
            .unwrap_or(&content);
        return Some(Intent::SetGoal(desc.to_string()));
    }

    // Task adding
    if content.starts_with("add task:") || content.starts_with("todo:") {
        let desc = content
            .strip_prefix("add task:")
            .or_else(|| content.strip_prefix("todo:"))
            .map(|s| s.trim())
            .unwrap_or(&content);
        return Some(Intent::AddTask(desc.to_string()));
    }

    // Completion
    if content.starts_with("done:") || content.starts_with("completed:") || content.starts_with("finished") {
        let desc = content
            .strip_prefix("done:")
            .or_else(|| content.strip_prefix("completed:"))
            .or_else(|| content.strip_prefix("finished"))
            .map(|s| s.trim())
            .unwrap_or(&content);
        return Some(Intent::Complete(desc.to_string()));
    }

    // Blocker reporting
    if content.starts_with("blocked:") || content.starts_with("blocker:") || content.contains("blocked by") {
        let desc = content
            .strip_prefix("blocked:")
            .or_else(|| content.strip_prefix("blocker:"))
            .map(|s| s.trim())
            .unwrap_or(&content);
        return Some(Intent::ReportBlocker(desc.to_string()));
    }

    // Priority change
    if content.starts_with("prioritize") || content.starts_with("focus on") {
        let desc = content
            .strip_prefix("prioritize")
            .or_else(|| content.strip_prefix("focus on"))
            .map(|s| s.trim())
            .unwrap_or(&content);
        return Some(Intent::SetPriority(desc.to_string()));
    }

    None
}

/// User intent parsed from message.
#[derive(Debug, Clone)]
pub enum Intent {
    /// Set a goal.
    SetGoal(String),
    /// Add a task.
    AddTask(String),
    /// Mark something complete.
    Complete(String),
    /// Report a blocker.
    ReportBlocker(String),
    /// Change priority.
    SetPriority(String),
}
