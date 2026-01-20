//! Palace-Zulip: Real-time communication for Palace agents.
//!
//! Provides Zulip Chat integration for:
//! - Agent-to-agent communication
//! - Operator-to-agent messaging
//! - Real-time monitoring of agent activity
//! - Topic-based organization (project/cycle/module/issue)
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    PALACE ZULIP                              │
//! │                                                              │
//! │  ┌──────────┐  ┌───────────┐  ┌──────────┐  ┌────────────┐ │
//! │  │ Operator │──│ ZulipHub  │──│  Agents  │──│ Dispatcher │ │
//! │  └──────────┘  └───────────┘  └──────────┘  └────────────┘ │
//! │                     │                            │          │
//! │  ┌─────────────────────────────────────────────────────┐   │
//! │  │                  Channels                            │   │
//! │  │  #general │ #project-PAL │ #cycle-C0.2 │ #issue-37  │   │
//! │  └─────────────────────────────────────────────────────┘   │
//! │                                                              │
//! └──────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Channel Conventions
//!
//! - `#general` - Global announcements, cross-project discussion
//! - `#project-{id}` - Project-specific channel (e.g., #project-PAL)
//! - `#cycle-{id}` - Cycle discussion (e.g., #cycle-C0.2.0)
//! - `#module-{id}` - Module discussion (e.g., #module-M001)
//! - `#agent-{id}` - Agent-specific channel for monitoring
//! - Within each channel, topics organize by issue/task

mod bot;
mod client;
mod error;
mod events;
mod messages;
mod reactions;
mod status_board;
mod streams;

pub use bot::{PalaceBot, PalaceBotConfig, PalaceBotState, SessionInfo};
pub use client::{ZulipClient, ZulipConfig};
pub use error::{ZulipError, ZulipResult};
pub use events::{
    ZulipEvent, EventListener, ParsedCommand, CommandHandler, CommandRegistry,
    commands as standard_commands,
};
pub use messages::{Message, MessageBuilder, MessageType, Sender};
pub use reactions::{Feedback, ReactionEvent, ReactionTracker, SurveyQuestion};
pub use status_board::{StatusBoard, StatusBoardManager, StatusEntry, StatusKind, Intent, parse_intent};
pub use streams::{Stream, Topic, StreamType, streams as standard_streams, topics as standard_topics};

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Central hub for Zulip communication.
pub struct ZulipHub {
    /// Zulip API client.
    client: Arc<ZulipClient>,
    /// Active subscriptions.
    subscriptions: Arc<RwLock<HashMap<String, broadcast::Sender<Message>>>>,
    /// Message handler callbacks.
    handlers: Arc<RwLock<Vec<Box<dyn MessageHandler + Send + Sync>>>>,
}

/// Handler for incoming messages.
#[async_trait::async_trait]
pub trait MessageHandler: Send + Sync {
    /// Handle an incoming message.
    async fn handle(&self, message: &Message) -> ZulipResult<Option<String>>;

    /// Check if this handler should process the message.
    fn should_handle(&self, message: &Message) -> bool;
}

impl ZulipHub {
    /// Create a new Zulip hub.
    pub fn new(config: ZulipConfig) -> ZulipResult<Self> {
        let client = ZulipClient::new(config)?;

        Ok(Self {
            client: Arc::new(client),
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            handlers: Arc::new(RwLock::new(Vec::new())),
        })
    }

    /// Connect to Zulip and start listening.
    pub async fn connect(&self) -> ZulipResult<()> {
        self.client.validate_connection().await?;
        tracing::info!("Connected to Zulip");
        Ok(())
    }

    /// Register a message handler.
    pub async fn register_handler(&self, handler: impl MessageHandler + 'static) {
        let mut handlers = self.handlers.write().await;
        handlers.push(Box::new(handler));
    }

    /// Subscribe to a stream.
    pub async fn subscribe(&self, stream_name: &str) -> ZulipResult<broadcast::Receiver<Message>> {
        let mut subs = self.subscriptions.write().await;

        if let Some(tx) = subs.get(stream_name) {
            return Ok(tx.subscribe());
        }

        // Create new subscription
        let (tx, rx) = broadcast::channel(1000);
        subs.insert(stream_name.to_string(), tx);

        // Subscribe to stream via API
        self.client.subscribe_to_stream(stream_name).await?;

        Ok(rx)
    }

    /// Send a message to a stream/topic.
    pub async fn send(&self, stream: &str, topic: &str, content: &str) -> ZulipResult<u64> {
        self.client.send_message(stream, topic, content).await
    }

    /// Send a message to the appropriate channel for a target.
    pub async fn send_for_target(
        &self,
        target: &TargetChannel,
        topic: &str,
        content: &str,
    ) -> ZulipResult<u64> {
        let stream = target.stream_name();
        self.send(&stream, topic, content).await
    }

