//! Palace Daemon Bot for Zulip.
//!
//! The Palace Bot manages agent coordination, session control, and real-time
//! communication through Zulip channels.

use crate::{
    CommandHandler, CommandRegistry, ParsedCommand, ZulipClient, ZulipConfig,
    ZulipError, ZulipResult, Feedback, ReactionTracker,
};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Palace Daemon Bot configuration.
#[derive(Debug, Clone)]
pub struct PalaceBotConfig {
    /// Bot name displayed in messages.
    pub name: String,
    /// Bot email (for authentication).
    pub email: String,
    /// API key.
    pub api_key: String,
    /// Zulip server URL.
    pub server_url: String,
    /// Default stream for announcements.
    pub default_stream: String,
    /// Skip SSL verification (for self-signed certs).
    pub insecure: bool,
}

impl Default for PalaceBotConfig {
    fn default() -> Self {
        Self {
            name: "Palace".to_string(),
            email: "palace-bot@localhost".to_string(),
            api_key: String::new(),
            server_url: "https://localhost:8443".to_string(),
            default_stream: "general".to_string(),
            insecure: false,
        }
    }
}

impl PalaceBotConfig {
    /// Create from environment variables.
    /// Loads .env file if present.
    pub fn from_env() -> ZulipResult<Self> {
        // Load .env file if present
        let _ = dotenvy::dotenv();

        let insecure = std::env::var("ZULIP_INSECURE")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        Ok(Self {
            name: std::env::var("PALACE_BOT_NAME").unwrap_or_else(|_| "Palace".to_string()),
            email: std::env::var("ZULIP_BOT_EMAIL")
                .map_err(|_| ZulipError::Auth("ZULIP_BOT_EMAIL not set".to_string()))?,
            api_key: std::env::var("ZULIP_API_KEY")
                .map_err(|_| ZulipError::Auth("ZULIP_API_KEY not set".to_string()))?,
            server_url: std::env::var("ZULIP_SERVER_URL")
                .unwrap_or_else(|_| "https://localhost:8443".to_string()),
            default_stream: std::env::var("PALACE_DEFAULT_STREAM")
                .unwrap_or_else(|_| "general".to_string()),
            insecure,
        })
    }
}

/// Palace Daemon Bot state.
pub struct PalaceBotState {
    /// Active sessions (sessions = agents).
    pub sessions: Vec<SessionInfo>,
    /// Current goals.
    pub goals: Vec<String>,
    /// Reaction tracker.
    pub reactions: ReactionTracker,
}

impl Default for PalaceBotState {
    fn default() -> Self {
        Self {
            sessions: Vec::new(),
            goals: Vec::new(),
            reactions: ReactionTracker::new(),
        }
    }
}

/// Session info for display (session = agent).
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub target: String,
    pub status: String,
    pub current_task: Option<String>,
}

/// The Palace Daemon Bot.
pub struct PalaceBot {
    config: PalaceBotConfig,
    client: ZulipClient,
    commands: CommandRegistry,
    state: Arc<RwLock<PalaceBotState>>,
}

impl PalaceBot {
    /// Create a new Palace bot.
    pub fn new(config: PalaceBotConfig) -> ZulipResult<Self> {
        let zulip_config = ZulipConfig {
            server_url: config.server_url.parse().map_err(ZulipError::Url)?,
            email: config.email.clone(),
            api_key: config.api_key.clone(),
            insecure: config.insecure,
        };

        let client = ZulipClient::new(zulip_config)?;
        let mut commands = CommandRegistry::new();

        // Register Palace-specific commands
        commands.register(SessionsCommand);
        commands.register(StartSessionCommand);
        commands.register(StopSessionCommand);
        commands.register(StatusCommand);
        commands.register(GoalCommand);
        commands.register(PriorityCommand);
        commands.register(HelpCommand);

        Ok(Self {
            config,
            client,
            commands,
            state: Arc::new(RwLock::new(PalaceBotState::default())),
        })
    }

    /// Get bot name.
    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// Connect and validate.
    pub async fn connect(&self) -> ZulipResult<()> {
        self.client.validate_connection().await?;
        tracing::info!("Palace bot connected to Zulip");
        Ok(())
    }

    /// Send a message to a stream.
    pub async fn send(&self, stream: &str, topic: &str, content: &str) -> ZulipResult<u64> {
        let prefixed = format!("**[{}]** {}", self.config.name, content);
        self.client.send_message(stream, topic, &prefixed).await
    }

    /// Announce bot startup.
    pub async fn announce_startup(&self) -> ZulipResult<u64> {
        self.send(
            &self.config.default_stream,
            "status",
            &format!(
                "🚀 Palace Daemon online and ready for commands.\n\n\
                Mention me with a command: `@**{}** help`",
                self.config.name
            ),
        ).await
    }

