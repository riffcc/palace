//! Palace Daemon: Persistent autonomous project manager.
//!
//! Runs continuously, analyzing the project, creating issue relations,
//! decomposing tasks, and managing the full project lifecycle.
//!
//! Features:
//! - Continuous analysis via GLM-4.7
//! - Issue relation discovery and management
//! - Automatic decomposition of large tasks
//! - Local webserver with 3D force graph visualization
//! - Virtual character tracking (Gource-style)
//! - Run recording for later analysis

use crate::{Director, DirectorResult, ExecutorConfig, Project, ProjectConfig};
use crate::orchestrator::parse_model;
use crate::zulip_reactor::{ZulipReactor, ZulipEvent, MessageEvent, emoji};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};

/// Event emitted by the daemon.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum DaemonEvent {
    /// Started working on an issue.
    IssueFocused { issue_id: String, title: String },
    /// Created a new issue.
    IssueCreated { issue_id: String, title: String },
    /// Linked two issues.
    IssueLinkCreated { from: String, to: String, link_type: String },
    /// Completed an issue.
    IssueCompleted { issue_id: String },
    /// Step executed.
    StepExecuted { step: String, success: bool },
    /// Agent moved to new location in codebase.
    AgentMoved { from: String, to: String },
    /// Analysis completed.
    AnalysisComplete { summary: String },
}

/// Node in the project graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub name: String,
    pub node_type: NodeType,
    pub status: String,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub z: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    Module,
    Cycle,
    Issue,
    File,
    Agent,
}

/// Link in the project graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphLink {
    pub source: String,
    pub target: String,
    pub link_type: String,
}

/// The project graph state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectGraph {
    pub nodes: Vec<GraphNode>,
    pub links: Vec<GraphLink>,
    pub agent_position: Option<String>,
}

impl ProjectGraph {
    /// Add a node if it doesn't exist.
    pub fn add_node(&mut self, node: GraphNode) {
        if !self.nodes.iter().any(|n| n.id == node.id) {
            self.nodes.push(node);
        }
    }

    /// Add a link if it doesn't exist.
    pub fn add_link(&mut self, link: GraphLink) {
        if !self.links.iter().any(|l| l.source == link.source && l.target == link.target) {
            self.links.push(link);
        }
    }

    /// Move the agent to a new position.
    pub fn move_agent(&mut self, to: &str) {
        self.agent_position = Some(to.to_string());
    }
}

/// Daemon configuration.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Project root path.
    pub project_path: PathBuf,
    /// Model to use (with route prefix: lm:/, z:/, or:/, ms:/).
    pub model: String,
    /// Override LLM endpoint URL (derived from model prefix if not set).
    pub llm_url_override: Option<String>,
    /// Web server address.
    pub web_addr: SocketAddr,
    /// Enable recording.
    pub record: bool,
    /// Recording output path.
    pub record_path: Option<PathBuf>,
    /// Analysis interval in seconds.
    pub analysis_interval_secs: u64,
    /// Enable Zulip event-driven mode.
    pub zulip_enabled: bool,
    /// Zulip stream for this project.
    pub zulip_stream: String,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            project_path: PathBuf::from("."),
            model: "lm:/glm-4.7-flash".to_string(),
            llm_url_override: None,
            web_addr: "127.0.0.1:3456".parse().unwrap(),
            record: false,
            record_path: None,
            analysis_interval_secs: 60,
            zulip_enabled: true,
            zulip_stream: "palace".to_string(),
        }
    }
}

/// The Palace Daemon.
pub struct Daemon {
    config: DaemonConfig,
    director: Director,
    graph: Arc<RwLock<ProjectGraph>>,
    events_tx: broadcast::Sender<DaemonEvent>,
    recording: Arc<RwLock<Vec<DaemonEvent>>>,
    /// Zulip event receiver (for event-driven mode).
    zulip_rx: Option<mpsc::Receiver<ZulipEvent>>,
    /// Zulip reactor (owned, moved to task).
    zulip_reactor: Option<ZulipReactor>,
}

