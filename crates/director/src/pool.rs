//! Director Pool: Multiple Directors in one daemon.
//!
//! Single process hosting N Directors with dual communication paths:
//! - **Telepathy**: Instant intra-node via shared channels (works if Zulip down)
//! - **Zulip**: Always logged for audit trail and multi-node visibility
//!
//! Verbosity controls what gets logged to Zulip (Quiet/Normal/Verbose/Debug).

use crate::{DirectorError, DirectorResult, ModelRegistry, ZulipTool, Verbosity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use uuid::Uuid;

/// Message between Directors (telepathy mode).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelepathyMessage {
    pub from: String,
    pub to: Option<String>, // None = broadcast
    pub kind: TelepathyKind,
    pub payload: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelepathyKind {
    /// Handoff work to another Director.
    Handoff,
    /// Request assistance.
    Assist,
    /// Share context/findings.
    Share,
    /// Claim work ("I have the controls").
    Claim,
    /// Release work ("You have the controls").
    Release,
    /// Status update.
    Status,
    /// Query another Director.
    Query,
    /// Response to query.
    Response,
}

/// A single Director instance within the pool.
pub struct DirectorInstance {
    pub name: String,
    pub model: String,
    pub session_id: Option<Uuid>,
    pub status: DirectorStatus,
    /// Channel for receiving telepathy messages.
    telepathy_rx: mpsc::Receiver<TelepathyMessage>,
    /// Handle for sending to the pool's broadcast.
    pool_tx: broadcast::Sender<TelepathyMessage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectorStatus {
    Idle,
    Working,
    Blocked,
    Offline,
}

impl DirectorInstance {
    /// Send a telepathy message to another Director (or broadcast).
    /// This is instant locally; Zulip logging happens at pool level.
    pub async fn send(&self, to: Option<&str>, kind: TelepathyKind, payload: &str) {
        let msg = TelepathyMessage {
            from: self.name.clone(),
            to: to.map(String::from),
            kind: kind.clone(),
            payload: payload.to_string(),
        };
        let _ = self.pool_tx.send(msg);
    }

    /// Receive next telepathy message (if any).
    pub async fn recv(&mut self) -> Option<TelepathyMessage> {
        self.telepathy_rx.recv().await
    }

    /// Try to receive without blocking.
    pub fn try_recv(&mut self) -> Option<TelepathyMessage> {
        self.telepathy_rx.try_recv().ok()
    }
}

impl TelepathyKind {
    /// Minimum verbosity needed to log this message type.
    pub fn min_verbosity(&self) -> Verbosity {
        match self {
            TelepathyKind::Handoff => Verbosity::Normal,
            TelepathyKind::Assist => Verbosity::Normal,
            TelepathyKind::Share => Verbosity::Verbose,
            TelepathyKind::Claim => Verbosity::Normal,
            TelepathyKind::Release => Verbosity::Normal,
            TelepathyKind::Status => Verbosity::Verbose,
            TelepathyKind::Query => Verbosity::Debug,
            TelepathyKind::Response => Verbosity::Debug,
        }
    }

    /// Emoji for this message type.
    pub fn emoji(&self) -> &'static str {
        match self {
            TelepathyKind::Handoff => "🤝",
            TelepathyKind::Assist => "🆘",
            TelepathyKind::Share => "📤",
            TelepathyKind::Claim => "✋",
            TelepathyKind::Release => "👋",
            TelepathyKind::Status => "📊",
            TelepathyKind::Query => "❓",
            TelepathyKind::Response => "💬",
        }
    }
}

/// Pool of Directors running in one daemon.
pub struct Pool {
    /// All Director instances by name.
    directors: HashMap<String, Arc<RwLock<DirectorInstance>>>,
    /// Broadcast channel for telepathy messages.
    telepathy_tx: broadcast::Sender<TelepathyMessage>,
    /// Model registry for resolving aliases.
    pub registry: ModelRegistry,
    /// Zulip tool for external communication.
    zulip: Option<ZulipTool>,
    /// Node name (hostname).
    node: String,
    /// Zulip logging verbosity.
    verbosity: Verbosity,
    /// Zulip stream for this pool.
    stream: String,
}

impl Pool {
    /// Create a new Director pool.
    pub fn new(node: &str) -> DirectorResult<Self> {
        let (telepathy_tx, _) = broadcast::channel(1000);
        let zulip = ZulipTool::from_env().ok();

        Ok(Self {
            directors: HashMap::new(),
            telepathy_tx,
            registry: ModelRegistry::standard(),
            zulip,
            node: node.to_string(),
            verbosity: Verbosity::Normal,
            stream: "palace".to_string(),
        })
    }

    /// Set Zulip logging verbosity.
    pub fn with_verbosity(mut self, verbosity: Verbosity) -> Self {
        self.verbosity = verbosity;
        self
    }

    /// Set Zulip stream.
    pub fn with_stream(mut self, stream: &str) -> Self {
        self.stream = stream.to_string();
        self
    }

    /// Get verbosity level.
    pub fn verbosity(&self) -> Verbosity {
        self.verbosity
    }

    /// Add a Director to the pool.
    pub fn add(&mut self, name: &str, model: &str) -> DirectorResult<()> {
        if self.directors.contains_key(name) {
            return Err(DirectorError::Config(format!("Director '{}' already exists", name)));
        }

        let resolved_model = self.registry.resolve(model);
        let (tx, rx) = mpsc::channel(100);

        let instance = DirectorInstance {
            name: name.to_string(),
            model: resolved_model,
            session_id: None,
            status: DirectorStatus::Idle,
            telepathy_rx: rx,
            pool_tx: self.telepathy_tx.clone(),
        };

        self.directors.insert(name.to_string(), Arc::new(RwLock::new(instance)));

        // Spawn telepathy router for this Director
        let mut broadcast_rx = self.telepathy_tx.subscribe();
        let director_tx = tx;
        let director_name = name.to_string();

        tokio::spawn(async move {
            while let Ok(msg) = broadcast_rx.recv().await {
                // Route message if it's for this Director or broadcast
                if msg.to.is_none() || msg.to.as_deref() == Some(&director_name) {
                    // Don't send to self
                    if msg.from != director_name {
                        let _ = director_tx.send(msg).await;
                    }
                }
            }
        });

        tracing::info!("Added Director '{}' to pool", name);
        Ok(())
    }

    /// Get a Director by name.
    pub fn get(&self, name: &str) -> Option<Arc<RwLock<DirectorInstance>>> {
        self.directors.get(name).cloned()
    }

    /// List all Directors.
    pub fn list(&self) -> Vec<String> {
        self.directors.keys().cloned().collect()
    }

    /// Broadcast a message to all Directors and log to Zulip if verbosity allows.
    pub async fn broadcast(&self, from: &str, kind: TelepathyKind, payload: &str) {
        let msg = TelepathyMessage {
            from: from.to_string(),
            to: None,
            kind: kind.clone(),
            payload: payload.to_string(),
        };

        // Instant local delivery via telepathy
        let _ = self.telepathy_tx.send(msg.clone());

        // Log to Zulip if verbosity allows
        if self.verbosity >= kind.min_verbosity() {
            self.log_to_zulip(&msg).await;
        }
    }

    /// Send to a specific Director and log to Zulip if verbosity allows.
    pub async fn send(&self, from: &str, to: &str, kind: TelepathyKind, payload: &str) {
        let msg = TelepathyMessage {
            from: from.to_string(),
            to: Some(to.to_string()),
            kind: kind.clone(),
            payload: payload.to_string(),
        };

        // Instant local delivery via telepathy
        let _ = self.telepathy_tx.send(msg.clone());

        // Log to Zulip if verbosity allows
        if self.verbosity >= kind.min_verbosity() {
            self.log_to_zulip(&msg).await;
        }
    }

    /// Log a telepathy message to Zulip.
    async fn log_to_zulip(&self, msg: &TelepathyMessage) {
        if let Some(ref zulip) = self.zulip {
            let target = msg.to.as_deref().unwrap_or("*");
            let content = format!(
                "{} `{}` → `{}`: {}",
                msg.kind.emoji(),
                msg.from,
                target,
                msg.payload
            );
            let topic = format!("director/{}", self.node);
            let _ = zulip.send(&self.stream, &topic, &content).await;
        }
    }

    /// Send to Zulip (message mode).
    pub async fn zulip_send(&self, stream: &str, topic: &str, content: &str) -> DirectorResult<u64> {
        match &self.zulip {
            Some(tool) => tool.send(stream, topic, content).await,
            None => Err(DirectorError::Zulip("Zulip not configured".to_string())),
        }
    }

    /// Get pool status summary.
    pub async fn status(&self) -> PoolStatus {
        let mut directors = Vec::new();
        for (name, instance) in &self.directors {
            let inst = instance.read().await;
            directors.push(DirectorInfo {
                name: name.clone(),
                model: inst.model.clone(),
                status: inst.status,
                session_id: inst.session_id,
            });
        }
        PoolStatus {
            node: self.node.clone(),
            directors,
        }
    }

    /// Run the Director pool daemon.
    ///
    /// This starts the control server, announces to Zulip, and runs the event loop
    /// handling control commands and Zulip messages.
    pub async fn run(self, project_path: std::path::PathBuf) -> DirectorResult<()> {
        use crate::{ControlServer, ControlCommand, SessionManager, SessionExecutor, SessionExecutorConfig};
        use crate::zulip_stream::{ZulipStreamer, StreamConfig};

        let pool = std::sync::Arc::new(tokio::sync::RwLock::new(self));

        // Get node name and director list for announcements
        let (node_name, director_list, stream) = {
            let p = pool.read().await;
            let list = p.directors.keys()
                .map(|n| {
                    let model = p.directors.get(n)
                        .map(|d| futures::executor::block_on(async { d.read().await.model.clone() }))
                        .unwrap_or_default();
                    format!("- `{}` ({})", n, model)
                })
                .collect::<Vec<_>>()
                .join("\n");
            (p.node.clone(), list, p.stream.clone())
        };

        // Announce startup
        tracing::info!("Director Pool '{}' starting...", node_name);
        {
            let p = pool.read().await;
            if let Some(ref zulip) = p.zulip {
                let msg = format!(
                    "🏰 **Director Pool `{}` Online**\n\n**Directors:**\n{}\n\nCommands: `session <target>`, `status`",
                    node_name, director_list
                );
                let _ = zulip.send(&stream, &format!("director/{}", node_name), &msg).await;
            }
        }

        // Set up control channel
        let (control_tx, mut control_rx) = tokio::sync::mpsc::channel::<ControlCommand>(100);

        // Start control server
        let control_server = ControlServer::new(&node_name, control_tx);
        let control_path = crate::socket_path(&node_name);
        tracing::info!("Control socket: {}", control_path.display());

        tokio::spawn(async move {
            if let Err(e) = control_server.run().await {
                tracing::error!("Control server error: {}", e);
            }
        });

        // Create session manager
        let session_manager = std::sync::Arc::new(SessionManager::new(project_path.clone()));

        // Main event loop
        loop {
            tokio::select! {
                // Handle control commands (responses handled by ControlServer)
                Some(cmd) = control_rx.recv() => {
                    match cmd {
                        ControlCommand::Ping => {
                            tracing::debug!("Ping received");
                        }
                        ControlCommand::Shutdown => {
                            tracing::info!("Shutdown requested");
                            break;
                        }
                        ControlCommand::Status => {
                            let status = pool.read().await.status().await;
                            tracing::debug!("Status: {:?}", status);
                        }
                        ControlCommand::Session { target, director } => {
                            tracing::info!("Session requested for target '{}' (director: {:?})", target, director);
                            // TODO: Use SessionManager to create and run session
                        }
                        _ => {
                            tracing::debug!("Control command received: {:?}", cmd);
                        }
                    }
                }

                // Graceful shutdown on ctrl-c
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("Ctrl-C received, shutting down");
                    break;
                }
            }
        }

        // Announce shutdown
        {
            let p = pool.read().await;
            if let Some(ref zulip) = p.zulip {
                let _ = zulip.send(&stream, &format!("director/{}", node_name), "🏰 Director Pool shutting down").await;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolStatus {
    pub node: String,
    pub directors: Vec<DirectorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectorInfo {
    pub name: String,
    pub model: String,
    pub status: DirectorStatus,
    pub session_id: Option<Uuid>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pool_creation() {
        let mut pool = Pool::new("test-node").unwrap();
        pool.add("alpha", "flash").unwrap();
        pool.add("beta", "orch").unwrap();

        assert_eq!(pool.list().len(), 2);
        assert!(pool.get("alpha").is_some());
        assert!(pool.get("beta").is_some());
        assert!(pool.get("gamma").is_none());
    }

    #[tokio::test]
    async fn test_telepathy() {
        let mut pool = Pool::new("test-node").unwrap();
        pool.add("alpha", "flash").unwrap();
        pool.add("beta", "flash").unwrap();

        // Alpha sends to Beta
        {
            let alpha = pool.get("alpha").unwrap();
            let alpha = alpha.read().await;
            alpha.send(Some("beta"), TelepathyKind::Query, "status?").await;
        }

        // Small delay for message routing
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Beta receives
        {
            let beta = pool.get("beta").unwrap();
            let mut beta = beta.write().await;
            let msg = beta.try_recv();
            assert!(msg.is_some());
            let msg = msg.unwrap();
            assert_eq!(msg.from, "alpha");
            assert_eq!(msg.payload, "status?");
        }
    }
}