    /// Handle an incoming message.
    pub async fn handle_message(&self, stream: &str, topic: &str, sender: &str, content: &str) -> ZulipResult<()> {
        // Skip our own messages
        if sender == self.config.email {
            return Ok(());
        }

        // Check if we're mentioned - Zulip uses @**Name** format
        let mentioned = self.is_mentioned(content);
        if !mentioned {
            return Ok(());
        }

        // Extract command from mention
        // Formats: "@**Palace** status" or "@**Palace** /status" or just "@**Palace**"
        let command_text = self.extract_command(content);

        let msg = crate::Message {
            id: 0,
            stream: Some(stream.to_string()),
            topic: Some(topic.to_string()),
            content: command_text.clone(),
            sender: crate::Sender {
                id: 0,
                email: sender.to_string(),
                name: sender.to_string(),
            },
            timestamp: chrono::Utc::now(),
            message_type: crate::MessageType::Stream,
        };

        // Parse and handle command
        if let Some(cmd) = ParsedCommand::parse(&msg) {
            let response = self.commands.handle(&cmd).await?;
            self.send(stream, topic, &response).await?;
        } else if command_text.trim().is_empty() {
            // Just mentioned with no command - show help
            let response = "👋 Hi! I'm the Palace daemon bot.\n\nTry: `@**Palace** status` or `@**Palace** help`".to_string();
            self.send(stream, topic, &response).await?;
        } else {
            // Try to interpret as a command without /
            let with_slash = format!("/{}", command_text.trim());
            let msg_with_slash = crate::Message {
                content: with_slash,
                ..msg
            };
            if let Some(cmd) = ParsedCommand::parse(&msg_with_slash) {
                let response = self.commands.handle(&cmd).await?;
                self.send(stream, topic, &response).await?;
            }
        }

        Ok(())
    }

    /// Check if bot is mentioned in message.
    fn is_mentioned(&self, content: &str) -> bool {
        // Zulip mention formats: @**Name** or @_**Name**
        let mention = format!("@**{}**", self.config.name);
        let silent_mention = format!("@_**{}**", self.config.name);
        content.contains(&mention) || content.contains(&silent_mention)
    }

    /// Extract command text after mention.
    fn extract_command(&self, content: &str) -> String {
        let mention = format!("@**{}**", self.config.name);
        let silent_mention = format!("@_**{}**", self.config.name);

        content
            .replace(&mention, "")
            .replace(&silent_mention, "")
            .trim()
            .to_string()
    }

    /// Handle a reaction.
    pub async fn handle_reaction(&self, message_id: u64, user: &str, emoji: &str) -> ZulipResult<()> {
        let mut state = self.state.write().await;
        let event = crate::ReactionEvent::new(message_id, 0, user, emoji);

        if let Some(feedback) = event.feedback {
            tracing::info!("Received {} feedback from {}: {:?}", emoji, user, feedback);

            if feedback.requires_wait() {
                tracing::warn!("Halt signal received from {}!", user);
            }
        }

        state.reactions.record(event);
        Ok(())
    }

    /// Get current state.
    pub async fn get_state(&self) -> PalaceBotState {
        self.state.read().await.clone()
    }

    /// Update sessions.
    pub async fn update_sessions(&self, sessions: Vec<SessionInfo>) {
        let mut state = self.state.write().await;
        state.sessions = sessions;
    }

    /// Get the Zulip client.
    pub fn client(&self) -> &ZulipClient {
        &self.client
    }

