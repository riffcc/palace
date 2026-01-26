//! Zulip Reporter - automatic session messaging to Zulip.
//!
//! Reports session progress, tool calls, blockers, and surveys to Zulip
//! for real-time visibility and feedback.

use crate::zulip_tool::ZulipTool;
use crate::{DirectorResult, SessionLogEntry, LogLevel};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Message slot types for tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageSlot {
    /// Session status (first message)
    Status,
    /// Task list (second message, live-edited)
    Tasks,
    /// Current progress/activity
    Progress,
    /// Active poll (only one at a time)
    Poll,
    /// Blocker message
    Blocker,
}

/// Zulip reporter for session events.
pub struct ZulipReporter {
    tool: ZulipTool,
    /// Stream to report to (default: "palace")
    stream: String,
    /// Topic prefix for sessions
    session_prefix: String,
    /// Track message IDs by session and slot (for reuse/editing)
    messages: HashMap<(Uuid, MessageSlot), u64>,
    /// Track pending surveys for response handling
    pending_surveys: HashMap<Uuid, Vec<SurveyMessage>>,
}

impl ZulipReporter {
    /// Create from environment (uses Palace bot credentials).
    pub fn from_env() -> DirectorResult<Self> {
        let tool = ZulipTool::from_env_palace()?;
        Ok(Self {
            tool,
            stream: "palace".to_string(),
            session_prefix: "session".to_string(),
            messages: HashMap::new(),
            pending_surveys: HashMap::new(),
        })
    }

    /// Get or create a message in a slot.
    /// If the slot has an existing message, update it; otherwise create new.
    async fn slot_message(
        &mut self,
        session_id: Uuid,
        name: &str,
        slot: MessageSlot,
        content: &str,
    ) -> DirectorResult<u64> {
        let topic = self.session_topic(session_id, name);
        let key = (session_id, slot);

        if let Some(&msg_id) = self.messages.get(&key) {
            // Update existing message
            self.tool.update_message(msg_id, content).await?;
            Ok(msg_id)
        } else {
            // Create new message
            let msg_id = self.tool.send(&self.stream, &topic, content).await?;
            self.messages.insert(key, msg_id);
            Ok(msg_id)
        }
    }

    /// Delete a message slot if it exists.
    async fn delete_slot(&mut self, session_id: Uuid, slot: MessageSlot) -> DirectorResult<()> {
        let key = (session_id, slot);
        if let Some(msg_id) = self.messages.remove(&key) {
            // Ignore errors on delete (message might already be gone)
            let _ = self.tool.delete_message(msg_id).await;
        }
        Ok(())
    }

    /// Clean up all messages for a session.
    pub async fn cleanup_session(&mut self, session_id: Uuid) -> DirectorResult<()> {
        let slots = [
            MessageSlot::Progress,
            MessageSlot::Poll,
            MessageSlot::Blocker,
        ];
        for slot in slots {
            self.delete_slot(session_id, slot).await?;
        }
        // Keep Status and Tasks for history
        Ok(())
    }

    /// Create with custom stream.
    pub fn with_stream(mut self, stream: impl Into<String>) -> Self {
        self.stream = stream.into();
        self
    }

    /// Get the topic for a session.
    fn session_topic(&self, _session_id: Uuid, session_name: &str) -> String {
        // Topic is just the session name: issue/PAL-88, module/FOO, etc.
        session_name.replace(":", "/")
    }

    /// Report session started (Status slot - persists).
    pub async fn session_started(&mut self, session_id: Uuid, name: &str, target: &str) -> DirectorResult<u64> {
        let content = format!(
            "🚀 **Session Started**\n\n\
            **Target:** `{}`\n\
            **ID:** `{}`\n\n\
            *React with emoji to provide feedback:*\n\
            ❤️ = great | 👍/👎 = feedback | 🛑 = halt",
            target, session_id
        );
        self.slot_message(session_id, name, MessageSlot::Status, &content).await
    }

    /// Report session progress (Progress slot - updates in place).
    pub async fn session_progress(
        &mut self,
        session_id: Uuid,
        name: &str,
        completed: u32,
        total: u32,
        current_task: &str,
    ) -> DirectorResult<u64> {
        let percent = if total > 0 { (completed * 100) / total } else { 0 };
        let bar = progress_bar(percent);

        let content = format!(
            "📊 **Progress** {} {}/{}  ({}%)\n\n\
            **Current:** {}",
            bar, completed, total, percent, current_task
        );
        self.slot_message(session_id, name, MessageSlot::Progress, &content).await
    }

