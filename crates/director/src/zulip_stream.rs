//! Zulip Streaming - real-time output streaming to Zulip.
//!
//! Provides a configurable system for streaming session output to Zulip
//! with verbosity controls and automatic channel/topic hierarchy.
//!
//! # Channel Structure
//!
//! ```text
//! palace-{project}           (stream per project)
//!   └─ session/{id}: {name}  (topic per session)
//!        └─ messages for targets, progress, tool calls, etc.
//!
//! palace-logs                (global logs stream)
//!   └─ {level}               (topic per log level)
//! ```

use crate::zulip_tool::ZulipTool;
use crate::{DirectorError, DirectorResult};
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Verbosity level for Zulip reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Verbosity {
    /// Only errors and blockers.
    Quiet,
    /// Start, complete, fail events.
    Normal,
    /// Progress updates and tool calls.
    Verbose,
    /// Everything including debug info.
    Debug,
}

impl Default for Verbosity {
    fn default() -> Self {
        Verbosity::Normal
    }
}

/// Event type for filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventType {
    SessionStart,
    SessionComplete,
    SessionFail,
    Progress,
    ToolCall,
    Blocker,
    Survey,
    Log,
    Debug,
}

impl EventType {
    /// Get the minimum verbosity level for this event type.
    fn min_verbosity(&self) -> Verbosity {
        match self {
            EventType::SessionFail | EventType::Blocker => Verbosity::Quiet,
            EventType::SessionStart | EventType::SessionComplete => Verbosity::Normal,
            EventType::Progress | EventType::ToolCall | EventType::Survey => Verbosity::Verbose,
            EventType::Log | EventType::Debug => Verbosity::Debug,
        }
    }
}

/// Configuration for a channel (stream).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    /// Stream name.
    pub name: String,
    /// Verbosity level.
    pub verbosity: Verbosity,
    /// Whether to auto-create if missing.
    pub auto_create: bool,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            name: "palace".to_string(),
            verbosity: Verbosity::Normal,
            auto_create: true,
        }
    }
}

/// Configuration for the streaming system.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamConfig {
    /// Default verbosity for new channels.
    pub default_verbosity: Verbosity,
    /// Per-channel verbosity overrides.
    pub channel_verbosity: HashMap<String, Verbosity>,
    /// Per-topic verbosity overrides (channel/topic -> verbosity).
    pub topic_verbosity: HashMap<String, Verbosity>,
    /// Whether to create streams/topics automatically.
    pub auto_create: bool,
    /// Stream name template for projects.
    pub project_stream_template: String,
    /// Topic template for sessions.
    pub session_topic_template: String,
}

impl StreamConfig {
    /// Create a new config with defaults.
    pub fn new() -> Self {
        Self {
            default_verbosity: Verbosity::Normal,
            channel_verbosity: HashMap::new(),
            topic_verbosity: HashMap::new(),
            auto_create: true,
            project_stream_template: "palace-{}".to_string(),
            session_topic_template: "session/{}: {}".to_string(),
        }
    }

    /// Set verbosity for a channel.
    pub fn set_channel_verbosity(&mut self, channel: &str, verbosity: Verbosity) {
        self.channel_verbosity.insert(channel.to_string(), verbosity);
    }

    /// Set verbosity for a topic.
    pub fn set_topic_verbosity(&mut self, channel: &str, topic: &str, verbosity: Verbosity) {
        let key = format!("{}/{}", channel, topic);
        self.topic_verbosity.insert(key, verbosity);
    }

    /// Get effective verbosity for a channel/topic.
    pub fn effective_verbosity(&self, channel: &str, topic: &str) -> Verbosity {
        // Check topic-specific first
        let key = format!("{}/{}", channel, topic);
        if let Some(v) = self.topic_verbosity.get(&key) {
            return *v;
        }

        // Then channel
        if let Some(v) = self.channel_verbosity.get(channel) {
            return *v;
        }

        // Default
        self.default_verbosity
    }

    /// Check if an event should be reported.
    pub fn should_report(&self, channel: &str, topic: &str, event: EventType) -> bool {
        let verbosity = self.effective_verbosity(channel, topic);
        event.min_verbosity() <= verbosity
    }
}

/// Streamer for sending events to Zulip.
pub struct ZulipStreamer {
    tool: ZulipTool,
    config: Arc<RwLock<StreamConfig>>,
    /// Track created streams for caching.
    created_streams: Arc<RwLock<HashMap<String, bool>>>,
    /// Message buffer for batching low-priority updates.
    buffer: Arc<RwLock<Vec<BufferedMessage>>>,
    /// Last message ID per topic (for editing).
    last_message: Arc<RwLock<HashMap<String, u64>>>,
}