    /// Run the bot event loop.
    pub async fn run(&self) -> ZulipResult<()> {
        tracing::info!("Registering event queue...");
        let (queue_id, mut last_event_id) = self.client
            .register_event_queue(&["message"])
            .await?;
        tracing::info!("Event queue registered: {}", queue_id);

        loop {
            match self.client.get_events(&queue_id, last_event_id).await {
                Ok(events) => {
                    for event in events {
                        last_event_id = event.id;

                        if event.event_type == "message" {
                            if let Some(msg) = event.message {
                                // Extract stream name from display_recipient
                                let stream = if let Some(s) = msg.display_recipient.as_str() {
                                    s.to_string()
                                } else if let Some(obj) = msg.display_recipient.as_object() {
                                    obj.get("name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("unknown")
                                        .to_string()
                                } else {
                                    "unknown".to_string()
                                };

                                if let Err(e) = self.handle_message(
                                    &stream,
                                    &msg.subject,
                                    &msg.sender_email,
                                    &msg.content,
                                ).await {
                                    tracing::error!("Error handling message: {}", e);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Error getting events: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    }
}

impl Clone for PalaceBotState {
    fn clone(&self) -> Self {
        Self {
            sessions: self.sessions.clone(),
            goals: self.goals.clone(),
            reactions: ReactionTracker::new(), // Don't clone tracker
        }
    }
}

// === Command Implementations ===

struct SessionsCommand;

#[async_trait::async_trait]
impl CommandHandler for SessionsCommand {
    fn command(&self) -> &str { "sessions" }

    async fn handle(&self, _cmd: &ParsedCommand) -> ZulipResult<String> {
        // TODO: Integrate with actual session manager
        Ok("**Active Sessions:**\n\n_No active sessions. Use `/start <target>` to begin._".to_string())
    }

    fn help(&self) -> &str { "List all active sessions" }
}

struct StartSessionCommand;

#[async_trait::async_trait]
impl CommandHandler for StartSessionCommand {
    fn command(&self) -> &str { "start" }

    async fn handle(&self, cmd: &ParsedCommand) -> ZulipResult<String> {
        if let Some(target) = cmd.arg(0) {
            let strategy = cmd.get("strategy").unwrap_or("simple");
            Ok(format!(
                "🚀 Starting session for **{}** with `{}` strategy...\n\n\
                _Session created. Agents will begin work shortly._",
                target, strategy
            ))
        } else {
            Err(ZulipError::Other(
                "Usage: `/start <target> [--strategy=simple|parallel|priority]`\n\n\
                Examples:\n\
                - `/start PAL-52` - Start session for issue PAL-52\n\
                - `/start C0.2.0` - Start session for cycle C0.2.0\n\
                - `/start PAL-37,PAL-43` - Start multi-target session".to_string()
            ))
        }
    }

    fn help(&self) -> &str { "Start a new session for an issue/cycle/module" }
}

struct StopSessionCommand;

#[async_trait::async_trait]
impl CommandHandler for StopSessionCommand {
    fn command(&self) -> &str { "stop" }

    async fn handle(&self, cmd: &ParsedCommand) -> ZulipResult<String> {
        if let Some(session) = cmd.arg(0) {
            Ok(format!("⏹️ Stopping session **{}**...", session))
        } else {
            Ok("⏹️ Stopping current session...".to_string())
        }
    }

    fn help(&self) -> &str { "Stop a running session" }
}

struct StatusCommand;

#[async_trait::async_trait]
impl CommandHandler for StatusCommand {
    fn command(&self) -> &str { "status" }

    async fn handle(&self, _cmd: &ParsedCommand) -> ZulipResult<String> {
        Ok("**Palace Daemon Status**\n\n\
            🟢 **Status:** Online\n\
            📊 **Sessions:** 0 active\n\
            🎯 **Goals:** None set\n\n\
            _Mention me: `@**Palace** start PAL-52`_".to_string())
    }

    fn help(&self) -> &str { "Show current daemon status" }
}

struct GoalCommand;

#[async_trait::async_trait]
impl CommandHandler for GoalCommand {
    fn command(&self) -> &str { "goal" }

    async fn handle(&self, cmd: &ParsedCommand) -> ZulipResult<String> {
        if cmd.args.is_empty() {
            Ok("**Current Goals:**\n\n_No goals set. Use `/goal <description>` to add one._".to_string())
        } else {
            let goal = cmd.args.join(" ");
            Ok(format!("🎯 Goal set: **{}**", goal))
        }
    }

    fn help(&self) -> &str { "Set or view current goals" }
}

struct PriorityCommand;

#[async_trait::async_trait]
impl CommandHandler for PriorityCommand {
    fn command(&self) -> &str { "priority" }

    async fn handle(&self, cmd: &ParsedCommand) -> ZulipResult<String> {
        match (cmd.arg(0), cmd.arg(1)) {
            (Some(target), Some(level)) => {
                Ok(format!("📌 Set priority of **{}** to **{}**", target, level))
            }
            (Some(target), None) => {
                Ok(format!("📌 Priority of **{}**: normal", target))
            }
            _ => {
                Ok("Usage: `/priority <target> [high|normal|low]`".to_string())
            }
        }
    }

    fn help(&self) -> &str { "Set or view priority of issues/sessions" }
}

struct HelpCommand;

#[async_trait::async_trait]
impl CommandHandler for HelpCommand {
    fn command(&self) -> &str { "help" }

    async fn handle(&self, _cmd: &ParsedCommand) -> ZulipResult<String> {
        Ok("**Palace Daemon Commands**\n\n\
            Mention me: `@**Palace** <command>`\n\n\
            📋 **Sessions (= Agents):**\n\
            - `sessions` - List active sessions\n\
            - `start <target>` - Start session for issue/cycle/module\n\
            - `stop [id]` - Stop a session\n\
            - `status` - Show daemon status\n\n\
            🎯 **Goals & Priority:**\n\
            - `goal [text]` - Set or view goals\n\
            - `priority <target> [high|normal|low]`\n\n\
            💡 **Feedback:**\n\
            - 👍/👎 = soft approve/disapprove\n\
            - ❤️ = great idea\n\
            - 🛑 = halt, wait for me\n\n\
            **Examples:**\n\
            `@**Palace** start PAL-52`\n\
            `@**Palace** start PAL-37,PAL-43 --strategy=parallel`".to_string())
    }

    fn help(&self) -> &str { "Show this help message" }
}