    /// Report a tool call (appended to Progress slot).
    pub async fn tool_call(
        &mut self,
        session_id: Uuid,
        name: &str,
        tool_name: &str,
        description: &str,
    ) -> DirectorResult<u64> {
        let content = format!(
            "🔧 `{}` - {}",
            tool_name, description
        );
        self.slot_message(session_id, name, MessageSlot::Progress, &content).await
    }

    /// Report a blocker (Blocker slot - one at a time, deleted when resolved).
    pub async fn blocker(
        &mut self,
        session_id: Uuid,
        name: &str,
        issue: &str,
        options: &[&str],
    ) -> DirectorResult<u64> {
        let options_text = options.iter()
            .enumerate()
            .map(|(i, o)| format!("{}. {}", i + 1, o))
            .collect::<Vec<_>>()
            .join("\n");

        let content = format!(
            "🚧 **Blocker**\n\n\
            **Issue:** {}\n\n\
            **Options:**\n{}\n\n\
            *Reply with choice or guidance.*",
            issue, options_text
        );
        self.slot_message(session_id, name, MessageSlot::Blocker, &content).await
    }

    /// Clear blocker when resolved.
    pub async fn clear_blocker(&mut self, session_id: Uuid) -> DirectorResult<()> {
        self.delete_slot(session_id, MessageSlot::Blocker).await
    }

    /// Report session completed (updates Status slot, cleans up transient messages).
    pub async fn session_completed(
        &mut self,
        session_id: Uuid,
        name: &str,
        summary: &str,
    ) -> DirectorResult<u64> {
        // Clean up transient slots
        self.cleanup_session(session_id).await?;

        // Update status to completed
        let content = format!(
            "✅ **Session Completed**\n\n\
            {}\n\n\
            *Session `{}` finished.*",
            summary, &session_id.to_string()[..8]
        );
        self.slot_message(session_id, name, MessageSlot::Status, &content).await
    }

    /// Report session failed (updates Status slot, keeps Blocker for context).
    pub async fn session_failed(
        &mut self,
        session_id: Uuid,
        name: &str,
        error: &str,
    ) -> DirectorResult<u64> {
        // Clean up progress but keep blocker for context
        self.delete_slot(session_id, MessageSlot::Progress).await?;
        self.delete_slot(session_id, MessageSlot::Poll).await?;

        let content = format!(
            "❌ **Session Failed**\n\n\
            **Error:** {}\n\n\
            *Session `{}`* | React 🔄 to retry",
            error, &session_id.to_string()[..8]
        );
        self.slot_message(session_id, name, MessageSlot::Status, &content).await
    }

    /// Send a native poll (Poll slot - replaces previous poll).
    pub async fn poll(
        &mut self,
        session_id: Uuid,
        name: &str,
        question: &str,
        options: &[&str],
    ) -> DirectorResult<u64> {
        // Delete any existing poll first (native polls can't be edited)
        self.delete_slot(session_id, MessageSlot::Poll).await?;

        let topic = self.session_topic(session_id, name);
        let msg_id = self.tool.send_poll(&self.stream, &topic, question, options).await?;
        self.messages.insert((session_id, MessageSlot::Poll), msg_id);
        Ok(msg_id)
    }

    /// Clear poll when answered.
    pub async fn clear_poll(&mut self, session_id: Uuid) -> DirectorResult<()> {
        self.delete_slot(session_id, MessageSlot::Poll).await
    }

    /// Send a survey with emoji options (legacy, use poll() for native).
    pub async fn survey(
        &mut self,
        session_id: Uuid,
        name: &str,
        question: &str,
        options: &[SurveyOption],
    ) -> DirectorResult<u64> {
        let options_text = options.iter()
            .map(|o| format!("{} = {}", o.emoji, o.label))
            .collect::<Vec<_>>()
            .join(" | ");

        let content = format!(
            "❓ {}\n\n*React:* {}",
            question, options_text
        );
        let msg_id = self.slot_message(session_id, name, MessageSlot::Poll, &content).await?;

        // Track pending survey
        let surveys = self.pending_surveys.entry(session_id).or_default();
        surveys.push(SurveyMessage {
            message_id: msg_id,
            question: question.to_string(),
            options: options.to_vec(),
        });

        Ok(msg_id)
    }

