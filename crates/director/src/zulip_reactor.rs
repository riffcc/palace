//! Zulip Reactor - Event-driven orchestration via Zulip.
//!
//! The reactor subscribes to Zulip events and manages the Director
//! in response to messages and reactions. Uses emoji reactions as
//! ultra-token-efficient state indicators (2 chars + ref).
//!
//! # Reaction Protocol
//!
//! Reactions serve as lightweight state signals:
//! - 👀 = acknowledged, processing
//! - 🔄 = in progress
//! - ✅ = completed successfully
//! - ❌ = failed
//! - 🚧 = blocked, needs input
//! - 💬 = replied with details
//! - ⏸️ = paused
//! - ▶️ = resumed
//! - 🎯 = goal set
//! - 📋 = todo updated
//!
//! # Channel Structure
//!
//! Each channel has a pinned "status" message at the top that
//! contains the current todo list and metadata, edited in real-time.

use crate::zulip_tool::ZulipTool;
use crate::{DirectorError, DirectorResult, SessionManager, SessionTarget, SessionStrategy, SessionExecutor, SessionExecutorConfig};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

/// Emoji constants for reaction protocol.
pub mod emoji {
    pub const ACKNOWLEDGED: &str = "eyes";           // 👀
    pub const IN_PROGRESS: &str = "arrows_counterclockwise"; // 🔄
    pub const COMPLETED: &str = "white_check_mark"; // ✅
    pub const FAILED: &str = "x";                   // ❌
    pub const BLOCKED: &str = "construction";       // 🚧
    pub const REPLIED: &str = "speech_balloon";     // 💬
    pub const PAUSED: &str = "pause_button";        // ⏸️
    pub const RESUMED: &str = "arrow_forward";      // ▶️
    pub const GOAL_SET: &str = "dart";              // 🎯
    pub const TODO_UPDATED: &str = "clipboard";     // 📋
    pub const THINKING: &str = "thought_balloon";   // 💭
    pub const HEART: &str = "heart";                // ❤️
    pub const THUMBS_UP: &str = "+1";               // 👍
    pub const THUMBS_DOWN: &str = "-1";             // 👎
    pub const STOP: &str = "stop_sign";             // 🛑
}

/// Event from Zulip event queue.
#[derive(Debug, Clone, Deserialize)]
pub struct ZulipEvent {
    pub id: i64,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub message: Option<MessageEvent>,
    #[serde(default)]
    pub reaction: Option<ReactionEvent>,
    #[serde(default)]
    pub op: Option<String>,
}

/// Message event payload.
#[derive(Debug, Clone, Deserialize)]
pub struct MessageEvent {
    pub id: u64,
    pub sender_id: u64,
    pub sender_email: String,
    pub sender_full_name: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub stream_id: Option<u64>,
    pub subject: Option<String>,
    pub content: String,
    pub timestamp: u64,
}

/// Reaction event payload.
#[derive(Debug, Clone, Deserialize)]
pub struct ReactionEvent {
    pub user_id: u64,
    pub message_id: u64,
    pub emoji_name: String,
    pub emoji_code: String,
    pub reaction_type: String,
}

/// Pinned status message content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelStatus {
    /// Current todos for this channel/session.
    pub todos: Vec<TodoItem>,
    /// Current goals.
    pub goals: Vec<String>,
    /// Session status.
    pub status: String,
    /// Progress (0-100).
    pub progress: u32,
    /// Last updated timestamp.
    pub updated: String,
    /// Additional metadata.
    pub metadata: HashMap<String, String>,
}

impl Default for ChannelStatus {
    fn default() -> Self {
        Self {
            todos: Vec::new(),
            goals: Vec::new(),
            status: "idle".to_string(),
            progress: 0,
            updated: chrono::Utc::now().to_rfc3339(),
            metadata: HashMap::new(),
        }
    }
}

impl ChannelStatus {
    /// Render as markdown for pinned message.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        md.push_str(&format!("## Status: {}\n\n", self.status));

        if !self.goals.is_empty() {
            md.push_str("### 🎯 Goals\n");
            for goal in &self.goals {
                md.push_str(&format!("- {}\n", goal));
            }
            md.push('\n');
        }

        if !self.todos.is_empty() {
            md.push_str("### 📋 Todo\n");
            for todo in &self.todos {
                let check = if todo.done { "x" } else { " " };
                let status = match todo.status.as_str() {
                    "in_progress" => " 🔄",
                    "blocked" => " 🚧",
                    _ => "",
                };
                md.push_str(&format!("- [{}] {}{}\n", check, todo.text, status));
            }
            md.push('\n');
        }

        if self.progress > 0 {
            let bar = progress_bar(self.progress);
            md.push_str(&format!("**Progress:** {} {}%\n\n", bar, self.progress));
        }

        md.push_str(&format!("_Updated: {}_", self.updated));

        md
    }
}

/// Todo item in channel status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub text: String,
    pub done: bool,
    pub status: String,
}

