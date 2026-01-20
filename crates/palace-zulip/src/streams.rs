//! Stream (channel) types for Zulip.

use serde::{Deserialize, Serialize};

/// A Zulip stream (channel).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stream {
    /// Stream ID.
    pub id: u64,
    /// Stream name.
    pub name: String,
    /// Description.
    pub description: Option<String>,
    /// Stream type.
    pub stream_type: StreamType,
    /// Whether the stream is invite-only.
    pub invite_only: bool,
}

/// Type of stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamType {
    /// General discussion.
    General,
    /// Project-specific.
    Project,
    /// Cycle-specific.
    Cycle,
    /// Module-specific.
    Module,
    /// Agent-specific.
    Agent,
    /// Custom/other.
    Custom,
}

impl StreamType {
    /// Get stream name prefix for this type.
    pub fn prefix(&self) -> &'static str {
        match self {
            StreamType::General => "",
            StreamType::Project => "project-",
            StreamType::Cycle => "cycle-",
            StreamType::Module => "module-",
            StreamType::Agent => "agent-",
            StreamType::Custom => "",
        }
    }

    /// Detect stream type from name.
    pub fn from_name(name: &str) -> Self {
        if name == "general" {
            StreamType::General
        } else if name.starts_with("project-") {
            StreamType::Project
        } else if name.starts_with("cycle-") {
            StreamType::Cycle
        } else if name.starts_with("module-") {
            StreamType::Module
        } else if name.starts_with("agent-") {
            StreamType::Agent
        } else {
            StreamType::Custom
        }
    }
}

/// A topic within a stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Topic {
    /// Topic name.
    pub name: String,
    /// Number of messages.
    pub message_count: u64,
    /// Last message timestamp.
    pub last_message: Option<chrono::DateTime<chrono::Utc>>,
}

impl Topic {
    /// Create a new topic.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            message_count: 0,
            last_message: None,
        }
    }
}

/// Standard Palace topics.
pub mod topics {
    /// Announcements topic.
    pub const ANNOUNCEMENTS: &str = "announcements";
    /// Progress updates topic.
    pub const PROGRESS: &str = "progress";
    /// Help requests topic.
    pub const HELP_REQUESTS: &str = "help-requests";
    /// Status updates topic.
    pub const STATUS: &str = "status";
    /// Errors and failures topic.
    pub const ERRORS: &str = "errors";
    /// Completed work topic.
    pub const COMPLETED: &str = "completed";
    /// Agent broadcast topic.
    pub const BROADCAST: &str = "broadcast";
    /// Agent messages topic.
    pub const MESSAGES: &str = "messages";

    /// Generate issue topic name.
    pub fn issue(id: &str) -> String {
        format!("issue-{}", id)
    }

    /// Generate task topic name.
    pub fn task(id: &str) -> String {
        format!("task-{}", id)
    }
}

/// Standard Palace streams.
pub mod streams {
    /// General stream name.
    pub const GENERAL: &str = "general";
    /// Agents coordination stream.
    pub const AGENTS: &str = "agents";

    /// Generate project stream name.
    pub fn project(id: &str) -> String {
        format!("project-{}", id)
    }

    /// Generate cycle stream name.
    pub fn cycle(id: &str) -> String {
        format!("cycle-{}", id)
    }

    /// Generate module stream name.
    pub fn module(id: &str) -> String {
        format!("module-{}", id)
    }

    /// Generate agent stream name.
    pub fn agent(id: &str) -> String {
        format!("agent-{}", id)
    }
}