impl Daemon {
    /// Create a new daemon.
    pub fn new(config: DaemonConfig) -> DirectorResult<Self> {
        let project_config = ProjectConfig {
            name: config.project_path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("project")
                .to_string(),
            root_path: config.project_path.clone(),
            ..Default::default()
        };

        // Derive LLM URL from model prefix (or use override)
        let (route, model_name) = parse_model(&config.model);
        let llm_url = config.llm_url_override.clone()
            .unwrap_or_else(|| route.base_url().to_string());

        let executor_config = ExecutorConfig {
            project_path: config.project_path.clone(),
            llm_url,
            model: model_name.to_string(),
            ..Default::default()
        };

        let project = Project::new(project_config);
        let director = Director::with_executor(project, executor_config);

        let (events_tx, _) = broadcast::channel(1000);

        // Set up Zulip reactor if enabled
        let (zulip_reactor, zulip_rx) = if config.zulip_enabled {
            match ZulipReactor::from_env() {
                Ok(reactor) => {
                    let (tx, rx) = mpsc::channel(100);
                    let reactor = reactor.with_event_channel(tx);
                    (Some(reactor), Some(rx))
                }
                Err(e) => {
                    tracing::warn!("Zulip reactor disabled: {}", e);
                    (None, None)
                }
            }
        } else {
            (None, None)
        };

        Ok(Self {
            config,
            director,
            graph: Arc::new(RwLock::new(ProjectGraph::default())),
            events_tx,
            recording: Arc::new(RwLock::new(Vec::new())),
            zulip_rx,
            zulip_reactor,
        })
    }