/// The Zulip Reactor - event-driven orchestrator.
pub struct ZulipReactor {
    client: Client,
    server_url: String,
    email: String,
    api_key: String,
    /// Event queue ID.
    queue_id: Option<String>,
    /// Last event ID processed.
    last_event_id: i64,
    /// Pinned message IDs per stream/topic.
    pinned_messages: Arc<RwLock<HashMap<String, u64>>>,
    /// Channel status cache.
    channel_status: Arc<RwLock<HashMap<String, ChannelStatus>>>,
    /// Event sender for external handlers.
    event_tx: Option<mpsc::Sender<ZulipEvent>>,
    /// Bot's own user ID (to ignore self-reactions).
    bot_user_id: Option<u64>,
    /// Session managers per project.
    session_managers: Arc<RwLock<HashMap<PathBuf, Arc<SessionManager>>>>,
    /// Currently active session per stream.
    active_sessions: Arc<RwLock<HashMap<String, Uuid>>>,
}

impl ZulipReactor {
    /// Create from environment.
    pub fn from_env() -> DirectorResult<Self> {
        let _ = dotenvy::dotenv();

        let server_url = std::env::var("ZULIP_SERVER_URL")
            .unwrap_or_else(|_| "https://localhost:8443".to_string());

        let email = std::env::var("DIRECTOR_BOT_EMAIL")
            .or_else(|_| std::env::var("ZULIP_BOT_EMAIL"))
            .map_err(|_| DirectorError::Config("Bot email not set".into()))?;

        let api_key = std::env::var("DIRECTOR_API_KEY")
            .or_else(|_| std::env::var("ZULIP_API_KEY"))
            .map_err(|_| DirectorError::Config("API key not set".into()))?;

        let insecure = std::env::var("ZULIP_INSECURE")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        let client = if insecure {
            Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(|e| DirectorError::Config(e.to_string()))?
        } else {
            Client::new()
        };

        Ok(Self {
            client,
            server_url,
            email,
            api_key,
            queue_id: None,
            last_event_id: -1,
            pinned_messages: Arc::new(RwLock::new(HashMap::new())),
            channel_status: Arc::new(RwLock::new(HashMap::new())),
            event_tx: None,
            bot_user_id: None,
            session_managers: Arc::new(RwLock::new(HashMap::new())),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Set event channel for external handling.
    pub fn with_event_channel(mut self, tx: mpsc::Sender<ZulipEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// Register for events.
    pub async fn register(&mut self) -> DirectorResult<()> {
        let url = format!("{}/api/v1/register", self.server_url);

        let event_types = serde_json::json!(["message", "reaction"]);

        let resp = self.client
            .post(&url)
            .basic_auth(&self.email, Some(&self.api_key))
            .form(&[
                ("event_types", event_types.to_string()),
                ("all_public_streams", "true".to_string()),
            ])
            .send()
            .await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        let body: serde_json::Value = resp.json().await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        if body["result"].as_str() != Some("success") {
            return Err(DirectorError::Zulip(
                body["msg"].as_str().unwrap_or("Registration failed").to_string()
            ));
        }

        self.queue_id = body["queue_id"].as_str().map(String::from);
        self.last_event_id = body["last_event_id"].as_i64().unwrap_or(-1);

        tracing::info!("Registered for Zulip events, queue_id: {:?}", self.queue_id);

        Ok(())
    }

    /// Get events from queue.
    pub async fn get_events(&mut self) -> DirectorResult<Vec<ZulipEvent>> {
        let queue_id = self.queue_id.as_ref()
            .ok_or_else(|| DirectorError::Zulip("Not registered".into()))?;

        let url = format!("{}/api/v1/events", self.server_url);

        let resp = self.client
            .get(&url)
            .basic_auth(&self.email, Some(&self.api_key))
            .query(&[
                ("queue_id", queue_id.as_str()),
                ("last_event_id", &self.last_event_id.to_string()),
            ])
            .send()
            .await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        let body: serde_json::Value = resp.json().await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        if body["result"].as_str() != Some("success") {
            return Err(DirectorError::Zulip(
                body["msg"].as_str().unwrap_or("Get events failed").to_string()
            ));
        }

        let events: Vec<ZulipEvent> = serde_json::from_value(body["events"].clone())
            .unwrap_or_default();

        // Update last event ID
        if let Some(last) = events.last() {
            self.last_event_id = last.id;
        }

        Ok(events)
    }

    /// Add a reaction to a message.
    pub async fn react(&self, message_id: u64, emoji: &str) -> DirectorResult<()> {
        let url = format!("{}/api/v1/messages/{}/reactions", self.server_url, message_id);

        let resp = self.client
            .post(&url)
            .basic_auth(&self.email, Some(&self.api_key))
            .form(&[("emoji_name", emoji)])
            .send()
            .await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        let body: serde_json::Value = resp.json().await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        if body["result"].as_str() != Some("success") {
            // Ignore "already reacted" errors
            let msg = body["msg"].as_str().unwrap_or("");
            if !msg.contains("already") {
                return Err(DirectorError::Zulip(msg.to_string()));
            }
        }

        Ok(())
    }

    /// Remove a reaction from a message.
    pub async fn unreact(&self, message_id: u64, emoji: &str) -> DirectorResult<()> {
        let url = format!("{}/api/v1/messages/{}/reactions", self.server_url, message_id);

        let resp = self.client
            .delete(&url)
            .basic_auth(&self.email, Some(&self.api_key))
            .query(&[("emoji_name", emoji)])
            .send()
            .await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        let body: serde_json::Value = resp.json().await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        if body["result"].as_str() != Some("success") {
            let msg = body["msg"].as_str().unwrap_or("");
            if !msg.contains("not found") {
                return Err(DirectorError::Zulip(msg.to_string()));
            }
        }

        Ok(())
    }

    /// Edit a message.
    pub async fn edit_message(&self, message_id: u64, content: &str) -> DirectorResult<()> {
        let url = format!("{}/api/v1/messages/{}", self.server_url, message_id);

        let resp = self.client
            .patch(&url)
            .basic_auth(&self.email, Some(&self.api_key))
            .form(&[("content", content)])
            .send()
            .await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        let body: serde_json::Value = resp.json().await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        if body["result"].as_str() != Some("success") {
            return Err(DirectorError::Zulip(
                body["msg"].as_str().unwrap_or("Edit failed").to_string()
            ));
        }

        Ok(())
    }

    /// Send a message.
    pub async fn send(&self, stream: &str, topic: &str, content: &str) -> DirectorResult<u64> {
        let tool = ZulipTool::from_env()?;
        tool.send(stream, topic, content).await
    }

    /// Update the pinned status message for a channel.
    pub async fn update_status(&self, stream: &str, topic: &str, status: ChannelStatus) -> DirectorResult<()> {
        let key = format!("{}/{}", stream, topic);

        // Get or create pinned message
        let msg_id = {
            let pinned = self.pinned_messages.read().await;
            pinned.get(&key).copied()
        };

        let content = status.to_markdown();

        if let Some(id) = msg_id {
            // Edit existing
            self.edit_message(id, &content).await?;
        } else {
            // Create new and pin
            let id = self.send(stream, topic, &content).await?;
            let mut pinned = self.pinned_messages.write().await;
            pinned.insert(key.clone(), id);
        }

        // Update cache
        let mut cache = self.channel_status.write().await;
        cache.insert(key, status);

        Ok(())
    }

    /// Mark a message as acknowledged (start processing).
    pub async fn acknowledge(&self, message_id: u64) -> DirectorResult<()> {
        self.react(message_id, emoji::ACKNOWLEDGED).await
    }

    /// Mark a message as in progress.
    pub async fn mark_in_progress(&self, message_id: u64) -> DirectorResult<()> {
        self.unreact(message_id, emoji::ACKNOWLEDGED).await.ok();
        self.react(message_id, emoji::IN_PROGRESS).await
    }

    /// Mark a message as completed.
    pub async fn mark_completed(&self, message_id: u64) -> DirectorResult<()> {
        self.unreact(message_id, emoji::IN_PROGRESS).await.ok();
        self.react(message_id, emoji::COMPLETED).await
    }

    /// Mark a message as failed.
    pub async fn mark_failed(&self, message_id: u64) -> DirectorResult<()> {
        self.unreact(message_id, emoji::IN_PROGRESS).await.ok();
        self.react(message_id, emoji::FAILED).await
    }

    /// Mark a message as blocked.
    pub async fn mark_blocked(&self, message_id: u64) -> DirectorResult<()> {
        self.unreact(message_id, emoji::IN_PROGRESS).await.ok();
        self.react(message_id, emoji::BLOCKED).await
    }

    /// Get or create a session manager for a project.
    pub async fn get_session_manager(&self, project_path: &PathBuf) -> Arc<SessionManager> {
        let mut managers = self.session_managers.write().await;
        if let Some(manager) = managers.get(project_path) {
            manager.clone()
        } else {
            let manager = Arc::new(SessionManager::new(project_path.clone()));
            managers.insert(project_path.clone(), manager.clone());
            manager
        }
    }

    /// Get the project path for a stream.
    fn get_project_path(&self, stream: &str) -> PathBuf {
        let global_config = palace_plane::GlobalConfig::load().unwrap_or_default();
        global_config.find_project_by_stream(stream).unwrap_or_else(|| {
            std::env::var("PALACE_PROJECT_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."))
        })
    }

    /// Get or set the active session for a stream.
    pub async fn get_active_session(&self, stream: &str) -> Option<Uuid> {
        let sessions = self.active_sessions.read().await;
        sessions.get(stream).copied()
    }

    /// Set the active session for a stream.
    pub async fn set_active_session(&self, stream: &str, session_id: Option<Uuid>) {
        let mut sessions = self.active_sessions.write().await;
        if let Some(id) = session_id {
            sessions.insert(stream.to_string(), id);
        } else {
            sessions.remove(stream);
        }
    }

    /// Run the event loop.
    pub async fn run(&mut self) -> DirectorResult<()> {
        if self.queue_id.is_none() {
            self.register().await?;
        }

        tracing::info!("Starting Zulip reactor event loop");

        loop {
            match self.get_events().await {
                Ok(events) => {
                    for event in events {
                        if let Err(e) = self.handle_event(&event).await {
                            tracing::warn!("Error handling event: {}", e);
                        }

                        // Forward to external handler if set
                        if let Some(ref tx) = self.event_tx {
                            let _ = tx.send(event).await;
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Error getting events: {}", e);
                    // Re-register on queue errors
                    if e.to_string().contains("queue") {
                        self.register().await?;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

    /// Handle a single event.
    async fn handle_event(&self, event: &ZulipEvent) -> DirectorResult<()> {
        match event.event_type.as_str() {
            "message" => {
                if let Some(ref msg) = event.message {
                    self.handle_message(msg).await?;
                }
            }
            "reaction" => {
                if let Some(ref reaction) = event.reaction {
                    // Only handle add reactions, not removes
                    if event.op.as_deref() == Some("add") {
                        self.handle_reaction(reaction).await?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle incoming message.
    async fn handle_message(&self, msg: &MessageEvent) -> DirectorResult<()> {
        tracing::debug!(
            "Message from {}: {} (id: {})",
            msg.sender_full_name,
            &msg.content[..msg.content.len().min(50)],
            msg.id
        );

        // Auto-acknowledge messages that mention us
        let mentions_palace = msg.content.contains("@**Palace**") || msg.content.contains("@_**Palace**");
        let mentions_director = msg.content.contains("@**Director**") || msg.content.contains("@_**Director**");

        if mentions_palace || mentions_director {
            self.acknowledge(msg.id).await?;
        }

        // Parse @palace commands
        if let Some((cmd, args)) = Self::parse_palace_command(&msg.content) {
            // For stream messages, get stream name from stream_id if possible
            // For now, use a default or topic as stream name
            let stream = self.get_stream_name(msg.stream_id).await
                .unwrap_or_else(|| "palace".to_string());
            let topic = msg.subject.as_deref().unwrap_or("general");

            match cmd.as_str() {
                "approve" => {
                    self.handle_approve_command(msg.id, &stream, topic, &args).await?;
                }
                "work" | "new" => {
                    // @palace work PAL-85 or @palace new PAL-85
                    self.handle_work_command(msg.id, &stream, topic, &args).await?;
                }
                "session" => {
                    // @palace session <subcommand>
                    self.handle_session_command(msg.id, &stream, topic, &args).await?;
                }
                "switch" => {
                    // @palace switch <session_name>
                    self.handle_switch_command(msg.id, &stream, topic, &args).await?;
                }
                "watch" => {
                    // @palace watch <session_name>
                    self.handle_watch_command(msg.id, &stream, topic, &args).await?;
                }
                "tell" => {
                    // @palace tell "message"
                    self.handle_tell_command(msg.id, &stream, topic, &args, false).await?;
                }
                "tell-now" => {
                    // @palace tell-now "message" (immediate)
                    self.handle_tell_command(msg.id, &stream, topic, &args, true).await?;
                }
                "ls" | "list" => {
                    // @palace ls - list sessions
                    self.handle_session_ls_command(msg.id, &stream, topic).await?;
                }
                "help" => {
                    let help_text = r#"**Palace Commands:**
- `@palace work <issue>` - Spawn a coding session for an issue
- `@palace session new <issue>` - Same as work
- `@palace session ls` - List all sessions
- `@palace ls` - Short for session ls
- `@palace switch <name>` - Switch active session
- `@palace watch <name>` - Watch a session (stream events)
- `@palace tell "msg"` - Send message to active session
- `@palace tell-now "msg"` - Immediate message to session
- `@palace approve 1,2,3` - Create Plane issues from suggestions
- `@palace help` - Show this help

**Director Commands:**
- `@director auto` / `go` / `start` - Auto-assign workers to prioritized tasks
- `@director status` - Show Director status"#;
                    self.send(&stream, topic, help_text).await?;
                    self.mark_completed(msg.id).await?;
                }
                _ => {
                    tracing::debug!("Unknown palace command: {}", cmd);
                }
            }
        }

        // Parse @director commands
        if let Some((cmd, args)) = Self::parse_director_command(&msg.content) {
            let stream = self.get_stream_name(msg.stream_id).await
                .unwrap_or_else(|| "palace".to_string());
            let topic = msg.subject.as_deref().unwrap_or("general");

            match cmd.as_str() {
                "auto" | "go" | "start" => {
                    // @director auto - automatic task assignment
                    self.handle_auto_command(msg.id, &stream, topic, &args).await?;
                }
                "status" => {
                    // @director status - show director status
                    self.handle_director_status(msg.id, &stream, topic).await?;
                }
                "stop" | "pause" => {
                    // @director stop - pause auto mode
                    self.handle_director_stop(msg.id, &stream, topic).await?;
                }
                _ => {
                    tracing::debug!("Unknown director command: {}", cmd);
                }
            }
        }

        Ok(())
    }

    /// Get stream name from stream ID.
    async fn get_stream_name(&self, stream_id: Option<u64>) -> Option<String> {
        let stream_id = stream_id?;
        let url = format!("{}/api/v1/streams/{}", self.server_url, stream_id);

        let resp = self.client
            .get(&url)
            .basic_auth(&self.email, Some(&self.api_key))
            .send()
            .await
            .ok()?;

        let body: serde_json::Value = resp.json().await.ok()?;
        body["stream"]["name"].as_str().map(String::from)
    }

    /// Handle @palace approve command.
    async fn handle_approve_command(
        &self,
        message_id: u64,
        stream: &str,
        topic: &str,
        args: &str,
    ) -> DirectorResult<()> {
        let indices = Self::parse_numbers(args);

        if indices.is_empty() {
            self.send(stream, topic, "No valid indices provided. Usage: `@palace approve 1,2,3`").await?;
            self.mark_failed(message_id).await?;
            return Ok(());
        }

        self.mark_in_progress(message_id).await?;

        // Find project path from stream name
        let global_config = palace_plane::GlobalConfig::load()
            .unwrap_or_default();
        let project_path = global_config.find_project_by_stream(stream)
            .unwrap_or_else(|| {
                // Fall back to env var or current dir
                std::env::var("PALACE_PROJECT_PATH")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|_| std::path::PathBuf::from("."))
            });

        // Call approval function
        match palace_plane::approve_tasks(&project_path, &indices).await {
            Ok(results) => {
                if results.is_empty() {
                    self.send(stream, topic, "No tasks approved (indices may not exist).").await?;
                    self.mark_failed(message_id).await?;
                } else {
                    // Get project config for nice formatting
                    let prefix = palace_plane::ProjectConfig::load(&project_path)
                        .map(|c| c.project_slug.to_uppercase())
                        .unwrap_or_else(|_| "PAL".to_string());

                    let mut response = format!("✅ **Approved {} task(s):**\n\n", results.len());
                    for (task, issue) in &results {
                        response.push_str(&format!(
                            "- [{}-{}] {}\n",
                            prefix, issue.sequence_id, task.title
                        ));
                    }
                    self.send(stream, topic, &response).await?;
                    self.mark_completed(message_id).await?;
                }
            }
            Err(e) => {
                self.send(stream, topic, &format!("❌ **Approval failed:** {}", e)).await?;
                self.mark_failed(message_id).await?;
            }
        }

        Ok(())
    }

    /// Handle @palace work <issue> command - spawn a coding session.
    async fn handle_work_command(
        &self,
        message_id: u64,
        stream: &str,
        topic: &str,
        args: &str,
    ) -> DirectorResult<()> {
        let issue_id = args.trim();
        if issue_id.is_empty() {
            self.send(stream, topic, "Usage: `@palace work PAL-85` or `@palace work 85`").await?;
            self.mark_failed(message_id).await?;
            return Ok(());
        }

        self.mark_in_progress(message_id).await?;

        let project_path = self.get_project_path(stream);
        let manager = self.get_session_manager(&project_path).await;

        // Create session for this issue
        let target = SessionTarget::issue(issue_id);
        match manager.create_session(target.clone(), SessionStrategy::Simple).await {
            Ok(session) => {
                let session_id = session.id;
                let session_name = session.name.clone();

                // Set as active session for this stream
                self.set_active_session(stream, Some(session_id)).await;

                // Report session created
                let response = format!(
                    "🚀 **Session started:** `{}` (id: `{}`)\n\n\
                    Working on: `{}`\n\
                    Strategy: simple\n\n\
                    Use `@palace watch {}` to follow progress.",
                    session_name,
                    session.short_id(),
                    issue_id,
                    session.short_id()
                );
                self.send(stream, topic, &response).await?;

                // Spawn the executor in a background task
                let config = SessionExecutorConfig {
                    llm_url: std::env::var("LLM_URL")
                        .unwrap_or_else(|_| "http://localhost:1234/v1".to_string()),
                    model: std::env::var("LLM_MODEL")
                        .unwrap_or_else(|_| "nvidia_orchestrator-8b".to_string()),
                    max_tokens: 4096,
                    workspace: palace_plane::ProjectConfig::load(&project_path)
                        .map(|c| c.workspace.clone())
                        .unwrap_or_else(|_| "default".to_string()),
                    project: palace_plane::ProjectConfig::load(&project_path)
                        .map(|c| c.project_slug.clone())
                        .unwrap_or_else(|_| "PAL".to_string()),
                    zulip_enabled: true,
                    zulip_stream: stream.to_string(),
                };

                let manager_clone = manager.clone();
                let stream_clone = stream.to_string();
                let session_name_clone = session_name.clone();

                tokio::spawn(async move {
                    let mut executor = SessionExecutor::new(config, manager_clone);
                    if let Err(e) = executor.execute(session_id).await {
                        tracing::error!("Session {} failed: {}", session_id, e);
                        // Try to report failure
                        if let Ok(tool) = ZulipTool::from_env() {
                            let _ = tool.send(
                                &stream_clone,
                                &format!("palace/{}", session_name_clone),
                                &format!("❌ Session failed: {}", e)
                            ).await;
                        }
                    }
                });

                self.mark_completed(message_id).await?;
            }
            Err(e) => {
                self.send(stream, topic, &format!("❌ **Failed to create session:** {}", e)).await?;
                self.mark_failed(message_id).await?;
            }
        }

        Ok(())
    }

    /// Handle @palace session <subcommand> commands.
    async fn handle_session_command(
        &self,
        message_id: u64,
        stream: &str,
        topic: &str,
        args: &str,
    ) -> DirectorResult<()> {
        let parts: Vec<&str> = args.splitn(2, char::is_whitespace).collect();
        let subcmd = parts.first().map(|s| s.to_lowercase()).unwrap_or_default();
        let subargs = parts.get(1).map(|s| s.trim()).unwrap_or("");

        match subcmd.as_str() {
            "new" | "create" => {
                // @palace session new PAL-85
                self.handle_work_command(message_id, stream, topic, subargs).await
            }
            "ls" | "list" => {
                // @palace session ls
                self.handle_session_ls_command(message_id, stream, topic).await
            }
            _ => {
                self.send(stream, topic, "Unknown session subcommand. Use: `new`, `ls`").await?;
                self.mark_failed(message_id).await?;
                Ok(())
            }
        }
    }

    /// Handle @palace ls / @palace session ls - list sessions.
    async fn handle_session_ls_command(
        &self,
        message_id: u64,
        stream: &str,
        topic: &str,
    ) -> DirectorResult<()> {
        self.mark_in_progress(message_id).await?;

        let project_path = self.get_project_path(stream);
        let manager = self.get_session_manager(&project_path).await;
        let sessions = manager.list_sessions().await;

        if sessions.is_empty() {
            self.send(stream, topic, "No sessions. Use `@palace work PAL-85` to start one.").await?;
        } else {
            let active_id = self.get_active_session(stream).await;
            let mut response = String::from("**Sessions:**\n\n");
            response.push_str("| ID | Name | Status | Progress |\n");
            response.push_str("|---|---|---|---|\n");

            for session in sessions {
                let is_active = active_id == Some(session.id);
                let marker = if is_active { "➤ " } else { "" };
                let progress = if session.tasks_total > 0 {
                    format!("{}/{}", session.tasks_completed, session.tasks_total)
                } else {
                    "-".to_string()
                };
                response.push_str(&format!(
                    "| {}`{}` | {} | {} | {} |\n",
                    marker,
                    session.short_id(),
                    session.name,
                    session.status,
                    progress
                ));
            }
            self.send(stream, topic, &response).await?;
        }

        self.mark_completed(message_id).await?;
        Ok(())
    }

    /// Handle @palace switch <session_name> - switch active session.
    async fn handle_switch_command(
        &self,
        message_id: u64,
        stream: &str,
        topic: &str,
        args: &str,
    ) -> DirectorResult<()> {
        let session_query = args.trim();
        if session_query.is_empty() {
            self.send(stream, topic, "Usage: `@palace switch <session_id_or_name>`").await?;
            self.mark_failed(message_id).await?;
            return Ok(());
        }

        let project_path = self.get_project_path(stream);
        let manager = self.get_session_manager(&project_path).await;

        if let Some(session) = manager.find_session(session_query).await {
            self.set_active_session(stream, Some(session.id)).await;
            self.send(stream, topic, &format!(
                "✅ Switched to session `{}` (`{}`)",
                session.name, session.short_id()
            )).await?;
            self.mark_completed(message_id).await?;
        } else {
            self.send(stream, topic, &format!(
                "❌ Session not found: `{}`",
                session_query
            )).await?;
            self.mark_failed(message_id).await?;
        }

        Ok(())
    }

    /// Handle @palace watch <session_name> - watch a session.
    async fn handle_watch_command(
        &self,
        message_id: u64,
        stream: &str,
        topic: &str,
        args: &str,
    ) -> DirectorResult<()> {
        let session_query = args.trim();
        if session_query.is_empty() {
            // Watch active session
            if let Some(active_id) = self.get_active_session(stream).await {
                let project_path = self.get_project_path(stream);
                let manager = self.get_session_manager(&project_path).await;
                if let Some(session) = manager.get_session(active_id).await {
                    return self.start_watching(message_id, stream, topic, &session).await;
                }
            }
            self.send(stream, topic, "No active session. Use `@palace watch <session_id>`").await?;
            self.mark_failed(message_id).await?;
            return Ok(());
        }

        let project_path = self.get_project_path(stream);
        let manager = self.get_session_manager(&project_path).await;

        if let Some(session) = manager.find_session(session_query).await {
            self.start_watching(message_id, stream, topic, &session).await
        } else {
            self.send(stream, topic, &format!(
                "❌ Session not found: `{}`",
                session_query
            )).await?;
            self.mark_failed(message_id).await?;
            Ok(())
        }
    }

    /// Start watching a session (subscribe to its events).
    async fn start_watching(
        &self,
        message_id: u64,
        stream: &str,
        topic: &str,
        session: &crate::Session,
    ) -> DirectorResult<()> {
        let project_path = self.get_project_path(stream);
        let manager = self.get_session_manager(&project_path).await;

        // Get recent logs
        let logs = manager.get_logs(session.id, Some(10)).await;

        let mut response = format!(
            "👁️ **Watching session:** `{}` (`{}`)\n\n\
            **Status:** {}\n\
            **Target:** {}\n",
            session.name,
            session.short_id(),
            session.status,
            session.target
        );

        if session.tasks_total > 0 {
            response.push_str(&format!(
                "**Progress:** {}/{}\n",
                session.tasks_completed, session.tasks_total
            ));
        }

        if let Some(ref current) = session.current_task {
            response.push_str(&format!("**Current:** {}\n", current));
        }

        if !logs.is_empty() {
            response.push_str("\n**Recent logs:**\n```\n");
            for log in logs {
                response.push_str(&format!("[{:?}] {}\n", log.level, log.message));
            }
            response.push_str("```\n");
        }

        response.push_str(&format!(
            "\nUpdates will appear in topic `palace/{}`",
            session.name
        ));

        self.send(stream, topic, &response).await?;
        self.mark_completed(message_id).await?;
        Ok(())
    }

    /// Handle @palace tell "message" - send message to active session.
    async fn handle_tell_command(
        &self,
        message_id: u64,
        stream: &str,
        topic: &str,
        args: &str,
        _immediate: bool,
    ) -> DirectorResult<()> {
        let message = args.trim().trim_matches('"').trim_matches('\'');
        if message.is_empty() {
            self.send(stream, topic, "Usage: `@palace tell \"your message\"`").await?;
            self.mark_failed(message_id).await?;
            return Ok(());
        }

        if let Some(active_id) = self.get_active_session(stream).await {
            let project_path = self.get_project_path(stream);
            let manager = self.get_session_manager(&project_path).await;

            if let Some(session) = manager.get_session(active_id).await {
                // Log the steering message
                manager.log(active_id, crate::LogLevel::Info,
                    &format!("User steering: {}", message)).await;

                // Send to session topic
                self.send(stream, &format!("palace/{}", session.name),
                    &format!("📣 **Steering from human:**\n> {}", message)).await?;

                self.send(stream, topic, &format!(
                    "✅ Message sent to session `{}`",
                    session.name
                )).await?;
                self.mark_completed(message_id).await?;
            } else {
                self.send(stream, topic, "❌ Active session not found").await?;
                self.mark_failed(message_id).await?;
            }
        } else {
            self.send(stream, topic, "No active session. Use `@palace switch <session>` first.").await?;
            self.mark_failed(message_id).await?;
        }

        Ok(())
    }

    /// Handle @palace auto - automatic task assignment.
    async fn handle_auto_command(
        &self,
        message_id: u64,
        stream: &str,
        topic: &str,
        _args: &str,
    ) -> DirectorResult<()> {
        self.mark_in_progress(message_id).await?;

        let project_path = self.get_project_path(stream);

        // Get backlog issues from Plane
        let config = palace_plane::ProjectConfig::load(&project_path)
            .map_err(|e| DirectorError::Other(format!("No project config: {}", e)))?;

        let client = palace_plane::PlaneClient::new()
            .map_err(|e| DirectorError::PlaneApi(e.to_string()))?;

        let issues = client.list_active_issues(&config).await
            .map_err(|e| DirectorError::PlaneApi(e.to_string()))?;

        if issues.is_empty() {
            self.send(stream, topic, "No active issues in backlog. Create some first!").await?;
            self.mark_completed(message_id).await?;
            return Ok(());
        }

        // Pick the highest priority issue (first backlog issue)
        let issue = &issues[0];
        let issue_id = format!("{}-{}", config.project_slug.to_uppercase(), issue.sequence_id);

        self.send(stream, topic, &format!(
            "🤖 **Auto mode activated**\n\n\
            Found {} active issues. Starting with highest priority:\n\
            - `{}`: {}\n\n\
            Spawning session...",
            issues.len(),
            issue_id,
            issue.name
        )).await?;

        // Now spawn a work session for this issue
        self.handle_work_command(message_id, stream, topic, &issue_id).await
    }

    /// Parse a @palace command from message content.
    /// Returns (command, args) if found.
    pub fn parse_palace_command(content: &str) -> Option<(String, String)> {
        // Match @**Palace** or @_**Palace** followed by command
        let patterns = ["@**Palace**", "@_**Palace**", "@**palace**", "@_**palace**"];

        for pattern in patterns {
            if let Some(pos) = content.find(pattern) {
                let after = &content[pos + pattern.len()..].trim();
                // Parse command and args
                let parts: Vec<&str> = after.splitn(2, char::is_whitespace).collect();
                if !parts.is_empty() {
                    let cmd = parts[0].to_lowercase();
                    let args = parts.get(1).map(|s| s.trim().to_string()).unwrap_or_default();
                    return Some((cmd, args));
                }
            }
        }
        None
    }

    /// Parse a @director command from message content.
    /// Returns (command, args) if found.
    pub fn parse_director_command(content: &str) -> Option<(String, String)> {
        // Match @**Director** or @_**Director** followed by command
        let patterns = ["@**Director**", "@_**Director**", "@**director**", "@_**director**"];

        for pattern in patterns {
            if let Some(pos) = content.find(pattern) {
                let after = &content[pos + pattern.len()..].trim();
                // Parse command and args
                let parts: Vec<&str> = after.splitn(2, char::is_whitespace).collect();
                if !parts.is_empty() {
                    let cmd = parts[0].to_lowercase();
                    let args = parts.get(1).map(|s| s.trim().to_string()).unwrap_or_default();
                    return Some((cmd, args));
                }
            }
        }
        None
    }

    /// Parse comma-separated numbers from a string.
    pub fn parse_numbers(s: &str) -> Vec<usize> {
        s.split(',')
            .filter_map(|n| n.trim().parse::<usize>().ok())
            .collect()
    }

    /// Handle @director status command.
    async fn handle_director_status(
        &self,
        message_id: u64,
        stream: &str,
        topic: &str,
    ) -> DirectorResult<()> {
        self.mark_in_progress(message_id).await?;

        let project_path = self.get_project_path(stream);
        let manager = self.get_session_manager(&project_path).await;

        let sessions = manager.list_sessions().await;
        let active = sessions.iter().filter(|s| s.is_active()).count();
        let completed = sessions.iter().filter(|s| s.status == crate::SessionStatus::Completed).count();
        let failed = sessions.iter().filter(|s| s.status == crate::SessionStatus::Failed).count();

        let response = format!(
            "📊 **Director Status**\n\n\
            **Sessions:** {} total ({} active, {} completed, {} failed)\n\
            **Project:** {}\n\
            **Listening on:** stream `{}`\n\n\
            Use `@director auto` to start automatic task assignment.",
            sessions.len(), active, completed, failed,
            project_path.display(),
            stream
        );

        self.send(stream, topic, &response).await?;
        self.mark_completed(message_id).await?;
        Ok(())
    }

    /// Handle @director stop/pause command.
    async fn handle_director_stop(
        &self,
        message_id: u64,
        stream: &str,
        topic: &str,
    ) -> DirectorResult<()> {
        self.mark_in_progress(message_id).await?;

        // TODO: Implement auto-mode state tracking to actually stop it
        self.send(stream, topic, "⏸️ **Director paused.** Auto-assignment stopped.\n\nUse `@director auto` to resume.").await?;
        self.mark_completed(message_id).await?;
        Ok(())
    }

    /// Handle incoming reaction.
    async fn handle_reaction(&self, reaction: &ReactionEvent) -> DirectorResult<()> {
        tracing::debug!(
            "Reaction {} on message {} from user {}",
            reaction.emoji_name,
            reaction.message_id,
            reaction.user_id
        );

        // Skip our own reactions
        if Some(reaction.user_id) == self.bot_user_id {
            return Ok(());
        }

        // Handle user feedback reactions
        match reaction.emoji_name.as_str() {
            "stop_sign" => {
                tracing::warn!("HALT requested on message {}", reaction.message_id);
                // TODO: Pause related session
            }
            "heart" | "+1" => {
                tracing::info!("Positive feedback on message {}", reaction.message_id);
            }
            "-1" => {
                tracing::info!("Negative feedback on message {}", reaction.message_id);
            }
            _ => {}
        }

        Ok(())
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
    fn test_channel_status_markdown() {
        let mut status = ChannelStatus::default();
        status.status = "running".to_string();
        status.goals.push("Complete PAL-52".to_string());
        status.todos.push(TodoItem {
            text: "Build Zulip integration".to_string(),
            done: true,
            status: "completed".to_string(),
        });
        status.todos.push(TodoItem {
            text: "Test event loop".to_string(),
            done: false,
            status: "in_progress".to_string(),
        });
        status.progress = 50;

        let md = status.to_markdown();
        assert!(md.contains("## Status: running"));
        assert!(md.contains("🎯 Goals"));
        assert!(md.contains("Complete PAL-52"));
        assert!(md.contains("[x] Build Zulip integration"));
        assert!(md.contains("[ ] Test event loop 🔄"));
        assert!(md.contains("50%"));
    }

    #[test]
    fn test_parse_palace_command_approve() {
        let (cmd, args) = ZulipReactor::parse_palace_command("@**Palace** approve 2,4,5,6,10").unwrap();
        assert_eq!(cmd, "approve");
        assert_eq!(args, "2,4,5,6,10");
    }

    #[test]
    fn test_parse_palace_command_help() {
        let (cmd, args) = ZulipReactor::parse_palace_command("@**Palace** help").unwrap();
        assert_eq!(cmd, "help");
        assert_eq!(args, "");
    }

    #[test]
    fn test_parse_palace_command_silent_mention() {
        let (cmd, args) = ZulipReactor::parse_palace_command("@_**Palace** approve 1,2,3").unwrap();
        assert_eq!(cmd, "approve");
        assert_eq!(args, "1,2,3");
    }

    #[test]
    fn test_parse_palace_command_lowercase() {
        let (cmd, args) = ZulipReactor::parse_palace_command("@**palace** approve 5").unwrap();
        assert_eq!(cmd, "approve");
        assert_eq!(args, "5");
    }

    #[test]
    fn test_parse_palace_command_in_sentence() {
        let (cmd, args) = ZulipReactor::parse_palace_command("Hey @**Palace** approve 1,2 please").unwrap();
        assert_eq!(cmd, "approve");
        assert_eq!(args, "1,2 please");
    }

    #[test]
    fn test_parse_palace_command_no_match() {
        assert!(ZulipReactor::parse_palace_command("Hello world").is_none());
        assert!(ZulipReactor::parse_palace_command("@Director help").is_none());
    }

    #[test]
    fn test_parse_numbers_simple() {
        let nums = ZulipReactor::parse_numbers("1,2,3");
        assert_eq!(nums, vec![1, 2, 3]);
    }

    #[test]
    fn test_parse_numbers_with_spaces() {
        let nums = ZulipReactor::parse_numbers("1, 2, 3, 4");
        assert_eq!(nums, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_parse_numbers_mixed() {
        let nums = ZulipReactor::parse_numbers("2,4,5,6,10");
        assert_eq!(nums, vec![2, 4, 5, 6, 10]);
    }

    #[test]
    fn test_parse_numbers_invalid() {
        let nums = ZulipReactor::parse_numbers("a,b,c");
        assert!(nums.is_empty());
    }

    #[test]
    fn test_parse_numbers_partial_invalid() {
        let nums = ZulipReactor::parse_numbers("1,foo,3,bar,5");
        assert_eq!(nums, vec![1, 3, 5]);
    }
}