    /// Report a log entry (appends to stream, not slotted).
    pub async fn log_entry(
        &self,
        session_id: Uuid,
        name: &str,
        entry: &SessionLogEntry,
    ) -> DirectorResult<u64> {
        let topic = self.session_topic(session_id, name);
        let emoji = match entry.level {
            LogLevel::Debug => "🔍",
            LogLevel::Info => "ℹ️",
            LogLevel::Warn => "⚠️",
            LogLevel::Error => "❌",
        };
        let content = format!("{} {}", emoji, entry.message);
        self.tool.send(&self.stream, &topic, &content).await
    }

    /// Send a generic message to a session topic.
    pub async fn message(
        &self,
        session_id: Uuid,
        name: &str,
        content: &str,
    ) -> DirectorResult<u64> {
        let topic = self.session_topic(session_id, name);
        self.tool.send(&self.stream, &topic, content).await
    }

    /// Send a command to Palace bot.
    pub async fn palace_command(&self, command: &str) -> DirectorResult<u64> {
        self.tool.palace(command).await
    }

    /// Send to arbitrary stream/topic.
    pub async fn send(&self, stream: &str, topic: &str, content: &str) -> DirectorResult<u64> {
        self.tool.send(stream, topic, content).await
    }

    /// Create or update a session's task list (Tasks slot - editable).
    pub async fn update_tasks(
        &mut self,
        session_id: Uuid,
        name: &str,
        title: &str,
        tasks: &[TodoTask],
    ) -> DirectorResult<u64> {
        // Format tasks as markdown checkboxes
        let tasks_md = tasks.iter()
            .map(|t| {
                let check = if t.completed { "x" } else { " " };
                format!("- [{}] {}", check, t.description)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let content = format!("📋 **{}**\n\n{}", title, tasks_md);
        self.slot_message(session_id, name, MessageSlot::Tasks, &content).await
    }

    /// Initialize a session with status and task list.
    pub async fn init_session_with_tasks(
        &mut self,
        session_id: Uuid,
        name: &str,
        target: &str,
        tasks: &[&str],
    ) -> DirectorResult<()> {
        // First message: session status
        self.session_started(session_id, name, target).await?;

        // Second message: task list (will be live-edited)
        let todo_tasks: Vec<TodoTask> = tasks.iter()
            .map(|t| TodoTask::new(t))
            .collect();
        self.update_tasks(session_id, name, "Tasks", &todo_tasks).await?;

        Ok(())
    }

    /// Mark a task as completed and update the task list.
    pub async fn complete_task(
        &mut self,
        session_id: Uuid,
        name: &str,
        tasks: &mut [TodoTask],
        task_index: usize,
    ) -> DirectorResult<u64> {
        if task_index < tasks.len() {
            tasks[task_index].completed = true;
        }
        self.update_tasks(session_id, name, "Tasks", tasks).await
    }
}

/// A task in a todo list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoTask {
    pub description: String,
    pub completed: bool,
}

impl TodoTask {
    pub fn new(description: &str) -> Self {
        Self {
            description: description.to_string(),
            completed: false,
        }
    }

    pub fn completed(description: &str) -> Self {
        Self {
            description: description.to_string(),
            completed: true,
        }
    }
}

/// Survey option with emoji and label.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurveyOption {
    pub emoji: String,
    pub label: String,
    pub value: String,
}

impl SurveyOption {
    /// Create a new survey option.
    pub fn new(emoji: &str, label: &str, value: &str) -> Self {
        Self {
            emoji: emoji.to_string(),
            label: label.to_string(),
            value: value.to_string(),
        }
    }
}

/// Pending survey message.
#[derive(Debug, Clone)]
struct SurveyMessage {
    message_id: u64,
    question: String,
    options: Vec<SurveyOption>,
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
    fn test_progress_bar() {
        assert_eq!(progress_bar(0), "[░░░░░░░░░░]");
        assert_eq!(progress_bar(50), "[█████░░░░░]");
        assert_eq!(progress_bar(100), "[██████████]");
    }

    #[test]
    fn test_survey_option() {
        let opt = SurveyOption::new("👍", "Yes", "yes");
        assert_eq!(opt.emoji, "👍");
        assert_eq!(opt.label, "Yes");
        assert_eq!(opt.value, "yes");
    }
}
