//! Message types for Zulip.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A Zulip message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Message ID.
    pub id: u64,
    /// Stream name (for stream messages).
    pub stream: Option<String>,
    /// Topic (subject).
    pub topic: Option<String>,
    /// Message content.
    pub content: String,
    /// Message sender.
    pub sender: Sender,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
    /// Message type.
    pub message_type: MessageType,
}

/// Message sender.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sender {
    /// Sender ID.
    pub id: u64,
    /// Sender email.
    pub email: String,
    /// Sender display name.
    pub name: String,
}

/// Type of message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageType {
    /// Stream message (public).
    Stream,
    /// Private message (direct).
    Private,
}

impl Message {
    /// Check if this is a command (starts with /).
    pub fn is_command(&self) -> bool {
        self.content.trim().starts_with('/')
    }

    /// Extract command and args if this is a command.
    pub fn parse_command(&self) -> Option<(&str, Vec<&str>)> {
        if !self.is_command() {
            return None;
        }

        let content = self.content.trim();
        let mut parts = content.split_whitespace();
        let cmd = parts.next()?.strip_prefix('/')?;
        let args: Vec<&str> = parts.collect();

        Some((cmd, args))
    }

    /// Check if message mentions a specific user.
    pub fn mentions(&self, user: &str) -> bool {
        self.content.contains(&format!("@**{}**", user)) ||
        self.content.contains(&format!("@{}", user))
    }

    /// Check if message is from an agent (has agent prefix).
    pub fn is_from_agent(&self) -> bool {
        self.sender.email.starts_with("agent-") ||
        self.sender.name.starts_with("[") // [agent-name] prefix format
    }

    /// Extract the agent name if this is from an agent.
    pub fn agent_name(&self) -> Option<&str> {
        if self.content.starts_with('[') {
            let end = self.content.find(']')?;
            Some(&self.content[1..end])
        } else {
            None
        }
    }
}

/// Builder for creating messages.
pub struct MessageBuilder {
    stream: Option<String>,
    topic: Option<String>,
    content: String,
}

impl MessageBuilder {
    /// Create a new message builder.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            stream: None,
            topic: None,
            content: content.into(),
        }
    }

    /// Set the stream.
    pub fn stream(mut self, stream: impl Into<String>) -> Self {
        self.stream = Some(stream.into());
        self
    }

    /// Set the topic.
    pub fn topic(mut self, topic: impl Into<String>) -> Self {
        self.topic = Some(topic.into());
        self
    }

    /// Format content as code block.
    pub fn code(content: impl Into<String>, lang: Option<&str>) -> String {
        let lang = lang.unwrap_or("");
        format!("```{}\n{}\n```", lang, content.into())
    }

    /// Format content as quote.
    pub fn quote(content: impl Into<String>) -> String {
        content.into().lines()
            .map(|l| format!("> {}", l))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Format as bold.
    pub fn bold(text: impl Into<String>) -> String {
        format!("**{}**", text.into())
    }

    /// Format as italic.
    pub fn italic(text: impl Into<String>) -> String {
        format!("*{}*", text.into())
    }

    /// Format as user mention.
    pub fn mention(user: impl Into<String>) -> String {
        format!("@**{}**", user.into())
    }

    /// Format as stream link.
    pub fn stream_link(stream: impl Into<String>) -> String {
        format!("#**{}**", stream.into())
    }

    /// Get stream.
    pub fn get_stream(&self) -> Option<&str> {
        self.stream.as_deref()
    }

    /// Get topic.
    pub fn get_topic(&self) -> Option<&str> {
        self.topic.as_deref()
    }

    /// Get content.
    pub fn get_content(&self) -> &str {
        &self.content
    }
}
