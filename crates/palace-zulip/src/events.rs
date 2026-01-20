//! Real-time event streaming for Zulip.

use crate::{Message, ZulipError, ZulipResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc;

/// Event types from Zulip.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ZulipEvent {
    /// New message received.
    Message {
        message: EventMessage,
    },
    /// Reaction added.
    ReactionAdded {
        message_id: u64,
        user_id: u64,
        emoji_name: String,
    },
    /// Reaction removed.
    ReactionRemoved {
        message_id: u64,
        user_id: u64,
        emoji_name: String,
    },
    /// Stream updated.
    StreamUpdated {
        stream_id: u64,
        name: Option<String>,
        description: Option<String>,
    },
    /// Heartbeat.
    Heartbeat,
}

/// Message from event stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMessage {
    pub id: u64,
    pub sender_id: u64,
    pub sender_email: String,
    pub sender_full_name: String,
    pub stream_id: Option<u64>,
    pub display_recipient: DisplayRecipient,
    pub subject: String,
    pub content: String,
    pub timestamp: u64,
}

/// Display recipient can be stream name or user list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DisplayRecipient {
    Stream(String),
    Users(Vec<RecipientUser>),
}

/// User in private message recipient list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipientUser {
    pub id: u64,
    pub email: String,
    pub full_name: String,
}

/// Command parsed from a message.
#[derive(Debug, Clone)]
pub struct ParsedCommand {
    /// The command name (without /).
    pub name: String,
    /// Command arguments.
    pub args: Vec<String>,
    /// Named arguments (--key=value).
    pub named_args: HashMap<String, String>,
    /// The original message.
    pub message: Message,
}

impl ParsedCommand {
    /// Parse a command from a message.
    pub fn parse(message: &Message) -> Option<Self> {
        let content = message.content.trim();
        if !content.starts_with('/') {
            return None;
        }

        let mut parts = content.split_whitespace();
        let cmd = parts.next()?.strip_prefix('/')?;

        let mut args = Vec::new();
        let mut named_args = HashMap::new();

        for part in parts {
            if part.starts_with("--") {
                if let Some(eq_pos) = part.find('=') {
                    let key = &part[2..eq_pos];
                    let value = &part[eq_pos + 1..];
                    named_args.insert(key.to_string(), value.to_string());
                } else {
                    // Flag without value
                    named_args.insert(part[2..].to_string(), "true".to_string());
                }
            } else {
                args.push(part.to_string());
            }
        }

        Some(Self {
            name: cmd.to_string(),
            args,
            named_args,
            message: message.clone(),
        })
    }

    /// Get a named argument.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.named_args.get(key).map(|s| s.as_str())
    }

    /// Check if a flag is set.
    pub fn has_flag(&self, key: &str) -> bool {
        self.named_args.contains_key(key)
    }

    /// Get positional argument by index.
    pub fn arg(&self, index: usize) -> Option<&str> {
        self.args.get(index).map(|s| s.as_str())
    }
}

/// Event listener for real-time Zulip events.
pub struct EventListener {
    /// Event sender channel.
    tx: mpsc::Sender<ZulipEvent>,
    /// Event receiver channel.
    rx: mpsc::Receiver<ZulipEvent>,
    /// Queue ID for the event stream.
    queue_id: Option<String>,
    /// Last event ID received.
    last_event_id: i64,
}

impl EventListener {
    /// Create a new event listener.
    pub fn new(buffer_size: usize) -> Self {
        let (tx, rx) = mpsc::channel(buffer_size);
        Self {
            tx,
            rx,
            queue_id: None,
            last_event_id: -1,
        }
    }

    /// Get a clone of the sender for pushing events.
    pub fn sender(&self) -> mpsc::Sender<ZulipEvent> {
        self.tx.clone()
    }

    /// Receive the next event.
    pub async fn recv(&mut self) -> Option<ZulipEvent> {
        self.rx.recv().await
    }

    /// Set queue ID after registration.
    pub fn set_queue(&mut self, queue_id: String, last_event_id: i64) {
        self.queue_id = Some(queue_id);
        self.last_event_id = last_event_id;
    }

    /// Get queue ID if registered.
    pub fn queue_id(&self) -> Option<&str> {
        self.queue_id.as_deref()
    }

    /// Get last event ID.
    pub fn last_event_id(&self) -> i64 {
        self.last_event_id
    }

    /// Update last event ID.
    pub fn update_last_event(&mut self, id: i64) {
        self.last_event_id = id;
    }
}

/// Trait for command handlers.
#[async_trait::async_trait]
pub trait CommandHandler: Send + Sync {
    /// The command name this handler responds to.
    fn command(&self) -> &str;

    /// Handle the command.
    async fn handle(&self, cmd: &ParsedCommand) -> ZulipResult<String>;

    /// Get help text for this command.
    fn help(&self) -> &str;
}

/// Registry of command handlers.
pub struct CommandRegistry {
    handlers: HashMap<String, Box<dyn CommandHandler>>,
}

