//! Local control interface for Director daemon.
//!
//! Provides a Unix socket for local CLI tools to interact with running Directors.
//! Commands can be sent via `palace-ctl` or by writing to the socket directly.

use crate::{DirectorError, DirectorResult};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

/// Control command from local client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum ControlCommand {
    /// Get current status.
    Status,
    /// Set a goal for the Director.
    Goal { description: String },
    /// Work on a specific issue.
    Issue { id: String },
    /// Work on a cycle.
    Cycle { id: String },
    /// Change model.
    Model { model: String },
    /// Send a message to Zulip.
    Say { message: String },
    /// Ask Palace to do something.
    Palace { command: String },
    /// Start a session for an issue/target.
    Session { target: String, director: Option<String> },
    /// List active sessions.
    Sessions,
    /// Steer an active session with guidance.
    Steer { session_id: String, guidance: String },

    // --- Rich LLM Agent Commands ---

    /// Execute an LLM prompt with full tool access.
    /// The Director becomes an agent that can use any tool.
    Exec {
        prompt: String,
        #[serde(default)]
        director: Option<String>,
        #[serde(default)]
        max_turns: Option<u32>,
    },
    /// Call a specific tool directly (no LLM).
    Tool {
        name: String,
        #[serde(default)]
        args: serde_json::Value,
    },
    /// Have the Director think/reason about something and respond.
    Think {
        question: String,
        #[serde(default)]
        context: Option<String>,
    },
    /// Read a file and return its contents.
    Read { path: String },
    /// Search for files matching a pattern.
    Glob { pattern: String },
    /// Search file contents.
    Grep { pattern: String, path: Option<String> },
    /// Run a shell command.
    Shell { command: String },
    /// Get Plane.so issue details.
    PlaneIssue { id: String },
    /// Create a Plane.so issue.
    PlaneCreate { title: String, description: String },

    /// Graceful shutdown.
    Shutdown,
    /// Ping (health check).
    Ping,
}

/// Control response to client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ControlResponse {
    Ok { message: String },
    Error { message: String },
    /// Rich data response (for reads, searches, etc.)
    Data {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        files: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        matches: Option<Vec<GrepMatch>>,
    },
    /// LLM response (for think/exec commands).
    LlmResponse {
        message: String,
        response: String,
        #[serde(default)]
        tool_calls: Vec<ToolCallResult>,
    },
    Status {
        name: String,
        model: String,
        uptime_secs: u64,
        sessions_active: u32,
        sessions_completed: u32,
    },
}

/// A grep match result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepMatch {
    pub file: String,
    pub line: u32,
    pub content: String,
}

/// Result of a tool call during LLM execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    pub tool: String,
    pub success: bool,
    pub output: String,
}

/// Control socket server.
pub struct ControlServer {
    socket_path: PathBuf,
    command_tx: mpsc::Sender<ControlCommand>,
}

impl ControlServer {
    /// Create a new control server.
    pub fn new(name: &str, command_tx: mpsc::Sender<ControlCommand>) -> Self {
        let socket_path = socket_path(name);
        Self {
            socket_path,
            command_tx,
        }
    }

    /// Start listening for connections.
    pub async fn run(self) -> DirectorResult<()> {
        // Remove old socket if exists
        let _ = std::fs::remove_file(&self.socket_path);

        // Create parent directory
        if let Some(parent) = self.socket_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DirectorError::Config(format!("Failed to create socket dir: {}", e)))?;
        }

        let listener = UnixListener::bind(&self.socket_path)
            .map_err(|e| DirectorError::Config(format!("Failed to bind socket: {}", e)))?;

        tracing::info!("Control socket listening at {}", self.socket_path.display());

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let tx = self.command_tx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_client(stream, tx).await {
                            tracing::warn!("Client error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::warn!("Accept error: {}", e);
                }
            }
        }
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

/// Handle a single client connection.
async fn handle_client(
    stream: UnixStream,
    command_tx: mpsc::Sender<ControlCommand>,
) -> DirectorResult<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    while reader.read_line(&mut line).await? > 0 {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            line.clear();
            continue;
        }

        let response = match serde_json::from_str::<ControlCommand>(trimmed) {
            Ok(cmd) => {
                // Send command to daemon
                if command_tx.send(cmd.clone()).await.is_err() {
                    ControlResponse::Error {
                        message: "Daemon not responding".to_string(),
                    }
                } else {
                    match cmd {
                        ControlCommand::Ping => ControlResponse::Ok {
                            message: "pong".to_string(),
                        },
                        ControlCommand::Shutdown => ControlResponse::Ok {
                            message: "Shutting down...".to_string(),
                        },
                        _ => ControlResponse::Ok {
                            message: "Command received".to_string(),
                        },
                    }
                }
            }
            Err(e) => ControlResponse::Error {
                message: format!("Invalid command: {}", e),
            },
        };

        let response_json = serde_json::to_string(&response)?;
        writer.write_all(response_json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        line.clear();
    }

    Ok(())
}

/// Get the socket path for a Director by name.
pub fn socket_path(name: &str) -> PathBuf {
    PathBuf::from(format!("/run/user/{}/palace-director-{}.sock",
        users::get_current_uid(), name))
}

/// Control client for sending commands to a running Director.
pub struct ControlClient {
    socket_path: PathBuf,
}

impl ControlClient {
    /// Connect to a Director by name.
    pub fn new(name: &str) -> Self {
        Self {
            socket_path: socket_path(name),
        }
    }

    /// Send a command and get response.
    pub async fn send(&self, cmd: ControlCommand) -> DirectorResult<ControlResponse> {
        let stream = UnixStream::connect(&self.socket_path)
            .await
            .map_err(|e| DirectorError::Config(format!("Failed to connect: {}", e)))?;

        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        // Send command
        let cmd_json = serde_json::to_string(&cmd)?;
        writer.write_all(cmd_json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        // Read response
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let response: ControlResponse = serde_json::from_str(line.trim())?;
        Ok(response)
    }

    /// Quick status check.
    pub async fn ping(&self) -> bool {
        matches!(self.send(ControlCommand::Ping).await, Ok(ControlResponse::Ok { .. }))
    }
}
