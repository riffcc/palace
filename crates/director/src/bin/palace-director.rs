//! Palace Director daemon binary.
//!
//! Long-running autonomous project manager that hosts multiple Directors
//! coordinating work via Zulip (external) and telepathy (internal).
//!
//! Directors can spawn and manage Palace sessions, steering them as needed.
//! Each Director is an LLM agent that can execute any tool call.

use director::{
    ControlCommand, ControlServer, Pool, TelepathyKind, Verbosity,
    ZulipTool, ZulipAgentTool, PlaneAgentTool, ZulipReactor, parse_model,
};
use llm_code_sdk::Client;
use llm_code_sdk::tools::{ToolRunner, ToolRunnerConfig, ToolEvent, create_editing_tools};
use llm_code_sdk::types::{MessageCreateParams, MessageParam};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc::{self, unbounded_channel}, RwLock};
use uuid::Uuid;

/// An active Palace session managed by a Director.
struct ManagedSession {
    id: Uuid,
    target: String,
    director: String,
    process: Child,
    stdin_tx: mpsc::Sender<String>,
}

/// Session manager state.
struct SessionManager {
    sessions: HashMap<Uuid, ManagedSession>,
    zulip: Option<ZulipTool>,
    node_name: String,
}

impl SessionManager {
    fn new(node_name: &str) -> Self {
        Self {
            sessions: HashMap::new(),
            zulip: ZulipTool::from_env().ok(),
            node_name: node_name.to_string(),
        }
    }

    /// Spawn a new Palace session for a target (issue ID, etc).
    async fn spawn_session(&mut self, target: &str, director: &str) -> anyhow::Result<Uuid> {
        let session_id = Uuid::new_v4();
        let short_id = &session_id.to_string()[..8];

        tracing::info!("Spawning session {} for target '{}' (director: {})", short_id, target, director);

        // Announce to Zulip
        if let Some(ref zulip) = self.zulip {
            let _ = zulip.send(
                "palace",
                &format!("session/{}", short_id),
                &format!(
                    "🚀 **Session Started**\n\n\
                    **Target:** `{}`\n\
                    **Director:** `{}`\n\
                    **ID:** `{}`\n\n\
                    *Director is steering this session.*",
                    target, director, short_id
                )
            ).await;
        }

        // Create stdin channel for steering
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(100);

        // Spawn pal next with the target
        // Use --glm for local LM Studio model
        let mut child = Command::new("pal")
            .arg("next")
            .arg(target)
            .arg("--glm")
            .current_dir(std::env::var("PALACE_PROJECT_PATH").unwrap_or_else(|_| ".".to_string()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Take stdin for writing guidance
        let mut child_stdin = child.stdin.take();

        // Spawn task to forward guidance to child stdin
        tokio::spawn(async move {
            while let Some(guidance) = stdin_rx.recv().await {
                if let Some(ref mut stdin) = child_stdin {
                    let _ = stdin.write_all(guidance.as_bytes()).await;
                    let _ = stdin.write_all(b"\n").await;
                    let _ = stdin.flush().await;
                }
            }
        });

        // Spawn task to read stdout and report to Zulip
        if let Some(stdout) = child.stdout.take() {
            let zulip: Option<ZulipTool> = self.zulip.clone();
            let sid = short_id.to_string();
            tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::info!("[session/{}] {}", sid, line);
                    // Periodically update Zulip with progress
                    if line.contains("✓") || line.contains("Complete") || line.contains("Error") {
                        if let Some(ref z) = zulip {
                            let _: Result<u64, _> = z.send("palace", &format!("session/{}", sid), &format!("```\n{}\n```", line)).await;
                        }
                    }
                }
            });
        }

        let session = ManagedSession {
            id: session_id,
            target: target.to_string(),
            director: director.to_string(),
            process: child,
            stdin_tx,
        };