    /// Create standard Palace streams.
    pub async fn setup_palace_streams(&self, project_id: &str) -> ZulipResult<()> {
        // Create project stream
        self.client.create_stream(&format!("project-{}", project_id), "Project discussion").await?;

        // Create agent stream
        self.client.create_stream("agents", "Agent coordination and status").await?;

        // Create general stream
        self.client.create_stream("general", "General discussion").await?;

        Ok(())
    }

    /// Get the underlying client.
    pub fn client(&self) -> &ZulipClient {
        &self.client
    }
}

/// Target channel for messaging.
#[derive(Debug, Clone)]
pub enum TargetChannel {
    /// General announcements.
    General,
    /// Project-specific.
    Project(String),
    /// Cycle-specific.
    Cycle(String),
    /// Module-specific.
    Module(String),
    /// Issue-specific (uses project stream with issue topic).
    Issue { project: String, issue: String },
    /// Agent-specific.
    Agent(String),
}

impl TargetChannel {
    /// Get the stream name for this target.
    pub fn stream_name(&self) -> String {
        match self {
            TargetChannel::General => "general".to_string(),
            TargetChannel::Project(id) => format!("project-{}", id),
            TargetChannel::Cycle(id) => format!("cycle-{}", id),
            TargetChannel::Module(id) => format!("module-{}", id),
            TargetChannel::Issue { project, .. } => format!("project-{}", project),
            TargetChannel::Agent(id) => format!("agent-{}", id),
        }
    }

    /// Get the default topic for this target.
    pub fn default_topic(&self) -> String {
        match self {
            TargetChannel::General => "announcements".to_string(),
            TargetChannel::Project(_) => "general".to_string(),
            TargetChannel::Cycle(id) => format!("cycle-{}", id),
            TargetChannel::Module(id) => format!("module-{}", id),
            TargetChannel::Issue { issue, .. } => format!("issue-{}", issue),
            TargetChannel::Agent(id) => format!("agent-{}", id),
        }
    }
}

/// Agent identity for Zulip messaging.
#[derive(Debug, Clone)]
pub struct AgentIdentity {
    /// Agent ID (usually session ID).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Current project.
    pub project: Option<String>,
    /// Current target being worked on.
    pub target: Option<String>,
}

impl AgentIdentity {
    /// Create a new agent identity.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            project: None,
            target: None,
        }
    }

    /// Set the project.
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Set the target.
    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }
}

/// Agent messaging interface.
pub struct AgentMessenger {
    hub: Arc<ZulipHub>,
    identity: AgentIdentity,
}

impl AgentMessenger {
    /// Create a new agent messenger.
    pub fn new(hub: Arc<ZulipHub>, identity: AgentIdentity) -> Self {
        Self { hub, identity }
    }

    /// Announce agent status.
    pub async fn announce_status(&self, status: &str) -> ZulipResult<u64> {
        let content = format!("**{}**: {}", self.identity.name, status);
        self.hub.send("agents", &self.identity.id, &content).await
    }

    /// Send a message to the project channel.
    pub async fn send_to_project(&self, topic: &str, content: &str) -> ZulipResult<u64> {
        if let Some(project) = &self.identity.project {
            let stream = format!("project-{}", project);
            let msg = format!("[{}] {}", self.identity.name, content);
            self.hub.send(&stream, topic, &msg).await
        } else {
            Err(ZulipError::NoProject)
        }
    }

    /// Log progress on current task.
    pub async fn log_progress(&self, message: &str) -> ZulipResult<u64> {
        let topic = self.identity.target.as_deref().unwrap_or("progress");
        self.send_to_project(topic, message).await
    }

    /// Request help from operator.
    pub async fn request_help(&self, question: &str) -> ZulipResult<u64> {
        let content = format!("**HELP NEEDED** from {}: {}", self.identity.name, question);
        if let Some(project) = &self.identity.project {
            let stream = format!("project-{}", project);
            self.hub.send(&stream, "help-requests", &content).await
        } else {
            self.hub.send("general", "help-requests", &content).await
        }
    }

    /// Communicate with another agent.
    pub async fn message_agent(&self, agent_id: &str, content: &str) -> ZulipResult<u64> {
        let stream = format!("agent-{}", agent_id);
        let msg = format!("[from {}] {}", self.identity.name, content);
        self.hub.send(&stream, "messages", &msg).await
    }

    /// Broadcast to all agents.
    pub async fn broadcast(&self, content: &str) -> ZulipResult<u64> {
        let msg = format!("[{}] {}", self.identity.name, content);
        self.hub.send("agents", "broadcast", &msg).await
    }
}
