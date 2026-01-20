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
use crate::{DirectorError, DirectorResult};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

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
        if msg.content.contains("@**Director**") || msg.content.contains("@_**Director**") {
            self.acknowledge(msg.id).await?;
        }

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
}