    /// Get the event broadcast receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<DaemonEvent> {
        self.events_tx.subscribe()
    }

    /// Get the current graph state.
    pub async fn graph(&self) -> ProjectGraph {
        self.graph.read().await.clone()
    }

    /// Emit an event.
    async fn emit(&self, event: DaemonEvent) {
        // Record if enabled
        if self.config.record {
            self.recording.write().await.push(event.clone());
        }

        // Broadcast
        let _ = self.events_tx.send(event);
    }

    /// Run the daemon loop.
    pub async fn run(mut self) -> DirectorResult<()> {
        tracing::info!("Palace Daemon starting...");

        // Initial graph population
        self.populate_graph_from_plane().await?;

        // Start web server in background
        let graph = self.graph.clone();
        let events_tx = self.events_tx.clone();
        let web_addr = self.config.web_addr;

        tokio::spawn(async move {
            if let Err(e) = run_web_server(web_addr, graph, events_tx).await {
                tracing::error!("Web server error: {}", e);
            }
        });

        tracing::info!("Visualization at http://{}", self.config.web_addr);

        // Start Zulip reactor if enabled
        if let Some(mut reactor) = self.zulip_reactor.take() {
            tracing::info!("Starting Zulip event-driven mode on stream: {}", self.config.zulip_stream);

            tokio::spawn(async move {
                if let Err(e) = reactor.run().await {
                    tracing::error!("Zulip reactor error: {}", e);
                }
            });
        }

        // Main event loop - combines analysis with Zulip events
        let analysis_interval = tokio::time::Duration::from_secs(self.config.analysis_interval_secs);

        loop {
            tokio::select! {
                // Handle Zulip events
                Some(event) = async {
                    if let Some(ref mut rx) = self.zulip_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    if let Err(e) = self.handle_zulip_event(&event).await {
                        tracing::warn!("Zulip event handling error: {}", e);
                    }
                }

                // Periodic analysis
                _ = tokio::time::sleep(analysis_interval) => {
                    if let Err(e) = self.analyze_and_update().await {
                        tracing::warn!("Analysis error: {}", e);
                    }
                }
            }
        }
    }

    /// Handle a Zulip event.
    async fn handle_zulip_event(&self, event: &ZulipEvent) -> DirectorResult<()> {
        match event.event_type.as_str() {
            "message" => {
                if let Some(ref msg) = event.message {
                    self.handle_zulip_message(msg).await?;
                }
            }
            "reaction" => {
                // Reactions are handled in the reactor itself
                // Here we can trigger additional logic based on user reactions
                tracing::debug!("Reaction event: {:?}", event.reaction);
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle a Zulip message that may contain commands.
    async fn handle_zulip_message(&self, msg: &MessageEvent) -> DirectorResult<()> {
        // Check if message mentions Director
        if !msg.content.contains("@**Director**") && !msg.content.contains("@_**Director**") {
            return Ok(());
        }

        tracing::info!("Director mentioned by {}: {}", msg.sender_full_name, &msg.content[..msg.content.len().min(100)]);

        // Extract command (text after mention)
        let command = msg.content
            .replace("@**Director**", "")
            .replace("@_**Director**", "")
            .trim()
            .to_string();

        // Emit event
        self.emit(DaemonEvent::AnalysisComplete {
            summary: format!("Command received: {}", command),
        }).await;

        // TODO: Parse and execute command
        // Examples:
        // - "start session PAL-52" -> create and run a session
        // - "status" -> report current status
        // - "goal: Complete Zulip integration" -> set a goal
        // - "pause" / "resume" -> control execution

        Ok(())
    }

    /// Populate the graph from Plane.so data.
    async fn populate_graph_from_plane(&self) -> DirectorResult<()> {
        // This would call the Plane API to get issues, modules, cycles
        // For now, create placeholder structure

        let mut graph = self.graph.write().await;

        // Add agent node
        graph.add_node(GraphNode {
            id: "agent".to_string(),
            name: "Director".to_string(),
            node_type: NodeType::Agent,
            status: "active".to_string(),
            x: None, y: None, z: None,
        });

        Ok(())
    }

    /// Run analysis and update the graph.
    async fn analyze_and_update(&self) -> DirectorResult<()> {
        self.emit(DaemonEvent::AnalysisComplete {
            summary: "Analysis cycle complete".to_string(),
        }).await;

        Ok(())
    }

    /// Save recording to file.
    pub async fn save_recording(&self) -> DirectorResult<()> {
        if let Some(path) = &self.config.record_path {
            let recording = self.recording.read().await;
            let content = serde_json::to_string_pretty(&*recording)
                .map_err(|e| crate::DirectorError::Other(e.to_string()))?;
            std::fs::write(path, content)
                .map_err(|e| crate::DirectorError::Io(e))?;
        }
        Ok(())
    }
}

/// Run the visualization web server.
async fn run_web_server(
    addr: SocketAddr,
    graph: Arc<RwLock<ProjectGraph>>,
    events_tx: broadcast::Sender<DaemonEvent>,
) -> DirectorResult<()> {
    use axum::{
        extract::{State, ws::{WebSocket, WebSocketUpgrade}},
        response::{Html, IntoResponse},
        routing::get,
        Json, Router,
    };

    #[derive(Clone)]
    struct AppState {
        graph: Arc<RwLock<ProjectGraph>>,
        events_tx: broadcast::Sender<DaemonEvent>,
    }

    let state = AppState { graph, events_tx };

    // Graph data endpoint
    async fn get_graph(State(state): State<AppState>) -> Json<ProjectGraph> {
        Json(state.graph.read().await.clone())
    }

    // WebSocket for live updates
    async fn ws_handler(
        ws: WebSocketUpgrade,
        State(state): State<AppState>,
    ) -> impl IntoResponse {
        ws.on_upgrade(move |socket| handle_socket(socket, state.events_tx))
    }

    async fn handle_socket(mut socket: WebSocket, events_tx: broadcast::Sender<DaemonEvent>) {
        let mut rx = events_tx.subscribe();

        while let Ok(event) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&event) {
                if socket.send(axum::extract::ws::Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    }

    // HTML page with 3D force graph
    async fn index() -> Html<&'static str> {
        Html(include_str!("daemon_ui.html"))
    }

    let app = Router::new()
        .route("/", get(index))
        .route("/api/graph", get(get_graph))
        .route("/ws", get(ws_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await
        .map_err(|e| crate::DirectorError::Other(e.to_string()))?;

    axum::serve(listener, app).await
        .map_err(|e| crate::DirectorError::Other(e.to_string()))?;

    Ok(())
}