        self.sessions.insert(session_id, session);
        Ok(session_id)
    }

    /// Send guidance to an active session.
    async fn steer(&self, session_id: Uuid, guidance: &str) -> anyhow::Result<()> {
        if let Some(session) = self.sessions.get(&session_id) {
            tracing::info!("Steering session {}: {}", &session_id.to_string()[..8], guidance);
            session.stdin_tx.send(guidance.to_string()).await?;

            // Log to Zulip
            if let Some(ref zulip) = self.zulip {
                let _ = zulip.send(
                    "palace",
                    &format!("session/{}", &session_id.to_string()[..8]),
                    &format!("🎯 **Director Guidance:**\n\n{}", guidance)
                ).await;
            }
            Ok(())
        } else {
            Err(anyhow::anyhow!("Session not found: {}", session_id))
        }
    }

    /// List active sessions.
    fn list(&self) -> Vec<(String, String, String)> {
        self.sessions.values()
            .map(|s| (s.id.to_string()[..8].to_string(), s.target.clone(), s.director.clone()))
            .collect()
    }

    /// Check and clean up finished sessions.
    async fn cleanup(&mut self) {
        let mut finished = Vec::new();
        for (id, session) in &mut self.sessions {
            if let Ok(Some(status)) = session.process.try_wait() {
                tracing::info!("Session {} finished with status: {:?}", &id.to_string()[..8], status);
                finished.push(*id);

                // Announce completion
                if let Some(ref zulip) = self.zulip {
                    let emoji = if status.success() { "✅" } else { "❌" };
                    let _ = zulip.send(
                        "palace",
                        &format!("session/{}", &id.to_string()[..8]),
                        &format!("{} **Session Completed**\n\nTarget: `{}`\nStatus: {:?}", emoji, session.target, status)
                    ).await;
                }
            }
        }
        for id in finished {
            self.sessions.remove(&id);
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("director=info".parse()?)
                .add_directive("palace_director=info".parse()?)
                .add_directive("llm_code_sdk=info".parse()?)
                .add_directive("hyper=warn".parse()?)
        )
        .init();

    // Load environment
    let _ = dotenvy::from_path(std::path::Path::new(&std::env::var("HOME")?).join("ai/zulip/.env"));

    // Parse arguments
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: palace-director <node-name> <director[:model]>...");
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  palace-director tealc alpha              # Director 'alpha' with default model");
        eprintln!("  palace-director tealc alpha:orch         # Director 'alpha' with nvidia orchestrator");
        eprintln!("  palace-director tealc alpha beta:flash   # Multiple Directors");
        std::process::exit(1);
    }

    let node_name = args[1].clone();
    let director_specs: Vec<(String, String)> = args[2..].iter()
        .map(|s| {
            // Handle model prefixes (lm:/, z:/, or:/, ms:/)
            // Format: <director-name>:<model> OR <full-model-string>
            // If it starts with a known prefix, use the prefix as the name
            if s.starts_with("lm:/") || s.starts_with("z:/") || s.starts_with("or:/") || s.starts_with("ms:/") {
                // Full model string - extract prefix as director name
                let prefix_end = s.find(":/").unwrap() + 2;
                let prefix = &s[..prefix_end - 2]; // "lm", "z", "or", or "ms"
                (prefix.to_string(), s.clone())
            } else if let Some((name, model)) = s.split_once(':') {
                // Explicit name:model format
                (name.to_string(), model.to_string())
            } else {
                // Just a name, use default model
                (s.clone(), "flash".to_string())
            }
        })
        .collect();

    // Create pool
    let mut pool = Pool::new(&node_name)?
        .with_verbosity(Verbosity::Normal)
        .with_stream("palace");

    // Add Directors
    for (name, model) in &director_specs {
        pool.add(name, model)?;
    }

    let pool = Arc::new(RwLock::new(pool));

    // Create session manager
    let session_mgr = Arc::new(RwLock::new(SessionManager::new(&node_name)));

    // Announce startup
    println!("🏰 Palace Director Pool '{}' starting...", node_name);
    for (name, model) in &director_specs {
        let resolved = {
            let p = pool.read().await;
            p.registry.resolve(model)
        };
        println!("   Director '{}' with model '{}'", name, resolved);
    }

    // Send startup announcement to Zulip
    if let Ok(tool) = ZulipTool::from_env() {
        let director_list = director_specs.iter()
            .map(|(name, model)| format!("- `{}` ({})", name, model))
            .collect::<Vec<_>>()
            .join("\n");

        let startup_msg = format!(
            "🏰 **Director Pool `{}` Online**\n\n\
            **Directors:**\n{}\n\n\
            Commands: `session <target>`, `sessions`, `steer <id> <guidance>`",
            node_name, director_list
        );
        if let Err(e) = tool.send("palace", &format!("director/{}", node_name), &startup_msg).await {
            tracing::warn!("Failed to announce startup: {}", e);
        }
    }

    // Set up control channel
    let (control_tx, mut control_rx) = mpsc::channel::<ControlCommand>(100);

    // Start control server
    let control_server = ControlServer::new(&node_name, control_tx);
    let control_path = director::socket_path(&node_name);
    println!("   Control socket: {}", control_path.display());

    tokio::spawn(async move {
        if let Err(e) = control_server.run().await {
            tracing::error!("Control server error: {}", e);
        }
    });

    // Cleanup task for finished sessions
    let session_mgr_cleanup = session_mgr.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            session_mgr_cleanup.write().await.cleanup().await;
        }
    });

    // Set up graceful shutdown
    let ctrl_c = tokio::signal::ctrl_c();
    let pool_clone = pool.clone();
    let session_mgr_clone = session_mgr.clone();
    let node_name_clone = node_name.clone();
    let first_director = director_specs.first().map(|(n, _)| n.clone()).unwrap_or_default();

    // Start Zulip reactor for @palace commands
    let zulip_reactor_handle = match ZulipReactor::from_env() {
        Ok(mut reactor) => {
            println!("   Zulip reactor: listening for @palace commands");
            Some(tokio::spawn(async move {
                if let Err(e) = reactor.run().await {
                    tracing::error!("Zulip reactor error: {}", e);
                }
            }))
        }
        Err(e) => {
            println!("   Zulip reactor: disabled ({})", e);
            None
        }
    };

    // Main event loop
    println!("\n🟢 Ready. Ctrl+C to shutdown.");

    tokio::select! {
        // Handle control commands
        _ = async {
            while let Some(cmd) = control_rx.recv().await {
                match cmd {
                    ControlCommand::Status => {
                        let pool = pool_clone.read().await;
                        let status = pool.status().await;
                        let sessions = session_mgr_clone.read().await.list();
                        tracing::info!("Status: {:?}, Sessions: {:?}", status, sessions);
                    }
                    ControlCommand::Shutdown => {
                        tracing::info!("Shutdown requested via control socket");
                        break;
                    }
                    ControlCommand::Ping => {
                        tracing::debug!("Ping received");
                    }
                    ControlCommand::Say { message } => {
                        let pool = pool_clone.read().await;
                        let _ = pool.zulip_send("palace", &format!("director/{}", node_name_clone), &message).await;
                    }
                    ControlCommand::Goal { description } => {
                        let pool = pool_clone.read().await;
                        pool.broadcast(&node_name_clone, TelepathyKind::Status, &format!("Goal: {}", description)).await;
                    }
                    ControlCommand::Issue { id } => {
                        // Start a session for this issue
                        let director = first_director.clone();
                        let mut mgr = session_mgr_clone.write().await;
                        match mgr.spawn_session(&id, &director).await {
                            Ok(session_id) => {
                                tracing::info!("Started session {} for issue {}", &session_id.to_string()[..8], id);
                                let pool = pool_clone.read().await;
                                pool.broadcast(&director, TelepathyKind::Claim, &format!("session:{}", &session_id.to_string()[..8])).await;
                            }
                            Err(e) => {
                                tracing::error!("Failed to start session: {}", e);
                            }
                        }
                    }
                    ControlCommand::Session { target, director } => {
                        let dir = director.unwrap_or_else(|| first_director.clone());
                        let mut mgr = session_mgr_clone.write().await;
                        match mgr.spawn_session(&target, &dir).await {
                            Ok(session_id) => {
                                tracing::info!("Started session {} for target {}", &session_id.to_string()[..8], target);
                                let pool = pool_clone.read().await;
                                pool.broadcast(&dir, TelepathyKind::Claim, &format!("session:{}", &session_id.to_string()[..8])).await;
                            }
                            Err(e) => {
                                tracing::error!("Failed to start session: {}", e);
                            }
                        }
                    }
                    ControlCommand::Sessions => {
                        let mgr = session_mgr_clone.read().await;
                        let sessions = mgr.list();
                        for (id, target, dir) in sessions {
                            tracing::info!("Session {}: {} (director: {})", id, target, dir);
                        }
                    }
                    ControlCommand::Steer { session_id, guidance } => {
                        if let Ok(uuid) = Uuid::parse_str(&session_id) {
                            let mgr = session_mgr_clone.read().await;
                            if let Err(e) = mgr.steer(uuid, &guidance).await {
                                tracing::error!("Failed to steer session: {}", e);
                            }
                        } else {
                            tracing::error!("Invalid session ID: {}", session_id);
                        }
                    }
                    // Rich tool commands
                    ControlCommand::Read { path } => {
                        match tokio::fs::read_to_string(&path).await {
                            Ok(content) => {
                                tracing::info!("Read {} ({} bytes)", path, content.len());
                            }
                            Err(e) => {
                                tracing::error!("Failed to read {}: {}", path, e);
                            }
                        }
                    }
                    ControlCommand::Shell { command } => {
                        tracing::info!("Executing shell: {}", command);
                        match tokio::process::Command::new("bash")
                            .arg("-c")
                            .arg(&command)
                            .output()
                            .await
                        {
                            Ok(output) => {
                                let stdout = String::from_utf8_lossy(&output.stdout);
                                let stderr = String::from_utf8_lossy(&output.stderr);
                                tracing::info!("Shell output:\n{}{}", stdout, stderr);
                            }
                            Err(e) => {
                                tracing::error!("Shell failed: {}", e);
                            }
                        }
                    }
                    ControlCommand::Glob { pattern } => {
                        tracing::info!("Glob: {}", pattern);
                        // Use glob crate or shell
                        match tokio::process::Command::new("bash")
                            .arg("-c")
                            .arg(format!("ls -1 {} 2>/dev/null | head -50", pattern))
                            .output()
                            .await
                        {
                            Ok(output) => {
                                let files = String::from_utf8_lossy(&output.stdout);
                                tracing::info!("Glob results:\n{}", files);
                            }
                            Err(e) => {
                                tracing::error!("Glob failed: {}", e);
                            }
                        }
                    }
                    ControlCommand::Grep { pattern, path } => {
                        let search_path = path.unwrap_or_else(|| ".".to_string());
                        tracing::info!("Grep '{}' in {}", pattern, search_path);
                        match tokio::process::Command::new("rg")
                            .arg("--line-number")
                            .arg("--max-count=20")
                            .arg(&pattern)
                            .arg(&search_path)
                            .output()
                            .await
                        {
                            Ok(output) => {
                                let matches = String::from_utf8_lossy(&output.stdout);
                                tracing::info!("Grep results:\n{}", matches);
                            }
                            Err(e) => {
                                tracing::error!("Grep failed: {}", e);
                            }
                        }
                    }
                    ControlCommand::Think { question, context } => {
                        tracing::info!("Think: {}", question);

                        // Get director's model from pool
                        let (model, llm_url) = {
                            let pool = pool_clone.read().await;
                            if let Some(director_inst) = pool.get(&first_director) {
                                let d = director_inst.read().await;
                                let (route, model_name) = parse_model(&d.model);
                                (model_name.to_string(), route.base_url().to_string())
                            } else {
                                ("glm-4-plus".to_string(), "http://localhost:1234/v1".to_string())
                            }
                        };

                        // Build prompt with context
                        let full_prompt = if let Some(ctx) = context {
                            format!("Context: {}\n\nQuestion: {}", ctx, question)
                        } else {
                            question.clone()
                        };

                        let zulip = ZulipTool::from_env().ok();
                        let node_for_task = node_name_clone.clone();

                        tokio::spawn(async move {
                            let client = match Client::openai_compatible(&llm_url) {
                                Ok(c) => c,
                                Err(e) => {
                                    tracing::error!("Failed to create LLM client: {}", e);
                                    return;
                                }
                            };

                            // Simple completion without tools for thinking
                            let params = MessageCreateParams {
                                model,
                                max_tokens: 2048,
                                messages: vec![MessageParam::user(&full_prompt)],
                                ..Default::default()
                            };

                            match client.messages().create(params).await {
                                Ok(response) => {
                                    let text = response.text().unwrap_or_default();
                                    tracing::info!("Think response: {}", &text[..text.len().min(200)]);

                                    if let Some(z) = zulip {
                                        let _ = z.send(
                                            "palace",
                                            &format!("director/{}", node_for_task),
                                            &format!("🤔 **Thought:**\n\n{}", text)
                                        ).await;
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Think failed: {}", e);
                                }
                            }
                        });
                    }
                    ControlCommand::Exec { prompt, director, max_turns: _ } => {
                        let dir = director.unwrap_or_else(|| first_director.clone());
                        tracing::info!("Exec (director: {}): {}", dir, &prompt[..prompt.len().min(100)]);

                        // Get director's model from pool
                        let (model, llm_url) = {
                            let pool = pool_clone.read().await;
                            if let Some(director_inst) = pool.get(&dir) {
                                let d = director_inst.read().await;
                                let (route, model_name) = parse_model(&d.model);
                                (model_name.to_string(), route.base_url().to_string())
                            } else {
                                ("glm-4-plus".to_string(), "http://localhost:1234/v1".to_string())
                            }
                        };

                        // Announce to Zulip
                        {
                            let pool = pool_clone.read().await;
                            let _ = pool.zulip_send(
                                "palace",
                                &format!("director/{}", node_name_clone),
                                &format!("🚀 **Executing task:**\n\n{}\n\n*Director: {} | Model: {}*", prompt, dir, model)
                            ).await;
                        }

                        // Get project path
                        let project_path = std::env::var("PALACE_PROJECT_PATH")
                            .map(PathBuf::from)
                            .unwrap_or_else(|_| PathBuf::from("."));

                        // Spawn LLM agent task
                        let zulip = ZulipTool::from_env().ok();
                        let node_for_task = node_name_clone.clone();
                        let prompt_clone = prompt.clone();

                        tokio::spawn(async move {
                            tracing::info!("Starting LLM agent task for: {}", &prompt_clone[..prompt_clone.len().min(50)]);

                            // Create LLM client
                            let client = match Client::openai_compatible(&llm_url) {
                                Ok(c) => c,
                                Err(e) => {
                                    tracing::error!("Failed to create LLM client: {}", e);
                                    if let Some(z) = &zulip {
                                        let _ = z.send(
                                            "palace",
                                            &format!("director/{}", node_for_task),
                                            &format!("❌ **Failed to connect to LLM**\n\n{}", e)
                                        ).await;
                                    }
                                    return;
                                }
                            };

                            tracing::debug!("LLM client created for: {}", llm_url);

                            // Create tools (full editing toolset + Director tools)
                            let mut tools = create_editing_tools(&project_path);

                            // Add Director-specific tools
                            tools.push(Arc::new(ZulipAgentTool::from_env()) as Arc<dyn llm_code_sdk::tools::Tool>);
                            tools.push(Arc::new(PlaneAgentTool::from_env()) as Arc<dyn llm_code_sdk::tools::Tool>);

                            tracing::debug!("Created {} tools (including Director tools)", tools.len());

                            // Create channel for tool events (sync callback -> async handler)
                            let (event_tx, mut event_rx) = mpsc::unbounded_channel::<ToolEvent>();

                            // Event callback sends to channel
                            let event_callback: Arc<dyn Fn(ToolEvent) + Send + Sync> = Arc::new(move |event| {
                                let _ = event_tx.send(event);
                            });

                            // Spawn task to handle events and update Zulip messages
                            let zulip_for_events = zulip.clone();
                            let topic = format!("director/{}", node_for_task);
                            tokio::spawn(async move {
                                // Track current message for editing
                                let mut current_msg_id: Option<u64> = None;
                                let mut current_content = String::new();
                                let mut pending_calls: Vec<(String, String)> = Vec::new(); // (name, input)

                                while let Some(event) = event_rx.recv().await {
                                    let Some(z) = &zulip_for_events else { continue };

                                    match event {
                                        ToolEvent::ToolCall { name, input } => {
                                            let display_name = format_tool_name(&name);
                                            let input_str = format_tool_input(&name, &input);
                                            pending_calls.push((display_name, input_str));
                                        }
                                        ToolEvent::ToolResult { name, success, output } => {
                                            let display_name = format_tool_name(&name);

                                            // Find matching pending call
                                            if let Some(idx) = pending_calls.iter().position(|(n, _)| n == &display_name) {
                                                let (_, input_str) = pending_calls.remove(idx);

                                                // Format with input + output
                                                let tool_block = format_tool_block(
                                                    &display_name,
                                                    &input_str,
                                                    &output,
                                                    success,
                                                );

                                                // Append to current content
                                                if !current_content.is_empty() {
                                                    current_content.push_str("\n\n");
                                                }
                                                current_content.push_str(&tool_block);

                                                // Update or create message
                                                if let Some(msg_id) = current_msg_id {
                                                    let _ = z.update_message(msg_id, &current_content).await;
                                                } else {
                                                    if let Ok(msg_id) = z.send("palace", &topic, &current_content).await {
                                                        current_msg_id = Some(msg_id);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            });

                            // Create runner - no max iterations, run until complete
                            let config = ToolRunnerConfig {
                                max_iterations: None,
                                verbose: true,
                                on_event: Some(event_callback),
                            };
                            let runner = ToolRunner::with_config(client, tools, config);

                            // Run the agentic loop
                            tracing::info!("Running LLM agent with model: {}", model);

                            let params = MessageCreateParams {
                                model: model.clone(),
                                max_tokens: 4096,
                                messages: vec![MessageParam::user(&prompt_clone)],
                                ..Default::default()
                            };

                            tracing::debug!("Sending request to LLM");
                            match runner.run(params).await {
                                Ok(response) => {
                                    let text = response.text().unwrap_or_default();
                                    tracing::info!("LLM agent completed, response: {} chars", text.len());

                                    // Format thinking in spoiler blocks
                                    let formatted = format_thinking_as_spoiler(&text);

                                    // Report to Zulip
                                    if let Some(z) = zulip {
                                        let _ = z.send(
                                            "palace",
                                            &format!("director/{}", node_for_task),
                                            &format!("✅ **Task Completed**\n\n{}", formatted)
                                        ).await;
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("LLM agent failed: {}", e);
                                    if let Some(z) = zulip {
                                        let _ = z.send(
                                            "palace",
                                            &format!("director/{}", node_for_task),
                                            &format!("❌ **Task Failed**\n\n{}", e)
                                        ).await;
                                    }
                                }
                            }
                        });
                    }
                    _ => {
                        tracing::debug!("Unhandled control command: {:?}", cmd);
                    }
                }
            }
        } => {
            tracing::info!("Control loop ended");
        }

        // Shutdown signal
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C, shutting down...");
        }
    }

    // Send shutdown message
    if let Ok(tool) = ZulipTool::from_env() {
        let _ = tool.send(
            "palace",
            &format!("director/{}", node_name),
            &format!("🛑 Director Pool `{}` shutting down.", node_name)
        ).await;
    }

    Ok(())
}

/// Format thinking/reasoning blocks as Zulip spoilers.
/// Looks for <thinking>...</thinking> or similar patterns and wraps them.
fn format_thinking_as_spoiler(text: &str) -> String {
    let mut result = text.to_string();

    // Handle <thinking>...</thinking> blocks
    while let Some(start) = result.find("<thinking>") {
        if let Some(end) = result[start..].find("</thinking>") {
            let thinking_content = &result[start + 10..start + end];
            let spoiler = format!(
                "```spoiler Thinking\n{}\n```",
                thinking_content.trim()
            );
            result = format!(
                "{}{}{}",
                &result[..start],
                spoiler,
                &result[start + end + 11..]
            );
        } else {
            break;
        }
    }

    // Handle <reasoning>...</reasoning> blocks
    while let Some(start) = result.find("<reasoning>") {
        if let Some(end) = result[start..].find("</reasoning>") {
            let reasoning_content = &result[start + 11..start + end];
            let spoiler = format!(
                "```spoiler Reasoning\n{}\n```",
                reasoning_content.trim()
            );
            result = format!(
                "{}{}{}",
                &result[..start],
                spoiler,
                &result[start + end + 12..]
            );
        } else {
            break;
        }
    }

    result
}

/// Format internal tool name to display name.
fn format_tool_name(name: &str) -> String {
    match name {
        "read_file" => "Read".to_string(),
        "write_file" => "Write".to_string(),
        "edit_file" => "Edit".to_string(),
        "list_directory" => "List".to_string(),
        "glob" => "Glob".to_string(),
        "grep" => "Grep".to_string(),
        "bash" => "Shell".to_string(),
        other => other.to_string(),
    }
}

/// Format tool input for display.
fn format_tool_input(name: &str, input: &std::collections::HashMap<String, serde_json::Value>) -> String {
    match name {
        "read_file" | "write_file" | "edit_file" => {
            input.get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string()
        }
        "list_directory" => {
            input.get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".")
                .to_string()
        }
        "glob" => {
            input.get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("*")
                .to_string()
        }
        "grep" => {
            let pattern = input.get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let path = input.get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            format!("{} in {}", pattern, path)
        }
        "bash" => {
            input.get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string()
        }
        _ => {
            serde_json::to_string_pretty(input)
                .unwrap_or_else(|_| format!("{:?}", input))
        }
    }
}

/// Format a tool call block with input and output.
/// Auto-unspoilers short outputs (< 3 lines).
fn format_tool_block(name: &str, input: &str, output: &str, success: bool) -> String {
    let icon = if success { "🔧" } else { "❌" };
    let output_lines = output.lines().count();

    // Format input (always show for context)
    let input_section = if input.len() > 80 || input.contains('\n') {
        format!("```spoiler Input\n{}\n```", input)
    } else {
        format!("`{}`", input)
    };

    // Format output - unspoiler if short
    let output_section = if output.is_empty() {
        String::new()
    } else if output_lines <= 3 && output.len() < 200 {
        // Short output - show inline
        format!("\n```\n{}\n```", output)
    } else {
        // Long output - spoiler it
        format!("\n```spoiler Output\n{}\n```", output)
    };

    format!("{} **{}** {}{}", icon, name, input_section, output_section)
}