impl CommandRegistry {
    /// Create a new command registry.
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a command handler.
    pub fn register(&mut self, handler: impl CommandHandler + 'static) {
        self.handlers
            .insert(handler.command().to_string(), Box::new(handler));
    }

    /// Handle a command.
    pub async fn handle(&self, cmd: &ParsedCommand) -> ZulipResult<String> {
        if let Some(handler) = self.handlers.get(&cmd.name) {
            handler.handle(cmd).await
        } else if cmd.name == "help" {
            Ok(self.help_text())
        } else {
            Err(ZulipError::Other(format!("Unknown command: /{}", cmd.name)))
        }
    }

    /// Get help text for all commands.
    pub fn help_text(&self) -> String {
        let mut help = String::from("**Available commands:**\n\n");
        for (name, handler) in &self.handlers {
            help.push_str(&format!("- `/{name}`: {}\n", handler.help()));
        }
        help.push_str("- `/help`: Show this help message\n");
        help
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Standard Palace commands.
pub mod commands {
    use super::*;

    /// Status command - show agent/session status.
    pub struct StatusCommand;

    #[async_trait::async_trait]
    impl CommandHandler for StatusCommand {
        fn command(&self) -> &str {
            "status"
        }

        async fn handle(&self, _cmd: &ParsedCommand) -> ZulipResult<String> {
            // This will be filled in when integrated with session manager
            Ok("**Status**: Ready\n\nNo active sessions.".to_string())
        }

        fn help(&self) -> &str {
            "Show current status of agents and sessions"
        }
    }

    /// List command - list sessions/issues/etc.
    pub struct ListCommand;

    #[async_trait::async_trait]
    impl CommandHandler for ListCommand {
        fn command(&self) -> &str {
            "list"
        }

        async fn handle(&self, cmd: &ParsedCommand) -> ZulipResult<String> {
            let what = cmd.arg(0).unwrap_or("sessions");
            match what {
                "sessions" => Ok("**Sessions**: None active".to_string()),
                "issues" => Ok("**Issues**: Use `/list sessions` to see active work".to_string()),
                _ => Ok(format!("Unknown list type: {what}")),
            }
        }

        fn help(&self) -> &str {
            "List sessions, issues, or other resources"
        }
    }

    /// Start command - start a session.
    pub struct StartCommand;

    #[async_trait::async_trait]
    impl CommandHandler for StartCommand {
        fn command(&self) -> &str {
            "start"
        }

        async fn handle(&self, cmd: &ParsedCommand) -> ZulipResult<String> {
            if let Some(target) = cmd.arg(0) {
                Ok(format!("Starting session for target: {target}"))
            } else {
                Err(ZulipError::Other("Usage: /start <target>".to_string()))
            }
        }

        fn help(&self) -> &str {
            "Start a new session for an issue/cycle/module"
        }
    }

    /// Stop command - stop a session.
    pub struct StopCommand;

    #[async_trait::async_trait]
    impl CommandHandler for StopCommand {
        fn command(&self) -> &str {
            "stop"
        }

        async fn handle(&self, cmd: &ParsedCommand) -> ZulipResult<String> {
            if let Some(session) = cmd.arg(0) {
                Ok(format!("Stopping session: {session}"))
            } else {
                Ok("Stopping current session".to_string())
            }
        }

        fn help(&self) -> &str {
            "Stop a running session"
        }
    }

    /// Goal command - set/view goals.
    pub struct GoalCommand;

    #[async_trait::async_trait]
    impl CommandHandler for GoalCommand {
        fn command(&self) -> &str {
            "goal"
        }

        async fn handle(&self, cmd: &ParsedCommand) -> ZulipResult<String> {
            if cmd.args.is_empty() {
                Ok("**Current goals**: None set\n\nUse `/goal <description>` to set a goal.".to_string())
            } else {
                let goal = cmd.args.join(" ");
                Ok(format!("Goal set: {goal}"))
            }
        }

        fn help(&self) -> &str {
            "Set or view current goals"
        }
    }

    /// Priority command - adjust priority.
    pub struct PriorityCommand;

    #[async_trait::async_trait]
    impl CommandHandler for PriorityCommand {
        fn command(&self) -> &str {
            "priority"
        }

        async fn handle(&self, cmd: &ParsedCommand) -> ZulipResult<String> {
            let target = cmd.arg(0);
            let level = cmd.arg(1);

            match (target, level) {
                (Some(t), Some(l)) => Ok(format!("Set priority of {t} to {l}")),
                (Some(t), None) => Ok(format!("Priority of {t}: normal")),
                _ => Ok("Usage: /priority <target> [high|normal|low]".to_string()),
            }
        }

        fn help(&self) -> &str {
            "Set or view priority of issues/sessions"
        }
    }

    /// Create a default command registry with standard commands.
    pub fn default_registry() -> CommandRegistry {
        let mut registry = CommandRegistry::new();
        registry.register(StatusCommand);
        registry.register(ListCommand);
        registry.register(StartCommand);
        registry.register(StopCommand);
        registry.register(GoalCommand);
        registry.register(PriorityCommand);
        registry
    }
}