/// Buffered message for batching.
#[derive(Debug, Clone)]
struct BufferedMessage {
    stream: String,
    topic: String,
    content: String,
    timestamp: DateTime<Utc>,
    event_type: EventType,
}

impl ZulipStreamer {
    /// Create from environment.
    pub fn from_env() -> DirectorResult<Self> {
        let tool = ZulipTool::from_env()?;
        Ok(Self {
            tool,
            config: Arc::new(RwLock::new(StreamConfig::new())),
            created_streams: Arc::new(RwLock::new(HashMap::new())),
            buffer: Arc::new(RwLock::new(Vec::new())),
            last_message: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Create with custom config.
    pub fn with_config(config: StreamConfig) -> DirectorResult<Self> {
        let tool = ZulipTool::from_env()?;
        Ok(Self {
            tool,
            config: Arc::new(RwLock::new(config)),
            created_streams: Arc::new(RwLock::new(HashMap::new())),
            buffer: Arc::new(RwLock::new(Vec::new())),
            last_message: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Get the stream name for a project.
    pub async fn project_stream(&self, project: &str) -> String {
        let config = self.config.read().await;
        config.project_stream_template.replace("{}", project)
    }

    /// Get the topic for a session.
    pub async fn session_topic(&self, session_id: Uuid, session_name: &str) -> String {
        let config = self.config.read().await;
        config.session_topic_template
            .replace("{}", &session_id.to_string()[..8])
            .replace("{}", session_name)
    }

    /// Set verbosity for a channel.
    pub async fn set_verbosity(&self, channel: &str, verbosity: Verbosity) {
        let mut config = self.config.write().await;
        config.set_channel_verbosity(channel, verbosity);
    }

    /// Set verbosity for a topic.
    pub async fn set_topic_verbosity(&self, channel: &str, topic: &str, verbosity: Verbosity) {
        let mut config = self.config.write().await;
        config.set_topic_verbosity(channel, topic, verbosity);
    }

    /// Ensure a stream exists (subscribe to it).
    async fn ensure_stream(&self, stream: &str) -> DirectorResult<()> {
        let created = {
            let streams = self.created_streams.read().await;
            streams.contains_key(stream)
        };

        if !created {
            let config = self.config.read().await;
            if config.auto_create {
                drop(config);
                self.tool.subscribe(stream).await?;
                let mut streams = self.created_streams.write().await;
                streams.insert(stream.to_string(), true);
            }
        }

        Ok(())
    }

    /// Send an event, respecting verbosity settings.
    pub async fn send_event(
        &self,
        stream: &str,
        topic: &str,
        event_type: EventType,
        content: &str,
    ) -> DirectorResult<Option<u64>> {
        // Check verbosity
        let should_report = {
            let config = self.config.read().await;
            config.should_report(stream, topic, event_type)
        };

        if !should_report {
            return Ok(None);
        }

        // Ensure stream exists
        self.ensure_stream(stream).await?;

        // Send message
        let msg_id = self.tool.send(stream, topic, content).await?;

        // Track last message for potential editing
        let key = format!("{}/{}", stream, topic);
        let mut last = self.last_message.write().await;
        last.insert(key, msg_id);

        Ok(Some(msg_id))
    }

    /// Session started event.
    pub async fn session_started(
        &self,
        project: &str,
        session_id: Uuid,
        session_name: &str,
        target: &str,
    ) -> DirectorResult<Option<u64>> {
        let stream = self.project_stream(project).await;
        let topic = format!("session/{}: {}", &session_id.to_string()[..8], session_name);

        let content = format!(
            "🚀 **Session Started**\n\n\
            **Target:** `{}`\n\
            **ID:** `{}`\n\n\
            _React with emoji to provide feedback:_\n\
            ❤️ = great | 👍👎 = soft | 🛑 = halt",
            target, session_id
        );

        self.send_event(&stream, &topic, EventType::SessionStart, &content).await
    }

    /// Session progress event.
    pub async fn session_progress(
        &self,
        project: &str,
        session_id: Uuid,
        session_name: &str,
        completed: u32,
        total: u32,
        current: &str,
    ) -> DirectorResult<Option<u64>> {
        let stream = self.project_stream(project).await;
        let topic = format!("session/{}: {}", &session_id.to_string()[..8], session_name);

        let percent = if total > 0 { (completed * 100) / total } else { 0 };
        let bar = progress_bar(percent);

        let content = format!(
            "📊 {} {}/{}  ({}%)\n**Current:** {}",
            bar, completed, total, percent, current
        );

        self.send_event(&stream, &topic, EventType::Progress, &content).await
    }

    /// Tool call event.
    pub async fn tool_call(
        &self,
        project: &str,
        session_id: Uuid,
        session_name: &str,
        tool_name: &str,
        description: &str,
    ) -> DirectorResult<Option<u64>> {
        let stream = self.project_stream(project).await;
        let topic = format!("session/{}: {}", &session_id.to_string()[..8], session_name);

        let content = format!("🔧 `{}`: {}", tool_name, description);

        self.send_event(&stream, &topic, EventType::ToolCall, &content).await
    }

    /// Blocker event - always sent regardless of verbosity.
    pub async fn blocker(
        &self,
        project: &str,
        session_id: Uuid,
        session_name: &str,
        issue: &str,
        options: &[&str],
    ) -> DirectorResult<Option<u64>> {
        let stream = self.project_stream(project).await;
        let topic = format!("session/{}: {}", &session_id.to_string()[..8], session_name);

        let options_text = options.iter()
            .enumerate()
            .map(|(i, o)| format!("{}. {}", i + 1, o))
            .collect::<Vec<_>>()
            .join("\n");

        let content = format!(
            "🚧 **Blocker**\n\n{}\n\n**Options:**\n{}\n\n_Reply with choice or guidance._",
            issue, options_text
        );

        self.send_event(&stream, &topic, EventType::Blocker, &content).await
    }

    /// Session completed event.
    pub async fn session_completed(
        &self,
        project: &str,
        session_id: Uuid,
        session_name: &str,
        summary: &str,
    ) -> DirectorResult<Option<u64>> {
        let stream = self.project_stream(project).await;
        let topic = format!("session/{}: {}", &session_id.to_string()[..8], session_name);

        let content = format!("✅ **Session Completed**\n\n{}", summary);

        self.send_event(&stream, &topic, EventType::SessionComplete, &content).await
    }

    /// Session failed event.
    pub async fn session_failed(
        &self,
        project: &str,
        session_id: Uuid,
        session_name: &str,
        error: &str,
    ) -> DirectorResult<Option<u64>> {
        let stream = self.project_stream(project).await;
        let topic = format!("session/{}: {}", &session_id.to_string()[..8], session_name);

        let content = format!(
            "❌ **Session Failed**\n\n**Error:** {}\n\n_React 🔄 to retry or reply with guidance._",
            error
        );

        self.send_event(&stream, &topic, EventType::SessionFail, &content).await
    }

    /// Log message event.
    pub async fn log(
        &self,
        project: &str,
        session_id: Uuid,
        session_name: &str,
        level: &str,
        message: &str,
    ) -> DirectorResult<Option<u64>> {
        let stream = self.project_stream(project).await;
        let topic = format!("session/{}: {}", &session_id.to_string()[..8], session_name);

        let emoji = match level {
            "error" => "❌",
            "warn" => "⚠️",
            "info" => "ℹ️",
            _ => "🔍",
        };

        let content = format!("{} {}", emoji, message);

        self.send_event(&stream, &topic, EventType::Log, &content).await
    }

    /// Direct message to any stream/topic.
    pub async fn message(
        &self,
        stream: &str,
        topic: &str,
        content: &str,
    ) -> DirectorResult<u64> {
        self.ensure_stream(stream).await?;
        self.tool.send(stream, topic, content).await
    }
}

/// Generate a text progress bar.
fn progress_bar(percent: u32) -> String {
    let filled = (percent / 10) as usize;
    let empty = 10 - filled;
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verbosity_ordering() {
        assert!(Verbosity::Quiet < Verbosity::Normal);
        assert!(Verbosity::Normal < Verbosity::Verbose);
        assert!(Verbosity::Verbose < Verbosity::Debug);
    }

    #[test]
    fn test_event_min_verbosity() {
        assert_eq!(EventType::Blocker.min_verbosity(), Verbosity::Quiet);
        assert_eq!(EventType::SessionStart.min_verbosity(), Verbosity::Normal);
        assert_eq!(EventType::ToolCall.min_verbosity(), Verbosity::Verbose);
        assert_eq!(EventType::Debug.min_verbosity(), Verbosity::Debug);
    }

    #[test]
    fn test_config_should_report() {
        let mut config = StreamConfig::new();
        config.default_verbosity = Verbosity::Normal;

        // Blocker should always be reported (Quiet level)
        assert!(config.should_report("test", "topic", EventType::Blocker));

        // SessionStart should be reported at Normal
        assert!(config.should_report("test", "topic", EventType::SessionStart));

        // ToolCall requires Verbose
        assert!(!config.should_report("test", "topic", EventType::ToolCall));

        // Set channel to Verbose
        config.set_channel_verbosity("test", Verbosity::Verbose);
        assert!(config.should_report("test", "topic", EventType::ToolCall));
    }
}
